//! Operation planning: resolve providers, check capabilities, construct executors.
//!
//! Concentrates all provider resolution and executor construction logic that was
//! previously inline in `FileManagerService::start_operation()`. The planner is
//! stateless per-call and testable without bootstrapping the full service.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fm_archive::{create_7z_archive, create_zip_archive};
use fm_domain::{EntryId, EntryKind, Location, ProviderId};
use fm_operations::{
    ConflictResolution, EntryFingerprint, ExecutionError, ExecutionOutcome, Operation,
    OperationExecutor, OperationPlan, OperationProgressReporter, OperationUndo, PauseToken,
    PlanItem, UndoAction, UndoPlan,
};
use fm_platform::{PlatformAdapter, PlatformCapabilities};
use fm_settings::Settings;
use fm_transport_dto::{
    ArchiveFormatDto, OperationKindDto, StartOperationRequestDto, SymlinkPolicyDto,
};
use fm_vfs::{
    CopyCommitOptions, EntryRef, FileSystemProvider, ListOptions, ProviderCapabilities,
    ProviderRegistry, RemoveOptions, TransferCapabilities, WriteOptions,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::error::ApplicationError;
use crate::folder_size::calculate_directory_size;

pub(crate) struct OperationPlanner {
    providers: ProviderRegistry,
    platform: Arc<dyn PlatformAdapter>,
    settings: Arc<Mutex<Settings>>,
    audit_log_path: PathBuf,
    force_cross_volume_moves: Arc<AtomicBool>,
}

impl OperationPlanner {
    pub(crate) fn new(
        providers: ProviderRegistry,
        platform: Arc<dyn PlatformAdapter>,
        settings: Arc<Mutex<Settings>>,
        audit_log_path: PathBuf,
        force_cross_volume_moves: Arc<AtomicBool>,
    ) -> Self {
        Self {
            providers,
            platform,
            settings,
            audit_log_path,
            force_cross_volume_moves,
        }
    }

    pub(crate) fn plan(
        &self,
        kind: OperationKindDto,
        request: &StartOperationRequestDto,
    ) -> Result<Arc<dyn OperationExecutor>, ApplicationError> {
        let destination = request.destination.clone().map(Into::into);
        Ok(match kind {
            OperationKindDto::CreateArchive | OperationKindDto::MoveToArchive => {
                if request.sources.is_empty() {
                    return Err(ApplicationError::InvalidRequest(
                        "createArchive requires at least one source".into(),
                    ));
                }
                let destination: Location = destination.clone().ok_or_else(|| {
                    ApplicationError::InvalidRequest(
                        "createArchive requires an archive destination".into(),
                    )
                })?;
                let destination_path = destination
                    .to_native_path()
                    .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;
                let format =
                    ArchiveCreationFormat::from_request(&destination_path, request.archive_format)?;
                let compression_level = match format {
                    ArchiveCreationFormat::Zip => {
                        let level = request.archive_compression_level.unwrap_or(6);
                        if !(0..=9).contains(&level) {
                            return Err(ApplicationError::InvalidRequest(
                                "archive compression level must be between 0 and 9".into(),
                            ));
                        }
                        level
                    }
                    ArchiveCreationFormat::SevenZip
                        if request.archive_compression_level.is_some() =>
                    {
                        return Err(ApplicationError::InvalidRequest(
                            "7z compression level is not supported by this backend".into(),
                        ));
                    }
                    ArchiveCreationFormat::SevenZip => 6,
                };
                let sources = request
                    .sources
                    .iter()
                    .map(|source| {
                        let source: Location = source.clone().into();
                        source
                            .to_native_path()
                            .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Arc::new(CreateArchiveExecutor {
                    destination: destination_path,
                    sources,
                    format,
                    compression_level,
                    remove_sources: kind == OperationKindDto::MoveToArchive,
                })
            }
            OperationKindDto::CreateDirectory => {
                let parent = destination.clone().ok_or_else(|| {
                    ApplicationError::InvalidRequest(
                        "createDirectory requires a destination directory".into(),
                    )
                })?;
                let name = request.name.clone().ok_or_else(|| {
                    ApplicationError::InvalidRequest("createDirectory requires a name".into())
                })?;
                let provider = self
                    .providers
                    .resolve(&parent)
                    .map_err(ApplicationError::from)?;
                provider
                    .capabilities_for(&parent)
                    .map_err(ApplicationError::from)?
                    .require(ProviderCapabilities::CREATE_DIRECTORY)
                    .map_err(ApplicationError::from)?;
                Arc::new(CreateDirectoryExecutor {
                    provider,
                    parent,
                    name,
                    create_intermediates: request.create_intermediate_directories,
                })
            }
            OperationKindDto::CreateFile => {
                let parent = destination.clone().ok_or_else(|| {
                    ApplicationError::InvalidRequest(
                        "createFile requires a destination directory".into(),
                    )
                })?;
                let name = request.name.clone().ok_or_else(|| {
                    ApplicationError::InvalidRequest("createFile requires a name".into())
                })?;
                let provider = self
                    .providers
                    .resolve(&parent)
                    .map_err(ApplicationError::from)?;
                // Reuses the WRITE capability rather than a dedicated CREATE_FILE bit: creating an
                // empty file is just opening a writer and immediately shutting it down with no
                // bytes written, so every provider that can write a file can already do this.
                provider
                    .capabilities_for(&parent)
                    .map_err(ApplicationError::from)?
                    .require(ProviderCapabilities::WRITE)
                    .map_err(ApplicationError::from)?;
                Arc::new(CreateFileExecutor {
                    provider,
                    parent,
                    name,
                })
            }
            OperationKindDto::Rename if request.destinations.is_empty() => {
                if request.sources.len() != 1 {
                    return Err(ApplicationError::InvalidRequest(
                        "rename requires exactly one source, or a destinations entry per source"
                            .into(),
                    ));
                }
                let destination = destination.clone().ok_or_else(|| {
                    ApplicationError::InvalidRequest("rename requires a destination".into())
                })?;
                let source: Location = request.sources[0].clone().into();
                let provider = self
                    .providers
                    .resolve(&source)
                    .map_err(ApplicationError::from)?;
                if source.provider_id != destination.provider_id {
                    return Err(ApplicationError::InvalidRequest(
                        "rename cannot cross providers".into(),
                    ));
                }
                provider
                    .capabilities_for(&source)
                    .map_err(ApplicationError::from)?
                    .require(ProviderCapabilities::RENAME)
                    .map_err(ApplicationError::from)?;
                Arc::new(RenameExecutor {
                    provider,
                    source,
                    destination,
                    source_fingerprint: Mutex::new(None),
                    destination_fingerprint: Mutex::new(None),
                })
            }
            OperationKindDto::Rename => {
                if request.sources.is_empty() {
                    return Err(ApplicationError::InvalidRequest(
                        "rename requires at least one source".into(),
                    ));
                }
                if request.destinations.len() != request.sources.len() {
                    return Err(ApplicationError::InvalidRequest(
                        "rename destinations must include exactly one entry per source".into(),
                    ));
                }
                let mut renames = Vec::with_capacity(request.sources.len());
                for (source_dto, destination_dto) in
                    request.sources.iter().zip(request.destinations.iter())
                {
                    let source: Location = source_dto.clone().into();
                    let destination: Location = destination_dto.clone().into();
                    let provider = self
                        .providers
                        .resolve(&source)
                        .map_err(ApplicationError::from)?;
                    if source.provider_id != destination.provider_id {
                        return Err(ApplicationError::InvalidRequest(
                            "rename cannot cross providers".into(),
                        ));
                    }
                    provider
                        .capabilities_for(&source)
                        .map_err(ApplicationError::from)?
                        .require(ProviderCapabilities::RENAME)
                        .map_err(ApplicationError::from)?;
                    renames.push(RenameExecutor {
                        provider,
                        source,
                        destination,
                        source_fingerprint: Mutex::new(None),
                        destination_fingerprint: Mutex::new(None),
                    });
                }
                Arc::new(RenameGroupExecutor { renames })
            }
            OperationKindDto::Copy => {
                if request.sources.is_empty() {
                    return Err(ApplicationError::InvalidRequest(
                        "copy requires at least one source".into(),
                    ));
                }
                let destination_directory = destination.clone().ok_or_else(|| {
                    ApplicationError::InvalidRequest("copy requires a destination directory".into())
                })?;
                let destination_provider = self
                    .providers
                    .resolve(&destination_directory)
                    .map_err(ApplicationError::from)?;
                destination_provider
                    .capabilities_for(&destination_directory)
                    .map_err(ApplicationError::from)?
                    .require(ProviderCapabilities::WRITE)
                    .map_err(ApplicationError::from)?;
                let mut copies = Vec::new();
                for source_dto in &request.sources {
                    let source: Location = source_dto.clone().into();
                    let source_provider = self
                        .providers
                        .resolve(&source)
                        .map_err(ApplicationError::from)?;
                    source_provider
                        .capabilities_for(&source)
                        .map_err(ApplicationError::from)?
                        .require(ProviderCapabilities::READ)
                        .map_err(ApplicationError::from)?;
                    let transfer = TransferPlan::resolve(
                        &source_provider,
                        &source,
                        &destination_provider,
                        &destination_directory,
                    )?;
                    copies.push(CopyExecutor {
                        source_provider,
                        destination_provider: Arc::clone(&destination_provider),
                        destination_directory: destination_directory.clone(),
                        temporary: Mutex::new(None),
                        planned: Mutex::new(HashMap::new()),
                        directories: Mutex::new(Vec::new()),
                        symlink_policy: request.symlink_policy,
                        root_name: Mutex::new(None),
                        source_override: Some(source),
                        continue_on_error: true,
                        completed_root_destination: Mutex::new(None),
                        created_destinations: Mutex::new(Vec::new()),
                        replaced_existing: AtomicBool::new(false),
                        transfer,
                    });
                }
                Arc::new(CopyGroupExecutor {
                    copies,
                    stale_sources: Mutex::new(HashMap::new()),
                })
            }
            OperationKindDto::Move => {
                if request.sources.is_empty() {
                    return Err(ApplicationError::InvalidRequest(
                        "move requires at least one source".into(),
                    ));
                }
                let destination_directory = destination.clone().ok_or_else(|| {
                    ApplicationError::InvalidRequest("move requires a destination directory".into())
                })?;
                let destination_provider = self
                    .providers
                    .resolve(&destination_directory)
                    .map_err(ApplicationError::from)?;
                let mut moves = Vec::new();
                for source_dto in &request.sources {
                    let source: Location = source_dto.clone().into();
                    let source_provider = self
                        .providers
                        .resolve(&source)
                        .map_err(ApplicationError::from)?;
                    let transfer = TransferPlan::resolve(
                        &source_provider,
                        &source,
                        &destination_provider,
                        &destination_directory,
                    )?;
                    let copy = CopyExecutor {
                        source_provider: Arc::clone(&source_provider),
                        destination_provider: Arc::clone(&destination_provider),
                        destination_directory: destination_directory.clone(),
                        temporary: Mutex::new(None),
                        planned: Mutex::new(HashMap::new()),
                        directories: Mutex::new(Vec::new()),
                        symlink_policy: request.symlink_policy,
                        root_name: Mutex::new(None),
                        source_override: Some(source.clone()),
                        continue_on_error: false,
                        completed_root_destination: Mutex::new(None),
                        created_destinations: Mutex::new(Vec::new()),
                        replaced_existing: AtomicBool::new(false),
                        transfer,
                    };
                    moves.push(MoveExecutor {
                        source,
                        source_provider,
                        destination_provider: Arc::clone(&destination_provider),
                        destination_directory: destination_directory.clone(),
                        copy,
                        fallback: Mutex::new(false),
                        force_fallback: self.force_cross_volume_moves.load(Ordering::Relaxed),
                        transfer,
                        source_fingerprint: Mutex::new(None),
                        destination_fingerprint: Mutex::new(None),
                    });
                }
                Arc::new(MoveGroupExecutor {
                    moves,
                    stale_sources: Mutex::new(HashMap::new()),
                })
            }
            OperationKindDto::Duplicate => {
                if request.sources.is_empty() {
                    return Err(ApplicationError::InvalidRequest(
                        "duplicate requires at least one source".into(),
                    ));
                }
                let mut copies = Vec::new();
                for source_dto in &request.sources {
                    let source: Location = source_dto.clone().into();
                    let parent = source
                        .parent()
                        .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?
                        .ok_or_else(|| {
                            ApplicationError::InvalidRequest(
                                "cannot duplicate a filesystem root".into(),
                            )
                        })?;
                    let provider = self
                        .providers
                        .resolve(&source)
                        .map_err(ApplicationError::from)?;
                    let transfer = TransferPlan::resolve(&provider, &source, &provider, &parent)?;
                    copies.push(CopyExecutor {
                        source_provider: Arc::clone(&provider),
                        destination_provider: provider,
                        destination_directory: parent,
                        temporary: Mutex::new(None),
                        planned: Mutex::new(HashMap::new()),
                        directories: Mutex::new(Vec::new()),
                        symlink_policy: request.symlink_policy,
                        root_name: Mutex::new(None),
                        source_override: Some(source),
                        continue_on_error: true,
                        completed_root_destination: Mutex::new(None),
                        created_destinations: Mutex::new(Vec::new()),
                        replaced_existing: AtomicBool::new(false),
                        transfer,
                    });
                }
                Arc::new(DuplicateExecutor { copies })
            }
            OperationKindDto::Delete => {
                if request.sources.is_empty() {
                    return Err(ApplicationError::InvalidRequest(
                        "delete requires at least one source".into(),
                    ));
                }
                let requires_confirmation = self
                    .settings
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .confirm_permanent_delete;
                let mut providers = HashMap::new();
                for source in &request.sources {
                    let location: Location = source.clone().into();
                    let provider = self
                        .providers
                        .resolve(&location)
                        .map_err(ApplicationError::from)?;
                    provider
                        .capabilities_for(&location)
                        .map_err(ApplicationError::from)?
                        .require(ProviderCapabilities::DELETE)
                        .map_err(ApplicationError::from)?;
                    providers.insert(location.provider_id.clone(), provider);
                }
                Arc::new(DeleteExecutor {
                    providers,
                    override_read_only: request.override_read_only,
                    audit_log_path: self.audit_log_path.clone(),
                    deleted: AtomicU64::new(0),
                    audited: AtomicBool::new(false),
                    requires_confirmation: requires_confirmation
                        && !request.permanent_delete_confirmed,
                })
            }
            OperationKindDto::Trash => {
                if request.sources.is_empty() {
                    return Err(ApplicationError::InvalidRequest(
                        "trash requires at least one source".into(),
                    ));
                }
                if !self
                    .platform
                    .capabilities()
                    .contains(PlatformCapabilities::TRASH)
                {
                    return Err(ApplicationError::PlatformOperationFailed(
                        fm_platform::PlatformError::Unsupported {
                            capability: PlatformCapabilities::TRASH,
                        }
                        .to_string(),
                    ));
                }
                Arc::new(TrashExecutor {
                    platform: Arc::clone(&self.platform),
                    providers: self.providers.clone(),
                    restored_entries: Mutex::new(Vec::new()),
                })
            }
            OperationKindDto::Undo => {
                return Err(ApplicationError::InvalidRequest(
                    "undo must target a completed operation through the undo endpoint".into(),
                ));
            }
            OperationKindDto::Search => {
                return Err(ApplicationError::InvalidRequest(
                    "search is handled via start_search, not the operation executor".into(),
                ));
            }
            OperationKindDto::Compare => {
                return Err(ApplicationError::InvalidRequest(
                    "compare is handled via start_comparison, not the operation executor".into(),
                ));
            }
        })
    }

    pub(crate) fn plan_undo(
        &self,
        plan: UndoPlan,
    ) -> Result<Arc<dyn OperationExecutor>, ApplicationError> {
        if plan.actions.is_empty() {
            return Err(ApplicationError::InvalidRequest(
                "This operation has no effects to undo.".into(),
            ));
        }
        let guard = UndoExecutor {
            providers: self.providers.clone(),
            platform: Arc::clone(&self.platform),
            plan: plan.clone(),
        };
        let cross_provider = plan.actions.iter().find_map(|action| match action {
            UndoAction::CrossProviderMoveBack {
                original,
                current_root,
                ..
            } => Some((original, current_root)),
            _ => None,
        });
        if let Some((original, current_root)) = cross_provider {
            if plan.actions.len() != 1 {
                return Err(ApplicationError::InvalidRequest(
                    "Cross-provider undo cannot be combined with other inverse actions.".into(),
                ));
            }
            let destination = original
                .parent()
                .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?
                .ok_or_else(|| {
                    ApplicationError::InvalidRequest(
                        "A provider root cannot be restored by move undo.".into(),
                    )
                })?;
            let request = StartOperationRequestDto {
                operation_type: OperationKindDto::Move,
                sources: vec![current_root.entry.location.clone().into()],
                destination: Some(destination.into()),
                destinations: Vec::new(),
                conflict_policy: fm_transport_dto::OperationConflictPolicyDto::Ask,
                name: None,
                archive_format: None,
                archive_compression_level: None,
                create_intermediate_directories: false,
                symlink_policy: SymlinkPolicyDto::default(),
                permanent_delete_confirmed: false,
                override_read_only: false,
            };
            let inner = self.plan(OperationKindDto::Move, &request)?;
            return Ok(Arc::new(GuardedUndoExecutor { guard, inner }));
        }
        Ok(Arc::new(guard))
    }
}

/* -------------------------------------------------------------------------- */
/*  Cross-provider transfer strategy selection (task 0108)                    */
/* -------------------------------------------------------------------------- */

/// How one file's bytes reach the destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransferStrategy {
    /// Both sides are the same backend and it can duplicate the bytes itself,
    /// so nothing is streamed through this process at all.
    ServerSideCopy,
    /// The source's reader is piped straight into the destination's writer.
    ///
    /// This is what makes `SFTP -> FTP` and `FTP -> SFTP` work without a
    /// temporary *local* file: the only staging area is the `.fm-copy-*`
    /// temporary the destination provider itself owns, which
    /// [`FileSystemProvider::commit_copy`] then publishes atomically.
    DirectStream,
}

/// How an entry is relocated for a move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MoveStrategy {
    /// One backend that can rename in place — no bytes are transferred.
    ServerSideMove,
    /// Different backends, or one that cannot rename: transfer the bytes, then
    /// delete the source once the destination is verified.
    CopyThenDelete,
}

/// The transfer decision for one source/destination pair.
///
/// Selection lives here, in the operation planner, and never in the UI or an
/// individual command: it is derived purely from the two sides'
/// [`TransferCapabilities`], so every operation kind that transfers bytes
/// (copy, move, duplicate) reaches the same conclusion from the same inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TransferPlan {
    /// Chosen byte-transfer path.
    pub(crate) strategy: TransferStrategy,
    /// Chosen relocation path, used only by move.
    pub(crate) move_strategy: MoveStrategy,
    /// Whether both sides sit on the same backend. Provider-native metadata
    /// preservation is only meaningful — and only attempted — when they do.
    pub(crate) same_endpoint: bool,
    /// Whether the destination advertised
    /// [`TransferCapabilities::resumable_upload`]. Direct-stream copying
    /// only attempts `source_provider.file_size` (to drive
    /// `open_write_sized`) when this is `true` - a provider that has no use
    /// for a declared size (the overwhelming majority: local, SFTP, FTP,
    /// S3, WebDAV) must never pay for, or be able to fail because of, an
    /// extra size lookup it never asked for.
    pub(crate) destination_resumable_upload: bool,
}

