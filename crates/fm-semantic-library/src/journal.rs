//! Crash-recoverable coordination of the three independently persisted parts
//! of a semantic library.
//!
//! Consent policy lives in the configuration directory (through the settings
//! migration machinery); the authoritative catalog and the runtime state —
//! pause plus last complete indexed generations — live under the
//! semantic-data root. Renaming three files atomically is not one
//! transaction: an interruption between the renames can leave revoked scopes
//! queryable, leave deleted evidence without the exclusion that justified
//! deleting it, or resume ingestion the user paused.
//!
//! A write-ahead journal beneath the semantic-data root closes that gap. Every
//! participant is staged first, an intent record is then made durable — the
//! commit point — and only afterwards are the staged documents installed.
//! Recovery is deterministic at every interruption: a record without a durable
//! intent is rolled back, and a record with one is completed.
//!
//! Every reader and every writer runs under the exclusive library lock, so at
//! most one thread *and* one process sits between recovery and commit.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::lock::{LibraryLock, LibraryLockGuard};
use crate::store::{CATALOG_FILE_NAME, STATE_FILE_NAME, write_atomically};
use crate::{
    LibraryId, OccurrenceId, POLICY_FILE_NAME, SemanticCatalog, SemanticCatalogStore,
    SemanticLibraryPolicy, SemanticLibraryPolicyStore, SemanticLibraryState,
    SemanticLibraryStateStore, StoreError,
};

const JOURNAL_DIRECTORY: &str = "journal";
const INTENT_FILE_NAME: &str = "commit.json";
const JOURNAL_SCHEMA_VERSION: u32 = 2;

/// Stable identity of one durable multi-document transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TransactionId(Uuid);

impl TransactionId {
    fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl std::fmt::Display for TransactionId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, formatter)
    }
}

/// Consent-relevant operation a transaction carries, recorded for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LibraryOperation {
    /// A root was enrolled or its consent metadata changed.
    Enrolment,
    /// A root or descendant was excluded, revoking consent immediately.
    ScopeRevocation,
    /// Destructive cleanup for a revoked scope was planned or advanced.
    ExclusionCleanup,
    /// Availability, root relocation, or a completed reconciliation.
    Reconciliation,
    /// Pause, resume, or a completed indexed generation.
    RuntimeState,
}

/// Participant document of a durable transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransactionParticipant {
    /// The authoritative catalog beneath the semantic-data root.
    Catalog,
    /// The consent policy in the configuration directory.
    Policy,
    /// Pause state and last complete indexed generations.
    State,
}

/// One durable step of the commit protocol.
///
/// The steps are exposed so a host — or a crash-recovery test — can stop
/// between any two of them and let [`SemanticLibraryCoordinator::recover`]
/// decide the outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CommitStep {
    /// Write the new catalog into the journal record.
    StageCatalog,
    /// Write the new policy into the journal record.
    StagePolicy,
    /// Write the new runtime state into the journal record.
    StageState,
    /// Make the intent to install every staged document durable. Commit point.
    RecordIntent,
    /// Install the staged catalog under the semantic-data root.
    InstallCatalog,
    /// Install the staged policy in the configuration directory.
    InstallPolicy,
    /// Install the staged runtime state under the semantic-data root.
    InstallState,
    /// Remove the completed journal record.
    ClearJournal,
}

impl CommitStep {
    /// Returns every commit step in protocol order.
    #[must_use]
    pub const fn all() -> &'static [Self; 8] {
        &[
            Self::StageCatalog,
            Self::StagePolicy,
            Self::StageState,
            Self::RecordIntent,
            Self::InstallCatalog,
            Self::InstallPolicy,
            Self::InstallState,
            Self::ClearJournal,
        ]
    }

    /// Reports whether a failure at this step happens at or after the durable
    /// commit point, so a caller's in-memory snapshot may already disagree
    /// with what recovery will install.
    #[must_use]
    pub const fn is_at_or_after_commit(self) -> bool {
        matches!(
            self,
            Self::RecordIntent
                | Self::InstallCatalog
                | Self::InstallPolicy
                | Self::InstallState
                | Self::ClearJournal
        )
    }
}

