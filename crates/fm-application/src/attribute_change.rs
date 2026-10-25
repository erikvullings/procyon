//! Permission, ownership and attribute editing as a cancellable operation (task 0258).
//!
//! The executor reads each entry's previous state, applies the change through the provider and
//! keeps the inverse, so the operation can be undone while the entries still hold the values it
//! set. Symbolic links are never followed or changed, and a refusal (for example `EPERM` on
//! `chown`) is a per-entry warning that does not stop the remaining entries.

use crate::rejection::{Rejection, reason};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fm_domain::{EntryKind, Location};
use fm_operations::{
    ConflictResolution, ExecutionError, ExecutionOutcome, Operation, OperationEntryError,
    OperationExecutor, OperationPlan, OperationProgressReporter, OperationUndo, PauseToken,
    PlanItem, UndoAction, UndoPlan,
};
use fm_transport_dto::{
    AttributeChangeDto, AttributeEntryDto, AttributeScopeDto, AttributeSnapshotDto,
    AttributeSupportDto, AttributeUnsupportedReasonDto, InspectAttributesRequestDto,
    InspectAttributesResponseDto, PreviewAttributeChangeRequestDto,
    PreviewAttributeChangeResponseDto, StartOperationRequestDto,
};
use fm_vfs::{
    AttributeChange, AttributeSnapshot, AttributeSupport, EDITABLE_MODE_MASK, EntryRef,
    FileSystemProvider, ListOptions, ProviderCapabilities, ProviderRegistry, VfsError,
};
use tokio_util::sync::CancellationToken;

use crate::error::ApplicationError;
use crate::image_metadata_edit::entry_of;

const MAX_SOURCES: usize = 5_000;
/// Entries a `changeAttributes` operation may touch.
const MAX_PLAN_ENTRIES: u64 = 2_000_000;
/// Entries counted by a preview before it reports a lower bound.
const MAX_PREVIEW_ENTRIES: u64 = 250_000;
const LIST_PAGE_SIZE: usize = 1_000;

fn change_from_dto(dto: AttributeChangeDto) -> AttributeChange {
    AttributeChange {
        mode: dto.mode,
        mode_mask: dto.mode_mask,
        uid: dto.uid,
        gid: dto.gid,
        read_only: dto.read_only,
        hidden: dto.hidden,
        archive: dto.archive,
    }
}

fn snapshot_to_dto(snapshot: AttributeSnapshot) -> AttributeSnapshotDto {
    AttributeSnapshotDto {
        mode: snapshot.mode,
        uid: snapshot.uid,
        gid: snapshot.gid,
        read_only: snapshot.read_only,
        hidden: snapshot.hidden,
        archive: snapshot.archive,
    }
}

const fn support_to_dto(support: AttributeSupport) -> AttributeSupportDto {
    AttributeSupportDto {
        mode: support.mode,
        ownership: support.ownership,
        windows_attributes: support.windows_attributes,
    }
}

fn scope_includes(scope: AttributeScopeDto, kind: EntryKind) -> bool {
    matches!(
        (scope, kind),
        (AttributeScopeDto::Both, _)
            | (AttributeScopeDto::FilesOnly, EntryKind::File)
            | (AttributeScopeDto::FoldersOnly, EntryKind::Directory)
    )
}

/// What a provider lets the user edit at `location`; empty when `SET_PERMISSIONS` is absent.
fn support_at(
    provider: &Arc<dyn FileSystemProvider>,
    location: &Location,
) -> Result<AttributeSupport, VfsError> {
    if !provider
        .capabilities_for(location)?
        .contains(ProviderCapabilities::SET_PERMISSIONS)
    {
        return Ok(AttributeSupport::default());
    }
    provider.attribute_support(location)
}

fn locations(
    sources: &[fm_transport_dto::LocationDto],
    what: &str,
) -> Result<Vec<Location>, ApplicationError> {
    if sources.is_empty() || sources.len() > MAX_SOURCES {
        return Err(ApplicationError::from(
            Rejection::new(
                reason::SELECTION_SIZE,
                format!("{what} needs between 1 and {MAX_SOURCES} entries"),
            )
            .with("max", MAX_SOURCES),
        ));
    }
    Ok(sources.iter().cloned().map(Into::into).collect())
}