impl TransferPlan {
    /// Chooses the safest fast path both sides can honour.
    ///
    /// A provider-native path requires *both* sides to advertise it *and* to
    /// name the same [`fm_vfs::TransferEndpoint`]; anything else falls back to
    /// direct streaming, which every provider supports through
    /// `open_read`/`open_write`.
    fn select(source: &TransferCapabilities, destination: &TransferCapabilities) -> Self {
        let same_endpoint = source.shares_endpoint_with(destination);
        let strategy = if same_endpoint && source.server_side_copy && destination.server_side_copy {
            TransferStrategy::ServerSideCopy
        } else {
            TransferStrategy::DirectStream
        };
        let move_strategy =
            if same_endpoint && source.server_side_move && destination.server_side_move {
                MoveStrategy::ServerSideMove
            } else {
                MoveStrategy::CopyThenDelete
            };
        Self {
            strategy,
            move_strategy,
            same_endpoint,
            destination_resumable_upload: destination.resumable_upload,
        }
    }

    /// Resolves both sides' capabilities from their providers and selects.
    fn resolve(
        source_provider: &Arc<dyn FileSystemProvider>,
        source: &Location,
        destination_provider: &Arc<dyn FileSystemProvider>,
        destination: &Location,
    ) -> Result<Self, ApplicationError> {
        Ok(Self::select(
            &source_provider
                .transfer_capabilities(source)
                .map_err(ApplicationError::from)?,
            &destination_provider
                .transfer_capabilities(destination)
                .map_err(ApplicationError::from)?,
        ))
    }
}

/* -------------------------------------------------------------------------- */
/*  Executor structs                                                          */
/* -------------------------------------------------------------------------- */

struct CreateDirectoryExecutor {
    provider: Arc<dyn FileSystemProvider>,
    parent: Location,
    name: String,
    create_intermediates: bool,
}

struct CreateFileExecutor {
    provider: Arc<dyn FileSystemProvider>,
    parent: Location,
    name: String,
}

struct RenameExecutor {
    provider: Arc<dyn FileSystemProvider>,
    source: Location,
    destination: Location,
    source_fingerprint: Mutex<Option<EntryFingerprint>>,
    destination_fingerprint: Mutex<Option<EntryFingerprint>>,
}

/// Batch rename (task 0072 multi-rename): one [`RenameExecutor`] per source/destination pair,
/// executed as a single cancellable operation. Never falls back to copy+delete.
struct RenameGroupExecutor {
    renames: Vec<RenameExecutor>,
}

#[derive(Clone)]
struct PlannedCopyEntry {
    kind: EntryKind,
    destination: Location,
    source: EntryRef,
    is_root: bool,
}

struct CopyExecutor {
    source_provider: Arc<dyn FileSystemProvider>,
    destination_provider: Arc<dyn FileSystemProvider>,
    destination_directory: Location,
    temporary: Mutex<Option<Location>>,
    planned: Mutex<HashMap<String, PlannedCopyEntry>>,
    directories: Mutex<Vec<(EntryRef, EntryRef)>>,
    symlink_policy: SymlinkPolicyDto,
    root_name: Mutex<Option<String>>,
    source_override: Option<Location>,
    continue_on_error: bool,
    completed_root_destination: Mutex<Option<Location>>,
    created_destinations: Mutex<Vec<Location>>,
    replaced_existing: AtomicBool,
    /// Strategy chosen by the planner for this source/destination pair
    /// (task 0108). Decided once, up front, from both sides' capabilities —
    /// execution only obeys it.
    transfer: TransferPlan,
}

struct CopyGroupExecutor {
    copies: Vec<CopyExecutor>,
    stale_sources: Mutex<HashMap<String, EntryRef>>,
}

/// One atomic archive-creation job.  The codec implementation lives in
/// `fm-archive`; this adapter only gives it normal operation lifecycle and
/// cancellation semantics.
struct CreateArchiveExecutor {
    destination: PathBuf,
    sources: Vec<PathBuf>,
    format: ArchiveCreationFormat,
    compression_level: i64,
    remove_sources: bool,
}

struct DuplicateExecutor {
    copies: Vec<CopyExecutor>,
}

struct DeleteExecutor {
    providers: HashMap<ProviderId, Arc<dyn FileSystemProvider>>,
    override_read_only: bool,
    audit_log_path: PathBuf,
    deleted: AtomicU64,
    audited: AtomicBool,
    requires_confirmation: bool,
}

struct TrashExecutor {
    platform: Arc<dyn PlatformAdapter>,
    providers: ProviderRegistry,
    restored_entries: Mutex<Vec<(Location, Location)>>,
}

struct MoveExecutor {
    source: Location,
    source_provider: Arc<dyn FileSystemProvider>,
    destination_provider: Arc<dyn FileSystemProvider>,
    destination_directory: Location,
    copy: CopyExecutor,
    fallback: Mutex<bool>,
    force_fallback: bool,
    transfer: TransferPlan,
    source_fingerprint: Mutex<Option<EntryFingerprint>>,
    destination_fingerprint: Mutex<Option<EntryFingerprint>>,
}

struct MoveGroupExecutor {
    moves: Vec<MoveExecutor>,
    stale_sources: Mutex<HashMap<String, EntryRef>>,
}

struct UndoExecutor {
    providers: ProviderRegistry,
    platform: Arc<dyn PlatformAdapter>,
    plan: UndoPlan,
}

struct GuardedUndoExecutor {
    guard: UndoExecutor,
    inner: Arc<dyn OperationExecutor>,
}

/* -------------------------------------------------------------------------- */
/*  Archive format inference                                                  */
/* -------------------------------------------------------------------------- */

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArchiveCreationFormat {
    Zip,
    SevenZip,
}

impl ArchiveCreationFormat {
    fn from_request(
        path: &Path,
        requested: Option<ArchiveFormatDto>,
    ) -> Result<Self, ApplicationError> {
        let inferred = match path.extension().and_then(|extension| extension.to_str()) {
            Some(extension) if extension.eq_ignore_ascii_case("zip") => Ok(Self::Zip),
            Some(extension) if extension.eq_ignore_ascii_case("7z") => Ok(Self::SevenZip),
            _ => Err(ApplicationError::InvalidRequest(
                "archive destination must end in .zip or .7z".into(),
            )),
        }?;
        let requested = match requested {
            Some(ArchiveFormatDto::Zip) => Self::Zip,
            Some(ArchiveFormatDto::SevenZip) => Self::SevenZip,
            None => inferred,
        };
        if requested != inferred {
            return Err(ApplicationError::InvalidRequest(
                "archive format must match the destination extension".into(),
            ));
        }
        Ok(requested)
    }
}

/* -------------------------------------------------------------------------- */
/*  OperationExecutor implementations                                         */
/* -------------------------------------------------------------------------- */

async fn fingerprint(
    provider: &Arc<dyn FileSystemProvider>,
    location: &Location,
    cancellation: &CancellationToken,
) -> Result<EntryFingerprint, ExecutionError> {
    let summary = provider
        .inspect(
            &EntryRef {
                id: EntryId::new(),
                location: location.clone(),
            },
            cancellation.clone(),
        )
        .await?;
    let content_hash = if summary.kind == EntryKind::File {
        let reader = provider
            .open_read(
                &EntryRef {
                    id: summary.id,
                    location: summary.location.clone(),
                },
                cancellation.clone(),
            )
            .await?;
        let hashes = fm_checksum::hash_stream(
            reader,
            &[fm_checksum::ChecksumAlgorithm::Blake3],
            cancellation,
        )
        .await
        .map_err(|error| ExecutionError::Failed(error.to_string()))?;
        hashes
            .get(fm_checksum::ChecksumAlgorithm::Blake3)
            .map(str::to_owned)
    } else {
        None
    };
    Ok(EntryFingerprint {
        entry: EntryRef {
            id: summary.id,
            location: summary.location,
        },
        stable_id: false,
        kind: summary.kind,
        size: summary.size,
        modified_at: summary.modified_at,
        content_hash,
    })
}

fn fingerprint_matches(expected: &EntryFingerprint, actual: &fm_domain::EntrySummary) -> bool {
    (!expected.stable_id || expected.entry.id == actual.id)
        && expected.kind == actual.kind
        && expected.size == actual.size
        && expected.modified_at == actual.modified_at
}

#[async_trait]
impl OperationExecutor for UndoExecutor {
    async fn plan(
        &self,
        _operation: &Operation,
        cancellation: &CancellationToken,
    ) -> Result<OperationPlan, ExecutionError> {
        let mut items = Vec::with_capacity(self.plan.actions.len());
        for action in &self.plan.actions {
            match action {
                UndoAction::MoveBack {
                    original, current, ..
                } => {
                    self.verify_fingerprint(current, cancellation).await?;
                    let original_provider = self.providers.resolve(original)?;
                    let original_entry = EntryRef {
                        id: EntryId::new(),
                        location: original.clone(),
                    };
                    match original_provider
                        .inspect(&original_entry, cancellation.clone())
                        .await
                    {
                        Err(fm_vfs::VfsError::NotFound { .. }) => {}
                        Ok(_) => {
                            return Err(ExecutionError::Failed(
                                "The original path is occupied, so undo would overwrite a later change."
                                    .into(),
                            ));
                        }
                        Err(error) => return Err(error.into()),
                    }
                    items.push(PlanItem::new(current.entry.clone(), 0));
                }
                UndoAction::RemoveCreated { entries } => {
                    for entry in entries {
                        self.verify_fingerprint(entry, cancellation).await?;
                    }
                    items.extend(
                        entries
                            .iter()
                            .rev()
                            .map(|entry| PlanItem::new(entry.entry.clone(), 0)),
                    );
                }
                UndoAction::CrossProviderMoveBack {
                    original,
                    current_entries,
                    ..
                } => {
                    for entry in current_entries {
                        self.verify_fingerprint(entry, cancellation).await?;
                    }
                    self.verify_original_absent(original, cancellation).await?;
                    items.extend(
                        current_entries
                            .iter()
                            .map(|entry| PlanItem::new(entry.entry.clone(), 0)),
                    );
                }
                UndoAction::RestoreTrash { original, trashed } => {
                    self.verify_fingerprint(trashed, cancellation).await?;
                    self.verify_original_absent(original, cancellation).await?;
                    items.push(PlanItem::new(trashed.entry.clone(), 0));
                }
            }
        }
        Ok(OperationPlan::new(items))
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
        let action = self
            .plan
            .actions
            .iter()
            .find(|action| match action {
                UndoAction::MoveBack { current, .. } => {
                    current.entry.location == item.entry.location
                }
                UndoAction::RemoveCreated { entries } => entries
                    .iter()
                    .any(|entry| entry.entry.location == item.entry.location),
                UndoAction::CrossProviderMoveBack {
                    current_entries, ..
                } => current_entries
                    .iter()
                    .any(|entry| entry.entry.location == item.entry.location),
                UndoAction::RestoreTrash { trashed, .. } => {
                    trashed.entry.location == item.entry.location
                }
            })
            .ok_or_else(|| ExecutionError::Failed("Undo plan entry is missing.".into()))?;
        match action {
            UndoAction::MoveBack {
                original, current, ..
            } => {
                self.verify_fingerprint(current, cancellation).await?;
                self.verify_original_absent(original, cancellation).await?;
                let provider = self.providers.resolve(&current.entry.location)?;
                provider
                    .rename(&current.entry, original, cancellation.clone())
                    .await?;
            }
            UndoAction::RemoveCreated { entries } => {
                let fingerprint = entries
                    .iter()
                    .find(|entry| entry.entry.location == item.entry.location)
                    .ok_or_else(|| ExecutionError::Failed("Undo fingerprint is missing.".into()))?;
                self.verify_fingerprint(fingerprint, cancellation).await?;
                let provider = self.providers.resolve(&fingerprint.entry.location)?;
                provider
                    .remove(
                        &fingerprint.entry,
                        RemoveOptions {
                            recursive: false,
                            use_trash: false,
                        },
                        cancellation.clone(),
                    )
                    .await?;
            }
            UndoAction::CrossProviderMoveBack { .. } => {
                unreachable!("cross-provider undo uses the guarded move executor")
            }
            UndoAction::RestoreTrash { original, trashed } => {
                self.verify_fingerprint(trashed, cancellation).await?;
                self.verify_original_absent(original, cancellation).await?;
                let trashed_path = trashed
                    .entry
                    .location
                    .to_native_path()
                    .map_err(|error| ExecutionError::Failed(error.to_string()))?;
                let original_path = original
                    .to_native_path()
                    .map_err(|error| ExecutionError::Failed(error.to_string()))?;
                self.platform
                    .restore_from_trash(&trashed_path, &original_path)
                    .map_err(|error| ExecutionError::Failed(error.to_string()))?;
            }
        }
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
        Ok(OperationUndo::unavailable(
            "Undo operations cannot themselves be undone.",
        ))
    }
}

impl UndoExecutor {
    async fn verify_fingerprint(
        &self,
        expected: &EntryFingerprint,
        cancellation: &CancellationToken,
    ) -> Result<(), ExecutionError> {
        let provider = self.providers.resolve(&expected.entry.location)?;
        let actual = provider
            .inspect(&expected.entry, cancellation.clone())
            .await
            .map_err(|error| match error {
                fm_vfs::VfsError::NotFound { .. } => ExecutionError::Failed(
                    "The operation output is missing, so it cannot be undone safely.".into(),
                ),
                error => error.into(),
            })?;
        if !fingerprint_matches(expected, &actual) {
            return Err(ExecutionError::Failed(
                "The operation output changed after completion, so it cannot be undone safely."
                    .into(),
            ));
        }
        if let Some(expected_hash) = &expected.content_hash {
            let reader = provider
                .open_read(
                    &EntryRef {
                        id: actual.id,
                        location: actual.location.clone(),
                    },
                    cancellation.clone(),
                )
                .await?;
            let hashes = fm_checksum::hash_stream(
                reader,
                &[fm_checksum::ChecksumAlgorithm::Blake3],
                cancellation,
            )
            .await
            .map_err(|error| ExecutionError::Failed(error.to_string()))?;
            if hashes.get(fm_checksum::ChecksumAlgorithm::Blake3) != Some(expected_hash.as_str()) {
                return Err(ExecutionError::Failed(
                    "The operation output content changed after completion, so it cannot be undone safely."
                        .into(),
                ));
            }
        }
        Ok(())
    }

