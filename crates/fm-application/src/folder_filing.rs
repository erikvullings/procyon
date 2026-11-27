//! Persistent filing queue. All filesystem mutations use ordinary operation jobs.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use chrono::{DateTime, TimeDelta, Utc};
use fm_domain::{EntryId, EntryKind, EntrySummary, Location, OperationId};
use fm_events::{
    BackendEventPayload, EventAudience, EventBus, NotificationLevelPayload, NotificationPayload,
};
use fm_settings::{SettingsStore, VersionedDocument};
use fm_transport_dto::LocationDto;
use fm_transport_dto::{
    OperationConflictPolicyDto, OperationKindDto, OperationStateDto, StartOperationRequestDto,
    watched_folders::{
        ApprovedMove, FileVersion, FilingRule, FilingStatus, OrganizerPreview, OrganizerRequest,
        OrganizerResponse, WatchedFolder,
    },
};
use fm_vfs::ProviderRegistry;

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tokio_util::sync::{CancellationToken, DropGuard};
use uuid::Uuid;

use crate::{
    ApplicationError,
    directory::list_all,
    llm_profiles::LlmProfileService,
    operations_coordinator::OperationsCoordinator,
    watched_folders::{CleanupJobs, check_local_root, file_version, is_candidate},
};

type AccessCheck = dyn Fn(&LocationDto) -> bool + Send + Sync;

#[derive(Serialize, Deserialize)]
struct Document {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    #[serde(rename = "watchedFolders")]
    folders: Vec<Folder>,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            schema_version: 1,
            folders: Vec::new(),
        }
    }
}

