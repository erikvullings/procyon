//! Mutation operation coordination (task 0119).

use crate::rejection::{Rejection, reason};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use fm_domain::{EntryId, Location, OperationId};
use fm_operations::{
    ConflictPolicy, ConflictResolution, Operation, OperationKind, Scheduler, SchedulerError,
};
use fm_transport_dto::{
    ConflictResolutionDto, OperationConflictPolicyDto, OperationDto, OperationPageDto,
    ResolveOperationConflictRequestDto, StartOperationRequestDto,
};
use fm_vfs::EntryRef;

use crate::error::ApplicationError;
use crate::operation_history::{OperationHistory, operation_dto};
use crate::operation_planner::OperationPlanner;
use fm_domain::ActionId;
use fm_transport_dto::OperationKindDto;

pub(crate) struct OperationsCoordinator {
    scheduler: Scheduler,
    history: Arc<OperationHistory>,
    planner: OperationPlanner,
    idempotency: Mutex<HashMap<String, OperationId>>,
    force_cross_volume_moves: Arc<AtomicBool>,
}

impl OperationsCoordinator {
    pub(crate) fn new(
        scheduler: Scheduler,
        history: Arc<OperationHistory>,
        planner: OperationPlanner,
        force_cross_volume_moves: Arc<AtomicBool>,
    ) -> Self {
        Self {
            scheduler,
            history,
            planner,
            idempotency: Mutex::new(HashMap::new()),
            force_cross_volume_moves,
        }
    }

    pub(crate) fn start(
        &self,
        request: StartOperationRequestDto,
        idempotency_key: Option<String>,
    ) -> Result<OperationDto, ApplicationError> {
        if request.conflict_policy == OperationConflictPolicyDto::KeepNewer {
            return Err(ApplicationError::from(Rejection::new(
                reason::KEEP_NEWER_UNSUPPORTED,
                "keepNewer conflict policy is not supported yet; choose ask, skip, overwrite, or renameNew",
            )));
        }
        let mut idempotency = self
            .idempotency
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(existing) = idempotency_key
            .as_ref()
            .and_then(|key| idempotency.get(key).copied())
        {
            return self.get(existing);
        }
        let destination: Option<Location> = request.destination.clone().map(Into::into);
        let executor = self.planner.plan(request.operation_type, &request)?;
        let sources: Vec<EntryRef> = request
            .sources
            .into_iter()
            .map(|location| EntryRef {
                id: EntryId::new(),
                location: location.into(),
            })
            .collect();
        let kind = operation_kind(request.operation_type);
        if matches!(
            kind,
            OperationKind::Delete | OperationKind::Copy | OperationKind::Move
        ) {
            for active in self.scheduler.list().into_iter().filter(|operation| {
                !operation.state.is_terminal()
                    && matches!(
                        operation.kind,
                        OperationKind::Delete | OperationKind::Copy | OperationKind::Move
                    )
            }) {
                if !sources.iter().any(|source| {
                    active
                        .sources
                        .iter()
                        .any(|other| paths_overlap(&source.location, &other.location))
                }) {
                    continue;
                }
                if active.kind == kind
                    && active.destination == destination
                    && sources.len() == active.sources.len()
                    && sources.iter().all(|source| {
                        active
                            .sources
                            .iter()
                            .any(|other| other.location == source.location)
                    })
                {
                    return self.get(active.id);
                }
                return Err(ApplicationError::from(Rejection::new(
                    reason::PATH_BUSY,
                    "another delete, copy, or move is already active for this path",
                )));
            }
        }
        let operation = Operation::new(
            kind,
            sources,
            destination,
            conflict_policy(request.conflict_policy),
        )
        // Only copies and duplicates can be verified: their output is private until published.
        .with_verification(
            request.verify_transfer
                && matches!(kind, OperationKind::Copy | OperationKind::Duplicate),
        );
        let id = self
            .scheduler
            .submit(operation, executor)
            .map_err(map_scheduler_error)?;
        if let Some(key) = idempotency_key {
            idempotency.insert(key, id);
        }
        self.get(id)
    }