    async fn verify_original_absent(
        &self,
        original: &Location,
        cancellation: &CancellationToken,
    ) -> Result<(), ExecutionError> {
        let provider = self.providers.resolve(original)?;
        let entry = EntryRef {
            id: EntryId::new(),
            location: original.clone(),
        };
        match provider.inspect(&entry, cancellation.clone()).await {
            Err(fm_vfs::VfsError::NotFound { .. }) => Ok(()),
            Ok(_) => Err(ExecutionError::Failed(
                "The original path is occupied, so undo would overwrite a later change.".into(),
            )),
            Err(error) => Err(error.into()),
        }
    }
}

#[async_trait]
impl OperationExecutor for GuardedUndoExecutor {
    async fn plan(
        &self,
        operation: &Operation,
        cancellation: &CancellationToken,
    ) -> Result<OperationPlan, ExecutionError> {
        self.guard.plan(operation, cancellation).await?;
        self.inner.plan(operation, cancellation).await
    }

    async fn execute(
        &self,
        operation: &Operation,
        item: &PlanItem,
        resolution: Option<ConflictResolution>,
        progress: &dyn OperationProgressReporter,
        pause: &PauseToken,
        cancellation: &CancellationToken,
    ) -> Result<ExecutionOutcome, ExecutionError> {
        if let Some(expected) = self
            .guard
            .plan
            .actions
            .iter()
            .find_map(|action| match action {
                UndoAction::CrossProviderMoveBack {
                    current_entries, ..
                } => current_entries
                    .iter()
                    .find(|entry| entry.entry.location == item.entry.location),
                _ => None,
            })
        {
            self.guard
                .verify_fingerprint(expected, cancellation)
                .await?;
        }
        if resolution.is_some() {
            return Err(ExecutionError::Failed(
                "Undo cannot overwrite or rename around an occupied original path.".into(),
            ));
        }
        if let Some(original) = self
            .guard
            .plan
            .actions
            .iter()
            .find_map(|action| match action {
                UndoAction::CrossProviderMoveBack {
                    original,
                    current_root,
                    ..
                } if current_root.entry.location == item.entry.location => Some(original),
                _ => None,
            })
        {
            self.guard
                .verify_original_absent(original, cancellation)
                .await?;
        }
        match self
            .inner
            .execute(operation, item, resolution, progress, pause, cancellation)
            .await
        {
            Err(ExecutionError::Conflict(_)) => Err(ExecutionError::Failed(
                "The original path became occupied, so undo was stopped without overwriting it."
                    .into(),
            )),
            result => result,
        }
    }

    async fn cleanup_partial(&self, operation: &Operation) -> Result<(), ExecutionError> {
        self.inner.cleanup_partial(operation).await
    }

    async fn finish(
        &self,
        operation: &Operation,
        cancellation: &CancellationToken,
    ) -> Result<(), ExecutionError> {
        self.inner.finish(operation, cancellation).await
    }

    async fn undo_evidence(
        &self,
        _operation: &Operation,
        _cancellation: &CancellationToken,
    ) -> Result<OperationUndo, ExecutionError> {
        Ok(OperationUndo::unavailable(
            "Undo operations cannot themselves be undone.",
        ))
    }
}

#[async_trait]
impl OperationExecutor for CreateArchiveExecutor {
    async fn plan(
        &self,
        _operation: &Operation,
        _cancellation: &CancellationToken,
    ) -> Result<OperationPlan, ExecutionError> {
        Ok(OperationPlan::new(vec![PlanItem::new(
            EntryRef {
                id: EntryId::new(),
                location: Location::from_native_path(&self.destination)
                    .map_err(|error| ExecutionError::Failed(error.to_string()))?,
            },
            0,
        )]))
    }

    async fn execute(
        &self,
        _operation: &Operation,
        _item: &PlanItem,
        _resolution: Option<ConflictResolution>,
        _progress: &dyn OperationProgressReporter,
        _pause: &PauseToken,
        cancellation: &CancellationToken,
    ) -> Result<ExecutionOutcome, ExecutionError> {
        let destination = self.destination.clone();
        let sources = self.sources.clone();
        let sources_for_archive = sources.clone();
        let format = self.format;
        let compression_level = self.compression_level;
        let remove_sources = self.remove_sources;
        let cancellation = cancellation.clone();
        let archive_cancellation = cancellation.clone();
        tokio::task::spawn_blocking(move || match format {
            ArchiveCreationFormat::Zip => create_zip_archive(
                &destination,
                &sources_for_archive,
                Some(compression_level),
                &archive_cancellation,
            ),
            ArchiveCreationFormat::SevenZip => {
                create_7z_archive(&destination, &sources_for_archive, &archive_cancellation)
            }
        })
        .await
        .map_err(|error| ExecutionError::Failed(error.to_string()))??;
        if remove_sources {
            for source in sources {
                if cancellation.is_cancelled() {
                    return Err(fm_vfs::VfsError::Cancelled.into());
                }
                let metadata = std::fs::symlink_metadata(&source)
                    .map_err(|error| ExecutionError::Failed(error.to_string()))?;
                if metadata.is_dir() {
                    std::fs::remove_dir_all(&source)
                } else {
                    std::fs::remove_file(&source)
                }
                .map_err(|error| ExecutionError::Failed(error.to_string()))?;
            }
        }
        Ok(ExecutionOutcome::Completed)
    }

    async fn cleanup_partial(&self, _operation: &Operation) -> Result<(), ExecutionError> {
        Ok(())
    }
}

impl DeleteExecutor {
    async fn write_audit(&self, operation: &Operation) -> Result<(), ExecutionError> {
        if self.audited.load(Ordering::Acquire) {
            return Ok(());
        }
        if let Some(parent) = self.audit_log_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(copy_stream_error)?;
        }
        let record = serde_json::json!({
            "timestamp": chrono::Utc::now(),
            "operationId": operation.id.to_string(),
            "kind": "permanentDelete",
            "sources": operation.sources.iter().map(|entry| &entry.location.uri).collect::<Vec<_>>(),
            "deletedItems": self.deleted.load(Ordering::Acquire),
        });
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.audit_log_path)
            .await
            .map_err(copy_stream_error)?;
        file.write_all(format!("{record}\n").as_bytes())
            .await
            .map_err(copy_stream_error)?;
        self.audited.store(true, Ordering::Release);
        Ok(())
    }
}

#[async_trait]
impl OperationExecutor for DeleteExecutor {
    fn requires_confirmation(&self) -> bool {
        self.requires_confirmation
    }
    async fn plan(
        &self,
        operation: &Operation,
        cancellation: &CancellationToken,
    ) -> Result<OperationPlan, ExecutionError> {
        let mut items = Vec::new();
        for source in &operation.sources {
            let provider = self
                .providers
                .get(&source.location.provider_id)
                .ok_or_else(|| ExecutionError::Failed("delete provider is missing".into()))?;
            let root = provider.inspect(source, cancellation.clone()).await?;
            let mut stack = vec![(root, false)];
            while let Some((summary, visited)) = stack.pop() {
                if cancellation.is_cancelled() {
                    return Err(fm_vfs::VfsError::Cancelled.into());
                }
                if summary.read_only && !self.override_read_only && !self.requires_confirmation {
                    return Err(ExecutionError::Failed(format!(
                        "read-only entry requires explicit override: {}",
                        summary.location.uri
                    )));
                }
                let entry = EntryRef {
                    id: summary.id,
                    location: summary.location.clone(),
                };
                if summary.kind == EntryKind::Directory && !visited {
                    stack.push((summary.clone(), true));
                    let mut continuation_token = None;
                    loop {
                        let page = provider
                            .list(
                                &summary.location,
                                ListOptions {
                                    page_size: 512,
                                    continuation_token,
                                },
                                cancellation.clone(),
                            )
                            .await?;
                        for child in page.entries.into_iter().rev() {
                            stack.push((child, false));
                        }
                        if !page.has_more {
                            break;
                        }
                        continuation_token = page.continuation_token;
                    }
                } else {
                    items.push(PlanItem::new(entry, summary.size.unwrap_or(0)));
                }
            }
        }
        Ok(OperationPlan::new(items))
    }

    async fn execute(
        &self,
        _operation: &Operation,
        item: &PlanItem,
        _resolution: Option<fm_operations::ConflictResolution>,
        _progress: &dyn OperationProgressReporter,
        _pause: &PauseToken,
        cancellation: &CancellationToken,
    ) -> Result<fm_operations::ExecutionOutcome, ExecutionError> {
        let provider = self
            .providers
            .get(&item.entry.location.provider_id)
            .ok_or_else(|| ExecutionError::Failed("delete provider is missing".into()))?;
        match provider
            .remove(
                &item.entry,
                RemoveOptions {
                    recursive: false,
                    use_trash: false,
                },
                cancellation.clone(),
            )
            .await
        {
            Ok(()) => {
                self.deleted.fetch_add(1, Ordering::Relaxed);
                Ok(ExecutionOutcome::Completed)
            }
            Err(error) => Err(ExecutionError::Warning {
                entry: item.entry.clone(),
                message: error.to_string(),
            }),
        }
    }

    async fn cleanup_partial(&self, operation: &Operation) -> Result<(), ExecutionError> {
        self.write_audit(operation).await
    }

    async fn finish(
        &self,
        operation: &Operation,
        _cancellation: &CancellationToken,
    ) -> Result<(), ExecutionError> {
        self.write_audit(operation).await
    }
}

/// Moves entries to the platform trash (task 0043). Unlike [`DeleteExecutor`],
/// this bypasses [`FileSystemProvider`] entirely: the platform adapter
/// natively relocates the whole entry (file or directory tree) in one call,
/// mirroring how `core.open`/`core.revealInSystemFileManager` dispatch
/// directly to `self.platform` (task 0061). Trash is reversible, so unlike
/// permanent delete there is no mandatory confirmation, read-only override,
/// or audit log.
#[async_trait]
impl OperationExecutor for TrashExecutor {
    async fn plan(
        &self,
        operation: &Operation,
        cancellation: &CancellationToken,
    ) -> Result<OperationPlan, ExecutionError> {
        let mut items = Vec::with_capacity(operation.sources.len());
        for source in &operation.sources {
            let provider = self.providers.resolve(&source.location)?;
            provider
                .capabilities_for(&source.location)?
                .require(ProviderCapabilities::LIST)?;
            let summary = provider.inspect(source, cancellation.clone()).await?;
            let bytes = match summary.kind {
                EntryKind::Directory => {
                    calculate_directory_size(
                        provider.as_ref(),
                        summary.location,
                        cancellation.clone(),
                    )
                    .await?
                    .total_bytes
                }
                EntryKind::File | EntryKind::Symlink => summary.size.unwrap_or(0),
            };
            items.push(PlanItem::new(source.clone(), bytes));
        }
        Ok(OperationPlan::new(items))
    }

    async fn execute(
        &self,
        _operation: &Operation,
        item: &PlanItem,
        _resolution: Option<ConflictResolution>,
        _progress: &dyn OperationProgressReporter,
        _pause: &PauseToken,
        _cancellation: &CancellationToken,
    ) -> Result<fm_operations::ExecutionOutcome, ExecutionError> {
        let path = item
            .entry
            .location
            .to_native_path()
            .map_err(|error| ExecutionError::Failed(error.to_string()))?;
        let restore_location =
            self.platform
                .trash_with_restore_location(&path)
                .map_err(|error| ExecutionError::Warning {
                    entry: item.entry.clone(),
                    message: error.to_string(),
                })?;
        if let Some(restore_path) = restore_location {
            let restore_location = Location::from_native_path(&restore_path)
                .map_err(|error| ExecutionError::Failed(error.to_string()))?;
            self.restored_entries
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push((item.entry.location.clone(), restore_location));
        }
        Ok(ExecutionOutcome::Completed)
    }

    async fn cleanup_partial(&self, _operation: &Operation) -> Result<(), ExecutionError> {
        Ok(())
    }

    async fn undo_evidence(
        &self,
        _operation: &Operation,
        cancellation: &CancellationToken,
    ) -> Result<OperationUndo, ExecutionError> {
        let restored_entries = self
            .restored_entries
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        if restored_entries.is_empty() {
            return Ok(OperationUndo::unavailable(
                "The platform did not provide a restorable trash location.",
            ));
        }
        let mut actions = Vec::with_capacity(restored_entries.len());
        for (original, trashed_location) in restored_entries {
            let provider = self.providers.resolve(&trashed_location)?;
            actions.push(UndoAction::RestoreTrash {
                original,
                trashed: fingerprint(&provider, &trashed_location, cancellation).await?,
            });
        }
        Ok(OperationUndo::available(UndoPlan { actions }))
    }
}

#[async_trait]
impl OperationExecutor for CopyGroupExecutor {
    async fn plan(
        &self,
        operation: &Operation,
        cancellation: &CancellationToken,
    ) -> Result<OperationPlan, ExecutionError> {
        let mut items = Vec::new();
        for executor in &self.copies {
            match executor.plan(operation, cancellation).await {
                Ok(plan) => items.extend(plan.items),
                Err(ExecutionError::Provider(fm_vfs::VfsError::NotFound { .. })) => {
                    let source = executor.source_override.clone().ok_or_else(|| {
                        ExecutionError::Failed("copy source is missing from its plan".into())
                    })?;
                    let entry = EntryRef {
                        id: EntryId::new(),
                        location: source,
                    };
                    self.stale_sources
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(entry.location.uri.clone(), entry.clone());
                    items.push(PlanItem::new(entry, 0));
                }
                Err(error) => return Err(error),
            }
        }
        Ok(OperationPlan::new(items))
    }

    async fn execute(
        &self,
        operation: &Operation,
        item: &PlanItem,
        resolution: Option<ConflictResolution>,
        progress: &dyn OperationProgressReporter,
        pause: &PauseToken,
        cancellation: &CancellationToken,
    ) -> Result<ExecutionOutcome, ExecutionError> {
        if self
            .stale_sources
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(&item.entry.location.uri)
        {
            return Err(ExecutionError::Warning {
                entry: item.entry.clone(),
                message: "Source no longer exists; skipped.".into(),
            });
        }
        for executor in &self.copies {
            if executor
                .planned
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .contains_key(&item.entry.location.uri)
            {
                return executor
                    .execute(operation, item, resolution, progress, pause, cancellation)
                    .await;
            }
        }
        Err(ExecutionError::Failed("copy plan entry is missing".into()))
    }

    async fn cleanup_partial(&self, operation: &Operation) -> Result<(), ExecutionError> {
        for executor in &self.copies {
            executor.cleanup_partial(operation).await?;
        }
        Ok(())
    }

    async fn finish(
        &self,
        operation: &Operation,
        cancellation: &CancellationToken,
    ) -> Result<(), ExecutionError> {
        for executor in &self.copies {
            executor.finish(operation, cancellation).await?;
        }
        Ok(())
    }

    async fn undo_evidence(
        &self,
        operation: &Operation,
        cancellation: &CancellationToken,
    ) -> Result<OperationUndo, ExecutionError> {
        copy_group_undo_evidence(&self.copies, operation, cancellation).await
    }
}

#[async_trait]
impl OperationExecutor for MoveGroupExecutor {
    async fn plan(
        &self,
        operation: &Operation,
        cancellation: &CancellationToken,
    ) -> Result<OperationPlan, ExecutionError> {
        let mut items = Vec::new();
        for executor in &self.moves {
            match executor.plan(operation, cancellation).await {
                Ok(plan) => items.extend(plan.items),
                Err(ExecutionError::Provider(fm_vfs::VfsError::NotFound { .. })) => {
                    let entry = EntryRef {
                        id: EntryId::new(),
                        location: executor.source.clone(),
                    };
                    self.stale_sources
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(entry.location.uri.clone(), entry.clone());
                    items.push(PlanItem::new(entry, 0));
                }
                Err(error) => return Err(error),
            }
        }
        Ok(OperationPlan::new(items))
    }

