//! Crash-consistent transfer journal (task 0299).
//!
//! The journal records the *intent* and *checkpoints* of queued and running
//! transfer jobs so a later process can tell what an interrupted job was doing.
//! It stores locations, source revisions and byte counters only; never live
//! provider sessions or credentials.
//!
//! # Format
//!
//! One append-only file per job, `<operation-id>.journal`, one record per line:
//! `<crc32 hex> <json>\n`. The first record is always `begin`. Because a record
//! is only trusted when its checksum matches, a crash that tears the final
//! write leaves a valid prefix: replay keeps everything before the first bad
//! record and reports the damage. A file whose `begin` record is unreadable, of
//! an unknown version, or that belongs to a different job is moved to
//! `quarantine/` and never blocks startup.
//!
//! # Durability
//!
//! * [`OperationJournal::begin`] resolves only after the intent is `fsync`ed
//!   (file and directory), so a job that started executing is always
//!   recoverable.
//! * [`OperationJournal::record`] never blocks. A dedicated writer thread
//!   group-commits queued records, flushing at most [`SYNC_INTERVAL`] after the
//!   oldest unsynced one, so per-file copies neither wait on nor compete with
//!   a synchronous flush each. A crash can lose up to that interval of
//!   records; recovery must therefore treat "destination exists but no
//!   commit record" as *completed-unconfirmed*, never as safe to overwrite.
//! * [`OperationJournal::finish`] removes the job's file once the job is
//!   terminal.
//!
//! The journal does not decide what to do with a recovered job; classification
//! is task 0300.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use fm_domain::{Location, OperationId};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::oneshot;

use crate::{ConflictPolicy, Operation, OperationKind};

/// Version of the record schema; bumped for incompatible changes.
pub const JOURNAL_VERSION: u32 = 1;

const FILE_EXTENSION: &str = "journal";
const QUARANTINE_DIRECTORY: &str = "quarantine";
const LOCK_FILE: &str = ".lock";

/// Journal failure.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum JournalError {
    /// The journal directory or a journal file could not be read or written.
    #[error("transfer journal I/O failed: {0}")]
    Io(String),
    /// The writer thread is gone, so nothing further can be recorded.
    #[error("transfer journal writer is not running")]
    Closed,
}

impl From<std::io::Error> for JournalError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

/// What a source looked like when the job planned it. Resume may only be
/// attempted while the source still matches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRevision {
    /// Size in bytes, when the provider reported one.
    pub size: Option<u64>,
    /// Modification time in milliseconds since the Unix epoch.
    pub modified_at_ms: Option<i64>,
    /// Provider validator such as an `ETag` or version id.
    pub validator: Option<String>,
}

/// Why a job exists, recorded when it is accepted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalIntent {
    /// Job identifier.
    pub operation_id: OperationId,
    /// Semantic mutation requested.
    pub kind: OperationKind,
    /// Requested source locations.
    pub sources: Vec<Location>,
    /// Requested destination, if any.
    pub destination: Option<Location>,
    /// Conflict behaviour selected by the caller.
    pub conflict_policy: ConflictPolicy,
    /// Time the job was accepted.
    pub created_at: DateTime<Utc>,
}

impl JournalIntent {
    /// Captures the intent of an accepted operation.
    #[must_use]
    pub fn of(operation: &Operation) -> Self {
        Self {
            operation_id: operation.id,
            kind: operation.kind,
            sources: operation
                .sources
                .iter()
                .map(|entry| entry.location.clone())
                .collect(),
            destination: operation.destination.clone(),
            conflict_policy: operation.conflict_policy,
            created_at: operation.created_at,
        }
    }
}

/// One planned unit of work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalItem {
    /// Source entry the item processes; unique within one job.
    pub source: Location,
    /// Payload size contributing to totals.
    pub bytes: u64,
}