    #[must_use]
    pub(crate) fn list(&self) -> Vec<OperationDto> {
        let mut active = self.scheduler.list();
        active.retain(|operation| !operation.state.is_terminal());
        let mut queued = active
            .iter()
            .filter(|operation| operation.state == fm_operations::OperationState::Queued)
            .map(|operation| (operation.id, operation.created_at))
            .collect::<Vec<_>>();
        queued.sort_by_key(|(_, created_at)| *created_at);
        let mut result = active
            .into_iter()
            .map(|operation| {
                let queue_position = queued
                    .iter()
                    .position(|(id, _)| *id == operation.id)
                    .and_then(|position| u64::try_from(position + 1).ok());
                operation_dto(operation, queue_position)
            })
            .chain(self.history.list())
            .collect::<Vec<_>>();
        result.sort_by_key(|operation| std::cmp::Reverse(operation.created_at));
        result
    }

    #[must_use]
    pub(crate) fn page(&self, offset: u64, limit: u16) -> OperationPageDto {
        let limit = limit.clamp(1, 100);
        let operations = self.list();
        let total = u64::try_from(operations.len()).unwrap_or(u64::MAX);
        let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(operations.len());
        let end = start
            .saturating_add(usize::from(limit))
            .min(operations.len());
        OperationPageDto {
            offset,
            limit,
            total,
            operations: operations[start..end].to_vec(),
        }
    }

    pub(crate) fn get(&self, id: OperationId) -> Result<OperationDto, ApplicationError> {
        match self.scheduler.get(id) {
            Ok(operation) if operation.state.is_terminal() => self
                .history
                .get(id)
                .or_else(|| Some(operation_dto(operation, None)))
                .ok_or(ApplicationError::NotFound),
            Ok(operation) => Ok(operation_dto(operation, None)),
            Err(error) => self
                .history
                .get(id)
                .ok_or_else(|| map_scheduler_error(error)),
        }
    }

    pub(crate) fn cancel(&self, id: OperationId) -> Result<(), SchedulerError> {
        self.scheduler.cancel(id)
    }

    pub(crate) fn undo(&self, id: OperationId) -> Result<OperationDto, ApplicationError> {
        let original = self.history.record(id).ok_or(ApplicationError::NotFound)?;
        let mut operation = Operation::new(
            OperationKind::Undo,
            original.sources.clone(),
            original.destination.clone(),
            ConflictPolicy::Ask,
        );
        operation.undo_of = Some(id);
        let undo_id = operation.id;
        let (_, plan) = self
            .history
            .reserve_undo(id, undo_id)
            .map_err(ApplicationError::from)?;
        let executor = match self.planner.plan_undo(plan) {
            Ok(executor) => executor,
            Err(error) => {
                self.history.release_undo(id, undo_id);
                return Err(error);
            }
        };
        if let Err(error) = self.scheduler.submit(operation, executor) {
            self.history.release_undo(id, undo_id);
            return Err(map_scheduler_error(error));
        }
        self.get(undo_id)
    }

    pub(crate) fn force_cross_volume_moves_for_tests(&self, force: bool) {
        self.force_cross_volume_moves
            .store(force, Ordering::Relaxed);
    }

    pub(crate) fn pause(&self, id: OperationId) -> Result<(), ApplicationError> {
        self.scheduler.pause(id).map_err(map_scheduler_error)
    }

    pub(crate) fn resume(&self, id: OperationId) -> Result<(), ApplicationError> {
        self.scheduler.resume(id).map_err(map_scheduler_error)
    }

    pub(crate) fn resolve_conflict(
        &self,
        id: OperationId,
        request: ResolveOperationConflictRequestDto,
    ) -> Result<(), ApplicationError> {
        if request.resolution == ConflictResolutionDto::Confirm {
            return self.scheduler.confirm(id).map_err(map_scheduler_error);
        }
        let resolution = match request.resolution {
            ConflictResolutionDto::Skip => ConflictResolution::Skip,
            ConflictResolutionDto::Overwrite => ConflictResolution::Overwrite,
            ConflictResolutionDto::RenameNew => ConflictResolution::RenameNew,
            ConflictResolutionDto::Confirm | ConflictResolutionDto::CancelOperation => {
                unreachable!("handled by the facade or above")
            }
        };
        self.scheduler
            .resolve_conflict(id, resolution, request.apply_to_all_similar)
            .map_err(map_scheduler_error)
    }

    pub(crate) fn republish_pending_conflicts(&self) {
        self.scheduler.republish_pending_conflicts();
    }
}

fn paths_overlap(left: &Location, right: &Location) -> bool {
    if left.provider_id != right.provider_id {
        return false;
    }
    let left = left.uri.trim_end_matches('/');
    let right = right.uri.trim_end_matches('/');
    left == right
        || left
            .strip_prefix(right)
            .is_some_and(|remaining| remaining.starts_with('/'))
        || right
            .strip_prefix(left)
            .is_some_and(|remaining| remaining.starts_with('/'))
}