    async fn execute(
        &self,
        operation: &Operation,
        item: &PlanItem,
        resolution: Option<ConflictResolution>,
        progress: &dyn OperationProgressReporter,
        pause: &PauseToken,
        cancellation: &CancellationToken,
    ) -> Result<ExecutionOutcome, ExecutionError> {
        if self
            .stale_sources
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(&item.entry.location.uri)
        {
            return Err(ExecutionError::Warning {
                entry: item.entry.clone(),
                message: "Source no longer exists; skipped.".into(),
            });
        }
        for executor in &self.moves {
            if item.entry.location == executor.source
                || executor
                    .copy
                    .planned
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .contains_key(&item.entry.location.uri)
            {
                return executor
                    .execute(operation, item, resolution, progress, pause, cancellation)
                    .await;
            }
        }
        Err(ExecutionError::Failed("move plan entry is missing".into()))
    }

    async fn cleanup_partial(&self, operation: &Operation) -> Result<(), ExecutionError> {
        for executor in &self.moves {
            executor.cleanup_partial(operation).await?;
        }
        Ok(())
    }

    async fn finish(
        &self,
        operation: &Operation,
        cancellation: &CancellationToken,
    ) -> Result<(), ExecutionError> {
        for executor in &self.moves {
            executor.finish(operation, cancellation).await?;
        }
        Ok(())
    }

    async fn undo_evidence(
        &self,
        operation: &Operation,
        cancellation: &CancellationToken,
    ) -> Result<OperationUndo, ExecutionError> {
        let mut actions = Vec::new();
        for executor in &self.moves {
            let evidence = executor.undo_evidence(operation, cancellation).await?;
            let Some(plan) = evidence.plan else {
                return Ok(evidence);
            };
            actions.extend(plan.actions);
        }
        Ok(OperationUndo::available(UndoPlan { actions }))
    }
}

#[async_trait]
impl OperationExecutor for DuplicateExecutor {
    async fn plan(
        &self,
        operation: &Operation,
        cancellation: &CancellationToken,
    ) -> Result<OperationPlan, ExecutionError> {
        let mut items = Vec::new();
        for copy in &self.copies {
            let source_location = copy
                .source_override
                .as_ref()
                .ok_or_else(|| ExecutionError::Failed("duplicate source is missing".into()))?;
            let source_name = source_location
                .name()
                .map_err(|error| ExecutionError::Failed(error.to_string()))?;
            let mut index = 1_u32;
            loop {
                let candidate = fm_operations::duplicate_name(&source_name, index);
                let destination = copy
                    .destination_directory
                    .join(&candidate)
                    .map_err(|error| ExecutionError::Failed(error.to_string()))?;
                let probe = EntryRef {
                    id: EntryId::new(),
                    location: destination,
                };
                match copy
                    .destination_provider
                    .inspect(&probe, cancellation.clone())
                    .await
                {
                    Err(fm_vfs::VfsError::NotFound { .. }) => {
                        *copy.root_name.lock().unwrap_or_else(|e| e.into_inner()) = Some(candidate);
                        break;
                    }
                    Ok(_) => index = index.saturating_add(1),
                    Err(error) => return Err(error.into()),
                }
            }
            items.extend(copy.plan(operation, cancellation).await?.items);
        }
        Ok(OperationPlan::new(items))
    }

    async fn execute(
        &self,
        operation: &Operation,
        item: &PlanItem,
        resolution: Option<ConflictResolution>,
        progress: &dyn OperationProgressReporter,
        pause: &PauseToken,
        cancellation: &CancellationToken,
    ) -> Result<ExecutionOutcome, ExecutionError> {
        for copy in &self.copies {
            if copy
                .planned
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .contains_key(&item.entry.location.uri)
            {
                return copy
                    .execute(operation, item, resolution, progress, pause, cancellation)
                    .await;
            }
        }
        Err(ExecutionError::Failed(
            "duplicate plan entry is missing".into(),
        ))
    }

    async fn cleanup_partial(&self, operation: &Operation) -> Result<(), ExecutionError> {
        for copy in &self.copies {
            copy.cleanup_partial(operation).await?;
        }
        Ok(())
    }

    async fn finish(
        &self,
        operation: &Operation,
        cancellation: &CancellationToken,
    ) -> Result<(), ExecutionError> {
        for copy in &self.copies {
            copy.finish(operation, cancellation).await?;
        }
        Ok(())
    }

    async fn undo_evidence(
        &self,
        operation: &Operation,
        cancellation: &CancellationToken,
    ) -> Result<OperationUndo, ExecutionError> {
        copy_group_undo_evidence(&self.copies, operation, cancellation).await
    }
}

#[async_trait]
impl OperationExecutor for MoveExecutor {
    async fn plan(
        &self,
        operation: &Operation,
        cancellation: &CancellationToken,
    ) -> Result<OperationPlan, ExecutionError> {
        let source = EntryRef {
            id: EntryId::new(),
            location: self.source.clone(),
        };
        *self
            .source_fingerprint
            .lock()
            .unwrap_or_else(|error| error.into_inner()) =
            Some(fingerprint(&self.source_provider, &self.source, cancellation).await?);
        let name = source
            .location
            .name()
            .map_err(|error| ExecutionError::Failed(error.to_string()))?;
        let destination = self
            .destination_directory
            .join(&name)
            .map_err(|error| ExecutionError::Failed(error.to_string()))?;
        fm_operations::validate_paths(&source.location, &destination, cfg!(not(windows)))
            .map_err(|error| ExecutionError::Failed(error.to_string()))?;
        // Task 0108: the planner already decided, from both sides'
        // `TransferCapabilities`, whether a server-native rename is even
        // conceivable — crucially this is *endpoint* identity, so two
        // different SFTP/FTP connections of the same provider type never
        // qualify. `same_filesystem` then remains the final authority, which
        // is what preserves the local provider's cross-volume semantics
        // unchanged (a local rename still fails across devices).
        let same_filesystem = !self.force_fallback
            && self.transfer.move_strategy == MoveStrategy::ServerSideMove
            && self
                .source_provider
                .same_filesystem(&source, &self.destination_directory, cancellation.clone())
                .await?;
        *self.fallback.lock().unwrap_or_else(|e| e.into_inner()) = !same_filesystem;
        if same_filesystem {
            Ok(OperationPlan::new(vec![PlanItem::new(source, 0)]))
        } else {
            self.copy.plan(operation, cancellation).await
        }
    }

    async fn execute(
        &self,
        operation: &Operation,
        item: &PlanItem,
        resolution: Option<ConflictResolution>,
        progress: &dyn OperationProgressReporter,
        pause: &PauseToken,
        cancellation: &CancellationToken,
    ) -> Result<ExecutionOutcome, ExecutionError> {
        if *self.fallback.lock().unwrap_or_else(|e| e.into_inner()) {
            return self
                .copy
                .execute(operation, item, resolution, progress, pause, cancellation)
                .await;
        }
        let name = item
            .entry
            .location
            .name()
            .map_err(|error| ExecutionError::Failed(error.to_string()))?;
        let mut destination = self
            .destination_directory
            .join(&name)
            .map_err(|error| ExecutionError::Failed(error.to_string()))?;
        let destination_entry = EntryRef {
            id: EntryId::new(),
            location: destination.clone(),
        };
        if let Ok(existing) = self
            .destination_provider
            .inspect(&destination_entry, cancellation.clone())
            .await
        {
            let source = self
                .source_provider
                .inspect(&item.entry, cancellation.clone())
                .await?;
            if source.kind != existing.kind {
                return Err(ExecutionError::Failed(
                    "a file and directory cannot replace one another".into(),
                ));
            }
            match effective_resolution(operation.conflict_policy, resolution) {
                None => return Err(conflict_error(&source, &existing)),
                Some(ConflictResolution::Skip) => return Ok(ExecutionOutcome::Skipped),
                Some(ConflictResolution::Overwrite) => {
                    self.copy.replaced_existing.store(true, Ordering::Release);
                    self.destination_provider
                        .remove(
                            &destination_entry,
                            RemoveOptions {
                                recursive: existing.kind == EntryKind::Directory,
                                use_trash: false,
                            },
                            cancellation.clone(),
                        )
                        .await?;
                }
                Some(ConflictResolution::RenameNew) => {
                    destination = self
                        .copy
                        .next_copy_destination(&destination, cancellation)
                        .await?;
                }
            }
        }
        let moved = self
            .source_provider
            .rename(&item.entry, &destination, cancellation.clone())
            .await?;
        *self
            .destination_fingerprint
            .lock()
            .unwrap_or_else(|error| error.into_inner()) =
            Some(fingerprint(&self.destination_provider, &moved.location, cancellation).await?);
        Ok(ExecutionOutcome::Completed)
    }

    async fn cleanup_partial(&self, operation: &Operation) -> Result<(), ExecutionError> {
        self.copy.cleanup_partial(operation).await
    }

    async fn finish(
        &self,
        operation: &Operation,
        cancellation: &CancellationToken,
    ) -> Result<(), ExecutionError> {
        if !*self.fallback.lock().unwrap_or_else(|e| e.into_inner()) {
            return Ok(());
        }
        self.copy.finish(operation, cancellation).await?;
        if cancellation.is_cancelled() {
            return Ok(());
        }
        let source = EntryRef {
            id: EntryId::new(),
            location: self.source.clone(),
        };
        let Some(destination) = self
            .copy
            .completed_root_destination
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        else {
            return Ok(());
        };
        self.destination_provider
            .inspect(
                &EntryRef {
                    id: EntryId::new(),
                    location: destination,
                },
                cancellation.clone(),
            )
            .await?;
        self.source_provider
            .remove(
                &source,
                RemoveOptions {
                    recursive: true,
                    use_trash: false,
                },
                cancellation.clone(),
            )
            .await?;
        Ok(())
    }

    async fn undo_evidence(
        &self,
        operation: &Operation,
        cancellation: &CancellationToken,
    ) -> Result<OperationUndo, ExecutionError> {
        let original_fingerprint = self
            .source_fingerprint
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let Some(original_fingerprint) = original_fingerprint else {
            return Ok(OperationUndo::unavailable(
                "The move did not retain its original source fingerprint.",
            ));
        };
        if !*self
            .fallback
            .lock()
            .unwrap_or_else(|error| error.into_inner())
        {
            let current = self
                .destination_fingerprint
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone();
            return Ok(match current {
                Some(current) => OperationUndo::available(UndoPlan {
                    actions: vec![UndoAction::MoveBack {
                        original: self.source.clone(),
                        original_fingerprint,
                        current,
                    }],
                }),
                None => OperationUndo::unavailable(
                    "The move destination was not completed, so there is nothing safe to undo.",
                ),
            });
        }
        let copy_evidence = self.copy.undo_evidence(operation, cancellation).await?;
        let Some(copy_plan) = copy_evidence.plan else {
            return Ok(copy_evidence);
        };
        let current_entries = copy_plan
            .actions
            .into_iter()
            .flat_map(|action| match action {
                UndoAction::RemoveCreated { entries } => entries,
                _ => Vec::new(),
            })
            .collect::<Vec<_>>();
        let current_root_location = self
            .copy
            .completed_root_destination
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
            .ok_or_else(|| ExecutionError::Failed("move destination evidence is missing".into()))?;
        if self.source.name().ok() != current_root_location.name().ok() {
            return Ok(OperationUndo::unavailable(
                "A collision changed the moved entry name, so cross-provider undo is unavailable.",
            ));
        }
        let current_root = current_entries
            .iter()
            .find(|entry| entry.entry.location == current_root_location)
            .cloned()
            .ok_or_else(|| ExecutionError::Failed("move root fingerprint is missing".into()))?;
        Ok(OperationUndo::available(UndoPlan {
            actions: vec![UndoAction::CrossProviderMoveBack {
                original: self.source.clone(),
                original_fingerprint,
                current_root,
                current_entries,
            }],
        }))
    }
}

#[async_trait]
impl OperationExecutor for CopyExecutor {
    async fn plan(
        &self,
        operation: &Operation,
        cancellation: &CancellationToken,
    ) -> Result<OperationPlan, ExecutionError> {
        let source = if let Some(location) = &self.source_override {
            EntryRef {
                id: EntryId::new(),
                location: location.clone(),
            }
        } else {
            operation
                .sources
                .first()
                .cloned()
                .ok_or_else(|| ExecutionError::Failed("copy source is missing".into()))?
        };
        let summary = self
            .source_provider
            .inspect(&source, cancellation.clone())
            .await?;
        let root_name = self
            .root_name
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .unwrap_or(summary.name.clone());
        let root_destination = self
            .destination_directory
            .join(&root_name)
            .map_err(|error| ExecutionError::Failed(error.to_string()))?;
        // Descendant/same-entry checks are meaningful only within one provider. Cross-provider
        // copies (including local ↔ archive) have disjoint namespaces by construction.
        if source.location.provider_id == root_destination.provider_id {
            fm_operations::validate_paths(&source.location, &root_destination, cfg!(not(windows)))
                .map_err(|error| ExecutionError::Failed(error.to_string()))?;
        }
        let root_source_uri = source.location.uri.clone();
        let mut stack = vec![(summary, root_destination)];
        let mut items = Vec::new();
        let mut planned = HashMap::new();
        let mut directories = Vec::new();
        let mut followed_directories = HashSet::new();
        while let Some((summary, destination)) = stack.pop() {
            if cancellation.is_cancelled() {
                return Err(fm_vfs::VfsError::Cancelled.into());
            }
            let plan_entry = EntryRef {
                id: summary.id,
                location: summary.location.clone(),
            };
            let (summary, followed_target) = if summary.kind == EntryKind::Symlink
                && self.symlink_policy == SymlinkPolicyDto::CopyTarget
            {
                let target = self
                    .source_provider
                    .resolve_symlink(&plan_entry, cancellation.clone())
                    .await?;
                if target.kind == EntryKind::Directory && !followed_directories.insert(target.id) {
                    continue;
                }
                (target, true)
            } else {
                (summary, false)
            };
            if summary.kind == EntryKind::Directory
                && !followed_target
                && self.symlink_policy == SymlinkPolicyDto::CopyTarget
            {
                followed_directories.insert(summary.id);
            }
            let source_entry = EntryRef {
                id: summary.id,
                location: summary.location.clone(),
            };
            let bytes = summary.size.unwrap_or(0);
            planned.insert(
                plan_entry.location.uri.clone(),
                PlannedCopyEntry {
                    kind: summary.kind,
                    destination: destination.clone(),
                    source: source_entry.clone(),
                    is_root: plan_entry.location.uri == root_source_uri,
                },
            );
            items.push(PlanItem::new(plan_entry, bytes));
            if summary.kind == EntryKind::Directory {
                directories.push((
                    source_entry,
                    EntryRef {
                        id: EntryId::new(),
                        location: destination.clone(),
                    },
                ));
                let mut continuation_token = None;
                loop {
                    let page = self
                        .source_provider
                        .list(
                            &summary.location,
                            ListOptions {
                                page_size: 512,
                                continuation_token,
                            },
                            cancellation.clone(),
                        )
                        .await?;
                    for child in page.entries.into_iter().rev() {
                        let child_destination = destination
                            .join(&child.name)
                            .map_err(|error| ExecutionError::Failed(error.to_string()))?;
                        stack.push((child, child_destination));
                    }
                    if !page.has_more {
                        break;
                    }
                    continuation_token = page.continuation_token;
                }
            }
        }
        *self.planned.lock().unwrap_or_else(|e| e.into_inner()) = planned;
        *self.directories.lock().unwrap_or_else(|e| e.into_inner()) = directories;
        Ok(OperationPlan::new(items))
    }

