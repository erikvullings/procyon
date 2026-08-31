//! Mutation operation coordination (task 0119).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use fm_domain::{EntryId, OperationId};
use fm_operations::{ConflictResolution, Operation, Scheduler, SchedulerError};
use fm_transport_dto::{
    ConflictResolutionDto, OperationConflictPolicyDto, OperationDto, OperationPageDto,
    ResolveOperationConflictRequestDto, StartOperationRequestDto,
};
use fm_vfs::EntryRef;

use crate::error::ApplicationError;
use crate::operation_history::{OperationHistory, operation_dto};
use crate::operation_planner::OperationPlanner;
use crate::operation_requests::{conflict_policy, map_scheduler_error, operation_kind};

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
            return Err(ApplicationError::InvalidRequest(
                "keepNewer conflict policy is not supported yet; choose ask, skip, overwrite, or renameNew".into(),
            ));
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
        let destination = request.destination.clone().map(Into::into);
        let executor = self.planner.plan(request.operation_type, &request)?;
        let sources = request
            .sources
            .into_iter()
            .map(|location| EntryRef {
                id: EntryId::new(),
                location: location.into(),
            })
            .collect();
        let operation = Operation::new(
            operation_kind(request.operation_type),
            sources,
            destination,
            conflict_policy(request.conflict_policy),
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
        self.scheduler
            .get(id)
            .map(|operation| operation_dto(operation, None))
            .map_err(map_scheduler_error)
            .or_else(|error| self.history.get(id).ok_or(error))
    }

    pub(crate) fn cancel(&self, id: OperationId) -> Result<(), SchedulerError> {
        self.scheduler.cancel(id)
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
            symlink_policy: Default::default(),
            permanent_delete_confirmed: false,
            override_read_only: false,
        };

        let error = coordinator(&directory)
            .start(request, None)
            .expect_err("keepNewer must remain rejected");

        assert!(matches!(error, ApplicationError::InvalidRequest(_)));
    }
}
