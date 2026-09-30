//! Link creation (task 0168): symbolic links, NTFS junctions and Windows `.lnk` shortcuts.
//!
//! The three kinds stay distinct end to end. Symbolic links and junctions are filesystem links
//! created through the VFS provider (`CREATE_SYMLINK` / `CREATE_JUNCTION`), while a shortcut is
//! an ordinary file written by the platform shell adapter (`CREATE_SHORTCUT`). Every kind runs
//! as a single-item operation job so it inherits conflict handling, audit history and undo.

use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fm_domain::{ActionId, EntryId, EntryKind, Location};
use fm_operations::{
    ConflictResolution, EntryFingerprint, ExecutionError, ExecutionOutcome, Operation,
    OperationExecutor, OperationPlan, OperationProgressReporter, OperationUndo, PauseToken,
    PlanItem, UndoAction, UndoPlan,
};
use fm_platform::{PlatformAdapter, PlatformCapabilities, PlatformError};
use fm_transport_dto::{
    LinkKindDto, LinkKindOptionDto, LinkOptionsDto, LinkRequestDto, LinkRequirementDto,
    LinkTargetStyleDto, StartOperationRequestDto,
};
use fm_vfs::{
    EntryRef, FileSystemProvider, ProviderCapabilities, ProviderRegistry, RemoveOptions, VfsError,
};
use tokio_util::sync::CancellationToken;

use crate::error::ApplicationError;
use crate::operation_planner::{conflict_error, copy_name, effective_resolution, fingerprint};

const SHORTCUT_EXTENSION: &str = ".lnk";
const CREATE_LINK_ACTION: &str = "core.createLink";

/// Builds the executor for a validated `createLink` request.
pub(crate) fn plan_create_link(
    providers: &ProviderRegistry,
    platform: &Arc<dyn PlatformAdapter>,
    request: &StartOperationRequestDto,
) -> Result<Arc<dyn OperationExecutor>, ApplicationError> {
    let link = request.link.ok_or_else(|| {
        ApplicationError::InvalidRequest("createLink requires link options".into())
    })?;
    let [target] = request.sources.as_slice() else {
        return Err(ApplicationError::InvalidRequest(
            "createLink requires exactly one target".into(),
        ));
    };
    let target: Location = target.clone().into();
    let parent: Location = request
        .destination
        .clone()
        .ok_or_else(|| {
            ApplicationError::InvalidRequest("createLink requires a destination directory".into())
        })?
        .into();
    let mut name = request
        .name
        .clone()
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| ApplicationError::InvalidRequest("createLink requires a name".into()))?;
    if target.provider_id != parent.provider_id {
        return Err(ApplicationError::InvalidRequest(
            "a link and its target must use the same provider".into(),
        ));
    }
    // Validates the name as one path component before any filesystem work.
    parent
        .join(&name)
        .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;
    let provider = providers.resolve(&parent).map_err(ApplicationError::from)?;
    let capabilities = provider
        .capabilities_for(&parent)
        .map_err(ApplicationError::from)?;
    match link.kind {
        LinkKindDto::SymbolicLink => capabilities
            .require(ProviderCapabilities::CREATE_SYMLINK)
            .map_err(ApplicationError::from)?,
        LinkKindDto::Junction => capabilities
            .require(ProviderCapabilities::CREATE_JUNCTION)
            .map_err(ApplicationError::from)?,
        LinkKindDto::Shortcut => {
            capabilities
                .require(ProviderCapabilities::WRITE)
                .map_err(ApplicationError::from)?;
            if !shortcuts_supported(platform, &parent) {
                return Err(ApplicationError::ActionUnavailable(ActionId::new(
                    CREATE_LINK_ACTION,
                )));
            }
            if !has_shortcut_extension(&name) {
                name.push_str(SHORTCUT_EXTENSION);
            }
        }
    }
    Ok(Arc::new(CreateLinkExecutor {
        provider,
        platform: Arc::clone(platform),
        target,
        parent,
        name,
        link,
        target_summary: Mutex::new(None),
        created: Mutex::new(None),
        replaced_existing: Mutex::new(false),
    }))
}