    async fn execute(
        &self,
        operation: &Operation,
        item: &PlanItem,
        resolution: Option<ConflictResolution>,
        progress: &dyn OperationProgressReporter,
        pause: &PauseToken,
        cancellation: &CancellationToken,
    ) -> Result<ExecutionOutcome, ExecutionError> {
        let mut planned = self
            .planned
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&item.entry.location.uri)
            .cloned()
            .ok_or_else(|| ExecutionError::Failed("copy plan entry is missing".into()))?;
        let destination_entry = EntryRef {
            id: EntryId::new(),
            location: planned.destination.clone(),
        };
        let mut reuse_destination_directory = false;
        if let Ok(destination) = self
            .destination_provider
            .inspect(&destination_entry, cancellation.clone())
            .await
        {
            let source = self
                .source_provider
                .inspect(&planned.source, cancellation.clone())
                .await?;
            if source.kind != destination.kind {
                return Err(ExecutionError::Failed(
                    "a file and directory cannot replace one another".into(),
                ));
            }
            match effective_resolution(operation.conflict_policy, resolution) {
                None => return Err(conflict_error(&source, &destination)),
                Some(ConflictResolution::Skip) => return Ok(ExecutionOutcome::Skipped),
                Some(ConflictResolution::Overwrite) => {
                    self.replaced_existing.store(true, Ordering::Release);
                    if planned.kind == EntryKind::Directory {
                        reuse_destination_directory = true;
                    }
                    if planned.kind != EntryKind::File && !reuse_destination_directory {
                        self.destination_provider
                            .remove(
                                &destination_entry,
                                RemoveOptions {
                                    recursive: planned.kind == EntryKind::Directory,
                                    use_trash: false,
                                },
                                cancellation.clone(),
                            )
                            .await?;
                    }
                }
                Some(ConflictResolution::RenameNew) => {
                    let renamed = self
                        .next_copy_destination(&planned.destination, cancellation)
                        .await?;
                    if planned.is_root && planned.kind == EntryKind::Directory {
                        self.rebase_planned_destinations(&planned.destination, &renamed);
                    }
                    planned.destination = renamed;
                }
            }
        }
        let result = if reuse_destination_directory {
            Ok(ExecutionOutcome::Completed)
        } else if planned.kind == EntryKind::Directory {
            let parent = planned
                .destination
                .parent()
                .map_err(|error| ExecutionError::Failed(error.to_string()))?
                .ok_or_else(|| ExecutionError::Failed("copy destination has no parent".into()))?;
            let name = planned
                .destination
                .name()
                .map_err(|error| ExecutionError::Failed(error.to_string()))?;
            self.destination_provider
                .create_directory(&parent, &name, cancellation.clone())
                .await
                .map(|_| ExecutionOutcome::Completed)
                .map_err(ExecutionError::from)
        } else if planned.kind == EntryKind::Symlink {
            self.destination_provider
                .copy_symlink(&item.entry, &planned.destination, cancellation.clone())
                .await
                .map(|_| ExecutionOutcome::Completed)
                .map_err(ExecutionError::from)
        } else {
            let source_item = PlanItem::new(planned.source.clone(), item.bytes);
            self.copy_file(
                &source_item,
                &planned.destination,
                effective_resolution(operation.conflict_policy, resolution),
                progress,
                pause,
                cancellation,
            )
            .await
        };
        let outcome = match result {
            Err(error) if self.continue_on_error && !planned.is_root => {
                Err(ExecutionError::Warning {
                    entry: item.entry.clone(),
                    message: error.to_string(),
                })
            }
            other => other,
        };
        if matches!(&outcome, Ok(ExecutionOutcome::Completed)) && !reuse_destination_directory {
            self.created_destinations
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(planned.destination.clone());
        }

        if outcome.is_ok() && planned.is_root {
            *self
                .completed_root_destination
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = Some(planned.destination);
        }
        outcome
    }

    async fn cleanup_partial(&self, _operation: &Operation) -> Result<(), ExecutionError> {
        let temporary = self
            .temporary
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(temporary) = temporary {
            self.destination_provider
                .discard_copy(&temporary, CancellationToken::new())
                .await?;
        }
        Ok(())
    }

    async fn finish(
        &self,
        _operation: &Operation,
        cancellation: &CancellationToken,
    ) -> Result<(), ExecutionError> {
        let mut directories = self
            .directories
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        directories.reverse();
        for (source, destination) in directories {
            if self.transfer.same_endpoint
                && self
                    .destination_provider
                    .capabilities_for(&destination.location)?
                    .contains(ProviderCapabilities::SET_TIMESTAMPS)
            {
                self.destination_provider
                    .preserve_metadata(&source, &destination, cancellation.clone())
                    .await?;
            }
        }
        Ok(())
    }

    async fn undo_evidence(
        &self,
        _operation: &Operation,
        cancellation: &CancellationToken,
    ) -> Result<OperationUndo, ExecutionError> {
        if self.replaced_existing.load(Ordering::Acquire) {
            return Ok(OperationUndo::unavailable(
                "Undo is unavailable because the copy replaced or merged existing entries.",
            ));
        }
        let locations = self
            .created_destinations
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let mut entries = Vec::with_capacity(locations.len());
        for location in locations {
            entries.push(fingerprint(&self.destination_provider, &location, cancellation).await?);
        }
        Ok(if entries.is_empty() {
            OperationUndo::unavailable("The copy created no entries to undo.")
        } else {
            OperationUndo::available(UndoPlan {
                actions: vec![UndoAction::RemoveCreated { entries }],
            })
        })
    }
}

async fn copy_group_undo_evidence(
    copies: &[CopyExecutor],
    operation: &Operation,
    cancellation: &CancellationToken,
) -> Result<OperationUndo, ExecutionError> {
    let mut actions = Vec::new();
    for copy in copies {
        let evidence = copy.undo_evidence(operation, cancellation).await?;
        match evidence.plan {
            Some(plan) => actions.extend(plan.actions),
            None if evidence.unavailable_reason.as_deref()
                == Some("The copy created no entries to undo.") => {}
            None => return Ok(evidence),
        }
    }
    Ok(if actions.is_empty() {
        OperationUndo::unavailable("The operation created no entries to undo.")
    } else {
        OperationUndo::available(UndoPlan { actions })
    })
}

impl CopyExecutor {
    fn rebase_planned_destinations(&self, old_root: &Location, new_root: &Location) {
        let rebase = |location: &Location| {
            location.uri.strip_prefix(&old_root.uri).map(|suffix| {
                Location::new(
                    location.provider_id.clone(),
                    format!("{}{suffix}", new_root.uri),
                )
            })
        };
        for planned in self
            .planned
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values_mut()
        {
            if let Some(destination) = rebase(&planned.destination) {
                planned.destination = destination;
            }
        }
        for (_, destination) in self
            .directories
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter_mut()
        {
            if let Some(location) = rebase(&destination.location) {
                destination.location = location;
            }
        }
    }

    async fn next_copy_destination(
        &self,
        original: &Location,
        cancellation: &CancellationToken,
    ) -> Result<Location, ExecutionError> {
        let parent = original
            .parent()
            .map_err(|error| ExecutionError::Failed(error.to_string()))?
            .ok_or_else(|| ExecutionError::Failed("copy destination has no parent".into()))?;
        let name = original
            .name()
            .map_err(|error| ExecutionError::Failed(error.to_string()))?;
        for suffix in 1_u32.. {
            let candidate = parent
                .join(&copy_name(&name, suffix))
                .map_err(|error| ExecutionError::Failed(error.to_string()))?;
            let probe = EntryRef {
                id: EntryId::new(),
                location: candidate.clone(),
            };
            match self
                .destination_provider
                .inspect(&probe, cancellation.clone())
                .await
            {
                Err(fm_vfs::VfsError::NotFound { .. }) => return Ok(candidate),
                Ok(_) => {}
                Err(error) => return Err(error.into()),
            }
        }
        unreachable!("u32 suffix iterator is non-empty")
    }

    async fn copy_file(
        &self,
        item: &PlanItem,
        final_destination: &Location,
        effective: Option<ConflictResolution>,
        progress: &dyn OperationProgressReporter,
        pause: &PauseToken,
        cancellation: &CancellationToken,
    ) -> Result<ExecutionOutcome, ExecutionError> {
        let destination_directory = final_destination
            .parent()
            .map_err(|error| ExecutionError::Failed(error.to_string()))?
            .ok_or_else(|| ExecutionError::Failed("copy destination has no parent".into()))?;
        let temporary = self
            .destination_directory
            .join(&format!(".fm-copy-{}", Uuid::new_v4()))
            .map_err(|error| ExecutionError::Failed(error.to_string()))?;
        *self.temporary.lock().unwrap_or_else(|e| e.into_inner()) = Some(temporary.clone());
        // Task 0108: the planner picked the strategy from both sides'
        // capabilities and endpoints; execution never re-derives it.
        let cloned = self.transfer.strategy == TransferStrategy::ServerSideCopy
            && self
                .source_provider
                .server_side_copy(&item.entry, &temporary, cancellation.clone())
                .await?;
        pause.checkpoint().await;
        if !cloned {
            // `file_size` is only ever attempted when the destination
            // actually benefits from a declared size - its
            // `TransferCapabilities` advertised `resumable_upload`
            // (Microsoft Graph's resumable upload sessions are the only
            // such case in this workspace today). Every other destination
            // (local, SFTP, FTP, S3, WebDAV) never pays for, and can never
            // fail because of, a size lookup it has no use for.
            //
            // `item.bytes` is only a *plan-time* snapshot that can go stale
            // between planning and this specific item's execution (a batch
            // operation plans everything up front, then executes items one
            // at a time), so when a size is actually needed it is re-read
            // immediately before opening the reader, as close to the
            // actual transfer as possible (task 0110 review). A failure
            // here - the source vanished, or it simply does not support
            // `file_size` - is not fatal: this specific transfer just falls
            // back to the unknown-size `open_write` path rather than
            // guessing or refusing outright; if the source has genuinely
            // vanished, `open_read` right below reports that instead.
            let expected_size = if self.transfer.destination_resumable_upload {
                self.source_provider
                    .file_size(&item.entry, cancellation.clone())
                    .await
                    .ok()
            } else {
                None
            };
            // Direct source -> destination streaming. Both handles belong to
            // their own provider, so `SFTP -> FTP` (and the reverse) never
            // stages bytes in a local temporary file: the reader pulls from
            // one server while the writer pushes to the other.
            let mut reader = self
                .source_provider
                .open_read(&item.entry, cancellation.clone())
                .await?;
            // Routing a known size through `open_write_sized` (task 0110)
            // rather than the unknown-size `open_write` lets a provider
            // whose native upload protocol needs the total declared
            // upfront drive a real bounded-memory, resumable transfer
            // instead of buffering the whole payload. Every provider that
            // does not override `open_write_sized` keeps working
            // unchanged, since its default forwards straight to
            // `open_write` and ignores the size hint.
            let mut writer = match expected_size {
                Some(expected_size) => {
                    self.destination_provider
                        .open_write_sized(
                            &temporary,
                            WriteOptions::default(),
                            expected_size,
                            cancellation.clone(),
                        )
                        .await?
                }
                None => {
                    self.destination_provider
                        .open_write(&temporary, WriteOptions::default(), cancellation.clone())
                        .await?
                }
            };
            let mut buffer = vec![0_u8; 128 * 1024];
            let transferred = loop {
                pause.checkpoint().await;
                if cancellation.is_cancelled() {
                    break Err(ExecutionError::from(fm_vfs::VfsError::Cancelled));
                }
                let read = tokio::select! {
                    () = cancellation.cancelled() => break Err(fm_vfs::VfsError::Cancelled.into()),
                    result = reader.read(&mut buffer) => match result {
                        Ok(read) => read,
                        Err(error) => break Err(copy_stream_error(error)),
                    },
                };
                if read == 0 {
                    break Ok(());
                }
                tokio::select! {
                    () = cancellation.cancelled() => break Err(fm_vfs::VfsError::Cancelled.into()),
                    result = writer.write_all(&buffer[..read]) => {
                        if let Err(error) = result {
                            break Err(copy_stream_error(error));
                        }
                    }
                }
                progress.report_bytes(read as u64);
            };
            // Cancellation must reach *both* sides, not just this loop:
            // dropping the reader releases the source provider's handle
            // (closing the SFTP file / ending the FTP data connection), and
            // shutting the writer down lets the destination provider finish
            // and release its own transfer before `cleanup_partial` discards
            // the temporary. Neither is best-effort noise: without them a
            // cancelled remote transfer would keep streaming in the
            // background and race the cleanup that follows.
            drop(reader);
            let shutdown = writer.shutdown().await;
            drop(writer);
            transferred?;
            shutdown.map_err(copy_stream_error)?;
        }
        pause.checkpoint().await;
        if cancellation.is_cancelled() {
            return Err(fm_vfs::VfsError::Cancelled.into());
        }

        let overwrite = effective == Some(ConflictResolution::Overwrite);
        let mut destination = final_destination.clone();
        let mut suffix = 1_u32;
        loop {
            match self
                .destination_provider
                .commit_copy(
                    &item.entry,
                    &temporary,
                    &destination,
                    CopyCommitOptions {
                        // Only one backend can meaningfully carry its own
                        // timestamps/permissions across (task 0108): between
                        // two different backends the source's metadata is not
                        // even expressible at the destination.
                        overwrite,
                        preserve_metadata: self.transfer.same_endpoint
                            && self
                                .destination_provider
                                .capabilities_for(&destination)?
                                .contains(ProviderCapabilities::SET_TIMESTAMPS),
                    },
                    cancellation.clone(),
                )
                .await
            {
                Ok(_) => break,
                Err(fm_vfs::VfsError::AlreadyExists { .. })
                    if effective == Some(ConflictResolution::RenameNew) =>
                {
                    let name = final_destination
                        .name()
                        .map_err(|error| ExecutionError::Failed(error.to_string()))?;
                    destination = destination_directory
                        .join(&copy_name(&name, suffix))
                        .map_err(|error| ExecutionError::Failed(error.to_string()))?;
                    suffix = suffix.saturating_add(1);
                }
                Err(fm_vfs::VfsError::AlreadyExists { .. }) if effective.is_none() => {
                    self.destination_provider
                        .discard_copy(&temporary, CancellationToken::new())
                        .await?;
                    *self.temporary.lock().unwrap_or_else(|e| e.into_inner()) = None;
                    let source = self
                        .source_provider
                        .inspect(&item.entry, cancellation.clone())
                        .await?;
                    let destination_summary = self
                        .destination_provider
                        .inspect(
                            &EntryRef {
                                id: EntryId::new(),
                                location: destination,
                            },
                            cancellation.clone(),
                        )
                        .await?;
                    if source.kind != destination_summary.kind {
                        return Err(ExecutionError::Failed(
                            "a file and directory cannot replace one another".into(),
                        ));
                    }
                    return Err(conflict_error(&source, &destination_summary));
                }
                Err(error) => return Err(error.into()),
            }
        }
        *self.temporary.lock().unwrap_or_else(|e| e.into_inner()) = None;
        Ok(ExecutionOutcome::Completed)
    }
}

#[async_trait]
impl OperationExecutor for RenameExecutor {
    async fn plan(
        &self,
        operation: &Operation,
        cancellation: &CancellationToken,
    ) -> Result<OperationPlan, ExecutionError> {
        let entry = operation
            .sources
            .first()
            .cloned()
            .ok_or_else(|| ExecutionError::Failed("rename source is missing".into()))?;
        *self
            .source_fingerprint
            .lock()
            .unwrap_or_else(|error| error.into_inner()) =
            Some(fingerprint(&self.provider, &self.source, cancellation).await?);
        Ok(OperationPlan::new(vec![PlanItem::new(entry, 0)]))
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
        let source = EntryRef {
            id: item.entry.id,
            location: self.source.clone(),
        };
        let destination = self
            .provider
            .rename(&source, &self.destination, cancellation.clone())
            .await?;
        *self
            .destination_fingerprint
            .lock()
            .unwrap_or_else(|error| error.into_inner()) =
            Some(fingerprint(&self.provider, &destination.location, cancellation).await?);
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
        let destination = self
            .destination_fingerprint
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let original_fingerprint = self
            .source_fingerprint
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        Ok(match (original_fingerprint, destination) {
            (Some(original_fingerprint), Some(current)) => OperationUndo::available(UndoPlan {
                actions: vec![UndoAction::MoveBack {
                    original: self.source.clone(),
                    original_fingerprint,
                    current,
                }],
            }),
            _ => OperationUndo::unavailable(
                "The renamed entry was not completed, so there is nothing safe to undo.",
            ),
        })
    }
}

#[async_trait]
impl OperationExecutor for RenameGroupExecutor {
    async fn plan(
        &self,
        _operation: &Operation,
        _cancellation: &CancellationToken,
    ) -> Result<OperationPlan, ExecutionError> {
        Ok(OperationPlan::new(
            self.renames
                .iter()
                .map(|executor| {
                    PlanItem::new(
                        EntryRef {
                            id: EntryId::new(),
                            location: executor.source.clone(),
                        },
                        0,
                    )
                })
                .collect(),
        ))
    }

    async fn execute(
        &self,
        operation: &Operation,
        item: &PlanItem,
        resolution: Option<ConflictResolution>,
        progress: &dyn OperationProgressReporter,
        pause: &PauseToken,
        cancellation: &CancellationToken,
    ) -> Result<ExecutionOutcome, ExecutionError> {
        for executor in &self.renames {
            if executor.source == item.entry.location {
                return executor
                    .execute(operation, item, resolution, progress, pause, cancellation)
                    .await;
            }
        }
        Err(ExecutionError::Failed(
            "rename plan entry is missing".into(),
        ))
    }

    async fn cleanup_partial(&self, _operation: &Operation) -> Result<(), ExecutionError> {
        Ok(())
    }