/// One journal record resolved by recovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoveredTransaction {
    /// Durable transaction identity.
    pub id: TransactionId,
    /// Consent-relevant operation the record carried.
    pub operation: LibraryOperation,
}

/// What deterministic recovery did to the journal.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecoveryOutcome {
    /// Records whose durable intent was replayed to completion, in commit
    /// order.
    pub completed: Vec<RecoveredTransaction>,
    /// Records interrupted before their intent became durable, which
    /// therefore carry no trustworthy operation.
    pub rolled_back: Vec<TransactionId>,
}

impl RecoveryOutcome {
    /// Reports whether recovery had nothing to do.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.completed.is_empty() && self.rolled_back.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IntentRecord {
    schema_version: u32,
    transaction_id: TransactionId,
    /// Monotonic commit order among concurrently existing records.
    ///
    /// The exclusive lock means a second record can only exist if a process
    /// died mid-commit, but recovery still replays strictly in this order so a
    /// later mutation can never be overwritten by an earlier one.
    sequence: u64,
    library_id: LibraryId,
    operation: LibraryOperation,
    participants: BTreeSet<TransactionParticipant>,
}

/// A durable library snapshot plus what recovery and scope proof had to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedLibrary {
    /// Consent policy, the authority for every scope decision.
    pub policy: SemanticLibraryPolicy,
    /// Authoritative catalog with only policy-proven occurrence scopes.
    pub catalog: SemanticCatalog,
    /// Pause state and last complete indexed generations.
    pub state: SemanticLibraryState,
    /// Occurrences whose persisted scopes were not provable and were dropped.
    pub unproven_scopes: BTreeSet<OccurrenceId>,
    /// Journal recovery performed before loading.
    pub recovery: RecoveryOutcome,
}

/// Crash-recoverable coordinator for the policy, catalog, and state files.
#[derive(Debug, Clone)]
pub struct SemanticLibraryCoordinator {
    policy_store: SemanticLibraryPolicyStore,
    catalog_store: SemanticCatalogStore,
    state_store: SemanticLibraryStateStore,
    semantic_data_root: PathBuf,
    lock: LibraryLock,
}

impl SemanticLibraryCoordinator {
    /// Creates a coordinator over a configuration directory and a semantic-data
    /// root.
    ///
    /// Construction is inert: no directory is created and no file is read
    /// until a lock, load, or transaction is requested. The first such request
    /// creates the semantic-data root and its lock file, because no journal
    /// may be recovered — not even by a read — without the cross-process lock.
    #[must_use]
    pub fn new(
        configuration_directory: impl Into<PathBuf>,
        semantic_data_root: impl Into<PathBuf>,
    ) -> Self {
        let semantic_data_root = semantic_data_root.into();
        Self {
            policy_store: SemanticLibraryPolicyStore::new(configuration_directory),
            catalog_store: SemanticCatalogStore::new(semantic_data_root.clone()),
            state_store: SemanticLibraryStateStore::new(semantic_data_root.clone()),
            lock: LibraryLock::new(semantic_data_root.clone()),
            semantic_data_root,
        }
    }

    /// Returns the consent-policy store.
    #[must_use]
    pub const fn policy_store(&self) -> &SemanticLibraryPolicyStore {
        &self.policy_store
    }

    /// Returns the catalog store.
    #[must_use]
    pub const fn catalog_store(&self) -> &SemanticCatalogStore {
        &self.catalog_store
    }

    /// Returns the runtime-state store.
    #[must_use]
    pub const fn state_store(&self) -> &SemanticLibraryStateStore {
        &self.state_store
    }

    /// Returns the journal directory beneath the semantic-data root.
    #[must_use]
    pub fn journal_directory(&self) -> PathBuf {
        self.semantic_data_root.join(JOURNAL_DIRECTORY)
    }