/// Reads support and the current state of every selected entry.
pub(crate) async fn inspect_attributes(
    providers: &ProviderRegistry,
    request: InspectAttributesRequestDto,
) -> Result<InspectAttributesResponseDto, ApplicationError> {
    let sources = locations(&request.sources, "inspecting attributes")?;
    let cancellation = CancellationToken::new();
    let mut entries = Vec::with_capacity(sources.len());
    let mut common: Option<AttributeSupport> = None;
    let mut every_entry_supported = true;
    let mut any_editable = false;
    for location in sources {
        let provider = providers.resolve(&location)?;
        let entry = entry_of(&location);
        let summary = provider.inspect(&entry, cancellation.clone()).await?;
        let mut attributes = None;
        if summary.kind != EntryKind::Symlink {
            let support = support_at(&provider, &location)?;
            if support.is_none() {
                every_entry_supported = false;
            }
            any_editable = true;
            common = Some(common.map_or(support, |common| AttributeSupport {
                mode: common.mode && support.mode,
                ownership: common.ownership && support.ownership,
                windows_attributes: common.windows_attributes && support.windows_attributes,
            }));
            if !support.is_none() {
                attributes = provider
                    .read_attributes(&entry, cancellation.clone())
                    .await
                    .ok()
                    .map(snapshot_to_dto);
            }
        }
        entries.push(AttributeEntryDto {
            location: location.into(),
            kind: summary.kind.into(),
            attributes,
        });
    }
    let support = common.unwrap_or_default();
    let unsupported_reason = if !any_editable {
        Some(AttributeUnsupportedReasonDto::SymbolicLink)
    } else if support.is_none() {
        Some(if every_entry_supported {
            AttributeUnsupportedReasonDto::MixedProviders
        } else {
            AttributeUnsupportedReasonDto::ProviderUnsupported
        })
    } else {
        None
    };
    Ok(InspectAttributesResponseDto {
        support: support_to_dto(support),
        unsupported_reason,
        entries,
    })
}

#[derive(Default)]
struct Enumeration {
    /// Pre-order entries to change (children come after their folder).
    entries: Vec<EntryRef>,
    files: u64,
    folders: u64,
    skipped_symlinks: u64,
    truncated: bool,
}

/// Walks the selection without following symbolic links. A selected symlink is kept in `entries`
/// so the executor can report it; symlinks met while descending are only counted.
async fn enumerate(
    providers: &ProviderRegistry,
    sources: &[Location],
    recursive: bool,
    scope: AttributeScopeDto,
    limit: u64,
    cancellation: &CancellationToken,
) -> Result<Enumeration, ExecutionError> {
    let mut result = Enumeration::default();
    let mut pending: Vec<Location> = Vec::new();
    for source in sources.iter().rev() {
        pending.push(source.clone());
    }
    let mut roots_left = sources.len();
    while let Some(location) = pending.pop() {
        if cancellation.is_cancelled() {
            return Err(ExecutionError::Provider(VfsError::Cancelled));
        }
        let is_root = roots_left > 0 && sources.contains(&location) && {
            roots_left -= 1;
            true
        };
        let provider = providers.resolve(&location)?;
        let entry = entry_of(&location);
        let summary = provider.inspect(&entry, cancellation.clone()).await?;
        let entry = EntryRef {
            id: summary.id,
            location: summary.location.clone(),
        };
        match summary.kind {
            EntryKind::Symlink => {
                result.skipped_symlinks += 1;
                if is_root {
                    result.entries.push(entry);
                }
                continue;
            }
            kind => {
                if scope_includes(scope, kind) {
                    if result.files + result.folders >= limit {
                        result.truncated = true;
                        return Ok(result);
                    }
                    result.entries.push(entry);
                    if kind == EntryKind::Directory {
                        result.folders += 1;
                    } else {
                        result.files += 1;
                    }
                }
                if kind != EntryKind::Directory || !recursive {
                    continue;
                }
            }
        }
        let mut token = None;
        loop {
            let page = provider
                .list(
                    &location,
                    ListOptions {
                        page_size: LIST_PAGE_SIZE,
                        continuation_token: token.take(),
                    },
                    cancellation.clone(),
                )
                .await?;
            for child in page.entries {
                match child.kind {
                    EntryKind::Symlink => result.skipped_symlinks += 1,
                    EntryKind::Directory => pending.push(child.location),
                    EntryKind::File => {
                        if scope_includes(scope, EntryKind::File) {
                            if result.files + result.folders >= limit {
                                result.truncated = true;
                                return Ok(result);
                            }
                            result.files += 1;
                            result.entries.push(EntryRef {
                                id: child.id,
                                location: child.location,
                            });
                        }
                    }
                }
            }
            if !page.has_more {
                break;
            }
            token = page.continuation_token;
        }
    }
    Ok(result)
}