/// A checkpoint appended after a job began.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum JournalEvent {
    /// Work units became known (after planning or directory expansion).
    Items {
        /// The new units.
        items: Vec<JournalItem>,
    },
    /// A private partial destination now exists for `source`.
    Partial {
        /// Source being transferred.
        source: Location,
        /// Private, not-yet-published destination.
        partial: Location,
        /// Where the finished copy will be published; recovery looks here to tell a
        /// published-but-unconfirmed item from an unfinished one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        destination: Option<Location>,
    },
    /// The source revision observed before transferring `source`.
    SourceRevision {
        /// Source being transferred.
        source: Location,
        /// What it looked like.
        revision: SourceRevision,
    },
    /// `source` was fully transferred and its destination published.
    Committed {
        /// Source that finished.
        source: Location,
        /// Bytes transferred.
        bytes: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Record {
    Begin {
        version: u32,
        intent: JournalIntent,
    },
    #[serde(untagged)]
    Event(JournalEvent),
}

/// Sink for transfer journal records, injected into the scheduler.
#[async_trait]
pub trait OperationJournal: Send + Sync + 'static {
    /// Durably records the intent of a job. Resolves once it is on disk.
    async fn begin(&self, intent: JournalIntent) -> Result<(), JournalError>;

    /// Queues a checkpoint without blocking.
    fn record(&self, operation: OperationId, event: JournalEvent);

    /// Removes a terminal job's record. Resolves once it is gone.
    async fn finish(&self, operation: OperationId);

    /// Queues the removal of a terminal job's record without waiting for it.
    ///
    /// The scheduler uses this at the end of every job: awaiting the removal would keep the
    /// scheduler (and with it the history writer) alive after the service is dropped, so a quick
    /// restart could read a history file that is missing the job's terminal snapshot. If the
    /// process stops before the writer gets to it, the record stays and is treated as an
    /// interrupted job on the next start, which is the conservative reading.
    fn discard(&self, operation: OperationId);
}

/// Damage found while replaying one journal file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JournalDamage {
    /// The final record was cut off, as a crash during append leaves it.
    TornTail,
    /// A record in the middle failed its checksum or did not parse; everything
    /// after it was discarded.
    CorruptRecord {
        /// 1-based line number of the first bad record.
        line: usize,
    },
}

/// Replayed state of one planned unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveredItem {
    /// Source entry.
    pub source: Location,
    /// Payload size planned.
    pub bytes: u64,
    /// Last private partial destination announced for this item.
    pub partial: Option<Location>,
    /// Where the item's finished copy is published, when announced.
    pub destination: Option<Location>,
    /// Source revision observed before transferring.
    pub revision: Option<SourceRevision>,
    /// Whether the item's commit record survived.
    pub committed: bool,
    /// Bytes recorded by the commit.
    pub committed_bytes: u64,
}

/// One job found in the journal directory at startup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveredJob {
    /// What the job was asked to do.
    pub intent: JournalIntent,
    /// Replayed work units, in the order they were planned.
    pub items: Vec<RecoveredItem>,
    /// Damage to the tail of the journal, if any.
    pub damage: Option<JournalDamage>,
}

/// A journal file that could not be trusted and was set aside.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuarantinedJournal {
    /// Where the file now lives.
    pub path: PathBuf,
    /// Why it was set aside.
    pub reason: String,
}

/// Everything found in the journal directory when it was opened.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recovery {
    /// Jobs that were queued or running when the previous process stopped.
    pub jobs: Vec<RecoveredJob>,
    /// Files that were unreadable and moved aside.
    pub quarantined: Vec<QuarantinedJournal>,
}

/// A pending `begin` acknowledgement and the outcome to report.
type BeginAck = (
    oneshot::Sender<Result<(), JournalError>>,
    Result<(), JournalError>,
);

enum Command {
    Begin {
        operation: OperationId,
        line: String,
        ack: oneshot::Sender<Result<(), JournalError>>,
    },
    Append {
        operation: OperationId,
        line: String,
    },
    Finish {
        operation: OperationId,
        ack: Option<oneshot::Sender<()>>,
    },
    Flush {
        ack: oneshot::Sender<()>,
    },
}

/// File-backed [`OperationJournal`] with a group-committing writer thread.
pub struct FileOperationJournal {
    sender: Mutex<Option<Sender<Command>>>,
    thread: Mutex<Option<JoinHandle<()>>>,
    /// Held for the journal's lifetime so a second process or service on the same
    /// settings directory cannot mistake this one's live jobs for interrupted ones.
    _lock: File,
}