    /// Acquires the exclusive cross-process library lock.
    ///
    /// Every read *and* every mutation must hold this: reads recover the
    /// journal, and mutations additionally need it across reload, the
    /// optimistic revision check, the mutation itself, and the durable commit.
    /// The returned session exposes the same operations as the coordinator
    /// *without* re-acquiring the lock, so nested calls cannot deadlock on the
    /// non-reentrant process mutex.
    ///
    /// Acquisition creates the semantic-data root and the lock file when they
    /// are absent; nothing else about the library is created.
    ///
    /// # Errors
    ///
    /// Returns a filesystem or unsafe-path failure.
    pub fn lock(&self) -> Result<LibrarySession<'_>, StoreError> {
        let guard = self.lock.acquire()?;
        Ok(LibrarySession {
            coordinator: self,
            _guard: guard,
        })
    }

    /// Completes or rolls back every interrupted transaction.
    ///
    /// A record whose intent never became durable is discarded; a record with a
    /// durable intent is installed again, which is idempotent. Recovery may
    /// therefore run any number of times and at any interruption point.
    ///
    /// # Errors
    ///
    /// Returns a typed filesystem, JSON, validation, journal-corruption, or
    /// lock failure. A committed record is never discarded to make an error go
    /// away.
    pub fn recover(&self) -> Result<RecoveryOutcome, StoreError> {
        self.lock()?.recover()
    }

    /// Recovers the journal and then loads a consistent library snapshot.
    ///
    /// Occurrence scopes that the policy cannot prove — a tampered record
    /// naming a sibling root, an unreferenced workspace, or a location outside
    /// the root — are dropped before the catalog is handed out, so a forged
    /// record degrades to no access instead of another root's access.
    ///
    /// # Errors
    ///
    /// Returns a typed recovery, filesystem, JSON, lock, or validation
    /// failure, and [`StoreError::PolicyMissing`] when no consent policy
    /// exists.
    pub fn load(&self) -> Result<LoadedLibrary, StoreError> {
        self.lock()?.load()
    }

    fn recover_locked(&self) -> Result<RecoveryOutcome, StoreError> {
        let mut outcome = RecoveryOutcome::default();
        for record in self.journal_records()? {
            match read_intent(&record)? {
                Some(intent) => {
                    self.install(&record, &intent)?;
                    outcome.completed.push(RecoveredTransaction {
                        id: intent.transaction_id,
                        operation: intent.operation,
                    });
                }
                None => {
                    let id = read_transaction_id(&record);
                    remove_record(&record)?;
                    if let Some(id) = id {
                        outcome.rolled_back.push(id);
                    }
                }
            }
        }
        Ok(outcome)
    }

    /// Returns journal records sorted by their durable commit sequence.
    ///
    /// The sequence is the record directory's zero-padded prefix, so the sort
    /// is a plain lexicographic sort and needs no file reads.
    fn journal_records(&self) -> Result<Vec<PathBuf>, StoreError> {
        let entries = match fs::read_dir(self.journal_directory()) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut records: Vec<PathBuf> = Vec::new();
        for entry in entries {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                records.push(entry.path());
            }
        }
        records.sort();
        Ok(records)
    }

    fn next_sequence(&self) -> Result<u64, StoreError> {
        let mut next = 1;
        for record in self.journal_records()? {
            if let Some(sequence) = record_sequence(&record) {
                next = next.max(sequence.saturating_add(1));
            }
        }
        Ok(next)
    }

    fn load_locked(&self) -> Result<LoadedLibrary, StoreError> {
        let recovery = self.recover_locked()?;
        let policy = self.policy_store.load()?;
        let library_id = policy.library().id();
        let mut catalog = self
            .catalog_store
            .load_optional()?
            .unwrap_or_else(|| SemanticCatalog::new(library_id));
        let state = self
            .state_store
            .load_optional()?
            .unwrap_or_else(|| SemanticLibraryState::new(library_id));
        if catalog.library_id() != library_id || state.library_id() != library_id {
            return Err(StoreError::LibraryMismatch);
        }
        let unproven_scopes = catalog.retain_proven_scopes(&policy)?;
        Ok(LoadedLibrary {
            policy,
            catalog,
            state,
            unproven_scopes,
            recovery,
        })
    }

    fn transaction_locked(
        &self,
        operation: LibraryOperation,
        policy: Option<&SemanticLibraryPolicy>,
        catalog: Option<&SemanticCatalog>,
        state: Option<&SemanticLibraryState>,
    ) -> Result<LibraryTransaction<'_>, StoreError> {
        let ids: Vec<LibraryId> = policy
            .map(|policy| policy.library().id())
            .into_iter()
            .chain(catalog.map(SemanticCatalog::library_id))
            .chain(state.map(SemanticLibraryState::library_id))
            .collect();
        let Some(library_id) = ids.first().copied() else {
            return Err(StoreError::EmptyTransaction);
        };
        if ids.iter().any(|id| *id != library_id) {
            return Err(StoreError::LibraryMismatch);
        }
        // A committed record still on disk means an earlier mutation has not
        // been installed. Staging a second one over it would install documents
        // derived from a snapshot that predates it, so refuse instead.
        for record in self.journal_records()? {
            if read_intent(&record)?.is_some() {
                return Err(StoreError::PendingJournalRecord);
            }
        }
        if let Some(policy) = policy {
            policy.validate_structure()?;
        }
        if let Some(catalog) = catalog {
            catalog.validate()?;
        }
        if let Some(state) = state {
            state.validate()?;
        }
        let mut participants = BTreeSet::new();
        let mut steps = Vec::new();
        if catalog.is_some() {
            participants.insert(TransactionParticipant::Catalog);
            steps.push(CommitStep::StageCatalog);
        }
        if policy.is_some() {
            participants.insert(TransactionParticipant::Policy);
            steps.push(CommitStep::StagePolicy);
        }
        if state.is_some() {
            participants.insert(TransactionParticipant::State);
            steps.push(CommitStep::StageState);
        }
        steps.push(CommitStep::RecordIntent);
        if catalog.is_some() {
            steps.push(CommitStep::InstallCatalog);
        }
        if policy.is_some() {
            steps.push(CommitStep::InstallPolicy);
        }
        if state.is_some() {
            steps.push(CommitStep::InstallState);
        }
        steps.push(CommitStep::ClearJournal);
        steps.reverse();
        let intent = IntentRecord {
            schema_version: JOURNAL_SCHEMA_VERSION,
            transaction_id: TransactionId::new(),
            sequence: self.next_sequence()?,
            library_id,
            operation,
            participants,
        };
        Ok(LibraryTransaction {
            coordinator: self,
            directory: self
                .journal_directory()
                .join(record_directory_name(&intent)),
            intent,
            policy: policy.cloned(),
            catalog: catalog.cloned(),
            state: state.cloned(),
            remaining: steps,
        })
    }

    fn install(&self, record: &Path, intent: &IntentRecord) -> Result<(), StoreError> {
        if intent
            .participants
            .contains(&TransactionParticipant::Catalog)
        {
            let catalog: SemanticCatalog = read_staged(record, CATALOG_FILE_NAME)?;
            if catalog.library_id() != intent.library_id {
                return Err(StoreError::JournalLibraryMismatch);
            }
            self.catalog_store.save(&catalog)?;
        }
        if intent
            .participants
            .contains(&TransactionParticipant::Policy)
        {
            let policy: SemanticLibraryPolicy = read_staged(record, POLICY_FILE_NAME)?;
            if policy.library().id() != intent.library_id {
                return Err(StoreError::JournalLibraryMismatch);
            }
            self.policy_store.save(&policy)?;
        }
        if intent.participants.contains(&TransactionParticipant::State) {
            let state = read_staged_state(record)?;
            if state.library_id() != intent.library_id {
                return Err(StoreError::JournalLibraryMismatch);
            }
            self.state_store.save(&state)?;
        }
        remove_record(record)
    }
}

