//! Classification and handling of transfers interrupted by a crash or restart (task 0300).
//!
//! The transfer journal (task 0299) says what each queued or running job was doing when the
//! previous process stopped. This module re-checks that against the providers *now* and decides
//! what is safe to do with each job:
//!
//! * **resumable**: the provider can prove the partial destination belongs to the job and the
//!   source is unchanged. No provider can prove that yet (task 0303), so nothing is classified
//!   resumable in production; the rule is in place and tested.
//! * **safely restartable**: nothing the job did is visible to users except private
//!   `.fm-copy-*` partials, which are discarded before running the remaining items again.
//! * **completed-unconfirmed**: every remaining item looks published but the job never confirmed
//!   it. The user acknowledges it instead of re-running it.
//! * **needs intervention**: a source changed or vanished, a destination was replaced, or a
//!   provider cannot be reached. A person decides.
//!
//! Nothing here acts without an explicit request: classification only reads. Restart and discard
//! are separate calls, and neither appends to an unverified destination.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use fm_domain::{EntryId, Location, OperationId};
use fm_operations::journal::{
    JournalDamage, OperationJournal, RecoveredItem, RecoveredJob, SourceRevision,
};
use fm_operations::{ConflictPolicy, OperationKind};
use fm_transport_dto::{
    InterruptedTransferDto, InterruptedTransfersDto, OperationConflictPolicyDto, OperationDto,
    OperationKindDto, StartOperationRequestDto, SymlinkPolicyDto, TransferRecoveryClassDto,
    TransferRecoveryReasonDto,
};
use fm_vfs::{EntryRef, ProviderRegistry, VfsError};
use tokio_util::sync::CancellationToken;

use crate::error::ApplicationError;

/// What a source looks like now, relative to what the job recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourceState {
    /// Present, and no recorded revision contradicts it.
    Unchanged,
    /// Present but its size or modification time differs from the recorded revision.
    Changed,
    /// No longer exists.
    Missing,
    /// The provider could not answer.
    Unavailable,
}

/// What a probed location looks like now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Presence {
    /// Does not exist.
    Absent,
    /// Exists, with its size when the provider reports one.
    Present(Option<u64>),
    /// The provider could not answer.
    Unavailable,
}

/// Everything the classifier needs to know about one planned item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ItemFacts {
    /// The job recorded finishing the item.
    pub committed: bool,
    /// The job announced a private partial for the item before stopping.
    pub announced: bool,
    /// The announcement also named where the result is published. Journals written before
    /// task 0300 name only the partial.
    pub destination_recorded: bool,
    /// The published destination is not older than the job, so the job (not an earlier run or a
    /// pre-existing file) is the likeliest author. True when the provider reports no timestamp.
    pub destination_fresh: bool,
    pub source: SourceState,
    /// Size the source reports now.
    pub source_size: Option<u64>,
    /// The published destination, when the job announced one.
    pub destination: Presence,
    /// The private partial, when the job announced one.
    pub partial: Presence,
    /// The provider can prove the partial belongs to this job and continues the same source.
    pub resume_proven: bool,
}

/// What to do about one item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ItemVerdict {
    /// Finished and confirmed.
    Done,
    /// Can continue from the partial.
    Resumable,
    /// Safe to run again from the start.
    Restartable(TransferRecoveryReasonDto),
    /// Looks published; only unconfirmed.
    Unconfirmed,
    /// A person has to decide.
    Intervention(TransferRecoveryReasonDto),
}

/// Decides what is safe for one item. Conservative: anything not provably safe needs a person,
/// and an unverified destination is never something to append to.
pub(crate) fn classify_item(facts: &ItemFacts) -> ItemVerdict {
    use TransferRecoveryReasonDto as Reason;
    if facts.committed {
        return ItemVerdict::Done;
    }
    if facts.source == SourceState::Unavailable
        || facts.destination == Presence::Unavailable
        || facts.partial == Presence::Unavailable
    {
        return ItemVerdict::Intervention(Reason::ProviderUnavailable);
    }
    if facts.announced && !facts.destination_recorded {
        // Where the result would be published is unknown, so whether it was published cannot
        // be checked; guessing could re-copy over a finished file or confirm an unwritten one.
        return ItemVerdict::Intervention(Reason::JournalDamaged);
    }
    if !facts.announced {
        return match facts.source {
            SourceState::Missing => ItemVerdict::Intervention(Reason::SourceMissing),
            SourceState::Changed => ItemVerdict::Intervention(Reason::SourceChanged),
            _ => ItemVerdict::Restartable(Reason::NotStarted),
        };
    }
    match (facts.destination, facts.partial) {
        // Published (the partial was renamed into place) but the commit record was lost. A
        // missing source is fine: a move deletes it after publishing.
        (Presence::Present(_), Presence::Absent) if !facts.destination_fresh => {
            // The destination predates the job: it was already there, so the original conflict
            // policy has to decide again rather than this being called finished.
            ItemVerdict::Restartable(Reason::PartialUnfinished)
        }
        (Presence::Present(size), Presence::Absent) => {
            let matches = match (size, facts.source_size) {
                (Some(destination), Some(source)) => destination == source,
                _ => true,
            };
            if matches {
                ItemVerdict::Unconfirmed
            } else {
                ItemVerdict::Intervention(Reason::DestinationReplaced)
            }
        }
        // A destination exists next to a live partial: it was not published by this job, so
        // the original conflict policy has to decide again on restart.
        (Presence::Present(_), Presence::Present(_)) => {
            ItemVerdict::Restartable(Reason::PartialUnfinished)
        }
        _ => match facts.source {
            SourceState::Missing => ItemVerdict::Intervention(Reason::SourceMissing),
            SourceState::Changed => ItemVerdict::Intervention(Reason::SourceChanged),
            _ if facts.resume_proven && matches!(facts.partial, Presence::Present(_)) => {
                ItemVerdict::Resumable
            }
            _ => ItemVerdict::Restartable(Reason::PartialUnfinished),
        },
    }
}