impl FileOperationJournal {
    /// Opens (creating if needed) the journal directory, replays what a previous
    /// process left behind, and starts the writer.
    ///
    /// Recovered jobs keep their files until [`OperationJournal::finish`] is
    /// called for them, so a restart during recovery loses nothing.
    pub fn open(directory: impl Into<PathBuf>) -> Result<(Self, Recovery), JournalError> {
        let directory = directory.into();
        create_private_directory(&directory)?;
        let lock = File::create(directory.join(LOCK_FILE))?;
        lock.try_lock().map_err(|error| {
            JournalError::Io(format!(
                "journal directory is in use by another instance: {error}"
            ))
        })?;
        let recovery = recover_directory(&directory);
        let (sender, receiver) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("transfer-journal-writer".into())
            .spawn(move || run_writer(&directory, &receiver))
            .map_err(JournalError::from)?;
        Ok((
            Self {
                sender: Mutex::new(Some(sender)),
                thread: Mutex::new(Some(thread)),
                _lock: lock,
            },
            recovery,
        ))
    }

    fn send(&self, command: Command) -> Result<(), JournalError> {
        let guard = self.sender.lock().unwrap_or_else(|e| e.into_inner());
        guard
            .as_ref()
            .ok_or(JournalError::Closed)?
            .send(command)
            .map_err(|_| JournalError::Closed)
    }

    /// Resolves once every record queued so far is on disk.
    pub async fn flush(&self) {
        let (ack, done) = oneshot::channel();
        if self.send(Command::Flush { ack }).is_ok() {
            let _ = done.await;
        }
    }
}

#[async_trait]
impl OperationJournal for FileOperationJournal {
    async fn begin(&self, intent: JournalIntent) -> Result<(), JournalError> {
        let operation = intent.operation_id;
        let line = encode(&Record::Begin {
            version: JOURNAL_VERSION,
            intent,
        })?;
        let (ack, done) = oneshot::channel();
        self.send(Command::Begin {
            operation,
            line,
            ack,
        })?;
        done.await.map_err(|_| JournalError::Closed)?
    }

    fn record(&self, operation: OperationId, event: JournalEvent) {
        match encode(&Record::Event(event)) {
            Ok(line) => {
                if let Err(error) = self.send(Command::Append { operation, line }) {
                    tracing::warn!(%operation, %error, "transfer journal record dropped");
                }
            }
            Err(error) => tracing::warn!(%operation, %error, "transfer journal record not encoded"),
        }
    }

    async fn finish(&self, operation: OperationId) {
        let (ack, done) = oneshot::channel();
        if self
            .send(Command::Finish {
                operation,
                ack: Some(ack),
            })
            .is_ok()
        {
            let _ = done.await;
        }
    }

    fn discard(&self, operation: OperationId) {
        if let Err(error) = self.send(Command::Finish {
            operation,
            ack: None,
        }) {
            tracing::warn!(%operation, %error, "transfer journal record could not be removed");
        }
    }
}

impl Drop for FileOperationJournal {
    fn drop(&mut self) {
        // Dropping the sender lets the writer drain its queue and exit.
        self.sender.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(thread) = self.thread.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = thread.join();
        }
    }
}

fn encode(record: &Record) -> Result<String, JournalError> {
    let json =
        serde_json::to_string(record).map_err(|error| JournalError::Io(error.to_string()))?;
    Ok(format!("{:08x} {json}\n", crc32fast::hash(json.as_bytes())))
}

/// Parses one line, returning `None` when the checksum or JSON is bad.
fn decode(line: &str) -> Option<Record> {
    let (crc, json) = line.split_once(' ')?;
    let expected = u32::from_str_radix(crc, 16).ok()?;
    if crc32fast::hash(json.as_bytes()) != expected {
        return None;
    }
    serde_json::from_str(json).ok()
}

/// Creates the journal directory readable by the owner only: its files name
/// source and destination locations.
fn create_private_directory(directory: &Path) -> Result<(), JournalError> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(directory)?;
    Ok(())
}

fn journal_path(directory: &Path, operation: OperationId) -> PathBuf {
    directory.join(format!("{operation}.{FILE_EXTENSION}"))
}