impl VersionedDocument for Document {
    type MigrationError = std::convert::Infallible;
    const FILE_NAME: &'static str = "watched-folders.json";
    const CURRENT_SCHEMA_VERSION: u32 = 1;
    fn migrate(
        value: serde_json::Value,
        _version: u32,
    ) -> Result<serde_json::Value, Self::MigrationError> {
        Ok(value)
    }
    fn validate(&self) -> Result<(), Self::MigrationError> {
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
struct Folder {
    root: Location,
    delay_hours: u16,
    paused: bool,
    rules: Vec<FilingRule>,
    error: Option<String>,
    seen: HashMap<String, SeenFile>,
    queue: Vec<Pending>,
}

#[derive(Serialize, Deserialize)]
struct SeenFile {
    version: FileVersion,
    new_arrival: bool,
    filed: bool,
}

#[derive(Serialize, Deserialize)]
struct Pending {
    key: Uuid,
    id: EntryId,
    size: Option<u64>,
    modified: Option<DateTime<Utc>>,
    arrival: DateTime<Utc>,
    #[serde(default)]
    approved: bool,
    attempted: bool,
    status: FilingStatus,
}

impl Pending {
    fn new(
        entry: &EntrySummary,
        folder: String,
        arrival: DateTime<Utc>,
        delay_hours: u16,
        approved: bool,
    ) -> Self {
        Self {
            key: Uuid::new_v4(),
            id: entry.id,
            size: entry.size,
            modified: entry.modified_at,
            arrival,
            approved,
            attempted: false,
            status: FilingStatus {
                assignment: ApprovedMove {
                    source: entry.location.clone().into(),
                    version: Some(file_version(entry)),
                    folder,
                },
                due_at: filing_time(arrival, delay_hours),
                operation_id: None,
                error: None,
            },
        }
    }
}

struct RootWatch {
    changed: Arc<AtomicBool>,
    _stop: DropGuard,
}

pub(crate) struct FolderFiling {
    store: SettingsStore,
    providers: ProviderRegistry,
    directories: crate::DirectoryService,
    operations: Arc<OperationsCoordinator>,
    events: EventBus,
    gate: Mutex<()>,
    started: AtomicBool,
    access: std::sync::RwLock<Box<AccessCheck>>,
}

impl FolderFiling {
    pub(crate) fn new(
        store: SettingsStore,
        providers: ProviderRegistry,
        directories: crate::DirectoryService,
        operations: Arc<OperationsCoordinator>,
        events: EventBus,
    ) -> Self {
        Self {
            store,
            providers,
            directories,
            operations,
            events,
            gate: Mutex::new(()),
            started: AtomicBool::new(false),
            access: std::sync::RwLock::new(Box::new(|_| true)),
        }
    }

    pub(crate) fn set_access(&self, access: impl Fn(&LocationDto) -> bool + Send + Sync + 'static) {
        *self
            .access
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Box::new(access);
    }

    fn allows(&self, location: &Location) -> bool {
        self.access
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)(&location.clone().into())
    }

    pub(crate) fn start(self: &Arc<Self>) {
        if self.started.swap(true, Ordering::AcqRel) {
            return;
        }
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(2));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut watches = HashMap::new();
            let mut worker_lock = None;
            let mut last_reconcile = tokio::time::Instant::now();
            loop {
                interval.tick().await;
                let Some(filing) = weak.upgrade() else { break };
                if worker_lock.is_none() {
                    if !filing.store.document_path::<Document>().exists() {
                        continue;
                    }
                    worker_lock = filing.try_lock("watched-folders-worker.lock").ok();
                    if worker_lock.is_none() {
                        continue;
                    }
                }
                let safety_scan = last_reconcile.elapsed() >= Duration::from_secs(60);
                if safety_scan {
                    last_reconcile = tokio::time::Instant::now();
                }
                if let Err(error) = filing.tick(&mut watches, safety_scan).await {
                    tracing::warn!(%error, "watched-folder reconciliation failed");
                }
            }
        });
    }

    pub(crate) async fn manage(
        self: &Arc<Self>,
        request: OrganizerRequest,
        jobs: &CleanupJobs,
        profiles: &LlmProfileService,
    ) -> Result<OrganizerResponse, ApplicationError> {
        let mut response = OrganizerResponse::default();
        match request {
            OrganizerRequest::List => response.folders = self.list().await?,
            OrganizerRequest::Preview { root } => {
                response.job_id =
                    Some(jobs.start(self.providers.clone(), profiles.clone(), root.into())?)
            }
            OrganizerRequest::Collect { job_id } => {
                response.preview = jobs.take(job_id)?.map(|preview| OrganizerPreview {
                    moves: preview
                        .moves
                        .into_iter()
                        .filter_map(|item| {
                            Some(ApprovedMove {
                                source: item.source.into(),
                                version: Some(item.version),
                                folder: item.destination.name().ok()?,
                            })
                        })
                        .collect(),
                    unmatched: preview.unmatched.into_iter().map(Into::into).collect(),
                    unprocessed: preview.unprocessed.into_iter().map(Into::into).collect(),
                });
            }
            OrganizerRequest::Cancel { job_id } => jobs.cancel(job_id)?,
            OrganizerRequest::Save {
                root,
                delay_hours,
                paused,
                rules,
                approved,
            } => {
                response.folders = self
                    .save(root.into(), delay_hours, paused, rules, approved)
                    .await?;
                self.start();
            }
            OrganizerRequest::Remove { root } => {
                response.folders = self.remove(root.into()).await?
            }
        }
        Ok(response)
    }

    pub(crate) async fn list(&self) -> Result<Vec<WatchedFolder>, ApplicationError> {
        let _guard = self.gate.lock().await;
        Ok(views(&self.load()?))
    }

    pub(crate) async fn save(
        &self,
        root: Location,
        delay_hours: u16,
        paused: bool,
        rules: Vec<FilingRule>,
        approved: Vec<ApprovedMove>,
    ) -> Result<Vec<WatchedFolder>, ApplicationError> {
        if !matches!(
            root.provider_id.as_str(),
            "local" | "sftp" | "webdav" | "onedrive"
        ) {
            return Err(invalid(
                "This provider does not support automatic moves that refuse overwrites.",
            ));
        }
        if delay_hours > 8760 || rules.len() > 128 || approved.len() > 1024 {
            return Err(invalid(
                "The grace period, rule count, or selection exceeds the supported limit.",
            ));
        }
        for rule in &rules {
            target(&root, &rule.folder)?;
            if rule.extension.is_none()
                && rule.content_type.is_none()
                && rule.minimum_bytes.is_none()
            {
                return Err(invalid(
                    "Every filing rule needs at least one match criterion.",
                ));
            }
        }
        let _guard = self.gate.lock().await;
        let _file_lock = self.try_lock("watched-folders.lock")?;
        let mut document = self.load()?;
        let index = document
            .folders
            .iter()
            .position(|folder| folder.root == root);
        if index.is_none() && document.folders.len() >= 16 {
            return Err(invalid("At most 16 folders can be watched at once."));
        }
        let entries = if index.is_none() || !approved.is_empty() {
            self.entries(&root).await?
        } else {
            Vec::new()
        };
        let folder = match index {
            Some(index) => &mut document.folders[index],
            None => {
                document.folders.push(Folder {
                    root: root.clone(),
                    delay_hours,
                    paused,
                    rules: Vec::new(),
                    error: None,
                    seen: entries
                        .iter()
                        .filter(|entry| is_candidate(entry))
                        .map(|entry| {
                            (
                                entry.location.uri.clone(),
                                SeenFile {
                                    version: file_version(entry),
                                    new_arrival: false,
                                    filed: false,
                                },
                            )
                        })
                        .collect(),
                    queue: Vec::new(),
                });
                document
                    .folders
                    .last_mut()
                    .ok_or(ApplicationError::Internal)?
            }
        };
        let now = Utc::now();
        for assignment in approved {
            target(&root, &assignment.folder)?;
            let source: Location = assignment.source.into();
            let entry = entries
                .iter()
                .find(|entry| entry.location == source && is_candidate(entry))
                .ok_or_else(|| {
                    invalid("A selected file is no longer available in the watched folder.")
                })?;
            if assignment
                .version
                .is_some_and(|version| version != file_version(entry))
            {
                return Err(invalid(
                    "A selected file changed since the preview. Scan it again before approving.",
                ));
            }
            if folder.queue.iter().any(|pending| {
                pending.id == entry.id
                    && pending.status.assignment.source == entry.location.clone().into()
                    && pending.status.error.is_none()
            }) {
                continue;
            }
            let arrival = entry
                .created_at
                .max(entry.modified_at)
                .unwrap_or(now)
                .min(now);
            folder.queue.push(Pending::new(
                entry,
                assignment.folder,
                arrival,
                delay_hours,
                true,
            ));
            folder.seen.insert(
                entry.location.uri.clone(),
                SeenFile {
                    version: file_version(entry),
                    new_arrival: false,
                    filed: false,
                },
            );
        }
        folder.delay_hours = delay_hours;
        folder.paused = paused;
        folder.rules = rules;
        for pending in &mut folder.queue {
            if !pending.attempted {
                pending.status.due_at = filing_time(pending.arrival, delay_hours);
            }
        }
        self.persist(&document)?;
        Ok(views(&document))
    }

    pub(crate) async fn remove(
        &self,
        root: Location,
    ) -> Result<Vec<WatchedFolder>, ApplicationError> {
        let _guard = self.gate.lock().await;
        let _file_lock = self.try_lock("watched-folders.lock")?;
        let mut document = self.load()?;
        document.folders.retain(|folder| folder.root != root);
        self.persist(&document)?;
        Ok(views(&document))
    }

    fn try_lock(&self, name: &str) -> Result<std::fs::File, ApplicationError> {
        let path = self.store.document_path::<Document>();
        let directory = path.parent().ok_or(ApplicationError::Internal)?;
        std::fs::create_dir_all(directory).map_err(|_| ApplicationError::Internal)?;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join(name))
            .map_err(|_| ApplicationError::Internal)?;
        fs2::FileExt::try_lock_exclusive(&file)
            .map_err(|_| invalid("Watched-folder settings are busy. Try again shortly."))?;
        Ok(file)
    }

    fn load(&self) -> Result<Document, ApplicationError> {
        self.store
            .load_document()
            .map(|document| document.unwrap_or_default())
            .map_err(|error| {
                tracing::warn!(%error, "could not load watched-folder settings");
                ApplicationError::Internal
            })
    }

    fn persist(&self, document: &Document) -> Result<(), ApplicationError> {
        self.store.save_document(document).map_err(|error| {
            tracing::warn!(%error, "could not persist watched-folder settings");
            ApplicationError::Internal
        })
    }

    async fn entries(&self, root: &Location) -> Result<Vec<EntrySummary>, ApplicationError> {
        if root.provider_id.as_str() == "local" {
            check_local_root(root).await?;
        }
        let provider = self.providers.resolve(root)?;
        let cancellation = CancellationToken::new();
        let _cancel_on_exit = cancellation.clone().drop_guard();
        tokio::time::timeout(
            Duration::from_secs(10),
            list_all(provider, root, cancellation),
        )
        .await
        .map_err(|_| invalid("The watched folder did not respond."))?
        .map_err(Into::into)
    }

    async fn tick(
        &self,
        watches: &mut HashMap<Location, RootWatch>,
        safety_scan: bool,
    ) -> Result<(), ApplicationError> {
        let _guard = self.gate.lock().await;
        if !self.store.document_path::<Document>().exists() {
            return Ok(());
        }
        let _file_lock = self.try_lock("watched-folders.lock")?;
        let mut document = self.load()?;
        watches.retain(|root, _| {
            document
                .folders
                .iter()
                .any(|folder| !folder.paused && folder.root == *root && self.allows(root))
        });
        let prior_failure_count = failure_count(&document);
        let mut changed = false;
        for index in 0..document.folders.len() {
            let folder = &mut document.folders[index];
            let root = folder.root.clone();
            changed |= self.collect_operations(folder);
            if folder.paused || !self.allows(&root) {
                continue;
            }
            let now = Utc::now();
            let mut needs_listing = safety_scan;
            if let Some(watch) = watches.get(&root) {
                needs_listing |= watch.changed.swap(false, Ordering::AcqRel);
            } else {
                needs_listing = true;
                let stop = CancellationToken::new();
                let signal = Arc::new(AtomicBool::new(false));
                // Validate a local path before registering a native watcher on it.
                let safe_path =
                    root.provider_id.as_str() != "local" || check_local_root(&root).await.is_ok();
                if safe_path
                    && self
                        .directories
                        .watch_for_filing(root.clone(), signal.clone(), stop.clone())
                        .await
                        .is_ok()
                {
                    watches.insert(
                        root.clone(),
                        RootWatch {
                            changed: signal,
                            _stop: stop.drop_guard(),
                        },
                    );
                }
            }
            let has_due = folder.queue.iter().any(|pending| {
                !pending.attempted && pending.status.error.is_none() && pending.status.due_at <= now
            });
            if !needs_listing && !has_due {
                continue;
            }
            let entries = match self.entries(&root).await {
                Ok(entries) => entries,
                Err(error) => {
                    let error = Some(error.to_string());
                    changed |= folder.error != error;
                    folder.error = error;
                    continue;
                }
            };
            changed |= folder.error.take().is_some();
            changed |= register_arrivals(folder, &entries, now);
            let Some(pending_index) = folder.queue.iter().position(|pending| {
                !pending.attempted && pending.status.error.is_none() && pending.status.due_at <= now
            }) else {
                continue;
            };
            let pending = &mut folder.queue[pending_index];
            let source: Location = pending.status.assignment.source.clone().into();
            let current = entries
                .iter()
                .find(|entry| entry.location == source && is_candidate(entry));
            let Some(entry) = current.filter(|entry| entry.id == pending.id) else {
                pending.status.error =
                    Some("The original file is unavailable or was replaced.".into());
                changed = true;
                continue;
            };
            if entry.size != pending.size || entry.modified_at != pending.modified {
                if pending.approved {
                    pending.status.error = Some(
                        "The file changed after approval. Review it again before filing.".into(),
                    );
                } else {
                    pending.size = entry.size;
                    pending.modified = entry.modified_at;
                    pending.status.assignment.version = Some(file_version(entry));
                    pending.arrival = now;
                    pending.status.due_at = filing_time(now, folder.delay_hours);
                }
                changed = true;
                continue;
            }
            let destination = target(&root, &pending.status.assignment.folder)?;
            if !self.allows(&destination) || !self.allows(&source) {
                pending.status.error =
                    Some("The filing path is outside the accessible roots.".into());
                changed = true;
                continue;
            }
            let existing = entries.iter().find(|entry| entry.location == destination);
            let destination_id = if let Some(existing) = existing {
                if existing.kind != EntryKind::Directory {
                    pending.status.error =
                        Some("The filing destination is not a directory.".into());
                    changed = true;
                    continue;
                }
                if destination.provider_id.as_str() == "local"
                    && let Err(error) = check_local_root(&destination).await
                {
                    pending.status.error = Some(error.to_string());
                    changed = true;
                    continue;
                }
                existing.id
            } else {
                let key = format!("organizer-directory-{}", pending.key);
                let request = operation_request(
                    OperationKindDto::CreateDirectory,
                    None,
                    root,
                    Some(pending.status.assignment.folder.clone()),
                );
                match self.operations.start_filing(
                    request,
                    key,
                    crate::operation_planner::FilingGuard::CreateDirectory,
                    None,
                ) {
                    Ok(operation)
                        if !matches!(
                            operation.state,
                            OperationStateDto::Failed
                                | OperationStateDto::Cancelled
                                | OperationStateDto::CompletedWithWarnings
                        ) => {}
                    Ok(_) => {
                        pending.status.error =
                            Some("Could not create the filing destination.".into());
                        changed = true;
                    }
                    Err(error) => {
                        pending.status.error = Some(error.to_string());
                        changed = true;
                    }
                }
                continue;
            };
            let guard = crate::operation_planner::FilingGuard::Move {
                source_id: pending.id,
                size: pending.size,
                modified: pending.modified,
                destination_id,
            };
            let key = pending.key;
            pending.attempted = true;
            pending.status.operation_id = Some(key);
            self.persist(&document)?;
            let request =
                operation_request(OperationKindDto::Move, Some(source), destination, None);
            let pending = &mut document.folders[index].queue[pending_index];
            match self.operations.start_filing(
                request,
                format!("organizer-move-{key}"),
                guard,
                Some(OperationId::from(key)),
            ) {
                Ok(operation) => pending.status.operation_id = Some(operation.id),
                Err(error) => pending.status.error = Some(error.to_string()),
            }
            self.persist(&document)?;
            changed = false;
        }
        if changed {
            self.persist(&document)?;
        }
        if failure_count(&document) > prior_failure_count {
            self.events.publish(EventAudience::Global, BackendEventPayload::NotificationCreated {
                notification: NotificationPayload::new(NotificationLevelPayload::Warning,
                    "A watched folder needs attention. See Settings → Watched folders for details."),
            });
        }
        Ok(())
    }

    fn collect_operations(&self, folder: &mut Folder) -> bool {
        let mut changed = false;
        folder.queue.retain_mut(|pending| {
            if !pending.attempted || pending.status.error.is_some() {
                return true;
            }
            let operation = pending.status.operation_id.and_then(|id| self.operations.get(OperationId::from(id)).ok());
            if operation.as_ref().is_some_and(|operation| operation.state == OperationStateDto::Completed && operation.progress.completed_items > 0) {
                if let Some(seen) = folder.seen.get_mut(&pending.status.assignment.source.uri) {
                    seen.filed = true;
                    seen.new_arrival = false;
                }
                changed = true;
                return false;
            }
            if operation.is_none() || operation.as_ref().is_some_and(|operation| operation.state == OperationStateDto::Interrupted) {
                // Submission may have been interrupted before the scheduler recorded the job.
                // The next execution must prove the original source and refuse any collision.
                pending.attempted = false;
                pending.status.operation_id = None;
                pending.key = Uuid::new_v4();
                changed = true;
            } else if operation.is_some_and(|operation| matches!(operation.state,
                OperationStateDto::Completed | OperationStateDto::CompletedWithWarnings
                    | OperationStateDto::Cancelled | OperationStateDto::Failed
            )) {
                pending.status.error = Some(
                    "Filing did not complete. Check the file and operation history before retrying.".into(),
                );
                changed = true;
            }
            true
        });
        changed
    }
}