/// Lists the link kinds that can be created for `target` inside `destination`.
pub(crate) async fn link_options(
    providers: &ProviderRegistry,
    platform: &Arc<dyn PlatformAdapter>,
    target: &Location,
    destination: &Location,
) -> Result<LinkOptionsDto, ApplicationError> {
    if target.provider_id != destination.provider_id {
        return Ok(LinkOptionsDto { kinds: Vec::new() });
    }
    let provider = providers
        .resolve(destination)
        .map_err(ApplicationError::from)?;
    let capabilities = provider
        .capabilities_for(destination)
        .map_err(ApplicationError::from)?;
    let summary = provider
        .inspect(
            &EntryRef {
                id: EntryId::new(),
                location: target.clone(),
            },
            CancellationToken::new(),
        )
        .await
        .map_err(ApplicationError::from)?;
    let target_kind = followed_kind(provider.as_ref(), &summary, &CancellationToken::new()).await;
    let base_name = target
        .name()
        .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;
    let mut kinds = Vec::new();
    if capabilities.contains(ProviderCapabilities::CREATE_SYMLINK) {
        kinds.push(LinkKindOptionDto {
            kind: LinkKindDto::SymbolicLink,
            supports_relative: true,
            requirements: if cfg!(windows) {
                vec![LinkRequirementDto::DeveloperModeOrAdministrator]
            } else {
                Vec::new()
            },
            suggested_name: base_name.clone(),
        });
    }
    if capabilities.contains(ProviderCapabilities::CREATE_JUNCTION)
        && target_kind == EntryKind::Directory
    {
        kinds.push(LinkKindOptionDto {
            kind: LinkKindDto::Junction,
            supports_relative: false,
            requirements: vec![LinkRequirementDto::LocalDirectoryTarget],
            suggested_name: base_name.clone(),
        });
    }
    if capabilities.contains(ProviderCapabilities::WRITE)
        && shortcuts_supported(platform, destination)
    {
        kinds.push(LinkKindOptionDto {
            kind: LinkKindDto::Shortcut,
            supports_relative: false,
            requirements: vec![LinkRequirementDto::ShellOnly],
            suggested_name: format!("{base_name}{SHORTCUT_EXTENSION}"),
        });
    }
    Ok(LinkOptionsDto { kinds })
}

fn shortcuts_supported(platform: &Arc<dyn PlatformAdapter>, location: &Location) -> bool {
    platform
        .capabilities()
        .contains(PlatformCapabilities::CREATE_SHORTCUT)
        && location.to_native_path().is_ok()
}

fn has_shortcut_extension(name: &str) -> bool {
    name.len() >= SHORTCUT_EXTENSION.len()
        && name.is_char_boundary(name.len() - SHORTCUT_EXTENSION.len())
        && name[name.len() - SHORTCUT_EXTENSION.len()..].eq_ignore_ascii_case(SHORTCUT_EXTENSION)
}

/// The planned target: its stable identity and the kind it resolves to.
#[derive(Debug, Clone)]
struct TargetSummary {
    summary: fm_domain::EntrySummary,
    kind: EntryKind,
}

/// The kind a link to `summary` should treat it as. Windows directory links must match their
/// target, so a directory reached through a symlink, junction or other reparse point counts as a
/// directory; an unresolvable (dangling) link keeps its own kind.
async fn followed_kind(
    provider: &dyn FileSystemProvider,
    summary: &fm_domain::EntrySummary,
    cancellation: &CancellationToken,
) -> EntryKind {
    if summary.kind != EntryKind::Symlink {
        return summary.kind;
    }
    let entry = EntryRef {
        id: summary.id,
        location: summary.location.clone(),
    };
    match provider.resolve_symlink(&entry, cancellation.clone()).await {
        Ok(resolved) if resolved.kind != EntryKind::Symlink => resolved.kind,
        _ => summary.kind,
    }
}

struct CreateLinkExecutor {
    provider: Arc<dyn FileSystemProvider>,
    platform: Arc<dyn PlatformAdapter>,
    target: Location,
    parent: Location,
    name: String,
    link: LinkRequestDto,
    target_summary: Mutex<Option<TargetSummary>>,
    created: Mutex<Option<EntryFingerprint>>,
    replaced_existing: Mutex<bool>,
}

