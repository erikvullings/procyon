//! Translates wire-level requests and action ids into the operations engine's own request/kind
//! types: scheduler error mapping, sync-plan-row request builders, and the action-id-to-operation-
//! kind lookups `start_operation`/`invoke_action` need.
//!
//! Split out of the `FileManagerService` facade (task 0119) — a self-contained cluster of pure
//! functions with no dependency on the rest of the facade.

use fm_domain::{ActionId, Location};
use fm_operations::SchedulerError;
use fm_transport_dto::{OperationConflictPolicyDto, OperationKindDto, StartOperationRequestDto};

use crate::error::ApplicationError;

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
        // Sync intentionally replaces a differing/older destination.
        conflict_policy: OperationConflictPolicyDto::Overwrite,
        name: None,
        archive_format: None,
        archive_compression_level: None,
        create_intermediate_directories: false,
        symlink_policy: fm_transport_dto::SymlinkPolicyDto::default(),
        permanent_delete_confirmed: false,
        override_read_only: false,
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
        symlink_policy: fm_transport_dto::SymlinkPolicyDto::default(),
        permanent_delete_confirmed: true,
        override_read_only: false,
    })
}

pub(crate) fn operation_kind(kind: OperationKindDto) -> fm_operations::OperationKind {
    match kind {
        OperationKindDto::CreateArchive => fm_operations::OperationKind::CreateArchive,
        OperationKindDto::MoveToArchive => fm_operations::OperationKind::MoveToArchive,
        OperationKindDto::CreateDirectory => fm_operations::OperationKind::CreateDirectory,
        OperationKindDto::CreateFile => fm_operations::OperationKind::CreateFile,
        OperationKindDto::Rename => fm_operations::OperationKind::Rename,
        OperationKindDto::Copy => fm_operations::OperationKind::Copy,
        OperationKindDto::Move => fm_operations::OperationKind::Move,
        OperationKindDto::Duplicate => fm_operations::OperationKind::Duplicate,
        OperationKindDto::Trash => fm_operations::OperationKind::Trash,
        OperationKindDto::Delete => fm_operations::OperationKind::Delete,
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
        "core.duplicate" => Some(OperationKindDto::Duplicate),
        _ => None,
    }
}