/// An exclusively locked semantic library.
///
/// Every method behaves exactly like its [`SemanticLibraryCoordinator`]
/// counterpart but reuses the already-held lock, so a mutation may recover,
/// reload, check, mutate, and commit without any window for another thread or
/// process, and without re-entering the non-reentrant process mutex.
#[derive(Debug)]
pub struct LibrarySession<'coordinator> {
    coordinator: &'coordinator SemanticLibraryCoordinator,
    _guard: LibraryLockGuard<'coordinator>,
}

// The guard field is deliberately named with a leading underscore because its
// value is the lock itself, not data: acquiring the session already took both
// the in-process and the cross-process lock.

impl<'coordinator> LibrarySession<'coordinator> {
    /// Completes or rolls back every interrupted transaction.
    ///
    /// # Errors
    ///
    /// Returns a typed filesystem, JSON, validation, or journal-corruption
    /// failure.
    pub fn recover(&self) -> Result<RecoveryOutcome, StoreError> {
        self.coordinator.recover_locked()
    }

    /// Reports whether a record whose intent is already durable is still
    /// waiting to be installed.
    ///
    /// This is the cheap correctness probe a cached reader must run *before*
    /// its durable-revision fast path: a writer that died between
    /// [`CommitStep::RecordIntent`] and the last install left the visible
    /// documents — and therefore the visible revision — at their old values,
    /// while the committed truth sits in the journal. Only a directory scan
    /// plus one small read per record is performed, so proving that nothing is
    /// pending stays far cheaper than reloading the catalog.
    ///
    /// # Errors
    ///
    /// Returns a typed filesystem or journal-corruption failure.
    pub fn has_pending_record(&self) -> Result<bool, StoreError> {
        for record in self.coordinator.journal_records()? {
            if read_intent(&record)?.is_some() {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Recovers the journal and loads a consistent library snapshot.
    ///
    /// # Errors
    ///
    /// Returns a typed recovery, filesystem, JSON, or validation failure, and
    /// [`StoreError::PolicyMissing`] when no consent policy exists.
    pub fn load(&self) -> Result<LoadedLibrary, StoreError> {
        self.coordinator.load_locked()
    }

    /// Reads only the durable policy revision, as a cheap staleness probe for
    /// a cached snapshot.
    ///
    /// The exclusive cross-process lock is already held, so the value cannot
    /// change underneath the caller for as long as the session lives — but it
    /// is only the *installed* revision, so [`Self::has_pending_record`] must
    /// be consulted first.
    ///
    /// # Errors
    ///
    /// Returns a typed filesystem, JSON, migration, or validation failure.
    pub fn durable_revision(&self) -> Result<Option<u64>, StoreError> {
        Ok(self
            .coordinator
            .policy_store
            .load_optional()?
            .map(|policy| policy.revision()))
    }

    /// Stages a durable transaction over any combination of participants.
    ///
    /// # Errors
    ///
    /// Rejects participants from different libraries, a transaction with no
    /// participants, a still-pending committed record, and filesystem or
    /// serialization failures.
    pub fn transaction(
        &self,
        operation: LibraryOperation,
        policy: Option<&SemanticLibraryPolicy>,
        catalog: Option<&SemanticCatalog>,
        state: Option<&SemanticLibraryState>,
    ) -> Result<LibraryTransaction<'coordinator>, StoreError> {
        self.coordinator
            .transaction_locked(operation, policy, catalog, state)
    }
}

/// A staged multi-document transaction whose commit steps can be driven one at
/// a time.
///
/// Dropping the value without committing simulates — and survives — a crash:
/// the journal record stays on disk untouched and the next
/// [`SemanticLibraryCoordinator::recover`] resolves it deterministically.
#[derive(Debug)]
pub struct LibraryTransaction<'coordinator> {
    coordinator: &'coordinator SemanticLibraryCoordinator,
    directory: PathBuf,
    intent: IntentRecord,
    policy: Option<SemanticLibraryPolicy>,
    catalog: Option<SemanticCatalog>,
    state: Option<SemanticLibraryState>,
    remaining: Vec<CommitStep>,
}

impl LibraryTransaction<'_> {
    /// Returns the durable transaction id.
    #[must_use]
    pub const fn id(&self) -> TransactionId {
        self.intent.transaction_id
    }