    async fn undo_evidence(
        &self,
        operation: &Operation,
        cancellation: &CancellationToken,
    ) -> Result<OperationUndo, ExecutionError> {
        let mut actions = Vec::new();
        for executor in &self.renames {
            let evidence = executor.undo_evidence(operation, cancellation).await?;
            let Some(plan) = evidence.plan else {
                return Ok(evidence);
            };
            actions.extend(plan.actions);
        }
        Ok(OperationUndo::available(UndoPlan { actions }))
    }
}

#[async_trait]
impl OperationExecutor for CreateDirectoryExecutor {
    async fn plan(
        &self,
        _operation: &Operation,
        _cancellation: &CancellationToken,
    ) -> Result<OperationPlan, ExecutionError> {
        Ok(OperationPlan::new(vec![PlanItem::new(
            EntryRef {
                id: EntryId::new(),
                location: self.parent.clone(),
            },
            0,
        )]))
    }

    async fn execute(
        &self,
        _operation: &Operation,
        _item: &PlanItem,
        _resolution: Option<ConflictResolution>,
        _progress: &dyn OperationProgressReporter,
        _pause: &PauseToken,
        cancellation: &CancellationToken,
    ) -> Result<ExecutionOutcome, ExecutionError> {
        if !self.create_intermediates {
            self.provider
                .create_directory(&self.parent, &self.name, cancellation.clone())
                .await?;
            return Ok(ExecutionOutcome::Completed);
        }
        let mut parent = self.parent.clone();
        let components = self.name.split(['/', '\\']).collect::<Vec<_>>();
        for (index, component) in components.iter().enumerate() {
            match self
                .provider
                .create_directory(&parent, component, cancellation.clone())
                .await
            {
                Ok(created) => parent = created.location,
                Err(fm_vfs::VfsError::AlreadyExists { .. }) if index + 1 < components.len() => {
                    let location = parent
                        .join(component)
                        .map_err(|error| ExecutionError::Failed(error.to_string()))?;
                    let existing = self
                        .provider
                        .inspect(
                            &EntryRef {
                                id: EntryId::new(),
                                location,
                            },
                            cancellation.clone(),
                        )
                        .await?;
                    if existing.kind != EntryKind::Directory {
                        return Err(ExecutionError::Failed(
                            "an intermediate path component is not a directory".into(),
                        ));
                    }
                    parent = existing.location;
                }
                Err(error) => return Err(error.into()),
            }
        }
        Ok(ExecutionOutcome::Completed)
    }

    async fn cleanup_partial(&self, _operation: &Operation) -> Result<(), ExecutionError> {
        Ok(())
    }
}

#[async_trait]
impl OperationExecutor for CreateFileExecutor {
    async fn plan(
        &self,
        _operation: &Operation,
        _cancellation: &CancellationToken,
    ) -> Result<OperationPlan, ExecutionError> {
        Ok(OperationPlan::new(vec![PlanItem::new(
            EntryRef {
                id: EntryId::new(),
                location: self.parent.clone(),
            },
            0,
        )]))
    }

    async fn execute(
        &self,
        _operation: &Operation,
        _item: &PlanItem,
        _resolution: Option<ConflictResolution>,
        _progress: &dyn OperationProgressReporter,
        _pause: &PauseToken,
        cancellation: &CancellationToken,
    ) -> Result<ExecutionOutcome, ExecutionError> {
        let destination = self
            .parent
            .join(&self.name)
            .map_err(|error| ExecutionError::Failed(error.to_string()))?;
        // Creating an empty file is just opening a writer and shutting it down without
        // writing any bytes, so this reuses the same streaming primitive `copy_file` uses
        // rather than requiring a dedicated provider capability/method.
        let mut writer = self
            .provider
            .open_write(&destination, WriteOptions::default(), cancellation.clone())
            .await?;
        writer.shutdown().await.map_err(copy_stream_error)?;
        drop(writer);
        Ok(ExecutionOutcome::Completed)
    }

    async fn cleanup_partial(&self, _operation: &Operation) -> Result<(), ExecutionError> {
        Ok(())
    }
}

/* -------------------------------------------------------------------------- */
/*  Helper functions                                                          */
/* -------------------------------------------------------------------------- */

fn copy_name(name: &str, suffix: u32) -> String {
    let path = std::path::Path::new(name);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(name);
    match path.extension().and_then(|value| value.to_str()) {
        Some(extension) => format!("{stem} (copy {suffix}).{extension}"),
        None => format!("{stem} (copy {suffix})"),
    }
}

fn effective_resolution(
    policy: fm_operations::ConflictPolicy,
    resolution: Option<ConflictResolution>,
) -> Option<ConflictResolution> {
    resolution.or(match policy {
        fm_operations::ConflictPolicy::Ask => None,
        fm_operations::ConflictPolicy::Skip => Some(ConflictResolution::Skip),
        fm_operations::ConflictPolicy::Overwrite => Some(ConflictResolution::Overwrite),
        fm_operations::ConflictPolicy::RenameNew => Some(ConflictResolution::RenameNew),
        fm_operations::ConflictPolicy::KeepNewer => None,
    })
}

fn conflict_error(
    source: &fm_domain::EntrySummary,
    destination: &fm_domain::EntrySummary,
) -> ExecutionError {
    ExecutionError::Conflict(fm_operations::OperationConflict {
        id: Uuid::new_v4().to_string(),
        source: conflict_entry(source),
        destination: conflict_entry(destination),
    })
}

fn conflict_entry(entry: &fm_domain::EntrySummary) -> fm_operations::ConflictEntry {
    fm_operations::ConflictEntry {
        name: entry.name.clone(),
        kind: entry.kind,
        size: entry.size,
        modified_at: entry.modified_at,
    }
}

fn copy_stream_error(error: std::io::Error) -> ExecutionError {
    fm_vfs::VfsError::Io {
        message: error.to_string(),
    }
    .into()
}

/* -------------------------------------------------------------------------- */
/*  Tests                                                                     */
/* -------------------------------------------------------------------------- */

#[cfg(test)]
mod tests {
    use super::*;
    use fm_vfs::TransferEndpoint;

    struct NoTrashPlatform;

    impl fm_platform::PlatformAdapter for NoTrashPlatform {
        fn capabilities(&self) -> PlatformCapabilities {
            PlatformCapabilities::empty()
        }
    }

    fn make_planner() -> (OperationPlanner, ProviderRegistry) {
        let providers = ProviderRegistry::new();
        let platform: Arc<dyn fm_platform::PlatformAdapter> = Arc::new(NoTrashPlatform);
        let settings = Arc::new(Mutex::new(Settings::default()));
        let planner = OperationPlanner::new(
            providers.clone(),
            platform,
            settings,
            PathBuf::from("/tmp/audit.jsonl"),
            Arc::new(AtomicBool::new(false)),
        );
        (planner, providers)
    }

    fn empty_request() -> StartOperationRequestDto {
        StartOperationRequestDto {
            operation_type: OperationKindDto::Copy,
            sources: Vec::new(),
            destination: None,
            name: None,
            conflict_policy: fm_transport_dto::OperationConflictPolicyDto::Ask,
            symlink_policy: SymlinkPolicyDto::CopyLink,
            archive_format: None,
            archive_compression_level: None,
            create_intermediate_directories: false,
            override_read_only: false,
            permanent_delete_confirmed: false,
            destinations: Vec::new(),
        }
    }

    /* ---------------------------------------------------------------- */
    /*  Task 0108: cross-provider transfer strategy selection            */
    /* ---------------------------------------------------------------- */

    /// Capabilities of a backend that can clone and rename entirely on its own
    /// server (the local filesystem's shape).
    fn native(endpoint: &str) -> TransferCapabilities {
        TransferCapabilities {
            endpoint: TransferEndpoint::new(endpoint),
            server_side_copy: true,
            server_side_move: true,
            resumable_upload: false,
            resumable_download: false,
            random_read: true,
            random_write: false,
        }
    }

    /// Capabilities of a remote backend that can rename in place but has no
    /// server-side clone (the SFTP/FTP shape).
    fn remote(endpoint: &str) -> TransferCapabilities {
        TransferCapabilities {
            endpoint: TransferEndpoint::new(endpoint),
            server_side_copy: false,
            server_side_move: true,
            resumable_upload: false,
            resumable_download: false,
            random_read: false,
            random_write: false,
        }
    }

    #[test]
    fn same_backend_with_a_native_clone_uses_the_server_side_copy() {
        let plan = TransferPlan::select(&native("local"), &native("local"));

        assert_eq!(plan.strategy, TransferStrategy::ServerSideCopy);
        assert_eq!(plan.move_strategy, MoveStrategy::ServerSideMove);
        assert!(plan.same_endpoint);
    }

    #[test]
    fn same_backend_without_a_native_clone_still_streams_but_moves_natively() {
        let plan = TransferPlan::select(&remote("sftp:a"), &remote("sftp:a"));

        assert_eq!(plan.strategy, TransferStrategy::DirectStream);
        assert_eq!(plan.move_strategy, MoveStrategy::ServerSideMove);
        assert!(plan.same_endpoint);
    }

    #[test]
    fn two_connections_of_the_same_provider_type_are_never_one_backend() {
        let plan = TransferPlan::select(&remote("sftp:a"), &remote("sftp:b"));

        assert_eq!(plan.strategy, TransferStrategy::DirectStream);
        assert_eq!(plan.move_strategy, MoveStrategy::CopyThenDelete);
        assert!(!plan.same_endpoint);
    }

    #[test]
    fn a_remote_to_remote_transfer_streams_directly_without_local_staging() {
        for (source, destination) in [
            (remote("sftp:a"), remote("ftp:x")),
            (remote("ftp:x"), remote("sftp:a")),
        ] {
            let plan = TransferPlan::select(&source, &destination);
            assert_eq!(plan.strategy, TransferStrategy::DirectStream);
            assert_eq!(plan.move_strategy, MoveStrategy::CopyThenDelete);
            assert!(!plan.same_endpoint);
        }
    }

    /// `TransferPlan::destination_resumable_upload` is a direct passthrough
    /// of the *destination's* `TransferCapabilities::resumable_upload` -
    /// never the source's, and never inferred from anything else (task 0110
    /// review: `copy_file` only attempts `file_size` when this is `true`).
    #[test]
    fn transfer_plan_carries_only_the_destinations_resumable_upload_capability() {
        let mut resumable_destination = remote("sftp:a");
        resumable_destination.resumable_upload = true;
        let mut resumable_source = remote("sftp:b");
        resumable_source.resumable_upload = true;

        let plan = TransferPlan::select(&remote("sftp:b"), &resumable_destination);
        assert!(plan.destination_resumable_upload);

        let plan = TransferPlan::select(&remote("sftp:b"), &remote("sftp:a"));
        assert!(!plan.destination_resumable_upload);

        // A resumable *source* paired with a non-resumable destination must
        // not flip the flag - only the destination side matters.
        let plan = TransferPlan::select(&resumable_source, &remote("sftp:a"));
        assert!(!plan.destination_resumable_upload);
    }

    /// A single scenario mixing every backend this workspace supports, rather
    /// than one isolated pair per test: the selection must depend only on the
    /// relationship between the two sides, never on the order in which pairs
    /// are evaluated or on which provider type happens to be on the left.
    #[test]
    fn every_direction_pair_across_five_backends_resolves_consistently() {
        let backends = [
            ("local", native("local")),
            ("sftp:a", remote("sftp:a")),
            ("sftp:b", remote("sftp:b")),
            ("ftp:x", remote("ftp:x")),
            ("ftps:x", remote("ftps:x")),
        ];

        let mut server_side_copies = Vec::new();
        let mut server_side_moves = Vec::new();
        for (source_name, source) in &backends {
            for (destination_name, destination) in &backends {
                let plan = TransferPlan::select(source, destination);
                let pair = format!("{source_name} -> {destination_name}");

                // Symmetry: reversing the pair must never change whether the
                // two sides are considered one backend.
                let reversed = TransferPlan::select(destination, source);
                assert_eq!(
                    plan.same_endpoint, reversed.same_endpoint,
                    "endpoint identity must be symmetric for {pair}"
                );
                assert_eq!(
                    plan.same_endpoint,
                    source_name == destination_name,
                    "only identical backends may share an endpoint ({pair})"
                );
                // None of these five backends advertise resumable uploads
                // today (task 0108's own known-limitations note), so the
                // destination's flag must pass through as `false`
                // everywhere in this matrix - the `true` case has its own
                // dedicated test.
                assert!(
                    !plan.destination_resumable_upload,
                    "no backend in this matrix advertises resumable_upload ({pair})"
                );
                // A fast path is never chosen across two different backends.
                if !plan.same_endpoint {
                    assert_eq!(plan.strategy, TransferStrategy::DirectStream, "{pair}");
                    assert_eq!(plan.move_strategy, MoveStrategy::CopyThenDelete, "{pair}");
                }
                if plan.strategy == TransferStrategy::ServerSideCopy {
                    server_side_copies.push(pair.clone());
                }
                if plan.move_strategy == MoveStrategy::ServerSideMove {
                    server_side_moves.push(pair);
                }
            }
        }

        // Exactly one pair in the matrix can clone server-side (local -> local,
        // the only backend advertising `server_side_copy`)...
        assert_eq!(server_side_copies, vec!["local -> local".to_owned()]);
        // ...while every same-backend pair can move server-side.
        assert_eq!(
            server_side_moves,
            vec![
                "local -> local".to_owned(),
                "sftp:a -> sftp:a".to_owned(),
                "sftp:b -> sftp:b".to_owned(),
                "ftp:x -> ftp:x".to_owned(),
                "ftps:x -> ftps:x".to_owned(),
            ]
        );
    }

    #[test]
    fn a_backend_that_cannot_move_natively_falls_back_to_copy_then_delete() {
        let mut immobile = native("archive:/tmp/a.zip");
        immobile.server_side_move = false;

        let plan = TransferPlan::select(&immobile, &immobile);

        assert!(plan.same_endpoint);
        assert_eq!(plan.strategy, TransferStrategy::ServerSideCopy);
        assert_eq!(plan.move_strategy, MoveStrategy::CopyThenDelete);
    }

    #[test]
    fn a_destination_that_cannot_clone_forces_streaming_even_on_one_backend() {
        let source = native("local");
        let mut destination = native("local");
        destination.server_side_copy = false;

        let plan = TransferPlan::select(&source, &destination);

        assert!(plan.same_endpoint);
        assert_eq!(plan.strategy, TransferStrategy::DirectStream);
    }

    #[test]
    fn archive_zip_format_inferred_from_extension() {
        assert_eq!(
            ArchiveCreationFormat::from_request(Path::new("/tmp/archive.zip"), None).ok(),
            Some(ArchiveCreationFormat::Zip),
        );
    }

    #[test]
    fn archive_7z_format_inferred_from_extension() {
        assert_eq!(
            ArchiveCreationFormat::from_request(Path::new("/tmp/archive.7z"), None).ok(),
            Some(ArchiveCreationFormat::SevenZip),
        );
    }

    #[test]
    fn archive_format_mismatch_is_rejected() {
        let result = ArchiveCreationFormat::from_request(
            Path::new("/tmp/archive.zip"),
            Some(ArchiveFormatDto::SevenZip),
        );
        assert!(result.is_err());
    }

    #[test]
    fn archive_unknown_extension_is_rejected() {
        let result = ArchiveCreationFormat::from_request(Path::new("/tmp/archive.tar"), None);
        assert!(result.is_err());
    }

    #[test]
    fn empty_sources_rejected_for_each_operation() {
        let (planner, _) = make_planner();
        let request = empty_request();
        for kind in [
            OperationKindDto::Copy,
            OperationKindDto::Move,
            OperationKindDto::Duplicate,
            OperationKindDto::Delete,
            OperationKindDto::Trash,
            OperationKindDto::CreateArchive,
        ] {
            let mut req = request.clone();
            req.operation_type = kind;
            let result = planner.plan(kind, &req);
            assert!(result.is_err(), "{:?} should reject empty sources", kind);
        }
    }

    #[test]
    fn search_returns_error() {
        let (planner, _) = make_planner();
        let request = empty_request();
        let result = planner.plan(OperationKindDto::Search, &request);
        match result {
            Err(e) => assert!(e.to_string().contains("start_search")),
            Ok(_) => panic!("expected error"),
        }
    }

    #[test]
    fn trash_platform_capability_rejected() {
        let (planner, _) = make_planner();
        let mut request = empty_request();
        request.sources = vec![fm_transport_dto::LocationDto {
            provider_id: "file".into(),
            uri: "file:///some/file".into(),
        }];
        let result = planner.plan(OperationKindDto::Trash, &request);
        match result {
            Err(e) => assert!(e.to_string().contains("TRASH")),
            Ok(_) => panic!("expected error"),
        }
    }