// --- Conversions (folded from `operation_requests`) ---

pub(crate) fn map_scheduler_error(error: SchedulerError) -> ApplicationError {
    match error {
        SchedulerError::UnknownOperation(_) => ApplicationError::NotFound,
        SchedulerError::Transition(error) => ApplicationError::InvalidRequest(error.to_string()),
        SchedulerError::Execution(_) => ApplicationError::Internal,
    }
}

/// Builds a `copy` request for one sync-plan row.
///
/// `destination_root` is the *other* side's root: the traversal that
/// produced the comparison only ever descends into directory pairs that
/// exist, matched, on both sides, so `relative_path`'s parent is guaranteed
/// resolvable there even when the leaf itself is not (see
/// [`fm_comparison::resolve_relative`]'s documentation).
pub(crate) fn copy_request(
    source_root: &Location,
    destination_root: &Location,
    relative_path: &str,
    conflict_policy: OperationConflictPolicyDto,
) -> Result<StartOperationRequestDto, ApplicationError> {
    let source = fm_comparison::resolve_relative(source_root, relative_path)
        .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;
    let destination_parent = fm_comparison::resolve_relative(
        destination_root,
        fm_comparison::relative_parent(relative_path),
    )
    .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;
    Ok(StartOperationRequestDto {
        operation_type: OperationKindDto::Copy,
        sources: vec![source.into()],
        destination: Some(destination_parent.into()),
        destinations: Vec::new(),
        // Sync replaces a differing/older destination unless the profile chose otherwise.
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
        symlink_policy: fm_transport_dto::SymlinkPolicyDto::default(),
        permanent_delete_confirmed: false,
        override_read_only: false,
        link: None,
        verify_transfer: false,
    })
}

/// Builds a `delete` request for one sync-plan deletion row.
///
/// Uses permanent delete rather than trash: trash requires a platform
/// [`fm_platform::PlatformCapabilities::TRASH`] adapter that browser/server
/// mode does not provide (spec §2.2/§22), so a trash-based sync delete would
/// fail outright on every non-desktop host. `permanent_delete_confirmed` is
/// set unconditionally because applying a sync plan is itself the reviewed,
/// explicit confirmation spec §35 requires — the plan was only reached after
/// the caller reviewed and, if desired, edited every row.
pub(crate) fn delete_request(
    root: &Location,
    relative_path: &str,
) -> Result<StartOperationRequestDto, ApplicationError> {
    let target = fm_comparison::resolve_relative(root, relative_path)
        .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;
    Ok(StartOperationRequestDto {
        operation_type: OperationKindDto::Delete,
        sources: vec![target.into()],
        destination: None,
        destinations: Vec::new(),
        conflict_policy: OperationConflictPolicyDto::Ask,
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
        symlink_policy: fm_transport_dto::SymlinkPolicyDto::default(),
        permanent_delete_confirmed: true,
        override_read_only: false,
        link: None,
        verify_transfer: false,
    })
}

pub(crate) fn operation_kind(kind: OperationKindDto) -> fm_operations::OperationKind {
    match kind {
        OperationKindDto::CreateArchive => fm_operations::OperationKind::CreateArchive,
        OperationKindDto::MoveToArchive => fm_operations::OperationKind::MoveToArchive,
        OperationKindDto::CreateDirectory => fm_operations::OperationKind::CreateDirectory,
        OperationKindDto::CreateFile => fm_operations::OperationKind::CreateFile,
        OperationKindDto::CreateLink => fm_operations::OperationKind::CreateLink,
        OperationKindDto::Rename => fm_operations::OperationKind::Rename,
        OperationKindDto::Copy => fm_operations::OperationKind::Copy,
        OperationKindDto::Move => fm_operations::OperationKind::Move,
        OperationKindDto::Duplicate => fm_operations::OperationKind::Duplicate,
        OperationKindDto::Trash => fm_operations::OperationKind::Trash,
        OperationKindDto::Delete => fm_operations::OperationKind::Delete,
        OperationKindDto::Undo => fm_operations::OperationKind::Undo,
        OperationKindDto::EditImageMetadata => fm_operations::OperationKind::EditImageMetadata,
        OperationKindDto::ChangeAttributes => fm_operations::OperationKind::ChangeAttributes,
        OperationKindDto::SplitFile => fm_operations::OperationKind::SplitFile,
        OperationKindDto::CombineFiles => fm_operations::OperationKind::CombineFiles,
        OperationKindDto::TransformGeo => fm_operations::OperationKind::TransformGeo,
        // Search is handled via start_search, not the executor.
        OperationKindDto::Search => {
            unreachable!("search must be handled before calling operation_kind")
        }
        // Compare is handled via start_comparison, not the executor.
        OperationKindDto::Compare => {
            unreachable!("compare must be handled before calling operation_kind")
        }
    }
}