    /// Returns the step that will run next, or `None` when finished.
    #[must_use]
    pub fn next_step(&self) -> Option<CommitStep> {
        self.remaining.last().copied()
    }

    /// Performs exactly one durable step.
    ///
    /// # Errors
    ///
    /// Returns a typed filesystem, JSON, or validation failure.
    pub fn advance(&mut self) -> Result<Option<CommitStep>, StoreError> {
        let Some(step) = self.remaining.pop() else {
            return Ok(None);
        };
        match step {
            CommitStep::StageCatalog => {
                let catalog = self
                    .catalog
                    .as_ref()
                    .ok_or(StoreError::TransactionOutOfOrder)?;
                write_atomically(
                    &self.directory,
                    CATALOG_FILE_NAME,
                    &serde_json::to_vec_pretty(catalog)?,
                )?;
            }
            CommitStep::StagePolicy => {
                let policy = self
                    .policy
                    .as_ref()
                    .ok_or(StoreError::TransactionOutOfOrder)?;
                write_atomically(
                    &self.directory,
                    POLICY_FILE_NAME,
                    &serde_json::to_vec_pretty(policy)?,
                )?;
            }
            CommitStep::StageState => {
                let state = self
                    .state
                    .as_ref()
                    .ok_or(StoreError::TransactionOutOfOrder)?;
                write_atomically(
                    &self.directory,
                    STATE_FILE_NAME,
                    &serde_json::to_vec_pretty(state)?,
                )?;
            }
            CommitStep::RecordIntent => {
                write_atomically(
                    &self.directory,
                    INTENT_FILE_NAME,
                    &serde_json::to_vec_pretty(&self.intent)?,
                )?;
            }
            CommitStep::InstallCatalog => {
                let catalog: SemanticCatalog = read_staged(&self.directory, CATALOG_FILE_NAME)?;
                self.coordinator.catalog_store.save(&catalog)?;
            }
            CommitStep::InstallPolicy => {
                let policy: SemanticLibraryPolicy = read_staged(&self.directory, POLICY_FILE_NAME)?;
                self.coordinator.policy_store.save(&policy)?;
            }
            CommitStep::InstallState => {
                let state = read_staged_state(&self.directory)?;
                self.coordinator.state_store.save(&state)?;
            }
            CommitStep::ClearJournal => remove_record(&self.directory)?,
        }
        Ok(Some(step))
    }