fn failure_count(document: &Document) -> usize {
    document
        .folders
        .iter()
        .map(|folder| {
            usize::from(folder.error.is_some())
                + folder
                    .queue
                    .iter()
                    .filter(|entry| entry.status.error.is_some())
                    .count()
        })
        .sum()
}

fn filing_time(arrival: DateTime<Utc>, hours: u16) -> DateTime<Utc> {
    arrival + TimeDelta::seconds((i64::from(hours) * 3600).max(10))
}

fn views(document: &Document) -> Vec<WatchedFolder> {
    document
        .folders
        .iter()
        .map(|folder| WatchedFolder {
            root: folder.root.clone().into(),
            delay_hours: folder.delay_hours,
            paused: folder.paused,
            rules: folder.rules.clone(),
            pending: folder
                .queue
                .iter()
                .map(|pending| pending.status.clone())
                .collect(),
            error: folder.error.clone(),
        })
        .collect()
}

fn register_arrivals(folder: &mut Folder, entries: &[EntrySummary], now: DateTime<Utc>) -> bool {
    let present: std::collections::HashSet<_> = entries
        .iter()
        .filter(|entry| is_candidate(entry))
        .map(|entry| entry.location.uri.as_str())
        .collect();
    let previous_count = folder.seen.len();
    folder
        .seen
        .retain(|uri, seen| seen.filed || present.contains(uri.as_str()));
    let mut changed = folder.seen.len() != previous_count;
    for entry in entries.iter().filter(|entry| is_candidate(entry)) {
        let version = file_version(entry);
        let uri = &entry.location.uri;
        let previous = folder.seen.get(uri);
        if previous.is_some_and(|seen| seen.version == version) {
            continue;
        }
        let new_arrival = previous.is_none_or(|seen| {
            seen.new_arrival || seen.filed || seen.version.entry_id != version.entry_id
        });
        folder.seen.insert(
            uri.clone(),
            SeenFile {
                version,
                new_arrival,
                filed: false,
            },
        );
        changed = true;
        if !new_arrival {
            continue;
        }
        let source = entry.location.clone().into();
        if folder.queue.iter().any(|pending| {
            pending.status.assignment.source == source
                && (pending.approved || pending.attempted)
                && pending.status.error.is_none()
        }) {
            continue;
        }
        folder.queue.retain(|pending| {
            pending.status.assignment.source != source
                || pending.approved
                || pending.attempted
                || pending.status.error.is_some()
        });
        if let Some(rule) = folder.rules.iter().find(|rule| matches_rule(rule, entry)) {
            folder.queue.push(Pending::new(
                entry,
                rule.folder.clone(),
                now,
                folder.delay_hours,
                false,
            ));
        }
    }
    changed
}