fn sync_directory(directory: &Path) {
    // Directory fsync is how a created/removed entry becomes durable on Unix;
    // other platforms either do not need it or refuse to open a directory.
    if let Ok(handle) = File::open(directory) {
        let _ = handle.sync_all();
    }
}

/// Appended records are flushed at most this long after they were queued.
///
/// One `fsync` per record would put the journal in direct competition with the
/// copy's own durable writes (a 10k-file copy took four times longer), so
/// appends are group-committed. This bounds how much a crash can lose.
const SYNC_INTERVAL: Duration = Duration::from_millis(250);

fn sync_dirty(files: &HashMap<OperationId, File>, dirty: &mut HashSet<OperationId>) {
    for operation in dirty.drain() {
        if let Some(file) = files.get(&operation) {
            let _ = file.sync_data();
        }
    }
}

fn run_writer(directory: &Path, receiver: &Receiver<Command>) {
    let mut files: HashMap<OperationId, File> = HashMap::new();
    let mut dirty: HashSet<OperationId> = HashSet::new();
    // When the oldest unsynced append must reach disk.
    let mut due: Option<Instant> = None;
    loop {
        let first = match due {
            None => match receiver.recv() {
                Ok(command) => command,
                Err(_) => break,
            },
            Some(at) => match receiver.recv_timeout(at.saturating_duration_since(Instant::now())) {
                Ok(command) => command,
                Err(RecvTimeoutError::Timeout) => {
                    sync_dirty(&files, &mut dirty);
                    due = None;
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => break,
            },
        };
        let mut batch = vec![first];
        while let Ok(next) = receiver.try_recv() {
            batch.push(next);
        }
        let mut flush_acks: Vec<oneshot::Sender<()>> = Vec::new();
        let mut finish_acks: Vec<oneshot::Sender<()>> = Vec::new();
        let mut begin_acks: Vec<BeginAck> = Vec::new();
        for command in batch {
            match command {
                Command::Begin {
                    operation,
                    line,
                    ack,
                } => {
                    let result = begin_file(directory, operation, &line, &mut files);
                    begin_acks.push((ack, result));
                }
                Command::Append { operation, line } => {
                    if let Some(file) = files.get_mut(&operation) {
                        if file.write_all(line.as_bytes()).is_ok() {
                            dirty.insert(operation);
                        } else {
                            // A job whose journal cannot be written stops being
                            // journaled; the file keeps its valid prefix.
                            tracing::warn!(
                                %operation,
                                "transfer journal write failed; the job is no longer recoverable past this point"
                            );
                            files.remove(&operation);
                            dirty.remove(&operation);
                        }
                    }
                }
                Command::Finish { operation, ack } => {
                    files.remove(&operation);
                    dirty.remove(&operation);
                    let _ = fs::remove_file(journal_path(directory, operation));
                    sync_directory(directory);
                    finish_acks.extend(ack);
                }
                Command::Flush { ack } => flush_acks.push(ack),
            }
        }
        if !flush_acks.is_empty() {
            sync_dirty(&files, &mut dirty);
        }
        if dirty.is_empty() {
            due = None;
        } else if due.is_none() {
            due = Some(Instant::now() + SYNC_INTERVAL);
        }
        for (ack, result) in begin_acks {
            let _ = ack.send(result);
        }
        for ack in flush_acks.into_iter().chain(finish_acks) {
            let _ = ack.send(());
        }
    }
    // The sender is gone (shutdown): nothing queued may stay unsynced.
    sync_dirty(&files, &mut dirty);
}

fn begin_file(
    directory: &Path,
    operation: OperationId,
    line: &str,
    files: &mut HashMap<OperationId, File>,
) -> Result<(), JournalError> {
    let mut options = OpenOptions::new();
    options.create_new(true).append(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(journal_path(directory, operation))?;
    file.write_all(line.as_bytes())?;
    file.sync_all()?;
    sync_directory(directory);
    files.insert(operation, file);
    Ok(())
}

fn recover_directory(directory: &Path) -> Recovery {
    let mut recovery = Recovery::default();
    let Ok(entries) = fs::read_dir(directory) else {
        return recovery;
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path.extension().and_then(|value| value.to_str()) == Some(FILE_EXTENSION)
        })
        .collect();
    paths.sort();
    for path in paths {
        match replay_file(&path) {
            Ok(job) => recovery.jobs.push(job),
            Err(ReplayError::NeverStarted) => {
                // A crash while `begin` was being written: `begin` is durable
                // before a job executes, so nothing ever ran. Not an error.
                let _ = fs::remove_file(&path);
            }
            Err(ReplayError::Untrusted(reason)) => {
                let moved = quarantine(directory, &path);
                tracing::warn!(path = %path.display(), %reason, "transfer journal quarantined");
                recovery.quarantined.push(QuarantinedJournal {
                    path: moved,
                    reason,
                });
            }
        }
    }
    recovery
}

fn quarantine(directory: &Path, path: &Path) -> PathBuf {
    let target_directory = directory.join(QUARANTINE_DIRECTORY);
    let name = path.file_name().map_or_else(
        || "unknown.journal".to_owned(),
        |name| name.to_string_lossy().into_owned(),
    );
    let target = target_directory.join(format!("{name}.{}.corrupt", Utc::now().timestamp_millis()));
    if fs::create_dir_all(&target_directory).is_ok() && fs::rename(path, &target).is_ok() {
        sync_directory(directory);
        return target;
    }
    // Could not move it; remove nothing, report where it still is.
    path.to_path_buf()
}

enum ReplayError {
    /// The file holds no complete `begin` record, so the job never started.
    NeverStarted,
    /// The file is unreadable or not ours; keep it for inspection.
    Untrusted(String),
}

fn replay_file(path: &Path) -> Result<RecoveredJob, ReplayError> {
    let untrusted = |reason: &str| ReplayError::Untrusted(reason.to_owned());
    let bytes =
        fs::read(path).map_err(|error| ReplayError::Untrusted(format!("unreadable: {error}")))?;
    let text = String::from_utf8_lossy(&bytes);
    let mut lines = text.split_inclusive('\n');
    let first = lines.next().ok_or(ReplayError::NeverStarted)?;
    let Some(first) = first.strip_suffix('\n') else {
        return Err(ReplayError::NeverStarted);
    };
    let Some(Record::Begin { version, intent }) = decode(first) else {
        return Err(untrusted("missing or unreadable begin record"));
    };
    if version != JOURNAL_VERSION {
        return Err(ReplayError::Untrusted(format!(
            "unsupported journal version {version}"
        )));
    }
    let expected_name = journal_path(Path::new(""), intent.operation_id);
    if path.file_name() != expected_name.file_name() {
        return Err(untrusted("journal belongs to a different operation"));
    }
    let mut items: Vec<RecoveredItem> = Vec::new();
    let mut index: HashMap<Location, usize> = HashMap::new();
    let mut damage = None;
    for (offset, line) in lines.enumerate() {
        let line_number = offset + 2;
        let Some(body) = line.strip_suffix('\n') else {
            damage = Some(JournalDamage::TornTail);
            break;
        };
        match decode(body) {
            Some(Record::Event(event)) => apply(&mut items, &mut index, event),
            Some(Record::Begin { .. }) | None => {
                damage = Some(JournalDamage::CorruptRecord { line: line_number });
                break;
            }
        }
    }
    Ok(RecoveredJob {
        intent,
        items,
        damage,
    })
}

fn apply(
    items: &mut Vec<RecoveredItem>,
    index: &mut HashMap<Location, usize>,
    event: JournalEvent,
) {
    match event {
        JournalEvent::Items { items: planned } => {
            for item in planned {
                if index.contains_key(&item.source) {
                    continue;
                }
                index.insert(item.source.clone(), items.len());
                items.push(RecoveredItem {
                    source: item.source,
                    bytes: item.bytes,
                    partial: None,
                    destination: None,
                    revision: None,
                    committed: false,
                    committed_bytes: 0,
                });
            }
        }
        JournalEvent::Partial {
            source,
            partial,
            destination,
        } => {
            if let Some(item) = index.get(&source).and_then(|at| items.get_mut(*at)) {
                item.partial = Some(partial);
                item.destination = destination;
            }
        }
        JournalEvent::SourceRevision { source, revision } => {
            if let Some(item) = index.get(&source).and_then(|at| items.get_mut(*at)) {
                item.revision = Some(revision);
            }
        }
        JournalEvent::Committed { source, bytes } => {
            if let Some(item) = index.get(&source).and_then(|at| items.get_mut(*at)) {
                item.committed = true;
                item.committed_bytes = bytes;
            }
        }
    }
}

/// Shared handle used by the scheduler.
pub type SharedJournal = Arc<dyn OperationJournal>;

#[cfg(test)]
mod tests {
    use super::*;

    fn location(name: &str) -> Location {
        Location::from_native_path(&std::env::temp_dir().join(name)).expect("location")
    }

    fn sample_intent() -> JournalIntent {
        JournalIntent {
            operation_id: OperationId::new(),
            kind: OperationKind::Copy,
            sources: vec![location("a"), location("b")],
            destination: Some(location("dest")),
            conflict_policy: ConflictPolicy::Ask,
            created_at: Utc::now(),
        }
    }

    fn item(name: &str, bytes: u64) -> JournalItem {
        JournalItem {
            source: location(name),
            bytes,
        }
    }

    async fn write_job(journal: &FileOperationJournal, intent: &JournalIntent) {
        journal.begin(intent.clone()).await.expect("begin");
        journal.record(
            intent.operation_id,
            JournalEvent::Items {
                items: vec![item("a", 10), item("b", 20)],
            },
        );
        journal.record(
            intent.operation_id,
            JournalEvent::Partial {
                source: location("a"),
                partial: location(".fm-copy-1"),
                destination: Some(location("dest/a")),
            },
        );
        journal.record(
            intent.operation_id,
            JournalEvent::SourceRevision {
                source: location("a"),
                revision: SourceRevision {
                    size: Some(10),
                    modified_at_ms: Some(5),
                    validator: Some("etag".into()),
                },
            },
        );
        journal.record(
            intent.operation_id,
            JournalEvent::Committed {
                source: location("a"),
                bytes: 10,
            },
        );
        journal.flush().await;
    }

    fn journal_file(directory: &Path, intent: &JournalIntent) -> PathBuf {
        journal_path(directory, intent.operation_id)
    }

    #[tokio::test]
    async fn a_started_job_is_recovered_with_its_checkpoints_after_a_restart() {
        let directory = tempfile::tempdir().expect("dir");
        let intent = sample_intent();
        {
            let (journal, recovery) = FileOperationJournal::open(directory.path()).expect("open");
            assert!(recovery.jobs.is_empty());
            write_job(&journal, &intent).await;
        } // process "crashes": the journal is dropped without finish()
        let (_journal, recovery) = FileOperationJournal::open(directory.path()).expect("reopen");
        assert_eq!(recovery.jobs.len(), 1);
        let job = &recovery.jobs[0];
        assert_eq!(job.intent, intent);
        assert_eq!(job.damage, None);
        assert_eq!(job.items.len(), 2);
        let first = &job.items[0];
        assert!(first.committed && first.committed_bytes == 10);
        assert_eq!(first.partial, Some(location(".fm-copy-1")));
        assert_eq!(first.destination, Some(location("dest/a")));
        assert_eq!(first.revision.as_ref().and_then(|r| r.size), Some(10));
        let second = &job.items[1];
        assert!(!second.committed && second.partial.is_none());
    }

    #[tokio::test]
    async fn a_finished_job_leaves_nothing_behind() {
        let directory = tempfile::tempdir().expect("dir");
        let intent = sample_intent();
        let (journal, _) = FileOperationJournal::open(directory.path()).expect("open");
        write_job(&journal, &intent).await;
        journal.finish(intent.operation_id).await;
        assert!(!journal_file(directory.path(), &intent).exists());
        drop(journal);
        let (_journal, recovery) = FileOperationJournal::open(directory.path()).expect("reopen");
        assert!(recovery.jobs.is_empty() && recovery.quarantined.is_empty());
    }

    #[tokio::test]
    async fn a_torn_final_record_keeps_the_valid_prefix_and_is_reported() {
        let directory = tempfile::tempdir().expect("dir");
        let intent = sample_intent();
        {
            let (journal, _) = FileOperationJournal::open(directory.path()).expect("open");
            write_job(&journal, &intent).await;
        }
        let path = journal_file(directory.path(), &intent);
        let mut file = OpenOptions::new().append(true).open(&path).expect("append");
        file.write_all(b"deadbeef {\"type\":\"commit")
            .expect("tear");
        drop(file);
        let (_journal, recovery) = FileOperationJournal::open(directory.path()).expect("reopen");
        assert!(recovery.quarantined.is_empty());
        let job = &recovery.jobs[0];
        assert_eq!(job.damage, Some(JournalDamage::TornTail));
        assert!(job.items[0].committed, "records before the tear survive");
    }

    #[tokio::test]
    async fn a_corrupt_middle_record_discards_everything_after_it() {
        let directory = tempfile::tempdir().expect("dir");
        let intent = sample_intent();
        {
            let (journal, _) = FileOperationJournal::open(directory.path()).expect("open");
            write_job(&journal, &intent).await;
        }
        let path = journal_file(directory.path(), &intent);
        let text = fs::read_to_string(&path).expect("read");
        let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
        // Line 3 is the partial record; flip a payload byte so its checksum fails.
        lines[2] = lines[2].replacen("fm-copy-1", "fm-copy-X", 1);
        fs::write(&path, format!("{}\n", lines.join("\n"))).expect("write");
        let (_journal, recovery) = FileOperationJournal::open(directory.path()).expect("reopen");
        let job = &recovery.jobs[0];
        assert_eq!(job.damage, Some(JournalDamage::CorruptRecord { line: 3 }));
        assert_eq!(job.items.len(), 2, "records before the corruption survive");
        assert!(job.items[0].partial.is_none());
        assert!(
            !job.items[0].committed,
            "records after the corruption are discarded"
        );
    }

    #[tokio::test]
    async fn an_unreadable_begin_record_quarantines_the_file_without_failing_startup() {
        let directory = tempfile::tempdir().expect("dir");
        let intent = sample_intent();
        {
            let (journal, _) = FileOperationJournal::open(directory.path()).expect("open");
            write_job(&journal, &intent).await;
        }
        let path = journal_file(directory.path(), &intent);
        fs::write(&path, b"garbage that is not a journal\n").expect("corrupt");
        let (journal, recovery) = FileOperationJournal::open(directory.path()).expect("reopen");
        assert!(recovery.jobs.is_empty());
        assert_eq!(recovery.quarantined.len(), 1);
        assert!(!path.exists(), "the bad file is moved aside");
        assert!(recovery.quarantined[0].path.exists());
        // The journal still works afterwards.
        let next = sample_intent();
        journal.begin(next).await.expect("begin after quarantine");
    }

    #[tokio::test]
    async fn an_empty_or_torn_begin_record_is_discarded_because_the_job_never_ran() {
        let directory = tempfile::tempdir().expect("dir");
        let first = sample_intent();
        let second = sample_intent();
        fs::write(journal_file(directory.path(), &first), b"").expect("empty");
        let begin = encode(&Record::Begin {
            version: JOURNAL_VERSION,
            intent: second.clone(),
        })
        .expect("encode");
        fs::write(
            journal_file(directory.path(), &second),
            &begin.as_bytes()[..begin.len() / 2],
        )
        .expect("truncate");
        let (_journal, recovery) = FileOperationJournal::open(directory.path()).expect("reopen");
        assert!(recovery.jobs.is_empty());
        assert!(
            recovery.quarantined.is_empty(),
            "a torn begin is not an error: begin is durable before a job executes"
        );
        assert!(!journal_file(directory.path(), &first).exists());
        assert!(!journal_file(directory.path(), &second).exists());
    }

    #[tokio::test]
    async fn an_unknown_version_or_foreign_job_is_not_trusted() {
        let directory = tempfile::tempdir().expect("dir");
        let future = sample_intent();
        let line = encode(&Record::Begin {
            version: JOURNAL_VERSION + 1,
            intent: future.clone(),
        })
        .expect("encode");
        fs::write(journal_file(directory.path(), &future), line).expect("write");
        let foreign = sample_intent();
        let other = sample_intent();
        let line = encode(&Record::Begin {
            version: JOURNAL_VERSION,
            intent: other,
        })
        .expect("encode");
        fs::write(journal_file(directory.path(), &foreign), line).expect("write");
        let (_journal, recovery) = FileOperationJournal::open(directory.path()).expect("open");
        assert!(recovery.jobs.is_empty());
        assert_eq!(recovery.quarantined.len(), 2);
    }

    #[tokio::test]
    async fn begin_twice_for_one_job_fails_instead_of_overwriting_its_journal() {
        let directory = tempfile::tempdir().expect("dir");
        let intent = sample_intent();
        let (journal, _) = FileOperationJournal::open(directory.path()).expect("open");
        journal.begin(intent.clone()).await.expect("first");
        assert!(matches!(
            journal.begin(intent).await,
            Err(JournalError::Io(_))
        ));
    }

    #[tokio::test]
    async fn interleaved_jobs_replay_independently_and_in_plan_order() {
        let directory = tempfile::tempdir().expect("dir");
        let left = sample_intent();
        let right = sample_intent();
        {
            let (journal, _) = FileOperationJournal::open(directory.path()).expect("open");
            journal.begin(left.clone()).await.expect("left");
            journal.begin(right.clone()).await.expect("right");
            for (job, names) in [(&left, ["z", "y", "x"]), (&right, ["c", "b", "a"])] {
                for name in names {
                    journal.record(
                        job.operation_id,
                        JournalEvent::Items {
                            items: vec![item(name, 1)],
                        },
                    );
                    journal.record(
                        job.operation_id,
                        JournalEvent::Committed {
                            source: location(name),
                            bytes: 1,
                        },
                    );
                }
            }
            journal.flush().await;
        }
        let (_journal, recovery) = FileOperationJournal::open(directory.path()).expect("reopen");
        let order = |id: OperationId| {
            recovery
                .jobs
                .iter()
                .find(|job| job.intent.operation_id == id)
                .map(|job| {
                    job.items
                        .iter()
                        .map(|item| item.source.uri.rsplit('/').next().unwrap_or("").to_owned())
                        .collect::<Vec<_>>()
                })
        };
        assert_eq!(
            order(left.operation_id),
            Some(vec!["z".into(), "y".into(), "x".into()])
        );
        assert_eq!(
            order(right.operation_id),
            Some(vec!["c".into(), "b".into(), "a".into()])
        );
    }

    #[tokio::test]
    async fn records_for_an_unbegun_job_are_ignored_rather_than_creating_files() {
        let directory = tempfile::tempdir().expect("dir");
        let (journal, _) = FileOperationJournal::open(directory.path()).expect("open");
        let stray = OperationId::new();
        journal.record(
            stray,
            JournalEvent::Committed {
                source: location("a"),
                bytes: 1,
            },
        );
        journal.flush().await;
        assert_eq!(
            fs::read_dir(directory.path())
                .expect("list")
                .filter_map(Result::ok)
                .filter(|entry| entry
                    .path()
                    .extension()
                    .is_some_and(|ext| ext == FILE_EXTENSION))
                .count(),
            0
        );
    }

    #[tokio::test]
    async fn a_second_instance_on_the_same_directory_is_refused() {
        let directory = tempfile::tempdir().expect("dir");
        let (first, _) = FileOperationJournal::open(directory.path()).expect("first");
        assert!(matches!(
            FileOperationJournal::open(directory.path()),
            Err(JournalError::Io(message)) if message.contains("in use")
        ));
        drop(first);
        assert!(
            FileOperationJournal::open(directory.path()).is_ok(),
            "the lock is released when the first instance goes away"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn journal_files_and_directory_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let parent = tempfile::tempdir().expect("dir");
        let directory = parent.path().join("journal");
        let intent = sample_intent();
        let (journal, _) = FileOperationJournal::open(&directory).expect("open");
        journal.begin(intent.clone()).await.expect("begin");
        let mode = |path: &Path| fs::metadata(path).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode(&directory), 0o700);
        assert_eq!(mode(&journal_file(&directory, &intent)), 0o600);
    }
}