/// The classification of one whole job plus the counts shown to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct JobVerdict {
    pub class: TransferRecoveryClassDto,
    pub reason: TransferRecoveryReasonDto,
    pub total: u64,
    pub committed: u64,
    pub unconfirmed: u64,
    pub partials: u64,
    pub remaining: u64,
}

/// Rolls item verdicts up to one job-level decision. `damage` is what replay found in the
/// journal itself.
pub(crate) fn classify_job(
    verdicts: &[(ItemVerdict, Presence)],
    damage: Option<&JournalDamage>,
) -> JobVerdict {
    use TransferRecoveryClassDto as Class;
    use TransferRecoveryReasonDto as Reason;
    let count = |predicate: &dyn Fn(&ItemVerdict) -> bool| -> u64 {
        verdicts
            .iter()
            .filter(|(verdict, _)| predicate(verdict))
            .count() as u64
    };
    let total = verdicts.len() as u64;
    let committed = count(&|verdict| *verdict == ItemVerdict::Done);
    let unconfirmed = count(&|verdict| *verdict == ItemVerdict::Unconfirmed);
    let partials = verdicts
        .iter()
        .filter(|(_, partial)| matches!(partial, Presence::Present(_)))
        .count() as u64;
    let remaining = total - committed - unconfirmed;
    let finish = |class, reason| JobVerdict {
        class,
        reason,
        total,
        committed,
        unconfirmed,
        partials,
        remaining,
    };
    // A corrupt record in the middle means how far the job got is unknown, whatever the
    // surviving records say. A torn tail is the ordinary shape of a crash.
    if matches!(damage, Some(JournalDamage::CorruptRecord { .. })) {
        return finish(Class::NeedsIntervention, Reason::JournalDamaged);
    }
    if let Some(reason) = verdicts.iter().find_map(|(verdict, _)| match verdict {
        ItemVerdict::Intervention(reason) => Some(*reason),
        _ => None,
    }) {
        return finish(Class::NeedsIntervention, reason);
    }
    let restartable = count(&|verdict| matches!(verdict, ItemVerdict::Restartable(_)));
    let resumable = count(&|verdict| *verdict == ItemVerdict::Resumable);
    if restartable > 0 {
        let unfinished = verdicts.iter().any(|(verdict, _)| {
            matches!(verdict, ItemVerdict::Restartable(Reason::PartialUnfinished))
        });
        let reason = if unfinished {
            Reason::PartialUnfinished
        } else {
            Reason::NotStarted
        };
        return finish(Class::SafelyRestartable, reason);
    }
    if resumable > 0 {
        return finish(Class::Resumable, Reason::PartialUnfinished);
    }
    if total == 0 {
        return finish(Class::SafelyRestartable, Reason::NotStarted);
    }
    finish(Class::CompletedUnconfirmed, Reason::PublishedUnconfirmed)
}

async fn probe_with_mtime(
    providers: &ProviderRegistry,
    location: &Location,
) -> (Presence, Option<i64>) {
    let Ok(provider) = providers.resolve(location) else {
        return (Presence::Unavailable, None);
    };
    let entry = EntryRef {
        id: EntryId::new(),
        location: location.clone(),
    };
    match provider.inspect(&entry, CancellationToken::new()).await {
        Ok(summary) => (
            Presence::Present(summary.size),
            summary.modified_at.map(|at| at.timestamp_millis()),
        ),
        Err(VfsError::NotFound { .. }) => (Presence::Absent, None),
        Err(_) => (Presence::Unavailable, None),
    }
}

async fn probe(providers: &ProviderRegistry, location: &Location) -> Presence {
    probe_with_mtime(providers, location).await.0
}

fn source_state(
    revision: Option<&SourceRevision>,
    probed: (Presence, Option<i64>),
) -> (SourceState, Option<u64>) {
    match probed {
        (Presence::Absent, _) => (SourceState::Missing, None),
        (Presence::Unavailable, _) => (SourceState::Unavailable, None),
        (Presence::Present(size), modified_at_ms) => {
            let size_changed = revision
                .and_then(|revision| revision.size)
                .zip(size)
                .is_some_and(|(recorded, now)| recorded != now);
            let time_changed = revision
                .and_then(|revision| revision.modified_at_ms)
                .zip(modified_at_ms)
                .is_some_and(|(recorded, now)| recorded != now);
            (
                if size_changed || time_changed {
                    SourceState::Changed
                } else {
                    SourceState::Unchanged
                },
                size,
            )
        }
    }
}

async fn item_facts(
    providers: &ProviderRegistry,
    item: &RecoveredItem,
    job_created_at_ms: i64,
) -> ItemFacts {
    let (source, source_size) = source_state(
        item.revision.as_ref(),
        probe_with_mtime(providers, &item.source).await,
    );
    let (destination, destination_modified) = match &item.destination {
        Some(location) => probe_with_mtime(providers, location).await,
        None => (Presence::Absent, None),
    };
    let partial = match &item.partial {
        Some(location) => probe(providers, location).await,
        None => Presence::Absent,
    };
    ItemFacts {
        committed: item.committed,
        announced: item.partial.is_some(),
        destination_recorded: item.destination.is_some(),
        destination_fresh: destination_modified.is_none_or(|at| at >= job_created_at_ms),
        source,
        source_size,
        destination,
        partial,
        // Task 0303 turns this on per provider once it can prove ownership of a partial.
        resume_proven: false,
    }
}