    /// Runs every remaining step.
    ///
    /// # Errors
    ///
    /// Returns the first failing step's typed failure; the journal record is
    /// left for recovery rather than silently discarded.
    pub fn commit(mut self) -> Result<TransactionId, StoreError> {
        while self.advance()?.is_some() {}
        Ok(self.intent.transaction_id)
    }

    /// Abandons the transaction as an interrupted process would, leaving the
    /// journal record exactly as the last completed step left it.
    pub fn interrupt(self) -> TransactionId {
        self.intent.transaction_id
    }
}

fn record_directory_name(intent: &IntentRecord) -> String {
    format!("{:020}-{}", intent.sequence, intent.transaction_id.0)
}

fn record_sequence(record: &Path) -> Option<u64> {
    record
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.split_once('-'))
        .and_then(|(sequence, _)| sequence.parse::<u64>().ok())
}

fn read_transaction_id(record: &Path) -> Option<TransactionId> {
    record
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.split_once('-'))
        .and_then(|(_, id)| id.parse::<Uuid>().ok())
        .map(TransactionId)
}

fn read_intent(record: &Path) -> Result<Option<IntentRecord>, StoreError> {
    let bytes = match fs::read(record.join(INTENT_FILE_NAME)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let intent: IntentRecord =
        serde_json::from_slice(&bytes).map_err(|_| StoreError::JournalCorrupt)?;
    if intent.schema_version != JOURNAL_SCHEMA_VERSION || intent.participants.is_empty() {
        return Err(StoreError::JournalCorrupt);
    }
    Ok(Some(intent))
}

/// Reads a staged runtime-state document, migrating a record that an older
/// build staged before it was interrupted.
fn read_staged_state(record: &Path) -> Result<SemanticLibraryState, StoreError> {
    let value: serde_json::Value = read_staged(record, STATE_FILE_NAME)?;
    SemanticLibraryState::migrate(value).map_err(|_| StoreError::JournalCorrupt)
}

fn read_staged<T: serde::de::DeserializeOwned>(
    record: &Path,
    file_name: &str,
) -> Result<T, StoreError> {
    let bytes = fs::read(record.join(file_name)).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            StoreError::JournalCorrupt
        } else {
            StoreError::Io(error)
        }
    })?;
    serde_json::from_slice(&bytes).map_err(|_| StoreError::JournalCorrupt)
}

fn remove_record(record: &Path) -> Result<(), StoreError> {
    match fs::remove_dir_all(record) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    }
    if let Some(parent) = record.parent() {
        match fm_settings::sync_directory(parent) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