    /// Fake destination provider that gates `open_write` on cancellation
    /// instead of racing a poll loop against a real OS copy call (see
    /// `cancelling_after_the_private_partial_exists_discards_it_and_reports_cancelled`).
    struct GatedDestinationProvider {
        /// Signals the partial destination's path once it exists on disk.
        partial_ready: Mutex<Option<tokio::sync::oneshot::Sender<PathBuf>>>,
        /// Records whether `discard_copy` ran.
        discard_called: AtomicBool,
    }

    #[async_trait]
    impl FileSystemProvider for GatedDestinationProvider {
        fn id(&self) -> ProviderId {
            ProviderId::new("gated-destination-test-double")
        }

        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities::WRITE
        }

        async fn list(
            &self,
            _location: &Location,
            _options: ListOptions,
            _cancellation: CancellationToken,
        ) -> Result<fm_vfs::DirectoryPage, fm_vfs::VfsError> {
            unreachable!("this test never lists the destination")
        }

        async fn metadata(
            &self,
            _entry: &EntryRef,
            _cancellation: CancellationToken,
        ) -> Result<fm_domain::EntryMetadata, fm_vfs::VfsError> {
            unreachable!("this test never reads destination metadata")
        }

        async fn create_directory(
            &self,
            _location: &Location,
            _name: &str,
            _cancellation: CancellationToken,
        ) -> Result<EntryRef, fm_vfs::VfsError> {
            unreachable!("this test copies a file, not a directory")
        }

        async fn rename(
            &self,
            _source: &EntryRef,
            _destination: &Location,
            _cancellation: CancellationToken,
        ) -> Result<EntryRef, fm_vfs::VfsError> {
            unreachable!("this test never renames on the destination")
        }

        async fn remove(
            &self,
            _entry: &EntryRef,
            _options: RemoveOptions,
            _cancellation: CancellationToken,
        ) -> Result<(), fm_vfs::VfsError> {
            unreachable!("this test never removes an existing destination entry")
        }

        async fn open_read(
            &self,
            _entry: &EntryRef,
            _cancellation: CancellationToken,
        ) -> Result<fm_vfs::ProviderReadStream, fm_vfs::VfsError> {
            unreachable!("this provider is only ever used as a copy destination")
        }

        async fn open_write(
            &self,
            destination: &Location,
            _options: WriteOptions,
            cancellation: CancellationToken,
        ) -> Result<fm_vfs::ProviderWriteStream, fm_vfs::VfsError> {
            // Write a real private partial file before gating, so the test
            // can observe it on disk.
            let path = destination.to_native_path().expect("native temp path");
            tokio::fs::write(&path, b"private partial bytes")
                .await
                .expect("write private partial");
            if let Some(sender) = self
                .partial_ready
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take()
            {
                let _ = sender.send(path);
            }
            // Only cancellation resolves this call: no poll loop, no race.
            cancellation.cancelled().await;
            Err(fm_vfs::VfsError::Cancelled)
        }

        async fn discard_copy(
            &self,
            temporary: &Location,
            _cancellation: CancellationToken,
        ) -> Result<(), fm_vfs::VfsError> {
            self.discard_called.store(true, Ordering::SeqCst);
            let path = temporary.to_native_path().expect("native temp path");
            let _ = tokio::fs::remove_file(&path).await;
            Ok(())
        }