/// An interrupted job together with its freshly computed verdict.
struct Assessed {
    job: RecoveredJob,
    verdict: JobVerdict,
    partials: Vec<Location>,
    /// A recorded partial could not be probed, so it may exist but cannot be removed.
    unreachable_partial: bool,
    /// Sources whose items finished or look published.
    finished: Vec<Location>,
}

/// Holds the jobs the previous process left behind and carries out the user's decisions.
pub(crate) struct TransferRecoveryCoordinator {
    providers: ProviderRegistry,
    journal: Option<Arc<dyn OperationJournal>>,
    jobs: Mutex<Vec<RecoveredJob>>,
    /// Jobs a restart or discard is working on, so a double click or a second client cannot
    /// run the same job twice or delete partials under a running restart.
    claimed: Mutex<HashSet<OperationId>>,
}

/// Holds a job exclusively until dropped.
struct Claim<'a> {
    claimed: &'a Mutex<HashSet<OperationId>>,
    id: OperationId,
}

impl Drop for Claim<'_> {
    fn drop(&mut self) {
        self.claimed
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&self.id);
    }
}

impl TransferRecoveryCoordinator {
    pub(crate) fn new(
        providers: ProviderRegistry,
        journal: Option<Arc<dyn OperationJournal>>,
        jobs: Vec<RecoveredJob>,
    ) -> Self {
        Self {
            providers,
            journal,
            jobs: Mutex::new(jobs),
            claimed: Mutex::new(HashSet::new()),
        }
    }

    fn snapshot(&self) -> Vec<RecoveredJob> {
        self.jobs
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    async fn assess(&self, job: RecoveredJob) -> Assessed {
        let mut verdicts = Vec::with_capacity(job.items.len());
        let mut partials = Vec::new();
        let mut finished = Vec::new();
        let mut unreachable_partial = false;
        let created_at_ms = job.intent.created_at.timestamp_millis();
        for item in &job.items {
            let facts = item_facts(&self.providers, item, created_at_ms).await;
            unreachable_partial |= facts.partial == Presence::Unavailable;
            if matches!(facts.partial, Presence::Present(_))
                && let Some(partial) = &item.partial
            {
                partials.push(partial.clone());
            }
            let verdict = classify_item(&facts);
            if matches!(verdict, ItemVerdict::Done | ItemVerdict::Unconfirmed) {
                finished.push(item.source.clone());
            }
            verdicts.push((verdict, facts.partial));
        }
        let verdict = classify_job(&verdicts, job.damage.as_ref());
        Assessed {
            job,
            verdict,
            partials,
            unreachable_partial,
            finished,
        }
    }

    fn claim(&self, id: OperationId) -> Result<Claim<'_>, ApplicationError> {
        if self
            .claimed
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(id)
        {
            Ok(Claim {
                claimed: &self.claimed,
                id,
            })
        } else {
            Err(ApplicationError::InvalidRequest(
                "this transfer is already being handled".into(),
            ))
        }
    }

    /// The sources and destination a job was asked to touch, read from its stored intent
    /// without probing any provider. Used to authorise an action cheaply.
    pub(crate) fn intent_locations(
        &self,
        id: OperationId,
    ) -> Option<(Vec<Location>, Option<Location>)> {
        self.snapshot()
            .into_iter()
            .find(|job| job.intent.operation_id == id)
            .map(|job| (job.intent.sources, job.intent.destination))
    }

    /// Lists the interrupted transfers with a fresh classification, oldest first.
    pub(crate) async fn list(&self) -> InterruptedTransfersDto {
        let mut transfers = Vec::new();
        for job in self
            .snapshot()
            .into_iter()
            .filter(|job| is_transfer(job.intent.kind))
        {
            let assessed = self.assess(job).await;
            transfers.push(to_dto(&assessed));
        }
        transfers.sort_by_key(|transfer| transfer.created_at);
        InterruptedTransfersDto { transfers }
    }

    fn find(&self, id: OperationId) -> Result<RecoveredJob, ApplicationError> {
        self.snapshot()
            .into_iter()
            .find(|job| job.intent.operation_id == id)
            .ok_or(ApplicationError::NotFound)
    }

    async fn remove_partials(&self, partials: &[Location]) -> Result<(), ApplicationError> {
        for partial in partials {
            let provider = self
                .providers
                .resolve(partial)
                .map_err(|_| ApplicationError::ProviderUnavailable)?;
            match provider
                .discard_copy(partial, CancellationToken::new())
                .await
            {
                Ok(()) | Err(VfsError::NotFound { .. }) => {}
                Err(error) => {
                    return Err(ApplicationError::InvalidRequest(format!(
                        "a private partial could not be removed: {error}"
                    )));
                }
            }
        }
        Ok(())
    }

    async fn forget(&self, id: OperationId) {
        if let Some(journal) = &self.journal {
            journal.finish(id).await;
        }
        self.jobs
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .retain(|job| job.intent.operation_id != id);
    }

    /// Acknowledges an interrupted transfer without running anything: its private partials are
    /// removed and its record is dropped. Finished destinations are left untouched.
    ///
    /// Refuses while a recorded partial cannot be reached: dropping the record then would orphan
    /// a partial nothing could ever clean up.
    pub(crate) async fn discard(&self, id: OperationId) -> Result<(), ApplicationError> {
        let _claim = self.claim(id)?;
        let assessed = self.assess(self.find(id)?).await;
        if assessed.unreachable_partial {
            return Err(ApplicationError::ProviderUnavailable);
        }
        self.remove_partials(&assessed.partials).await?;
        self.forget(id).await;
        Ok(())
    }