fn target(root: &Location, folder: &str) -> Result<Location, ApplicationError> {
    if folder.trim() != folder
        || folder.starts_with('.')
        || folder.ends_with('.')
        || folder.len() > 120
        || folder.contains([':', '*', '?', '"', '<', '>', '|'])
        || folder.chars().any(char::is_control)
        || fm_domain::location::is_windows_device_name(folder)
    {
        return Err(invalid("Choose a valid immediate destination folder name."));
    }
    root.join(folder)
        .map_err(|_| invalid("Choose a valid immediate destination folder name."))
}

fn matches_rule(rule: &FilingRule, entry: &EntrySummary) -> bool {
    rule.extension.as_ref().is_none_or(|extension| {
        entry
            .extension
            .as_ref()
            .is_some_and(|actual| actual.eq_ignore_ascii_case(extension.trim_start_matches('.')))
    }) && rule.content_type.as_ref().is_none_or(|content_type| {
        entry
            .mime_type
            .as_ref()
            .is_some_and(|actual| actual.eq_ignore_ascii_case(content_type))
    }) && rule
        .minimum_bytes
        .is_none_or(|minimum| entry.size.is_some_and(|size| size >= minimum))
}

fn invalid(message: &str) -> ApplicationError {
    ApplicationError::InvalidRequest(message.to_owned())
}

fn operation_request(
    kind: OperationKindDto,
    source: Option<Location>,
    destination: Location,
    name: Option<String>,
) -> StartOperationRequestDto {
    StartOperationRequestDto {
        operation_type: kind,
        sources: source.into_iter().map(Into::into).collect(),
        destination: Some(destination.into()),
        destinations: Vec::new(),
        conflict_policy: OperationConflictPolicyDto::Skip,
        name,
        archive_format: None,
        archive_compression_level: None,
        create_intermediate_directories: false,
        initial_content: None,
        skip_space_check: false,
        image_metadata_edit: None,
        attribute_change: None,
        split_file: None,
        geo_transform: None,
        symlink_policy: Default::default(),
        permanent_delete_confirmed: false,
        override_read_only: false,
        link: None,
        verify_transfer: false,
    }
}