        async fn watch(
            &self,
            _location: &Location,
            _cancellation: CancellationToken,
        ) -> Result<fm_vfs::ProviderChangeStream, fm_vfs::VfsError> {
            unreachable!("this test never watches the destination")
        }
    }

    #[tokio::test]
    async fn cancelling_after_the_private_partial_exists_discards_it_and_reports_cancelled() {
        let root = tempfile::tempdir().expect("temp root");
        let source_path = root.path().join("source.bin");
        std::fs::write(&source_path, b"deterministic fixture bytes").expect("write source");
        let destination_directory_path = root.path().join("destination");
        std::fs::create_dir(&destination_directory_path).expect("create destination dir");

        let source_location = Location::from_native_path(&source_path).expect("source location");
        let destination_directory =
            Location::from_native_path(&destination_directory_path).expect("destination location");

        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let gated_provider = Arc::new(GatedDestinationProvider {
            partial_ready: Mutex::new(Some(ready_tx)),
            discard_called: AtomicBool::new(false),
        });
        let destination_provider: Arc<dyn FileSystemProvider> = Arc::clone(&gated_provider) as _;
        let source_provider: Arc<dyn FileSystemProvider> =
            Arc::new(fm_vfs_local::LocalFileSystemProvider::new());

        let executor = Arc::new(CopyExecutor {
            source_provider,
            destination_provider: Arc::clone(&destination_provider),
            destination_directory: destination_directory.clone(),
            temporary: Mutex::new(None),
            planned: Mutex::new(HashMap::new()),
            directories: Mutex::new(Vec::new()),
            symlink_policy: SymlinkPolicyDto::CopyLink,
            root_name: Mutex::new(None),
            source_override: Some(source_location),
            continue_on_error: false,
            completed_root_destination: Mutex::new(None),
            created_destinations: Mutex::new(Vec::new()),
            replaced_existing: AtomicBool::new(false),
            transfer: TransferPlan {
                strategy: TransferStrategy::DirectStream,
                move_strategy: MoveStrategy::CopyThenDelete,
                same_endpoint: false,
                destination_resumable_upload: false,
            },
        });

        let operation = Operation::new(
            fm_operations::OperationKind::Copy,
            Vec::new(),
            Some(destination_directory),
            fm_operations::ConflictPolicy::Ask,
        );
        let cancellation = CancellationToken::new();
        let plan = executor
            .plan(&operation, &cancellation)
            .await
            .expect("plan succeeds");
        let item = plan.items.first().cloned().expect("one planned item");

        let task_executor = Arc::clone(&executor);
        let task_operation = operation.clone();
        let task_item = item.clone();
        let task_cancellation = cancellation.clone();
        let execute_task = tokio::spawn(async move {
            task_executor
                .execute(
                    &task_operation,
                    &task_item,
                    None,
                    &|_| {},
                    &PauseToken::default(),
                    &task_cancellation,
                )
                .await
        });

        // Wait for the partial to exist; no polling, no timing window.
        let partial_path = ready_rx.await.expect("provider reports partial path");
        assert!(
            std::fs::metadata(&partial_path).is_ok(),
            "private partial destination must exist before cancellation"
        );

        cancellation.cancel();

        let outcome = execute_task.await.expect("execute task did not panic");
        assert!(
            matches!(
                outcome,
                Err(ExecutionError::Provider(fm_vfs::VfsError::Cancelled))
            ),
            "cancelling after the private partial exists must report Cancelled, got {outcome:?}"
        );

        executor
            .cleanup_partial(&operation)
            .await
            .expect("cleanup_partial succeeds");

        assert!(
            std::fs::metadata(&partial_path).is_err(),
            "private partial destination must be removed after cancellation"
        );
        assert!(
            gated_provider.discard_called.load(Ordering::SeqCst),
            "discard_copy must actually run during cleanup"
        );
        assert!(
            !destination_directory_path.join("source.bin").exists(),
            "the public destination must never be published"
        );
    }

    /// Fake destination provider recording the `expected_size` it was
    /// called with, and panicking if the unknown-size `open_write` is ever
    /// called instead - proving `copy_file` routes an already-known source
    /// size through `open_write_sized` (task 0110).
    struct SizeRecordingDestinationProvider {
        recorded_expected_size: Mutex<Option<u64>>,
    }

    #[async_trait]
    impl FileSystemProvider for SizeRecordingDestinationProvider {
        fn id(&self) -> ProviderId {
            ProviderId::new("size-recording-test-double")
        }

        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities::WRITE
        }

        async fn list(
            &self,
            _location: &Location,
            _options: ListOptions,
            _cancellation: CancellationToken,
        ) -> Result<fm_vfs::DirectoryPage, fm_vfs::VfsError> {
            unreachable!("this test never lists the destination")
        }

        async fn metadata(
            &self,
            _entry: &EntryRef,
            _cancellation: CancellationToken,
        ) -> Result<fm_domain::EntryMetadata, fm_vfs::VfsError> {
            unreachable!("this test never reads destination metadata")
        }

        async fn create_directory(
            &self,
            _location: &Location,
            _name: &str,
            _cancellation: CancellationToken,
        ) -> Result<EntryRef, fm_vfs::VfsError> {
            unreachable!("this test copies a file, not a directory")
        }

        async fn rename(
            &self,
            _source: &EntryRef,
            _destination: &Location,
            _cancellation: CancellationToken,
        ) -> Result<EntryRef, fm_vfs::VfsError> {
            unreachable!("this test never renames on the destination")
        }

        async fn remove(
            &self,
            _entry: &EntryRef,
            _options: RemoveOptions,
            _cancellation: CancellationToken,
        ) -> Result<(), fm_vfs::VfsError> {
            unreachable!("this test never removes an existing destination entry")
        }

        async fn open_read(
            &self,
            _entry: &EntryRef,
            _cancellation: CancellationToken,
        ) -> Result<fm_vfs::ProviderReadStream, fm_vfs::VfsError> {
            unreachable!("this provider is only ever used as a copy destination")
        }

        async fn open_write(
            &self,
            _destination: &Location,
            _options: WriteOptions,
            _cancellation: CancellationToken,
        ) -> Result<fm_vfs::ProviderWriteStream, fm_vfs::VfsError> {
            unreachable!(
                "copy_file must route an already-known source size through open_write_sized, never the unknown-size open_write"
            )
        }

        async fn open_write_sized(
            &self,
            _destination: &Location,
            _options: WriteOptions,
            expected_size: u64,
            _cancellation: CancellationToken,
        ) -> Result<fm_vfs::ProviderWriteStream, fm_vfs::VfsError> {
            *self
                .recorded_expected_size
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = Some(expected_size);
            Ok(Box::pin(tokio::io::sink()))
        }

        async fn commit_copy(
            &self,
            _source: &EntryRef,
            _temporary: &Location,
            destination: &Location,
            _options: CopyCommitOptions,
            _cancellation: CancellationToken,
        ) -> Result<EntryRef, fm_vfs::VfsError> {
            Ok(EntryRef {
                id: EntryId::new(),
                location: destination.clone(),
            })
        }

        async fn discard_copy(
            &self,
            _temporary: &Location,
            _cancellation: CancellationToken,
        ) -> Result<(), fm_vfs::VfsError> {
            Ok(())
        }

        async fn watch(
            &self,
            _location: &Location,
            _cancellation: CancellationToken,
        ) -> Result<fm_vfs::ProviderChangeStream, fm_vfs::VfsError> {
            unreachable!("this test never watches the destination")
        }
    }

    #[tokio::test]
    async fn copy_file_routes_the_already_known_source_size_through_open_write_sized() {
        let root = tempfile::tempdir().expect("temp root");
        let source_path = root.path().join("source.bin");
        let source_bytes = b"deterministic fixture content for size routing";
        let expected_len = source_bytes.len() as u64;
        std::fs::write(&source_path, source_bytes).expect("write source");
        let destination_directory_path = root.path().join("destination");
        std::fs::create_dir(&destination_directory_path).expect("create destination dir");

        let source_location = Location::from_native_path(&source_path).expect("source location");
        let destination_directory =
            Location::from_native_path(&destination_directory_path).expect("destination location");

        let recording_provider = Arc::new(SizeRecordingDestinationProvider {
            recorded_expected_size: Mutex::new(None),
        });
        let destination_provider: Arc<dyn FileSystemProvider> =
            Arc::clone(&recording_provider) as _;
        let source_provider: Arc<dyn FileSystemProvider> =
            Arc::new(fm_vfs_local::LocalFileSystemProvider::new());

        let executor = CopyExecutor {
            source_provider,
            destination_provider,
            destination_directory: destination_directory.clone(),
            temporary: Mutex::new(None),
            planned: Mutex::new(HashMap::new()),
            directories: Mutex::new(Vec::new()),
            symlink_policy: SymlinkPolicyDto::CopyLink,
            root_name: Mutex::new(None),
            source_override: Some(source_location),
            continue_on_error: false,
            completed_root_destination: Mutex::new(None),
            created_destinations: Mutex::new(Vec::new()),
            replaced_existing: AtomicBool::new(false),
            transfer: TransferPlan {
                strategy: TransferStrategy::DirectStream,
                move_strategy: MoveStrategy::CopyThenDelete,
                same_endpoint: false,
                destination_resumable_upload: true,
            },
        };

        let operation = Operation::new(
            fm_operations::OperationKind::Copy,
            Vec::new(),
            Some(destination_directory),
            fm_operations::ConflictPolicy::Ask,
        );
        let cancellation = CancellationToken::new();
        let plan = executor
            .plan(&operation, &cancellation)
            .await
            .expect("plan succeeds");
        let item = plan.items.first().cloned().expect("one planned item");
        assert_eq!(
            item.bytes, expected_len,
            "the plan must already know the source's real size"
        );
        let reported_bytes = AtomicU64::new(0);

        executor
            .execute(
                &operation,
                &item,
                None,
                &|additional_bytes| {
                    reported_bytes.fetch_add(additional_bytes, Ordering::Relaxed);
                },
                &PauseToken::default(),
                &cancellation,
            )
            .await
            .expect("execute succeeds");

        assert_eq!(
            *recording_provider.recorded_expected_size.lock().unwrap(),
            Some(expected_len),
            "open_write_sized must receive the plan's already-known source size"
        );
        assert_eq!(reported_bytes.load(Ordering::Relaxed), expected_len);
    }

    /// Task 0110 review: `PlanItem::bytes` is only a plan-time snapshot,
    /// which can go stale if the source changes between planning and this
    /// specific item's execution (a batch operation plans everything up
    /// front, then executes items one at a time - possibly much later).
    /// `copy_file` must re-read the size immediately before opening the
    /// destination writer rather than trusting the stale value, so a
    /// provider whose native protocol treats the declared size as an
    /// immutable contract (Microsoft Graph's resumable upload sessions)
    /// never publishes a truncated file for a source that grew, or fails
    /// confusingly for one that shrank.
    #[tokio::test]
    async fn copy_file_uses_the_fresh_execution_time_size_not_a_stale_plan_time_size() {
        let root = tempfile::tempdir().expect("temp root");
        let source_path = root.path().join("source.bin");
        let stale_bytes = b"short";
        std::fs::write(&source_path, stale_bytes).expect("write initial source");
        let destination_directory_path = root.path().join("destination");
        std::fs::create_dir(&destination_directory_path).expect("create destination dir");

        let source_location = Location::from_native_path(&source_path).expect("source location");
        let destination_directory =
            Location::from_native_path(&destination_directory_path).expect("destination location");

        let recording_provider = Arc::new(SizeRecordingDestinationProvider {
            recorded_expected_size: Mutex::new(None),
        });
        let destination_provider: Arc<dyn FileSystemProvider> =
            Arc::clone(&recording_provider) as _;
        let source_provider: Arc<dyn FileSystemProvider> =
            Arc::new(fm_vfs_local::LocalFileSystemProvider::new());

        let executor = CopyExecutor {
            source_provider,
            destination_provider,
            destination_directory: destination_directory.clone(),
            temporary: Mutex::new(None),
            planned: Mutex::new(HashMap::new()),
            directories: Mutex::new(Vec::new()),
            symlink_policy: SymlinkPolicyDto::CopyLink,
            root_name: Mutex::new(None),
            source_override: Some(source_location),
            continue_on_error: false,
            completed_root_destination: Mutex::new(None),
            created_destinations: Mutex::new(Vec::new()),
            replaced_existing: AtomicBool::new(false),
            transfer: TransferPlan {
                strategy: TransferStrategy::DirectStream,
                move_strategy: MoveStrategy::CopyThenDelete,
                same_endpoint: false,
                destination_resumable_upload: true,
            },
        };

        let operation = Operation::new(
            fm_operations::OperationKind::Copy,
            Vec::new(),
            Some(destination_directory),
            fm_operations::ConflictPolicy::Ask,
        );
        let cancellation = CancellationToken::new();
        let plan = executor
            .plan(&operation, &cancellation)
            .await
            .expect("plan succeeds");
        let item = plan.items.first().cloned().expect("one planned item");
        assert_eq!(
            item.bytes,
            stale_bytes.len() as u64,
            "sanity: plan captured the original size"
        );

        // The source changes *after* planning but *before* this item is
        // executed - e.g. another process appended to it, or a much
        // earlier item in a large batch delayed this one.
        let fresh_bytes = b"a much longer replacement payload written after planning";
        std::fs::write(&source_path, fresh_bytes).expect("mutate source after planning");
        assert_ne!(
            stale_bytes.len(),
            fresh_bytes.len(),
            "the test fixture must actually change size, or this proves nothing"
        );

        executor
            .execute(
                &operation,
                &item,
                None,
                &|_| {},
                &PauseToken::default(),
                &cancellation,
            )
            .await
            .expect("execute succeeds");

        assert_eq!(
            *recording_provider.recorded_expected_size.lock().unwrap(),
            Some(fresh_bytes.len() as u64),
            "open_write_sized must receive the size as of execution time, not the stale plan-time size"
        );
    }

    /// Source provider that reports `EntrySummary`/reads bytes directly
    /// from disk (so a size mutation between planning and execution is
    /// genuinely observed) and counts every `file_size` call it receives.
    struct CountingSourceProvider {
        file_size_calls: std::sync::atomic::AtomicUsize,
    }

    #[async_trait]
    impl FileSystemProvider for CountingSourceProvider {
        fn id(&self) -> ProviderId {
            ProviderId::new("counting-source-test-double")
        }

        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities::READ
        }

        async fn list(
            &self,
            _location: &Location,
            _options: ListOptions,
            _cancellation: CancellationToken,
        ) -> Result<fm_vfs::DirectoryPage, fm_vfs::VfsError> {
            unreachable!("this test copies a single file, never lists")
        }

        async fn metadata(
            &self,
            _entry: &EntryRef,
            _cancellation: CancellationToken,
        ) -> Result<fm_domain::EntryMetadata, fm_vfs::VfsError> {
            unreachable!("this test never reads extended metadata")
        }

        async fn inspect(
            &self,
            entry: &EntryRef,
            _cancellation: CancellationToken,
        ) -> Result<fm_domain::EntrySummary, fm_vfs::VfsError> {
            let path = entry.location.to_native_path().expect("native path");
            let metadata =
                tokio::fs::metadata(&path)
                    .await
                    .map_err(|error| fm_vfs::VfsError::Io {
                        message: error.to_string(),
                    })?;
            Ok(fm_domain::EntrySummary {
                id: entry.id,
                location: entry.location.clone(),
                name: entry.location.name().unwrap_or_default(),
                kind: EntryKind::File,
                size: Some(metadata.len()),
                modified_at: None,
                created_at: None,
                hidden: false,
                read_only: false,
                extension: None,
                mime_type: None,
                icon_key: None,
                metadata_revision: 0,
                git_status: None,
            })
        }

        async fn file_size(
            &self,
            entry: &EntryRef,
            _cancellation: CancellationToken,
        ) -> Result<u64, fm_vfs::VfsError> {
            self.file_size_calls.fetch_add(1, Ordering::SeqCst);
            let path = entry.location.to_native_path().expect("native path");
            let metadata =
                tokio::fs::metadata(&path)
                    .await
                    .map_err(|error| fm_vfs::VfsError::Io {
                        message: error.to_string(),
                    })?;
            Ok(metadata.len())
        }

        async fn create_directory(
            &self,
            _location: &Location,
            _name: &str,
            _cancellation: CancellationToken,
        ) -> Result<EntryRef, fm_vfs::VfsError> {
            unreachable!("this test never creates a directory on the source")
        }

        async fn rename(
            &self,
            _source: &EntryRef,
            _destination: &Location,
            _cancellation: CancellationToken,
        ) -> Result<EntryRef, fm_vfs::VfsError> {
            unreachable!("this test never renames the source")
        }

        async fn remove(
            &self,
            _entry: &EntryRef,
            _options: RemoveOptions,
            _cancellation: CancellationToken,
        ) -> Result<(), fm_vfs::VfsError> {
            unreachable!("this test never removes the source")
        }

        async fn open_read(
            &self,
            entry: &EntryRef,
            _cancellation: CancellationToken,
        ) -> Result<fm_vfs::ProviderReadStream, fm_vfs::VfsError> {
            let path = entry.location.to_native_path().expect("native path");
            let file =
                tokio::fs::File::open(&path)
                    .await
                    .map_err(|error| fm_vfs::VfsError::Io {
                        message: error.to_string(),
                    })?;
            Ok(Box::pin(file))
        }

        async fn open_write(
            &self,
            _destination: &Location,
            _options: WriteOptions,
            _cancellation: CancellationToken,
        ) -> Result<fm_vfs::ProviderWriteStream, fm_vfs::VfsError> {
            unreachable!("this provider is only ever used as a copy source")
        }

        async fn watch(
            &self,
            _location: &Location,
            _cancellation: CancellationToken,
        ) -> Result<fm_vfs::ProviderChangeStream, fm_vfs::VfsError> {
            unreachable!("this test never watches the source")
        }
    }

    /// Source provider identical to [`CountingSourceProvider`] except it
    /// never overrides `file_size` at all, inheriting
    /// `FileSystemProvider::file_size`'s default
    /// (`UnsupportedCapability`) - simulating a provider that genuinely
    /// cannot report a size.
    struct NoFileSizeSourceProvider;

    #[async_trait]
    impl FileSystemProvider for NoFileSizeSourceProvider {
        fn id(&self) -> ProviderId {
            ProviderId::new("no-file-size-source-test-double")
        }

        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities::READ
        }

        async fn list(
            &self,
            _location: &Location,
            _options: ListOptions,
            _cancellation: CancellationToken,
        ) -> Result<fm_vfs::DirectoryPage, fm_vfs::VfsError> {
            unreachable!("this test copies a single file, never lists")
        }

        async fn metadata(
            &self,
            _entry: &EntryRef,
            _cancellation: CancellationToken,
        ) -> Result<fm_domain::EntryMetadata, fm_vfs::VfsError> {
            unreachable!("this test never reads extended metadata")
        }

        async fn inspect(
            &self,
            entry: &EntryRef,
            _cancellation: CancellationToken,
        ) -> Result<fm_domain::EntrySummary, fm_vfs::VfsError> {
            let path = entry.location.to_native_path().expect("native path");
            let metadata =
                tokio::fs::metadata(&path)
                    .await
                    .map_err(|error| fm_vfs::VfsError::Io {
                        message: error.to_string(),
                    })?;
            Ok(fm_domain::EntrySummary {
                id: entry.id,
                location: entry.location.clone(),
                name: entry.location.name().unwrap_or_default(),
                kind: EntryKind::File,
                size: Some(metadata.len()),
                modified_at: None,
                created_at: None,
                hidden: false,
                read_only: false,
                extension: None,
                mime_type: None,
                icon_key: None,
                metadata_revision: 0,
                git_status: None,
            })
        }

        // `file_size` deliberately not overridden - inherits the
        // `UnsupportedCapability` default.

        async fn create_directory(
            &self,
            _location: &Location,
            _name: &str,
            _cancellation: CancellationToken,
        ) -> Result<EntryRef, fm_vfs::VfsError> {
            unreachable!("this test never creates a directory on the source")
        }

        async fn rename(
            &self,
            _source: &EntryRef,
            _destination: &Location,
            _cancellation: CancellationToken,
        ) -> Result<EntryRef, fm_vfs::VfsError> {
            unreachable!("this test never renames the source")
        }

        async fn remove(
            &self,
            _entry: &EntryRef,
            _options: RemoveOptions,
            _cancellation: CancellationToken,
        ) -> Result<(), fm_vfs::VfsError> {
            unreachable!("this test never removes the source")
        }

        async fn open_read(
            &self,
            entry: &EntryRef,
            _cancellation: CancellationToken,
        ) -> Result<fm_vfs::ProviderReadStream, fm_vfs::VfsError> {
            let path = entry.location.to_native_path().expect("native path");
            let file =
                tokio::fs::File::open(&path)
                    .await
                    .map_err(|error| fm_vfs::VfsError::Io {
                        message: error.to_string(),
                    })?;
            Ok(Box::pin(file))
        }

        async fn open_write(
            &self,
            _destination: &Location,
            _options: WriteOptions,
            _cancellation: CancellationToken,
        ) -> Result<fm_vfs::ProviderWriteStream, fm_vfs::VfsError> {
            unreachable!("this provider is only ever used as a copy source")
        }

        async fn watch(
            &self,
            _location: &Location,
            _cancellation: CancellationToken,
        ) -> Result<fm_vfs::ProviderChangeStream, fm_vfs::VfsError> {
            unreachable!("this test never watches the source")
        }
    }

    /// Destination provider with a real `open_write` (counted) but an
    /// `open_write_sized` that panics if ever called - proving `copy_file`
    /// never routes through the sized path for a destination that has no
    /// use for a declared size, or when a size could not be obtained.
    struct OpenWriteOnlyDestinationProvider {
        open_write_calls: std::sync::atomic::AtomicUsize,
    }

    #[async_trait]
    impl FileSystemProvider for OpenWriteOnlyDestinationProvider {
        fn id(&self) -> ProviderId {
            ProviderId::new("open-write-only-test-double")
        }

        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities::WRITE
        }

        async fn list(
            &self,
            _location: &Location,
            _options: ListOptions,
            _cancellation: CancellationToken,
        ) -> Result<fm_vfs::DirectoryPage, fm_vfs::VfsError> {
            unreachable!("this test never lists the destination")
        }

        async fn metadata(
            &self,
            _entry: &EntryRef,
            _cancellation: CancellationToken,
        ) -> Result<fm_domain::EntryMetadata, fm_vfs::VfsError> {
            unreachable!("this test never reads destination metadata")
        }

        async fn create_directory(
            &self,
            _location: &Location,
            _name: &str,
            _cancellation: CancellationToken,
        ) -> Result<EntryRef, fm_vfs::VfsError> {
            unreachable!("this test copies a file, not a directory")
        }

        async fn rename(
            &self,
            _source: &EntryRef,
            _destination: &Location,
            _cancellation: CancellationToken,
        ) -> Result<EntryRef, fm_vfs::VfsError> {
            unreachable!("this test never renames on the destination")
        }

        async fn remove(
            &self,
            _entry: &EntryRef,
            _options: RemoveOptions,
            _cancellation: CancellationToken,
        ) -> Result<(), fm_vfs::VfsError> {
            unreachable!("this test never removes an existing destination entry")
        }

        async fn open_read(
            &self,
            _entry: &EntryRef,
            _cancellation: CancellationToken,
        ) -> Result<fm_vfs::ProviderReadStream, fm_vfs::VfsError> {
            unreachable!("this provider is only ever used as a copy destination")
        }

        async fn open_write(
            &self,
            _destination: &Location,
            _options: WriteOptions,
            _cancellation: CancellationToken,
        ) -> Result<fm_vfs::ProviderWriteStream, fm_vfs::VfsError> {
            self.open_write_calls.fetch_add(1, Ordering::SeqCst);
            Ok(Box::pin(tokio::io::sink()))
        }

        async fn open_write_sized(
            &self,
            _destination: &Location,
            _options: WriteOptions,
            _expected_size: u64,
            _cancellation: CancellationToken,
        ) -> Result<fm_vfs::ProviderWriteStream, fm_vfs::VfsError> {
            unreachable!(
                "open_write_sized must never be called when the destination is not resumable, \
                 or when file_size could not be obtained"
            )
        }

        async fn commit_copy(
            &self,
            _source: &EntryRef,
            _temporary: &Location,
            destination: &Location,
            _options: CopyCommitOptions,
            _cancellation: CancellationToken,
        ) -> Result<EntryRef, fm_vfs::VfsError> {
            Ok(EntryRef {
                id: EntryId::new(),
                location: destination.clone(),
            })
        }

        async fn watch(
            &self,
            _location: &Location,
            _cancellation: CancellationToken,
        ) -> Result<fm_vfs::ProviderChangeStream, fm_vfs::VfsError> {
            unreachable!("this test never watches the destination")
        }
    }

    fn copy_only_executor(
        source_provider: Arc<dyn FileSystemProvider>,
        destination_provider: Arc<dyn FileSystemProvider>,
        source_location: Location,
        destination_directory: Location,
        destination_resumable_upload: bool,
    ) -> CopyExecutor {
        CopyExecutor {
            source_provider,
            destination_provider,
            destination_directory,
            temporary: Mutex::new(None),
            planned: Mutex::new(HashMap::new()),
            directories: Mutex::new(Vec::new()),
            symlink_policy: SymlinkPolicyDto::CopyLink,
            root_name: Mutex::new(None),
            source_override: Some(source_location),
            continue_on_error: false,
            completed_root_destination: Mutex::new(None),
            created_destinations: Mutex::new(Vec::new()),
            replaced_existing: AtomicBool::new(false),
            transfer: TransferPlan {
                strategy: TransferStrategy::DirectStream,
                move_strategy: MoveStrategy::CopyThenDelete,
                same_endpoint: false,
                destination_resumable_upload,
            },
        }
    }

    /// Task 0110 review: the shared copy path must not unconditionally
    /// require or call `file_size` for every destination - only a
    /// destination that actually advertised `resumable_upload` benefits
    /// from a declared size.
    #[tokio::test]
    async fn copy_file_never_calls_file_size_and_uses_plain_open_write_when_the_destination_is_not_resumable()
     {
        let root = tempfile::tempdir().expect("temp root");
        let source_path = root.path().join("source.bin");
        std::fs::write(&source_path, b"irrelevant content").expect("write source");
        let destination_directory_path = root.path().join("destination");
        std::fs::create_dir(&destination_directory_path).expect("create destination dir");

        let source_location = Location::from_native_path(&source_path).expect("source location");
        let destination_directory =
            Location::from_native_path(&destination_directory_path).expect("destination location");

        let source_provider = Arc::new(CountingSourceProvider {
            file_size_calls: std::sync::atomic::AtomicUsize::new(0),
        });
        let destination_provider = Arc::new(OpenWriteOnlyDestinationProvider {
            open_write_calls: std::sync::atomic::AtomicUsize::new(0),
        });

        let executor = copy_only_executor(
            Arc::clone(&source_provider) as _,
            Arc::clone(&destination_provider) as _,
            source_location,
            destination_directory.clone(),
            false,
        );

        let operation = Operation::new(
            fm_operations::OperationKind::Copy,
            Vec::new(),
            Some(destination_directory),
            fm_operations::ConflictPolicy::Ask,
        );
        let cancellation = CancellationToken::new();
        let plan = executor
            .plan(&operation, &cancellation)
            .await
            .expect("plan succeeds");
        let item = plan.items.first().cloned().expect("one planned item");

        executor
            .execute(
                &operation,
                &item,
                None,
                &|_| {},
                &PauseToken::default(),
                &cancellation,
            )
            .await
            .expect("execute succeeds");

        assert_eq!(
            source_provider.file_size_calls.load(Ordering::SeqCst),
            0,
            "a non-resumable destination must never trigger a file_size call"
        );
        assert_eq!(
            destination_provider.open_write_calls.load(Ordering::SeqCst),
            1,
            "the unknown-size open_write must have been used"
        );
    }

    /// Task 0110 review: when the destination *is* resumable but the
    /// source cannot report a size (`file_size` returns
    /// `UnsupportedCapability`), the copy must still succeed by falling
    /// back to the unknown-size `open_write`, not fail outright.
    #[tokio::test]
    async fn copy_file_falls_back_to_open_write_when_the_source_cannot_report_a_file_size() {
        let root = tempfile::tempdir().expect("temp root");
        let source_path = root.path().join("source.bin");
        std::fs::write(&source_path, b"irrelevant content").expect("write source");
        let destination_directory_path = root.path().join("destination");
        std::fs::create_dir(&destination_directory_path).expect("create destination dir");

        let source_location = Location::from_native_path(&source_path).expect("source location");
        let destination_directory =
            Location::from_native_path(&destination_directory_path).expect("destination location");

        let source_provider = Arc::new(NoFileSizeSourceProvider);
        let destination_provider = Arc::new(OpenWriteOnlyDestinationProvider {
            open_write_calls: std::sync::atomic::AtomicUsize::new(0),
        });

        let executor = copy_only_executor(
            source_provider as _,
            Arc::clone(&destination_provider) as _,
            source_location,
            destination_directory.clone(),
            true,
        );

        let operation = Operation::new(
            fm_operations::OperationKind::Copy,
            Vec::new(),
            Some(destination_directory),
            fm_operations::ConflictPolicy::Ask,
        );
        let cancellation = CancellationToken::new();
        let plan = executor
            .plan(&operation, &cancellation)
            .await
            .expect("plan succeeds");
        let item = plan.items.first().cloned().expect("one planned item");

        executor
            .execute(
                &operation,
                &item,
                None,
                &|_| {},
                &PauseToken::default(),
                &cancellation,
            )
            .await
            .expect("execute succeeds even though the source could not report a size");

        assert_eq!(
            destination_provider.open_write_calls.load(Ordering::SeqCst),
            1,
            "a failed file_size lookup must fall back to the unknown-size open_write"
        );
    }
}