    /// Runs the unfinished work again as a new operation.
    ///
    /// The new operation is started first, and only then are the private partials removed, so a
    /// failed start loses nothing. Sources that already finished are left out; when a finished
    /// item is a child of a directory source (which cannot be left out), the skip policy keeps it
    /// from being copied again or reported as a conflict.
    pub(crate) async fn restart(
        &self,
        id: OperationId,
        start: impl FnOnce(StartOperationRequestDto) -> Result<OperationDto, ApplicationError>,
    ) -> Result<OperationDto, ApplicationError> {
        let _claim = self.claim(id)?;
        let assessed = self.assess(self.find(id)?).await;
        if assessed.unreachable_partial {
            return Err(ApplicationError::ProviderUnavailable);
        }
        let request = restart_request(&assessed)?;
        let started = start(request)?;
        if let Err(error) = self.remove_partials(&assessed.partials).await {
            // The transfer is already running again; a leftover partial is only wasted space.
            tracing::warn!(%error, "an interrupted transfer's partial could not be removed");
        }
        self.forget(id).await;
        Ok(started)
    }
}

fn is_transfer(kind: OperationKind) -> bool {
    matches!(
        kind,
        OperationKind::Copy | OperationKind::Move | OperationKind::Duplicate
    )
}

fn restart_request(assessed: &Assessed) -> Result<StartOperationRequestDto, ApplicationError> {
    let intent = &assessed.job.intent;
    let kind = match intent.kind {
        OperationKind::Copy => OperationKindDto::Copy,
        OperationKind::Move => OperationKindDto::Move,
        OperationKind::Duplicate => OperationKindDto::Duplicate,
        other => {
            return Err(ApplicationError::InvalidRequest(format!(
                "{other:?} transfers cannot be restarted"
            )));
        }
    };
    let finished: HashSet<&Location> = assessed.finished.iter().collect();
    let sources: Vec<Location> = intent
        .sources
        .iter()
        .filter(|source| !finished.contains(source))
        .cloned()
        .collect();
    if sources.is_empty() {
        return Err(ApplicationError::InvalidRequest(
            "nothing is left to run; mark this transfer as done instead".into(),
        ));
    }
    let roots: HashSet<&Location> = intent.sources.iter().collect();
    let child_finished = assessed
        .finished
        .iter()
        .any(|source| !roots.contains(source));
    let conflict_policy = if child_finished {
        OperationConflictPolicyDto::Skip
    } else {
        match intent.conflict_policy {
            ConflictPolicy::Ask | ConflictPolicy::KeepNewer => OperationConflictPolicyDto::Ask,
            ConflictPolicy::Skip => OperationConflictPolicyDto::Skip,
            ConflictPolicy::Overwrite => OperationConflictPolicyDto::Overwrite,
            ConflictPolicy::RenameNew => OperationConflictPolicyDto::RenameNew,
        }
    };
    Ok(StartOperationRequestDto {
        operation_type: kind,
        sources: sources.into_iter().map(Into::into).collect(),
        destination: intent.destination.clone().map(Into::into),
        destinations: Vec::new(),
        conflict_policy,
        name: None,
        archive_format: None,
        archive_compression_level: None,
        create_intermediate_directories: false,
        initial_content: None,
        skip_space_check: false,
        image_metadata_edit: None,
        attribute_change: None,
        split_file: None,
        geo_transform: None,
        symlink_policy: SymlinkPolicyDto::default(),
        permanent_delete_confirmed: false,
        override_read_only: false,
        link: None,
        verify_transfer: false,
    })
}