/// Counts what a change would touch, so the user can confirm before applying it.
pub(crate) async fn preview_attribute_change(
    providers: &ProviderRegistry,
    request: PreviewAttributeChangeRequestDto,
) -> Result<PreviewAttributeChangeResponseDto, ApplicationError> {
    let sources = locations(&request.sources, "previewing an attribute change")?;
    let found = enumerate(
        providers,
        &sources,
        request.recursive,
        request.scope,
        MAX_PREVIEW_ENTRIES,
        &CancellationToken::new(),
    )
    .await
    .map_err(execution_to_application)?;
    Ok(PreviewAttributeChangeResponseDto {
        total: found.files + found.folders,
        files: found.files,
        folders: found.folders,
        skipped_symlinks: found.skipped_symlinks,
        truncated: found.truncated,
    })
}

fn execution_to_application(error: ExecutionError) -> ApplicationError {
    match error {
        ExecutionError::Provider(error) => error.into(),
        other => ApplicationError::InvalidRequest(other.to_string()),
    }
}

/// Builds the executor for a validated `changeAttributes` request.
pub(crate) fn plan_change_attributes(
    providers: &ProviderRegistry,
    request: &StartOperationRequestDto,
) -> Result<Arc<dyn OperationExecutor>, ApplicationError> {
    let dto = request.attribute_change.ok_or_else(|| {
        ApplicationError::InvalidRequest("changeAttributes requires attributeChange".into())
    })?;
    let change = change_from_dto(dto.change);
    if change.is_empty() {
        return Err(ApplicationError::from(Rejection::new(
            reason::NO_ATTRIBUTE_SELECTED,
            "changeAttributes needs at least one attribute to change",
        )));
    }
    if change
        .mode
        .into_iter()
        .chain(change.mode_mask)
        .any(|bits| bits > EDITABLE_MODE_MASK)
    {
        return Err(ApplicationError::from(Rejection::new(
            reason::PERMISSION_BITS_RANGE,
            "permission bits must be between 0 and 0o7777",
        )));
    }
    let sources = locations(&request.sources, "changeAttributes")?;
    for source in &sources {
        let provider = providers.resolve(source)?;
        let support = support_at(&provider, source)?;
        if support.is_none() || change.exceeds(support) {
            return Err(VfsError::UnsupportedCapability {
                capability: ProviderCapabilities::SET_PERMISSIONS,
            }
            .into());
        }
    }
    Ok(Arc::new(ChangeAttributesExecutor {
        providers: providers.clone(),
        sources,
        change,
        recursive: dto.recursive,
        scope: dto.scope,
        applied: Mutex::new(Vec::new()),
    }))
}

struct AppliedChange {
    entry: EntryRef,
    restore: AttributeChange,
}

struct ChangeAttributesExecutor {
    providers: ProviderRegistry,
    sources: Vec<Location>,
    change: AttributeChange,
    recursive: bool,
    scope: AttributeScopeDto,
    applied: Mutex<Vec<AppliedChange>>,
}

/// Explains a provider refusal in terms the user can act on, with a stable code for clients.
fn describe_failure(entry: &EntryRef, name: &str, error: &VfsError) -> ExecutionOutcome {
    let (code, message) = match error {
        VfsError::PermissionDenied { .. } => (
            fm_operations::codes::entry_error::ATTRIBUTE_NOT_PERMITTED,
            format!(
                "Not permitted to change {name}: the operation was refused (only the owner or an administrator may change permissions and ownership)."
            ),
        ),
        VfsError::UnsupportedCapability { .. } => (
            fm_operations::codes::entry_error::ATTRIBUTE_UNSUPPORTED,
            format!(
                "{name} was not changed: symbolic links and unsupported entries are left alone."
            ),
        ),
        VfsError::NotFound { .. } => (
            fm_operations::codes::entry_error::ATTRIBUTE_ENTRY_MISSING,
            format!("{name} no longer exists."),
        ),
        other => (
            fm_operations::codes::entry_error::ATTRIBUTE_FAILED,
            format!("{name} was not changed: {other}."),
        ),
    };
    let mut failure = OperationEntryError::new(entry.clone(), message)
        .with_code(code)
        .with_param("name", name);
    if code == fm_operations::codes::entry_error::ATTRIBUTE_FAILED {
        failure = failure.with_param("detail", error);
    }
    ExecutionOutcome::CompletedWithWarnings(vec![failure])
}