impl CreateLinkExecutor {
    async fn inspect(
        &self,
        location: &Location,
        cancellation: &CancellationToken,
    ) -> Result<Option<fm_domain::EntrySummary>, ExecutionError> {
        match self
            .provider
            .inspect(
                &EntryRef {
                    id: EntryId::new(),
                    location: location.clone(),
                },
                cancellation.clone(),
            )
            .await
        {
            Ok(summary) => Ok(Some(summary)),
            Err(VfsError::NotFound { .. }) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn location_for(&self, name: &str) -> Result<Location, ExecutionError> {
        self.parent
            .join(name)
            .map_err(|error| ExecutionError::Failed(error.to_string()))
    }

    async fn create(
        &self,
        link: &Location,
        target_kind: EntryKind,
        cancellation: &CancellationToken,
    ) -> Result<(), ExecutionError> {
        match self.link.kind {
            LinkKindDto::SymbolicLink => {
                let text = self.symlink_text(link)?;
                self.provider
                    .create_symlink(
                        link,
                        &text,
                        target_kind == EntryKind::Directory,
                        cancellation.clone(),
                    )
                    .await?;
            }
            LinkKindDto::Junction => {
                self.provider
                    .create_junction(link, &self.target, cancellation.clone())
                    .await?;
            }
            LinkKindDto::Shortcut => {
                let shortcut = native_path(link)?;
                let target = native_path(&self.target)?;
                let platform = Arc::clone(&self.platform);
                let location = link.uri.clone();
                tokio::task::spawn_blocking(move || platform.create_shortcut(&shortcut, &target))
                    .await
                    .map_err(|error| ExecutionError::Failed(error.to_string()))?
                    .map_err(|error| shortcut_error(error, location))?;
            }
        }
        Ok(())
    }

    fn symlink_text(&self, link: &Location) -> Result<String, ExecutionError> {
        let target = native_path(&self.target)?;
        let text = match self.link.target_style {
            LinkTargetStyleDto::Absolute => target,
            LinkTargetStyleDto::Relative => {
                let link_path = native_path(link)?;
                let from = link_path.parent().ok_or_else(|| {
                    ExecutionError::Failed("the link has no parent directory".into())
                })?;
                relative_path(from, &target).ok_or_else(|| {
                    ExecutionError::Failed(
                        "a relative target is not possible across volumes; choose an absolute target"
                            .into(),
                    )
                })?
            }
        };
        text.into_os_string()
            .into_string()
            .map_err(|_| ExecutionError::Failed("the link target is not valid Unicode".into()))
    }
}

#[async_trait]
impl OperationExecutor for CreateLinkExecutor {
    async fn plan(
        &self,
        _operation: &Operation,
        cancellation: &CancellationToken,
    ) -> Result<OperationPlan, ExecutionError> {
        let summary = self
            .inspect(&self.target, cancellation)
            .await?
            .ok_or_else(|| VfsError::NotFound {
                location: self.target.uri.clone(),
            })?;
        let target_kind = followed_kind(self.provider.as_ref(), &summary, cancellation).await;
        if self.link.kind == LinkKindDto::Junction && target_kind != EntryKind::Directory {
            return Err(VfsError::NotADirectory {
                location: self.target.uri.clone(),
            }
            .into());
        }
        let link = self.location_for(&self.name)?;
        if creates_cycle(&link, &self.target, target_kind) {
            return Err(VfsError::LinkCycle { location: link.uri }.into());
        }
        let entry = EntryRef {
            id: summary.id,
            location: summary.location.clone(),
        };
        *self
            .target_summary
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(TargetSummary {
            summary,
            kind: target_kind,
        });
        Ok(OperationPlan::new(vec![PlanItem::new(entry, 0)]))
    }

    async fn execute(
        &self,
        operation: &Operation,
        _item: &PlanItem,
        resolution: Option<ConflictResolution>,
        _progress: &dyn OperationProgressReporter,
        _pause: &PauseToken,
        cancellation: &CancellationToken,
    ) -> Result<ExecutionOutcome, ExecutionError> {
        let target = self
            .target_summary
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
            .ok_or_else(|| ExecutionError::Failed("the link target was not planned".into()))?;
        let mut link = self.location_for(&self.name)?;
        if let Some(existing) = self.inspect(&link, cancellation).await? {
            // A case-insensitive filesystem, or a parent reached through a link, can make the
            // link name resolve to the target itself; replacing it would destroy the target.
            if existing.id == target.summary.id {
                return Err(VfsError::LinkCycle { location: link.uri }.into());
            }
            match effective_resolution(operation.conflict_policy, resolution) {
                None => return Err(conflict_error(&target.summary, &existing)),
                Some(ConflictResolution::Skip) => return Ok(ExecutionOutcome::Skipped),
                Some(ConflictResolution::Overwrite) => {
                    if existing.kind == EntryKind::Directory {
                        return Err(VfsError::IsADirectory { location: link.uri }.into());
                    }
                    self.provider
                        .remove(
                            &EntryRef {
                                id: existing.id,
                                location: existing.location,
                            },
                            RemoveOptions {
                                recursive: false,
                                use_trash: false,
                            },
                            cancellation.clone(),
                        )
                        .await?;
                    *self
                        .replaced_existing
                        .lock()
                        .unwrap_or_else(|error| error.into_inner()) = true;
                }
                Some(ConflictResolution::RenameNew) => {
                    link = self.next_free_location(cancellation).await?;
                }
            }
        }
        if cancellation.is_cancelled() {
            return Err(VfsError::Cancelled.into());
        }
        self.create(&link, target.kind, cancellation).await?;
        *self
            .created
            .lock()
            .unwrap_or_else(|error| error.into_inner()) =
            Some(fingerprint(&self.provider, &link, cancellation).await?);
        Ok(ExecutionOutcome::Completed)
    }

    async fn cleanup_partial(&self, _operation: &Operation) -> Result<(), ExecutionError> {
        Ok(())
    }

    async fn undo_evidence(
        &self,
        _operation: &Operation,
        _cancellation: &CancellationToken,
    ) -> Result<OperationUndo, ExecutionError> {
        if *self
            .replaced_existing
            .lock()
            .unwrap_or_else(|error| error.into_inner())
        {
            return Ok(OperationUndo::unavailable(
                "The link replaced an existing entry, which cannot be restored.",
            ));
        }
        let created = self
            .created
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        Ok(match created {
            Some(entry) => OperationUndo::available(UndoPlan {
                actions: vec![UndoAction::RemoveCreated {
                    entries: vec![entry],
                }],
            }),
            None => OperationUndo::unavailable("No link was created, so there is nothing to undo."),
        })
    }
}

impl CreateLinkExecutor {
    async fn next_free_location(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<Location, ExecutionError> {
        for index in 1..=u32::from(u16::MAX) {
            let candidate = copy_name(&self.name, index);
            let location = self.location_for(&candidate)?;
            if self.inspect(&location, cancellation).await?.is_none() {
                return Ok(location);
            }
        }
        Err(ExecutionError::Failed(
            "no free name is available for the link".into(),
        ))
    }
}

fn native_path(location: &Location) -> Result<PathBuf, ExecutionError> {
    location
        .to_native_path()
        .map_err(|error| ExecutionError::Failed(error.to_string()))
}

/// A link that is the target itself, or that sits inside the directory it points at, makes a
/// recursive walk loop forever.
fn creates_cycle(link: &Location, target: &Location, target_kind: EntryKind) -> bool {
    if link == target {
        return true;
    }
    if target_kind != EntryKind::Directory {
        return false;
    }
    match (link.to_native_path(), target.to_native_path()) {
        (Ok(link), Ok(target)) => link.starts_with(target),
        _ => link
            .uri
            .starts_with(&format!("{}/", target.uri.trim_end_matches('/'))),
    }
}

/// Lexical relative path from directory `from` to `target`; `None` across roots or volumes.
fn relative_path(from: &Path, target: &Path) -> Option<PathBuf> {
    let from: Vec<Component<'_>> = from.components().collect();
    let to: Vec<Component<'_>> = target.components().collect();
    let common = from
        .iter()
        .zip(&to)
        .take_while(|(left, right)| left == right)
        .count();
    let rooted = |components: &[Component<'_>]| {
        components
            .iter()
            .any(|component| matches!(component, Component::RootDir | Component::Prefix(_)))
    };
    if rooted(&from) != rooted(&to) || (rooted(&from) && !rooted(&from[..common])) {
        return None;
    }
    let mut relative = PathBuf::new();
    for _ in common..from.len() {
        relative.push("..");
    }
    for component in &to[common..] {
        relative.push(component.as_os_str());
    }
    if relative.as_os_str().is_empty() {
        relative.push(".");
    }
    Some(relative)
}

fn shortcut_error(error: PlatformError, location: String) -> ExecutionError {
    match error {
        PlatformError::AlreadyExists { .. } => VfsError::AlreadyExists { location }.into(),
        PlatformError::NotFound { .. } => VfsError::NotFound { location }.into(),
        PlatformError::Unsupported { .. } => VfsError::UnsupportedCapability {
            capability: ProviderCapabilities::WRITE,
        }
        .into(),
        other => VfsError::Io {
            message: other.to_string(),
        }
        .into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths_walk_up_to_the_common_ancestor() {
        assert_eq!(
            relative_path(Path::new("/a/b/links"), Path::new("/a/c/doel ✓.txt")),
            Some(PathBuf::from("../../c/doel ✓.txt"))
        );
        assert_eq!(
            relative_path(Path::new("/a/b"), Path::new("/a/b/file")),
            Some(PathBuf::from("file"))
        );
        assert_eq!(
            relative_path(Path::new("/a/b"), Path::new("/a/b")),
            Some(PathBuf::from("."))
        );
        assert_eq!(
            relative_path(Path::new("/a/b"), Path::new("/")),
            Some(PathBuf::from("../.."))
        );
    }

    #[cfg(windows)]
    #[test]
    fn relative_paths_are_impossible_across_drives() {
        assert_eq!(relative_path(Path::new(r"C:\a"), Path::new(r"D:\b")), None);
    }

    #[test]
    fn shortcut_extension_is_detected_case_insensitively() {
        assert!(has_shortcut_extension("Report.LNK"));
        assert!(has_shortcut_extension("a.lnk"));
        assert!(!has_shortcut_extension("lnk"));
        assert!(!has_shortcut_extension("ünï"));
    }
}