pub(crate) const fn conflict_policy(
    policy: OperationConflictPolicyDto,
) -> fm_operations::ConflictPolicy {
    match policy {
        OperationConflictPolicyDto::Ask => fm_operations::ConflictPolicy::Ask,
        OperationConflictPolicyDto::Skip => fm_operations::ConflictPolicy::Skip,
        OperationConflictPolicyDto::Overwrite => fm_operations::ConflictPolicy::Overwrite,
        OperationConflictPolicyDto::RenameNew => fm_operations::ConflictPolicy::RenameNew,
        OperationConflictPolicyDto::KeepNewer => fm_operations::ConflictPolicy::KeepNewer,
    }
}

/// Maps a mutating action id to the operation kind it delegates to, or
/// `None` for actions with no backing operation (unimplemented actions, and
/// the frontend-only selection/navigation actions reserved by task 0028).
pub(crate) fn mutating_operation_kind(id: &ActionId) -> Option<OperationKindDto> {
    match id.as_str() {
        "core.pack" => Some(OperationKindDto::CreateArchive),
        "core.moveToArchive" => Some(OperationKindDto::MoveToArchive),
        "core.copy" => Some(OperationKindDto::Copy),
        "core.move" => Some(OperationKindDto::Move),
        "core.rename" => Some(OperationKindDto::Rename),
        "core.trash" => Some(OperationKindDto::Trash),
        "core.delete" => Some(OperationKindDto::Delete),
        "core.createDirectory" => Some(OperationKindDto::CreateDirectory),
        "core.createFile" => Some(OperationKindDto::CreateFile),
        "core.createLink" => Some(OperationKindDto::CreateLink),
        "core.duplicate" => Some(OperationKindDto::Duplicate),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};

    use fm_archive::ArchiveFileSystemProvider;
    use fm_events::EventBus;
    use fm_operations::Scheduler;
    use fm_platform::FallbackPlatformAdapter;
    use fm_settings::Settings;
    use fm_transport_dto::{
        OperationConflictPolicyDto, OperationKindDto, StartOperationRequestDto,
    };
    use fm_vfs::ProviderRegistry;
    use fm_vfs_local::LocalFileSystemProvider;

    use super::OperationsCoordinator;
    use crate::error::ApplicationError;
    use crate::operation_history::OperationHistory;
    use crate::operation_planner::OperationPlanner;

    fn coordinator(directory: &tempfile::TempDir) -> OperationsCoordinator {
        let mut providers = ProviderRegistry::new();
        providers.register(Arc::new(LocalFileSystemProvider));
        providers.register(Arc::new(ArchiveFileSystemProvider::new()));
        let settings = Arc::new(Mutex::new(Settings::default()));
        let force_cross_volume_moves = Arc::new(AtomicBool::new(false));
        OperationsCoordinator::new(
            Scheduler::new(1, EventBus::default()),
            Arc::new(OperationHistory::load(directory.path())),
            OperationPlanner::new(
                providers,
                Arc::new(FallbackPlatformAdapter),
                settings,
                directory.path().join("audit.jsonl"),
                Arc::clone(&force_cross_volume_moves),
            ),
            force_cross_volume_moves,
        )
    }

    #[test]
    fn start_rejects_the_unimplemented_keep_newer_policy() {
        let directory = tempfile::tempdir().expect("temp directory");
        let request = StartOperationRequestDto {
            operation_type: OperationKindDto::Copy,
            sources: Vec::new(),
            destination: None,
            destinations: Vec::new(),
            conflict_policy: OperationConflictPolicyDto::KeepNewer,
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
            symlink_policy: Default::default(),
            permanent_delete_confirmed: false,
            override_read_only: false,
            link: None,
            verify_transfer: false,
        };

        let error = coordinator(&directory)
            .start(request, None)
            .expect_err("keepNewer must remain rejected");

        assert!(matches!(
            error,
            ApplicationError::InvalidRequest(_) | ApplicationError::Rejected(_)
        ));
    }
}