fn to_dto(assessed: &Assessed) -> InterruptedTransferDto {
    let intent = &assessed.job.intent;
    InterruptedTransferDto {
        operation_id: intent.operation_id.to_string(),
        kind: match intent.kind {
            OperationKind::Move => OperationKindDto::Move,
            OperationKind::Duplicate => OperationKindDto::Duplicate,
            _ => OperationKindDto::Copy,
        },
        created_at: intent.created_at,
        class: assessed.verdict.class,
        reason: assessed.verdict.reason,
        sources: intent.sources.iter().cloned().map(Into::into).collect(),
        destination: intent.destination.clone().map(Into::into),
        total_items: assessed.verdict.total,
        committed_items: assessed.verdict.committed,
        unconfirmed_items: assessed.verdict.unconfirmed,
        partial_items: assessed.verdict.partials,
        remaining_items: assessed.verdict.remaining,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fm_operations::journal::{JournalIntent, RecoveredItem};

    use TransferRecoveryReasonDto as Reason;

    fn facts() -> ItemFacts {
        ItemFacts {
            committed: false,
            announced: true,
            destination_recorded: true,
            destination_fresh: true,
            source: SourceState::Unchanged,
            source_size: Some(10),
            destination: Presence::Absent,
            partial: Presence::Present(Some(4)),
            resume_proven: false,
        }
    }

    #[test]
    fn a_confirmed_item_is_done_whatever_else_changed() {
        let verdict = classify_item(&ItemFacts {
            committed: true,
            source: SourceState::Missing,
            ..facts()
        });
        assert_eq!(verdict, ItemVerdict::Done);
    }

    #[test]
    fn an_unfinished_partial_is_restarted_never_resumed_without_proof() {
        assert_eq!(
            classify_item(&facts()),
            ItemVerdict::Restartable(Reason::PartialUnfinished)
        );
    }

    #[test]
    fn a_proven_partial_with_an_unchanged_source_is_resumable() {
        let proven = ItemFacts {
            resume_proven: true,
            ..facts()
        };
        assert_eq!(classify_item(&proven), ItemVerdict::Resumable);
        let changed = ItemFacts {
            source: SourceState::Changed,
            ..proven
        };
        assert_eq!(
            classify_item(&changed),
            ItemVerdict::Intervention(Reason::SourceChanged),
            "a stale source is never resumable"
        );
    }

    #[test]
    fn a_published_destination_with_no_partial_is_unconfirmed_when_sizes_agree() {
        let published = ItemFacts {
            destination: Presence::Present(Some(10)),
            partial: Presence::Absent,
            ..facts()
        };
        assert_eq!(classify_item(&published), ItemVerdict::Unconfirmed);
        let moved = ItemFacts {
            source: SourceState::Missing,
            ..published
        };
        assert_eq!(
            classify_item(&moved),
            ItemVerdict::Unconfirmed,
            "a finished move has deleted its source"
        );
    }

    #[test]
    fn a_destination_of_the_wrong_size_was_replaced() {
        let replaced = ItemFacts {
            destination: Presence::Present(Some(99)),
            partial: Presence::Absent,
            ..facts()
        };
        assert_eq!(
            classify_item(&replaced),
            ItemVerdict::Intervention(Reason::DestinationReplaced)
        );
    }

    #[test]
    fn a_destination_next_to_a_live_partial_is_left_to_the_conflict_policy() {
        let facts = ItemFacts {
            destination: Presence::Present(Some(3)),
            partial: Presence::Present(Some(1)),
            ..facts()
        };
        assert_eq!(
            classify_item(&facts),
            ItemVerdict::Restartable(Reason::PartialUnfinished)
        );
    }

    #[test]
    fn a_missing_or_changed_source_needs_a_person() {
        for (state, reason) in [
            (SourceState::Missing, Reason::SourceMissing),
            (SourceState::Changed, Reason::SourceChanged),
        ] {
            let facts = ItemFacts {
                source: state,
                ..facts()
            };
            assert_eq!(classify_item(&facts), ItemVerdict::Intervention(reason));
        }
    }

    #[test]
    fn an_unreachable_provider_is_never_guessed_at() {
        for broken in [
            ItemFacts {
                source: SourceState::Unavailable,
                ..facts()
            },
            ItemFacts {
                destination: Presence::Unavailable,
                ..facts()
            },
            ItemFacts {
                partial: Presence::Unavailable,
                ..facts()
            },
        ] {
            assert_eq!(
                classify_item(&broken),
                ItemVerdict::Intervention(Reason::ProviderUnavailable)
            );
        }
    }

    #[test]
    fn an_item_that_never_started_is_restartable_unless_its_source_is_gone() {
        let unstarted = ItemFacts {
            announced: false,
            partial: Presence::Absent,
            ..facts()
        };
        assert_eq!(
            classify_item(&unstarted),
            ItemVerdict::Restartable(Reason::NotStarted)
        );
        let gone = ItemFacts {
            source: SourceState::Missing,
            ..unstarted
        };
        assert_eq!(
            classify_item(&gone),
            ItemVerdict::Intervention(Reason::SourceMissing)
        );
    }

    fn rolled(verdicts: &[ItemVerdict], damage: Option<&JournalDamage>) -> JobVerdict {
        let pairs: Vec<(ItemVerdict, Presence)> = verdicts
            .iter()
            .map(|verdict| (*verdict, Presence::Absent))
            .collect();
        classify_job(&pairs, damage)
    }

    #[test]
    fn one_item_needing_a_person_decides_the_whole_job_even_among_interleaved_verdicts() {
        let verdict = rolled(
            &[
                ItemVerdict::Done,
                ItemVerdict::Restartable(Reason::NotStarted),
                ItemVerdict::Unconfirmed,
                ItemVerdict::Intervention(Reason::SourceChanged),
                ItemVerdict::Resumable,
            ],
            None,
        );
        assert_eq!(verdict.class, TransferRecoveryClassDto::NeedsIntervention);
        assert_eq!(verdict.reason, Reason::SourceChanged);
        assert_eq!(
            (
                verdict.total,
                verdict.committed,
                verdict.unconfirmed,
                verdict.remaining
            ),
            (5, 1, 1, 3)
        );
    }

    #[test]
    fn restartable_beats_resumable_and_unconfirmed() {
        let verdict = rolled(
            &[
                ItemVerdict::Resumable,
                ItemVerdict::Unconfirmed,
                ItemVerdict::Restartable(Reason::PartialUnfinished),
            ],
            None,
        );
        assert_eq!(verdict.class, TransferRecoveryClassDto::SafelyRestartable);
        assert_eq!(verdict.reason, Reason::PartialUnfinished);
    }

    #[test]
    fn a_job_of_only_resumable_and_done_items_is_resumable() {
        let verdict = rolled(&[ItemVerdict::Done, ItemVerdict::Resumable], None);
        assert_eq!(verdict.class, TransferRecoveryClassDto::Resumable);
    }

    #[test]
    fn a_job_whose_items_are_all_published_or_done_is_completed_unconfirmed() {
        let verdict = rolled(&[ItemVerdict::Done, ItemVerdict::Unconfirmed], None);
        assert_eq!(
            verdict.class,
            TransferRecoveryClassDto::CompletedUnconfirmed
        );
        assert_eq!(verdict.reason, Reason::PublishedUnconfirmed);
    }

    #[test]
    fn a_job_that_stopped_before_planning_is_restartable() {
        let verdict = rolled(&[], None);
        assert_eq!(verdict.class, TransferRecoveryClassDto::SafelyRestartable);
        assert_eq!(verdict.reason, Reason::NotStarted);
    }

    #[test]
    fn a_corrupt_journal_needs_a_person_but_a_torn_tail_does_not() {
        let done = [ItemVerdict::Done];
        assert_eq!(
            rolled(&done, Some(&JournalDamage::CorruptRecord { line: 4 })).class,
            TransferRecoveryClassDto::NeedsIntervention
        );
        assert_eq!(
            rolled(&done, Some(&JournalDamage::TornTail)).class,
            TransferRecoveryClassDto::CompletedUnconfirmed
        );
    }

    // ---- coordinator, against real local files ----

    fn registry() -> ProviderRegistry {
        let mut providers = ProviderRegistry::new();
        providers.register(Arc::new(fm_vfs_local::LocalFileSystemProvider::new()));
        providers
    }

    fn local(path: &std::path::Path) -> Location {
        Location::from_native_path(path).expect("location")
    }

    fn job(
        root: &std::path::Path,
        kind: OperationKind,
        policy: ConflictPolicy,
        items: Vec<RecoveredItem>,
    ) -> RecoveredJob {
        RecoveredJob {
            intent: JournalIntent {
                operation_id: OperationId::new(),
                kind,
                sources: items.iter().map(|item| item.source.clone()).collect(),
                destination: Some(local(&root.join("dest"))),
                conflict_policy: policy,
                created_at: chrono::Utc::now(),
            },
            items,
            damage: None,
        }
    }

    fn item(
        source: &std::path::Path,
        partial: &std::path::Path,
        dest: &std::path::Path,
    ) -> RecoveredItem {
        RecoveredItem {
            source: local(source),
            bytes: 5,
            partial: Some(local(partial)),
            destination: Some(local(dest)),
            revision: None,
            committed: false,
            committed_bytes: 0,
        }
    }

    struct Fixture {
        root: tempfile::TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            let root = tempfile::tempdir().expect("root");
            std::fs::create_dir(root.path().join("dest")).expect("dest");
            Self { root }
        }
        fn path(&self, name: &str) -> std::path::PathBuf {
            self.root.path().join(name)
        }
        fn write(&self, name: &str, content: &[u8]) -> std::path::PathBuf {
            let path = self.path(name);
            std::fs::write(&path, content).expect("write");
            path
        }
    }

    #[tokio::test]
    async fn listing_classifies_a_real_unfinished_copy_as_safely_restartable() {
        let fixture = Fixture::new();
        let source = fixture.write("a.txt", b"hello");
        let partial = fixture.write("dest/.fm-copy-1", b"he");
        let job = job(
            fixture.root.path(),
            OperationKind::Copy,
            ConflictPolicy::Ask,
            vec![item(&source, &partial, &fixture.path("dest/a.txt"))],
        );
        let coordinator = TransferRecoveryCoordinator::new(registry(), None, vec![job]);
        let listed = coordinator.list().await;
        assert_eq!(listed.transfers.len(), 1);
        let transfer = &listed.transfers[0];
        assert_eq!(transfer.class, TransferRecoveryClassDto::SafelyRestartable);
        assert_eq!(transfer.reason, Reason::PartialUnfinished);
        assert_eq!(transfer.partial_items, 1);
        assert_eq!(transfer.remaining_items, 1);
    }

    #[tokio::test]
    async fn a_published_but_unrecorded_copy_is_completed_unconfirmed() {
        let fixture = Fixture::new();
        let source = fixture.write("a.txt", b"hello");
        fixture.write("dest/a.txt", b"hello");
        let job = job(
            fixture.root.path(),
            OperationKind::Copy,
            ConflictPolicy::Ask,
            vec![item(
                &source,
                &fixture.path("dest/.fm-copy-1"),
                &fixture.path("dest/a.txt"),
            )],
        );
        let coordinator = TransferRecoveryCoordinator::new(registry(), None, vec![job]);
        let transfer = coordinator.list().await.transfers.remove(0);
        assert_eq!(
            transfer.class,
            TransferRecoveryClassDto::CompletedUnconfirmed
        );
        assert_eq!(transfer.unconfirmed_items, 1);
    }

    #[tokio::test]
    async fn a_replaced_destination_and_a_vanished_source_need_intervention() {
        let fixture = Fixture::new();
        let replaced = fixture.write("a.txt", b"hello");
        fixture.write("dest/a.txt", b"something else entirely");
        let gone = fixture.path("gone.txt");
        let job = job(
            fixture.root.path(),
            OperationKind::Copy,
            ConflictPolicy::Ask,
            vec![item(
                &replaced,
                &fixture.path("dest/.fm-copy-1"),
                &fixture.path("dest/a.txt"),
            )],
        );
        let missing = job_with_missing_source(&fixture, &gone);
        let coordinator = TransferRecoveryCoordinator::new(registry(), None, vec![job, missing]);
        let listed = coordinator.list().await.transfers;
        let reasons: Vec<_> = listed.iter().map(|transfer| transfer.reason).collect();
        assert!(reasons.contains(&Reason::DestinationReplaced));
        assert!(reasons.contains(&Reason::SourceMissing));
        assert!(
            listed
                .iter()
                .all(|transfer| transfer.class == TransferRecoveryClassDto::NeedsIntervention)
        );
    }

    fn job_with_missing_source(fixture: &Fixture, gone: &std::path::Path) -> RecoveredJob {
        job(
            fixture.root.path(),
            OperationKind::Copy,
            ConflictPolicy::Ask,
            vec![item(
                gone,
                &fixture.path("dest/.fm-copy-2"),
                &fixture.path("dest/gone.txt"),
            )],
        )
    }

    #[tokio::test]
    async fn discard_removes_the_private_partial_keeps_published_files_and_drops_the_record() {
        let fixture = Fixture::new();
        let source = fixture.write("a.txt", b"hello");
        let partial = fixture.write("dest/.fm-copy-1", b"he");
        let published = fixture.write("dest/b.txt", b"keep me");
        let job = job(
            fixture.root.path(),
            OperationKind::Copy,
            ConflictPolicy::Ask,
            vec![item(&source, &partial, &fixture.path("dest/a.txt"))],
        );
        let id = job.intent.operation_id;
        let coordinator = TransferRecoveryCoordinator::new(registry(), None, vec![job]);
        coordinator.discard(id).await.expect("discard");
        assert!(!partial.exists(), "the private partial is removed");
        assert!(published.exists(), "other files are untouched");
        assert!(source.exists(), "the source is untouched");
        assert!(coordinator.list().await.transfers.is_empty());
        assert!(matches!(
            coordinator.discard(id).await,
            Err(ApplicationError::NotFound)
        ));
    }

    #[tokio::test]
    async fn restart_builds_the_original_request_and_only_clears_the_partial_once_it_started() {
        let fixture = Fixture::new();
        let source = fixture.write("a.txt", b"hello");
        let partial = fixture.write("dest/.fm-copy-1", b"he");
        let job = job(
            fixture.root.path(),
            OperationKind::Move,
            ConflictPolicy::Overwrite,
            vec![item(&source, &partial, &fixture.path("dest/a.txt"))],
        );
        let id = job.intent.operation_id;
        let coordinator = TransferRecoveryCoordinator::new(registry(), None, vec![job]);
        let seen = Mutex::new(None);
        let started = coordinator
            .restart(id, |request| {
                *seen.lock().unwrap() = Some(request.clone());
                let operation = fm_operations::Operation::new(
                    OperationKind::Move,
                    Vec::new(),
                    None,
                    ConflictPolicy::Overwrite,
                );
                Ok(crate::operation_history::operation_dto(operation, None))
            })
            .await
            .expect("restart");
        let request = seen.lock().unwrap().take().expect("request");
        assert_eq!(request.operation_type, OperationKindDto::Move);
        assert_eq!(
            request.conflict_policy,
            OperationConflictPolicyDto::Overwrite
        );
        assert_eq!(request.sources.len(), 1);
        assert_eq!(started.operation_type, OperationKindDto::Move);
        assert!(
            !partial.exists(),
            "the partial is cleared after the new run started"
        );
        assert!(
            coordinator.list().await.transfers.is_empty(),
            "the record is dropped once the restart is running"
        );
        assert!(source.exists(), "restart never touches the source itself");
    }

    #[tokio::test]
    async fn restart_leaves_out_finished_sources_and_keeps_the_original_policy() {
        let fixture = Fixture::new();
        let done = fixture.write("done.txt", b"hello");
        fixture.write("dest/done.txt", b"hello");
        let pending = fixture.write("pending.txt", b"world");
        let mut finished = item(
            &done,
            &fixture.path("dest/.fm-copy-1"),
            &fixture.path("dest/done.txt"),
        );
        finished.committed = true;
        let todo = RecoveredItem {
            partial: None,
            destination: None,
            ..item(&pending, &fixture.path("x"), &fixture.path("y"))
        };
        let job = job(
            fixture.root.path(),
            OperationKind::Copy,
            ConflictPolicy::RenameNew,
            vec![finished, todo],
        );
        let id = job.intent.operation_id;
        let coordinator = TransferRecoveryCoordinator::new(registry(), None, vec![job]);
        let seen = Mutex::new(None);
        let _ = coordinator
            .restart(id, |request| {
                *seen.lock().unwrap() = Some(request);
                Err(ApplicationError::InvalidRequest("stop".into()))
            })
            .await;
        let request = seen.lock().unwrap().take().expect("request");
        assert_eq!(request.sources.len(), 1, "the finished source is left out");
        assert!(request.sources[0].uri.ends_with("pending.txt"));
        assert_eq!(
            request.conflict_policy,
            OperationConflictPolicyDto::RenameNew,
            "no skip is forced when finished sources are simply excluded"
        );
    }

    #[tokio::test]
    async fn restart_skips_conflicts_when_a_child_of_a_directory_source_already_finished() {
        let fixture = Fixture::new();
        let tree = fixture.path("tree");
        std::fs::create_dir(&tree).expect("tree");
        let child = fixture.write("tree/a.txt", b"hello");
        let other = fixture.write("tree/b.txt", b"world");
        fixture.write("dest/a.txt", b"hello");
        let mut finished = item(
            &child,
            &fixture.path("dest/.fm-copy-1"),
            &fixture.path("dest/a.txt"),
        );
        finished.committed = true;
        let todo = RecoveredItem {
            partial: None,
            destination: None,
            ..item(&other, &fixture.path("x"), &fixture.path("y"))
        };
        let mut job = job(
            fixture.root.path(),
            OperationKind::Copy,
            ConflictPolicy::Ask,
            vec![finished, todo],
        );
        job.intent.sources = vec![local(&tree)];
        let id = job.intent.operation_id;
        let coordinator = TransferRecoveryCoordinator::new(registry(), None, vec![job]);
        let seen = Mutex::new(None);
        let _ = coordinator
            .restart(id, |request| {
                *seen.lock().unwrap() = Some(request);
                Err(ApplicationError::InvalidRequest("stop".into()))
            })
            .await;
        let request = seen.lock().unwrap().take().expect("request");
        assert_eq!(
            request.sources.len(),
            1,
            "the directory root cannot be left out"
        );
        assert_eq!(request.conflict_policy, OperationConflictPolicyDto::Skip);
    }

    #[tokio::test]
    async fn restart_with_nothing_left_to_run_is_refused_and_keeps_the_record() {
        let fixture = Fixture::new();
        let done = fixture.write("done.txt", b"hello");
        let mut finished = item(
            &done,
            &fixture.path("dest/.fm-copy-1"),
            &fixture.path("dest/done.txt"),
        );
        finished.committed = true;
        let job = job(
            fixture.root.path(),
            OperationKind::Copy,
            ConflictPolicy::Ask,
            vec![finished],
        );
        let id = job.intent.operation_id;
        let coordinator = TransferRecoveryCoordinator::new(registry(), None, vec![job]);
        let result = coordinator
            .restart(id, |_| unreachable!("nothing to start"))
            .await;
        assert!(matches!(result, Err(ApplicationError::InvalidRequest(_))));
        assert_eq!(coordinator.list().await.transfers.len(), 1);
    }

    #[tokio::test]
    async fn a_failed_start_keeps_the_partial_so_nothing_is_lost() {
        let fixture = Fixture::new();
        let source = fixture.write("a.txt", b"hello");
        let partial = fixture.write("dest/.fm-copy-1", b"he");
        let job = job(
            fixture.root.path(),
            OperationKind::Copy,
            ConflictPolicy::Ask,
            vec![item(&source, &partial, &fixture.path("dest/a.txt"))],
        );
        let id = job.intent.operation_id;
        let coordinator = TransferRecoveryCoordinator::new(registry(), None, vec![job]);
        let result = coordinator
            .restart(id, |_| Err(ApplicationError::InvalidRequest("busy".into())))
            .await;
        assert!(result.is_err());
        assert!(partial.exists(), "the partial survives a start that failed");
        assert_eq!(coordinator.list().await.transfers.len(), 1);
    }

    #[tokio::test]
    async fn a_job_cannot_be_claimed_twice_at_once() {
        let fixture = Fixture::new();
        let job = job(
            fixture.root.path(),
            OperationKind::Copy,
            ConflictPolicy::Ask,
            Vec::new(),
        );
        let id = job.intent.operation_id;
        let coordinator = TransferRecoveryCoordinator::new(registry(), None, vec![job]);
        let first = coordinator.claim(id).expect("first claim");
        assert!(matches!(
            coordinator.claim(id),
            Err(ApplicationError::InvalidRequest(_))
        ));
        assert!(matches!(
            coordinator.discard(id).await,
            Err(ApplicationError::InvalidRequest(_))
        ));
        drop(first);
        assert!(
            coordinator.claim(id).is_ok(),
            "released when the first holder is done"
        );
    }

    #[tokio::test]
    async fn discard_refuses_while_a_recorded_partial_cannot_be_reached() {
        let fixture = Fixture::new();
        let source = fixture.write("a.txt", b"hello");
        // No provider is registered for this scheme, so the partial cannot be probed.
        let unreachable = Location::new(
            fm_domain::ProviderId::new("sftp"),
            "sftp://offline/dest/.fm-copy-9",
        );
        let mut announced = item(
            &source,
            &fixture.path("unused"),
            &fixture.path("dest/a.txt"),
        );
        announced.partial = Some(unreachable);
        let job = job(
            fixture.root.path(),
            OperationKind::Copy,
            ConflictPolicy::Ask,
            vec![announced],
        );
        let id = job.intent.operation_id;
        let coordinator = TransferRecoveryCoordinator::new(registry(), None, vec![job]);
        assert!(matches!(
            coordinator.discard(id).await,
            Err(ApplicationError::ProviderUnavailable)
        ));
        assert_eq!(
            coordinator.list().await.transfers.len(),
            1,
            "the record is kept so the partial can still be cleaned up later"
        );
    }

    #[tokio::test]
    async fn the_intent_is_readable_without_probing_any_provider() {
        let fixture = Fixture::new();
        let source = fixture.write("a.txt", b"hello");
        let job = job(
            fixture.root.path(),
            OperationKind::Copy,
            ConflictPolicy::Ask,
            vec![item(&source, &fixture.path("p"), &fixture.path("d"))],
        );
        let id = job.intent.operation_id;
        let coordinator = TransferRecoveryCoordinator::new(registry(), None, vec![job]);
        let (sources, destination) = coordinator.intent_locations(id).expect("known job");
        assert_eq!(sources, vec![local(&source)]);
        assert!(destination.is_some());
        assert!(coordinator.intent_locations(OperationId::new()).is_none());
    }

    #[test]
    fn a_source_with_the_same_size_but_a_new_modification_time_has_changed() {
        let revision = SourceRevision {
            size: Some(10),
            modified_at_ms: Some(1_000),
            validator: None,
        };
        let (state, _) = source_state(Some(&revision), (Presence::Present(Some(10)), Some(2_000)));
        assert_eq!(state, SourceState::Changed);
        let (same, _) = source_state(Some(&revision), (Presence::Present(Some(10)), Some(1_000)));
        assert_eq!(same, SourceState::Unchanged);
        let (unknown_time, _) = source_state(Some(&revision), (Presence::Present(Some(10)), None));
        assert_eq!(
            unknown_time,
            SourceState::Unchanged,
            "no timestamp is no evidence of change"
        );
    }

    #[test]
    fn a_partial_with_no_recorded_destination_is_not_guessed_at() {
        let legacy = ItemFacts {
            destination_recorded: false,
            destination: Presence::Absent,
            partial: Presence::Absent,
            ..facts()
        };
        assert_eq!(
            classify_item(&legacy),
            ItemVerdict::Intervention(Reason::JournalDamaged)
        );
    }

    #[test]
    fn a_destination_older_than_the_job_is_not_called_finished() {
        let stale = ItemFacts {
            destination: Presence::Present(Some(10)),
            partial: Presence::Absent,
            destination_fresh: false,
            ..facts()
        };
        assert_eq!(
            classify_item(&stale),
            ItemVerdict::Restartable(Reason::PartialUnfinished),
            "a pre-existing file is left to the conflict policy, never confirmed"
        );
    }

    #[tokio::test]
    async fn only_transfers_can_be_restarted() {
        let fixture = Fixture::new();
        let job = job(
            fixture.root.path(),
            OperationKind::Delete,
            ConflictPolicy::Ask,
            Vec::new(),
        );
        let id = job.intent.operation_id;
        let coordinator = TransferRecoveryCoordinator::new(registry(), None, vec![job]);
        let result = coordinator
            .restart(id, |_| unreachable!("a delete is never started"))
            .await;
        assert!(matches!(result, Err(ApplicationError::InvalidRequest(_))));
    }
}