#[async_trait]
impl OperationExecutor for ChangeAttributesExecutor {
    async fn plan(
        &self,
        _operation: &Operation,
        cancellation: &CancellationToken,
    ) -> Result<OperationPlan, ExecutionError> {
        let found = enumerate(
            &self.providers,
            &self.sources,
            self.recursive,
            self.scope,
            MAX_PLAN_ENTRIES,
            cancellation,
        )
        .await?;
        if found.truncated {
            return Err(ExecutionError::from(
                fm_operations::CodedFailure::new(
                    fm_operations::codes::failure::TOO_MANY_ENTRIES,
                    format!(
                        "too many entries (more than {MAX_PLAN_ENTRIES}); select a smaller tree"
                    ),
                )
                .with_param("limit", MAX_PLAN_ENTRIES),
            ));
        }
        // Deepest first: a folder must not lose its search permission before its children change.
        Ok(OperationPlan::new(
            found
                .entries
                .into_iter()
                .rev()
                .map(|entry| PlanItem::new(entry, 0))
                .collect(),
        ))
    }

    async fn execute(
        &self,
        _operation: &Operation,
        item: &PlanItem,
        _resolution: Option<ConflictResolution>,
        _progress: &dyn OperationProgressReporter,
        _pause: &PauseToken,
        cancellation: &CancellationToken,
    ) -> Result<ExecutionOutcome, ExecutionError> {
        let provider = self.providers.resolve(&item.entry.location)?;
        let name = item.entry.location.name().unwrap_or_default();
        let previous = match provider
            .read_attributes(&item.entry, cancellation.clone())
            .await
        {
            Ok(previous) => previous,
            Err(VfsError::Cancelled) => return Err(VfsError::Cancelled.into()),
            Err(error) => return Ok(describe_failure(&item.entry, &name, &error)),
        };
        if self.change.is_applied_to(&previous) {
            return Ok(ExecutionOutcome::Skipped);
        }
        match provider
            .change_attributes(&item.entry, self.change, cancellation.clone())
            .await
        {
            Ok(()) => {
                self.applied
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .push(AppliedChange {
                        entry: item.entry.clone(),
                        restore: self.change.inverse_from(&previous),
                    });
                Ok(ExecutionOutcome::Completed)
            }
            Err(VfsError::Cancelled) => Err(VfsError::Cancelled.into()),
            Err(error) => Ok(describe_failure(&item.entry, &name, &error)),
        }
    }

    async fn cleanup_partial(&self, _operation: &Operation) -> Result<(), ExecutionError> {
        Ok(())
    }

    async fn undo_evidence(
        &self,
        _operation: &Operation,
        _cancellation: &CancellationToken,
    ) -> Result<OperationUndo, ExecutionError> {
        let applied = self
            .applied
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if applied.is_empty() {
            return Ok(OperationUndo::unavailable(
                fm_operations::codes::undo::NOTHING_TO_UNDO,
                "No entry was changed.",
            ));
        }
        let actions = applied
            .iter()
            .rev()
            .map(|record| UndoAction::RestoreAttributes {
                entry: record.entry.clone(),
                restore: record.restore,
                applied: self.change,
            })
            .collect();
        Ok(OperationUndo::available(UndoPlan { actions }))
    }
}

/// Puts previous attributes back, but only while the entry still holds the values set earlier.
pub(crate) async fn restore_attributes(
    providers: &ProviderRegistry,
    entry: &EntryRef,
    restore: AttributeChange,
    applied: AttributeChange,
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    let provider = providers.resolve(&entry.location)?;
    let current = provider
        .read_attributes(entry, cancellation.clone())
        .await
        .map_err(|error| match error {
            VfsError::NotFound { .. } => ExecutionError::from(fm_operations::CodedFailure::new(
                fm_operations::codes::failure::UNDO_ENTRY_MISSING,
                "The entry is missing, so its attributes cannot be restored.",
            )),
            error => error.into(),
        })?;
    // Only fields this entry's own restore touches are compared.
    let expected = AttributeChange {
        mode: restore.mode.and(applied.mode),
        mode_mask: applied.mode_mask,
        uid: restore.uid.and(applied.uid),
        gid: restore.gid.and(applied.gid),
        read_only: restore.read_only.and(applied.read_only),
        hidden: restore.hidden.and(applied.hidden),
        archive: restore.archive.and(applied.archive),
    };
    if !expected.is_applied_to(&current) {
        return Err(ExecutionError::from(fm_operations::CodedFailure::new(
            fm_operations::codes::failure::UNDO_ATTRIBUTES_CHANGED,
            "The attributes changed after the operation, so they cannot be restored safely.",
        )));
    }
    provider
        .change_attributes(entry, restore, cancellation.clone())
        .await?;
    Ok(())
}
