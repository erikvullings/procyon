//! The `FileManagerService` facade (specification §7).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use fm_archive::ArchiveFileSystemProvider;
use fm_checksum::{ChecksumEngine, ChecksumResultsStore, DuplicateResultsStore};
use fm_comparison::{ComparisonEngine, ComparisonResultsStore};
use fm_connections::{ConnectionService, JsonFileConnectionRepository};
use fm_credentials::{CredentialStore, InMemoryCredentialStore, SessionCredentialStore};
use fm_domain::OperationId;
use fm_domain::{ActionId, DirectorySnapshot, EntryMetadata, Location, PaneId};
use fm_events::{
    BackendEventPayload, EventAudience, EventBus, NotificationLevelPayload, NotificationPayload,
};
use fm_operations::{Scheduler, SchedulerError};
use fm_platform::{FallbackPlatformAdapter, PlatformAdapter};
use fm_plugin_runtime::{PluginDiscovery, PluginRuntime};
use fm_search::{SearchEngine, SearchFileSystemProvider, SearchResultsStore};
use fm_search_acceleration::{SearchAcceleration, UnsupportedSearchAccelerator};
use fm_settings::{Settings, SettingsStore};
use fm_transport_dto::{
    ActionDescriptorDto, ActionResultDto, ApplicationUninstallCandidateDto,
    ApplySyncPlanRequestDto, ApplySyncPlanResponseDto, ArchiveSummaryRequestDto,
    ArchiveSummaryResponseDto, ChecksumFileDto, ChecksumPageDto, ComparisonPageDto,
    ConflictResolutionDto, ConnectionDto, CreateConnectionRequestDto, DeleteLlmProfileRequestDto,
    DeleteRagConversationRequestDto, DirectorySnapshotDto,
    DiscoverApplicationUninstallCandidatesRequestDto,
    DiscoverApplicationUninstallCandidatesResponseDto, DuplicatePageDto, EntryMetadataRequest,
    FinderTagsDto, GenerateRagAnswerRequestDto, GenerateRagAnswerResponseDto,
    GenerateSyncPlanRequestDto, GetFileGitHistoryRequestDto, GetFileGitHistoryResponseDto,
    InvokeActionRequestDto, ListDirectoryRequest, LlmProfileDto, LlmProfileExportDto,
    LlmProfilePresetDto, LlmProfileTestResultDto, NavigateRequest, OperationDto,
    PluginDescriptorDto, PluginLogEntryDto, PreviewRagRequestDto, RagPreviewDto, RagScopeDto,
    RagScopeKindDto, ReadFileRangeRequestDto, ReadFileRangeResponseDto,
    RemoveApplicationDockIconRequestDto, RemoveApplicationDockIconResponseDto,
    RenderChecksumFileRequestDto, ResolveOperationConflictRequestDto, ResolveRagCitationRequestDto,
    ResolvedRagCitationDto, RuntimeCapabilitiesDto, RuntimeKindDto, SaveLlmProfileRequestDto,
    SaveRagConversationRequestDto, SavedRagConversationDto, SearchInFileRequestDto,
    SearchInFileResponseDto, SetPaneActivityRequest, SettingsDto, SpotlightCommentDto,
    StartChecksumRequestDto, StartChecksumResponseDto, StartComparisonRequestDto,
    StartComparisonResponseDto, StartDuplicateScanRequestDto, StartDuplicateScanResponseDto,
    StartOperationRequestDto, StartSearchRequestDto, StartSearchResponseDto, SyncPlanDto,
    UpdateConnectionRequestDto, VerificationReportDto, VerifyChecksumFileRequestDto,
    WorkspaceCommandDto, WorkspaceDto, WorkspaceSummaryDto,
};
use fm_vfs::ProviderRegistry;
use fm_vfs_local::LocalFileSystemProvider;
use uuid::Uuid;

use crate::DirectoryService;
use crate::action::ActionRegistry;
use crate::action_invoker::ActionInvoker;
use crate::checksum_coordinator::ChecksumCoordinator;
use crate::connection_facade::ConnectionFacade;
use crate::content_streaming;
use crate::disk_usage_coordinator::DiskUsageCoordinator;
use crate::document_conversion::DocumentConversionService;
use crate::document_summary::{
    DocumentSummaryCapability, DocumentSummaryCoordinator, GenerateDocumentSummary,
    UnavailableDocumentSummaryCapability,
};
use crate::document_summary_mapping::{
    preview_to_dto, summary_error_to_application, summary_to_dto,
};
use crate::docx_preview::DocxPreviewService;
use crate::error::ApplicationError;
use crate::file_editor::FileEditorService;
use crate::llm_profile_mapping::{
    disposition_from_dto, draft_from_dto, preset_to_profile_dto, profile_to_dto,
    profile_to_export_dto, test_result_to_dto,
};
use crate::llm_profiles::{LlmHostPolicy, LlmProfileService, ReqwestLlmProbeTransport};
use crate::operation_history::{ApplicationOperationObserver, OperationHistory};
use crate::operation_planner::OperationPlanner;
use crate::operation_requests::map_scheduler_error;
use crate::operations_coordinator::OperationsCoordinator;
use crate::platform_mapping::{
    self, action_capabilities_for_runtime, discover_system_locations, discover_volumes,
    map_platform_error, runtime_capabilities_dto, runtime_capabilities_dto_with_semantic,
    volume_capacity,
};
use crate::plugin_manager::PluginManager;
use crate::pptx_preview::PptxPreviewService;
use crate::rag::{
    AuthorizedRagRequest, GenerateRagAnswer, RagAnswerEvent, RagConversationStore, RagCoordinator,
    RagCoverage, RagHistoryTurn, RagRetrievalCapability, RagSourceDisplay, SavedRagConversation,
    SavedRagTurn, UnavailableRagRetrievalCapability,
};
use crate::rag_mapping::{
    events_to_dto, preview_to_dto as rag_preview_to_dto, rag_error_to_application, saved_to_dto,
    scope_from_dto,
};
use crate::remote_terminal::RemoteTerminalService;
use crate::search_comparison_coordinator::SearchComparisonCoordinator;
use crate::semantic::{
    DocumentIngestion, SemanticCapability, SemanticError, SemanticHealth, SemanticIngestionJob,
    SemanticJobId, SemanticOperationId, SemanticProgressEvent, SemanticQuery, SemanticScope,
    SemanticSearchResult, SemanticService,
};
use crate::semantic_components::{
    AdministratorProvisionedSemanticComponentCapability, FakeSemanticComponentCapability,
    RemoveSemanticIndexRequest, SemanticComponentCapabilities, SemanticComponentCapability,
    SemanticComponentError, SemanticComponentOperation, SemanticComponentService,
    SemanticComponentStatus, SemanticDataMoveReceipt, SemanticIndexRemovalConfirmation,
    SemanticIndexRemovalPlan, SemanticIndexRemovalReceipt, SemanticIndexRetentionDecision,
    SemanticInstallReceipt, SemanticInstallationConsent, SemanticInstallationOffer,
    SemanticLocalModelImportRequest, SemanticModelMigrationCheckpoint,
    SemanticModelMigrationConfirmation, SemanticModelMigrationId, SemanticModelMigrationPlan,
    SemanticModelMigrationProgress, SemanticModelProfile, SemanticModelSelection, SemanticProfile,
    SemanticReindexEstimate, SemanticUninstallReceipt, SemanticWorkerPatchRequest,
};
use crate::semantic_indexing::{
    SemanticIndexingError, SemanticIndexingReport, SemanticIndexingService,
};
use crate::semantic_library::SemanticLibraryComposition;
use crate::semantic_library::{
    RagScopeSelection, SemanticAccessContext, SemanticEnrolmentPreview, SemanticExclusionPlan,
    SemanticFeedCandidate, SemanticFolderContext, SemanticFolderStatus,
    SemanticLibraryCapabilities, SemanticLibraryError, SemanticLibraryOperation,
    SemanticLibraryService, SemanticLibraryStatus, SemanticWorkerFeedPlan,
};
use crate::settings_mapping::{settings_from_dto, settings_to_dto};
use crate::structured_view::StructuredViewService;
use crate::thumbnails::ThumbnailService;
use crate::workspace::{JsonFileWorkspaceRepository, WorkspaceService, WorkspaceSummary};

/// Central application service that every host (Axum, Tauri, CLI) calls into.
///
/// Only the capabilities needed by the current milestone are implemented; the
/// remaining fields from the specification's example facade (directories,
/// operations, actions, plugins, events) are added incrementally as their
/// crates land, rather than stubbed out ahead of time.
///
/// Holds a concrete [`WorkspaceService<JsonFileWorkspaceRepository>`] rather
/// than being generic over the repository type: making this facade generic
/// would propagate a type parameter into every host's `AppState`, for no
/// benefit since every host uses the same JSON-file-backed repository.
pub struct FileManagerService {
    runtime: RuntimeKindDto,
    platform: Arc<dyn PlatformAdapter>,
    workspaces: WorkspaceService<JsonFileWorkspaceRepository>,
    connections: ConnectionFacade,
    llm_profiles: LlmProfileService,
    onedrive: crate::onedrive::OneDriveAuthorizationService<JsonFileConnectionRepository>,
    remote_terminals: RemoteTerminalService,
    directories: DirectoryService,
    editor: FileEditorService,
    docx_preview: DocxPreviewService,
    document_conversion: DocumentConversionService,
    document_summaries: DocumentSummaryCoordinator,
    rag: RagCoordinator,
    rag_conversation_path: PathBuf,
    semantic_vocabulary_path: PathBuf,
    rag_ephemeral: Mutex<HashMap<Uuid, SavedRagConversation>>,
    pptx_preview: PptxPreviewService,
    structured_view: StructuredViewService,
    providers: ProviderRegistry,
    archive_provider: Arc<ArchiveFileSystemProvider>,
    events: EventBus,
    settings_store: SettingsStore,
    settings: Arc<Mutex<Settings>>,
    plugin_manager: PluginManager,
    operations: OperationsCoordinator,
    action_invoker: ActionInvoker,
    search_comparison: SearchComparisonCoordinator,
    checksums: ChecksumCoordinator,
    disk_usage: DiskUsageCoordinator,
    thumbnails: ThumbnailService,
    semantic: SemanticService,
    semantic_indexing: SemanticIndexingService,
    semantic_components: SemanticComponentService,
    semantic_library: SemanticLibraryComposition,
}

impl FileManagerService {
    /// Discovers OS-managed locations and maps their native paths to the existing local provider.
    pub async fn system_locations(
        &self,
    ) -> Result<Vec<fm_transport_dto::SystemLocationDto>, ApplicationError> {
        discover_system_locations(Arc::clone(&self.platform)).await
    }

    /// Discovers currently mounted local/removable/disk-image volumes (task
    /// 0144) and maps their native paths to the existing local provider.
    pub async fn volumes(&self) -> Result<Vec<fm_transport_dto::VolumeDto>, ApplicationError> {
        discover_volumes(Arc::clone(&self.platform)).await
    }

    /// Returns a reference to the platform adapter, for platform-specific
    /// operations like native menu installation (task 0131, Windows).
    pub fn platform_adapter(&self) -> Arc<dyn PlatformAdapter> {
        Arc::clone(&self.platform)
    }

    /// Returns the current user's home directory as a native path, or `None` if it cannot be
    /// determined. Lets the frontend expand a leading `~` typed into an address bar, mirroring
    /// the `~` convention `ssh.rs` already honors for SFTP connection dialogs - the local
    /// filesystem provider otherwise has no such expansion at any layer.
    pub fn home_directory(&self) -> Option<String> {
        dirs::home_dir().map(|path| path.to_string_lossy().into_owned())
    }

    /// Builds a service for the given host runtime, persisting workspaces
    /// under `workspace_directory`.
    pub fn new(
        runtime: RuntimeKindDto,
        workspace_directory: impl Into<PathBuf>,
        settings_directory: impl Into<PathBuf>,
    ) -> Self {
        Self::with_event_bus(
            runtime,
            workspace_directory,
            settings_directory,
            EventBus::default(),
        )
    }

    /// Builds a service using a caller-provided event bus.
    ///
    /// Uses [`FallbackPlatformAdapter`]; hosts with real native integration
    /// available should call [`Self::with_platform_adapter`] instead.
    pub fn with_event_bus(
        runtime: RuntimeKindDto,
        workspace_directory: impl Into<PathBuf>,
        settings_directory: impl Into<PathBuf>,
        events: EventBus,
    ) -> Self {
        Self::with_platform_adapter(
            runtime,
            workspace_directory,
            settings_directory,
            events,
            Arc::new(FallbackPlatformAdapter),
        )
    }

    /// Builds a service using a caller-provided event bus and platform
    /// adapter.
    ///
    /// [`Self::runtime_capabilities`] derives its native-integration flags
    /// from `platform`, so the frontend responds to capabilities rather than
    /// detecting the operating system itself (spec §21). Browser/server mode
    /// should pass [`FallbackPlatformAdapter`] (it has no native access to a
    /// remote client's OS); a desktop host should pass a real per-OS adapter
    /// once one exists.
    ///
    /// Credentials are stored through [`InMemoryCredentialStore`] - a real
    /// host on macOS/Windows must call
    /// [`Self::with_platform_adapter_and_credential_store`] instead to get
    /// Keychain/Credential Manager-backed storage (task 0103's acceptance
    /// criterion); this constructor exists for callers (mainly this crate's
    /// own tests) that do not care which credential store is used.
    pub fn with_platform_adapter(
        runtime: RuntimeKindDto,
        workspace_directory: impl Into<PathBuf>,
        settings_directory: impl Into<PathBuf>,
        events: EventBus,
        platform: Arc<dyn PlatformAdapter>,
    ) -> Self {
        Self::with_platform_adapter_and_credential_store(
            runtime,
            workspace_directory,
            settings_directory,
            events,
            platform,
            Arc::new(InMemoryCredentialStore::new()),
        )
    }

    /// Builds a service using a caller-provided event bus, platform adapter
    /// and credential store.
    ///
    /// Every real host (`apps/fm-server`, `apps/fm-desktop`) should call this
    /// constructor with the OS-appropriate [`CredentialStore`] (`dev.fm`'s
    /// `fm-credentials-macos`/`fm-credentials-windows`, selected the same way
    /// each host already selects its [`PlatformAdapter`] - see each host's
    /// `credentials` module), not [`Self::with_platform_adapter`], which
    /// defaults to the non-protected in-memory store.
    pub fn with_platform_adapter_and_credential_store(
        runtime: RuntimeKindDto,
        workspace_directory: impl Into<PathBuf>,
        settings_directory: impl Into<PathBuf>,
        events: EventBus,
        platform: Arc<dyn PlatformAdapter>,
        credential_store: Arc<dyn CredentialStore>,
    ) -> Self {
        Self::with_platform_adapter_and_credential_store_and_search_accelerator(
            runtime,
            workspace_directory,
            settings_directory,
            events,
            platform,
            credential_store,
            Arc::new(UnsupportedSearchAccelerator),
        )
    }

    /// Builds a service with separately injected platform and native-search
    /// adapters. The accelerator is intentionally not part of
    /// [`PlatformAdapter`]: index search is a narrow optional optimization,
    /// not a VFS or general platform capability.
    pub fn with_platform_adapter_and_credential_store_and_search_accelerator(
        runtime: RuntimeKindDto,
        workspace_directory: impl Into<PathBuf>,
        settings_directory: impl Into<PathBuf>,
        events: EventBus,
        platform: Arc<dyn PlatformAdapter>,
        credential_store: Arc<dyn CredentialStore>,
        search_accelerator: Arc<dyn SearchAcceleration>,
    ) -> Self {
        let settings_directory = settings_directory.into();
        let rag_conversation_path = settings_directory.join("rag-conversations.json");
        let semantic_vocabulary_path = settings_directory.join("semantic-vocabularies.json");
        let credential_store: Arc<dyn CredentialStore> =
            Arc::new(SessionCredentialStore::new(credential_store));
        let mut providers = ProviderRegistry::new();
        providers.register(Arc::new(LocalFileSystemProvider));
        let archive_provider = Arc::new(ArchiveFileSystemProvider::new());
        providers.register(archive_provider.clone());
        let search_store = Arc::new(SearchResultsStore::new());
        providers.register(Arc::new(SearchFileSystemProvider::new(Arc::clone(
            &search_store,
        ))));
        // SSH/SFTP (task 0104, spec §6). `known_hosts` is a sibling of
        // `connections` under the same settings directory, following that
        // repository's own convention; `ssh_connections` is shared between
        // the dialer (connect/test from the Connections UI) and the SFTP
        // provider (browsing), so a successful connect/test and a later
        // browse reuse the same pooled session instead of dialing twice.
        let ssh_known_hosts = Arc::new(fm_ssh::JsonFileKnownHostsStore::new(
            settings_directory.join("ssh-known-hosts.json"),
        ));
        let ssh_connections = Arc::new(fm_ssh::SshConnectionManager::new(ssh_known_hosts));
        providers.register(Arc::new(fm_vfs_sftp::SftpFileSystemProvider::new(
            ssh_connections.clone(),
            Arc::new(crate::ssh::SshResolver::new(
                JsonFileConnectionRepository::new(settings_directory.join("connections")),
                credential_store.clone(),
            )),
        )));
        providers.register(Arc::new(fm_vfs_ftp::FtpFileSystemProvider::new(Arc::new(
            crate::ftp::FtpResolver::new(
                JsonFileConnectionRepository::new(settings_directory.join("connections")),
                credential_store.clone(),
            ),
        ))));
        providers.register(Arc::new(fm_vfs_s3::S3FileSystemProvider::new(Arc::new(
            crate::s3::S3Resolver::new(
                JsonFileConnectionRepository::new(settings_directory.join("connections")),
                credential_store.clone(),
            ),
        ))));
        providers.register(Arc::new(fm_vfs_webdav::WebDavFileSystemProvider::new(
            Arc::new(crate::webdav::WebDavResolver::new(
                JsonFileConnectionRepository::new(settings_directory.join("connections")),
                credential_store.clone(),
            )),
        )));
        // Native OneDrive (task 0110). `onedrive_token_resolver` is shared
        // between the `FileSystemProvider` (browsing), the `ConnectionDialer`
        // registered below (connect/test) and `OneDriveAuthorizationService`
        // (authorization completion re-verifying `/me/drive`) - one cache,
        // one per-connection refresh serialization, for every code path that
        // needs a currently valid Graph bearer token for a given connection.
        let onedrive_config = crate::onedrive::OneDriveAuthorizationServiceConfig::production();
        let onedrive_token_resolver = crate::onedrive::token_resolver(
            Arc::new(JsonFileConnectionRepository::new(
                settings_directory.join("connections"),
            )),
            credential_store.clone(),
            onedrive_config.oauth.clone(),
            onedrive_config.http.clone(),
        );
        providers.register(Arc::new(fm_vfs_onedrive::OneDriveFileSystemProvider::new(
            onedrive_token_resolver.clone(),
        )));
        // A second, independently-constructed `SshResolver` for the embedded
        // terminal's remote shell channel (task 0105) - safe to construct
        // separately for the same reason `SshResolver::new` above is: a
        // stateless, file-per-connection repository with no in-memory cache
        // to desynchronize.
        let remote_terminals = RemoteTerminalService::new(
            ssh_connections.clone(),
            Arc::new(crate::ssh::SshResolver::new(
                JsonFileConnectionRepository::new(settings_directory.join("connections")),
                credential_store.clone(),
            )),
        );
        let settings_store = SettingsStore::new(&settings_directory);
        let llm_transport = Arc::new(ReqwestLlmProbeTransport::new());
        let llm_policy = match runtime {
            RuntimeKindDto::BrowserServer => LlmHostPolicy::server(
                std::env::var("PROCYON_LLM_ALLOWED_HOSTS")
                    .unwrap_or_default()
                    .split(',')
                    .map(str::trim)
                    .filter(|host| !host.is_empty())
                    .map(str::to_owned),
            ),
            RuntimeKindDto::Tauri | RuntimeKindDto::Mock => LlmHostPolicy::desktop(),
        };
        let llm_profiles = LlmProfileService::new(
            settings_store.clone(),
            credential_store.clone(),
            llm_transport.clone(),
            llm_policy.clone(),
        )
        .unwrap_or_else(|_| {
            events.publish(
                EventAudience::Global,
                BackendEventPayload::NotificationCreated {
                    notification: NotificationPayload {
                        id: Uuid::new_v4().to_string(),
                        level: NotificationLevelPayload::Warning,
                        message:
                            "LLM profiles could not be read. No generation profiles were loaded."
                                .to_owned(),
                    },
                },
            );
            LlmProfileService::empty(
                settings_store.clone(),
                credential_store.clone(),
                llm_transport,
                llm_policy,
            )
        });
        let loaded = settings_store
            .load()
            .unwrap_or_else(|_| fm_settings::LoadOutcome {
                settings: Settings::default(),
                warning: Some(
                    "Settings could not be read. Application defaults were loaded.".into(),
                ),
            });
        if let Some(message) = loaded.warning {
            events.publish(
                EventAudience::Global,
                BackendEventPayload::NotificationCreated {
                    notification: NotificationPayload {
                        id: Uuid::new_v4().to_string(),
                        level: NotificationLevelPayload::Warning,
                        message,
                    },
                },
            );
        }
        let operation_concurrency = loaded.settings.operation_concurrency;
        let operation_history = Arc::new(OperationHistory::load(&settings_directory));
        let directories = DirectoryService::with_event_bus(providers.clone(), events.clone());
        let operation_observer = Arc::new(ApplicationOperationObserver::new(
            operation_history.clone(),
            directories.clone(),
        ));
        let platform_capabilities =
            action_capabilities_for_runtime(runtime, platform.capabilities());
        let search = SearchEngine::with_accelerator(
            search_store,
            events.clone(),
            providers.clone(),
            search_accelerator,
        );
        let comparison_store = Arc::new(ComparisonResultsStore::new());
        let comparison = ComparisonEngine::new(
            Arc::clone(&comparison_store),
            events.clone(),
            providers.clone(),
        );
        let semantic = SemanticService::unavailable();
        let semantic_indexing = SemanticIndexingService::new(providers.clone());
        let search_comparison = SearchComparisonCoordinator::new(
            search,
            comparison,
            comparison_store,
            events.clone(),
            semantic.clone(),
        );
        let checksum_store = Arc::new(ChecksumResultsStore::new());
        let duplicate_store = Arc::new(DuplicateResultsStore::new());
        let checksum = ChecksumEngine::new(
            Arc::clone(&checksum_store),
            Arc::clone(&duplicate_store),
            events.clone(),
            providers.clone(),
        );
        let checksums =
            ChecksumCoordinator::new(checksum, checksum_store, duplicate_store, events.clone());
        let disk_usage = DiskUsageCoordinator::new(events.clone());
        let audit_log_path = settings_directory.join("audit.jsonl");
        let settings_mutex = Arc::new(Mutex::new(loaded.settings));
        let force_cross_volume_moves = Arc::new(AtomicBool::new(false));
        let planner = OperationPlanner::new(
            providers.clone(),
            Arc::clone(&platform),
            Arc::clone(&settings_mutex),
            audit_log_path.clone(),
            Arc::clone(&force_cross_volume_moves),
        );
        let plugin_manager = PluginManager::new(
            PluginDiscovery::new(settings_directory.join("plugins")).with_bundled_directory(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins"),
            ),
            PluginRuntime::default(),
            Arc::clone(&settings_mutex),
            settings_store.clone(),
            events.clone(),
        );
        let action_invoker = ActionInvoker::new(
            ActionRegistry::with_core_actions(platform_capabilities),
            Arc::clone(&platform),
            Arc::clone(&settings_mutex),
        );
        let connection_service = Arc::new(
            ConnectionService::new(
                JsonFileConnectionRepository::new(settings_directory.join("connections")),
                credential_store,
                events.clone(),
            )
            .with_dialer(
                fm_connections::ConnectionKind::Ssh,
                Arc::new(crate::ssh::SshDialer::new(ssh_connections.clone())),
            )
            .with_dialer(
                fm_connections::ConnectionKind::Ftp,
                Arc::new(crate::ftp::FtpDialer),
            )
            .with_dialer(
                fm_connections::ConnectionKind::Ftps,
                Arc::new(crate::ftp::FtpDialer),
            )
            .with_dialer(
                fm_connections::ConnectionKind::S3,
                Arc::new(crate::s3::S3Dialer),
            )
            .with_dialer(
                fm_connections::ConnectionKind::WebDav,
                Arc::new(crate::webdav::WebDavDialer),
            )
            .with_dialer(
                fm_connections::ConnectionKind::OneDrive,
                crate::onedrive::dialer(
                    Arc::clone(&onedrive_token_resolver),
                    onedrive_config.graph_base_url.clone(),
                    onedrive_config.http.clone(),
                ),
            ),
        );
        let onedrive = crate::onedrive::OneDriveAuthorizationService::new(
            Arc::clone(&connection_service),
            onedrive_token_resolver,
            onedrive_config,
        );
        Self {
            runtime,
            platform,
            workspaces: WorkspaceService::new(JsonFileWorkspaceRepository::new(
                workspace_directory,
            )),
            connections: ConnectionFacade::new(connection_service, ssh_connections),
            llm_profiles,
            onedrive,
            remote_terminals,
            directories,
            editor: FileEditorService::new(providers.clone(), audit_log_path.clone()),
            docx_preview: DocxPreviewService::new(providers.clone()),
            document_conversion: DocumentConversionService::new(providers.clone()),
            document_summaries: DocumentSummaryCoordinator::new(Arc::new(
                UnavailableDocumentSummaryCapability,
            )),
            rag: RagCoordinator::new(Arc::new(UnavailableRagRetrievalCapability)),
            rag_conversation_path,
            semantic_vocabulary_path,
            rag_ephemeral: Mutex::new(HashMap::new()),
            pptx_preview: PptxPreviewService::new(providers.clone()),
            structured_view: StructuredViewService::new(providers.clone()),
            providers,
            archive_provider,
            events: events.clone(),
            settings_store: settings_store.clone(),
            settings: settings_mutex.clone(),
            plugin_manager,
            operations: OperationsCoordinator::new(
                Scheduler::new(operation_concurrency, events).with_observer(operation_observer),
                operation_history,
                planner,
                force_cross_volume_moves,
            ),
            action_invoker,
            search_comparison,
            checksums,
            disk_usage,
            thumbnails: ThumbnailService::new(settings_directory.join("thumbnails")),
            semantic,
            semantic_indexing,
            semantic_components: match runtime {
                RuntimeKindDto::BrowserServer => SemanticComponentService::new(Arc::new(
                    AdministratorProvisionedSemanticComponentCapability::new(
                        SemanticComponentStatus::absent(None),
                        Vec::new(),
                    ),
                )),
                RuntimeKindDto::Mock => {
                    SemanticComponentService::new(Arc::new(FakeSemanticComponentCapability::new()))
                }
                // Desktop management is injection-ready, but remains unavailable
                // until the host supplies an evaluated signed catalog and real adapters.
                RuntimeKindDto::Tauri => SemanticComponentService::unavailable(),
            },
            semantic_library: SemanticLibraryComposition::new(runtime, settings_directory),
        }
    }

    /// Converts one document into normalized structural units for semantic
    /// retrieval (task 0180).
    ///
    /// Thin delegation to the conversion service: there is deliberately no
    /// HTTP route or Tauri command behind this yet. Ingestion decides what to
    /// convert and what to do with the result (task 0182).
    pub async fn convert_document_for_semantics(
        &self,
        location: fm_domain::Location,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<fm_semantic_conversion::ConversionOutcome, ApplicationError> {
        self.document_conversion
            .convert(location, cancellation)
            .await
    }

    /// Reports semantic-library authority and the operations this caller may
    /// perform, without touching storage.
    pub async fn semantic_library_capabilities(
        &self,
        access: &SemanticAccessContext,
    ) -> SemanticLibraryCapabilities {
        self.semantic_library().await.capabilities(access)
    }

    /// Lists device-local vocabularies after applying the semantic-library authority check.
    pub async fn list_semantic_vocabularies(
        &self,
        access: &SemanticAccessContext,
    ) -> Result<Vec<fm_transport_dto::SemanticVocabularyDto>, ApplicationError> {
        self.semantic_library()
            .await
            .status(access)
            .map_err(crate::semantic_vocabulary_mapping::library_error)?;
        crate::semantic_vocabulary::VocabularyStore::open(&self.semantic_vocabulary_path)
            .map_err(crate::semantic_vocabulary_mapping::vocabulary_error)?
            .list()
            .map(|items| {
                items
                    .into_iter()
                    .map(crate::semantic_vocabulary_mapping::vocabulary_to_dto)
                    .collect()
            })
            .map_err(crate::semantic_vocabulary_mapping::vocabulary_error)
    }

    /// Imports one validated deterministic SKOS JSON source.
    pub async fn import_semantic_vocabulary(
        &self,
        access: &SemanticAccessContext,
        request: fm_transport_dto::ImportSemanticVocabularyRequestDto,
    ) -> Result<fm_transport_dto::SemanticVocabularyDto, ApplicationError> {
        self.semantic_library()
            .await
            .status(access)
            .map_err(crate::semantic_vocabulary_mapping::library_error)?;
        crate::semantic_vocabulary::VocabularyStore::open(&self.semantic_vocabulary_path)
            .map_err(crate::semantic_vocabulary_mapping::vocabulary_error)?
            .import(&request.skos_json)
            .map(crate::semantic_vocabulary_mapping::vocabulary_to_dto)
            .map_err(crate::semantic_vocabulary_mapping::vocabulary_error)
    }

    /// Exports the authoritative source and accepted edits without derived annotations.
    pub async fn export_semantic_vocabulary(
        &self,
        access: &SemanticAccessContext,
        vocabulary_id: &str,
    ) -> Result<fm_transport_dto::ExportSemanticVocabularyResponseDto, ApplicationError> {
        self.semantic_library()
            .await
            .status(access)
            .map_err(crate::semantic_vocabulary_mapping::library_error)?;
        let vocabulary =
            crate::semantic_vocabulary::VocabularyStore::open(&self.semantic_vocabulary_path)
                .map_err(crate::semantic_vocabulary_mapping::vocabulary_error)?
                .get(vocabulary_id)
                .map_err(crate::semantic_vocabulary_mapping::vocabulary_error)?;
        Ok(fm_transport_dto::ExportSemanticVocabularyResponseDto {
            skos_json: crate::semantic_vocabulary::export_skos_json(&vocabulary)
                .map_err(crate::semantic_vocabulary_mapping::vocabulary_error)?,
        })
    }

    /// Attaches a vocabulary only to roots/workspaces already authorized for this caller.
    pub async fn attach_semantic_vocabulary(
        &self,
        access: &SemanticAccessContext,
        request: fm_transport_dto::AttachSemanticVocabularyRequestDto,
    ) -> Result<fm_transport_dto::SemanticVocabularyDto, ApplicationError> {
        let status = self
            .semantic_library()
            .await
            .status(access)
            .map_err(crate::semantic_vocabulary_mapping::library_error)?;
        if request
            .root_id
            .as_ref()
            .is_some_and(|root_id| !status.roots.iter().any(|root| root.id == *root_id))
            || request.workspace_id.as_ref().is_some_and(|workspace_id| {
                !status.roots.iter().any(|root| {
                    root.workspace_references
                        .iter()
                        .any(|id| id.to_string() == *workspace_id)
                })
            })
        {
            return Err(ApplicationError::PermissionDenied);
        }
        crate::semantic_vocabulary::VocabularyStore::open(&self.semantic_vocabulary_path)
            .map_err(crate::semantic_vocabulary_mapping::vocabulary_error)?
            .update(&request.vocabulary_id, |vocabulary| {
                vocabulary.attach(request.workspace_id, request.root_id)
            })
            .map(crate::semantic_vocabulary_mapping::vocabulary_to_dto)
            .map_err(crate::semantic_vocabulary_mapping::vocabulary_error)
    }

    /// Applies an explicit accept/edit/reject decision to a review-only candidate.
    pub async fn review_semantic_concept_candidate(
        &self,
        access: &SemanticAccessContext,
        request: fm_transport_dto::ReviewConceptCandidateRequestDto,
    ) -> Result<fm_transport_dto::SemanticVocabularyDto, ApplicationError> {
        self.semantic_library()
            .await
            .status(access)
            .map_err(crate::semantic_vocabulary_mapping::library_error)?;
        use fm_transport_dto::ReviewConceptCandidateActionDto;
        let decision = match request.action {
            ReviewConceptCandidateActionDto::Accept => {
                crate::semantic_vocabulary::ReviewDecision::Accept
            }
            ReviewConceptCandidateActionDto::Reject => {
                crate::semantic_vocabulary::ReviewDecision::Reject
            }
            ReviewConceptCandidateActionDto::Edit => {
                crate::semantic_vocabulary::ReviewDecision::AcceptEdited {
                    concept_uri: request.concept_uri.ok_or_else(|| {
                        ApplicationError::InvalidRequest("concept URI is required".into())
                    })?,
                    pref_label: request.preferred_label.ok_or_else(|| {
                        ApplicationError::InvalidRequest("preferred label is required".into())
                    })?,
                }
            }
        };
        crate::semantic_vocabulary::VocabularyStore::open(&self.semantic_vocabulary_path)
            .map_err(crate::semantic_vocabulary_mapping::vocabulary_error)?
            .update(&request.vocabulary_id, |vocabulary| {
                vocabulary.review_candidate(&request.candidate_id, decision)
            })
            .map(crate::semantic_vocabulary_mapping::vocabulary_to_dto)
            .map_err(crate::semantic_vocabulary_mapping::vocabulary_error)
    }

    /// Previews or confirms vocabulary deletion, reporting every affected attachment.
    pub async fn delete_semantic_vocabulary(
        &self,
        access: &SemanticAccessContext,
        request: fm_transport_dto::DeleteSemanticVocabularyRequestDto,
    ) -> Result<fm_transport_dto::DeleteSemanticVocabularyImpactDto, ApplicationError> {
        self.semantic_library()
            .await
            .status(access)
            .map_err(crate::semantic_vocabulary_mapping::library_error)?;
        let store =
            crate::semantic_vocabulary::VocabularyStore::open(&self.semantic_vocabulary_path)
                .map_err(crate::semantic_vocabulary_mapping::vocabulary_error)?;
        let vocabulary = store
            .get(&request.vocabulary_id)
            .map_err(crate::semantic_vocabulary_mapping::vocabulary_error)?;
        let requires_confirmation =
            !vocabulary.workspace_ids.is_empty() || !vocabulary.root_ids.is_empty();
        let impact = fm_transport_dto::DeleteSemanticVocabularyImpactDto {
            vocabulary_id: request.vocabulary_id.clone(),
            affected_workspace_ids: vocabulary.workspace_ids.iter().cloned().collect(),
            affected_root_ids: vocabulary.root_ids.iter().cloned().collect(),
            requires_confirmation,
            deleted: request.confirm_affected || !requires_confirmation,
        };
        if impact.deleted {
            store
                .delete(&request.vocabulary_id)
                .map_err(crate::semantic_vocabulary_mapping::vocabulary_error)?;
        }
        Ok(impact)
    }

    pub(crate) async fn ensure_semantic_library_operation(
        &self,
        access: &SemanticAccessContext,
        operation: SemanticLibraryOperation,
    ) -> Result<(), SemanticLibraryError> {
        self.semantic_library()
            .await
            .ensure_operation_allowed(access, operation)
    }

    /// Reports the safe semantic-library policy/catalog/state projection.
    ///
    /// # Errors
    ///
    /// Returns an authorization, lock, or persistence failure.
    pub async fn semantic_library_status(
        &self,
        access: &SemanticAccessContext,
    ) -> Result<SemanticLibraryStatus, SemanticLibraryError> {
        self.semantic_library().await.status(access)
    }

    /// Replaces the composed default with an explicitly configured library
    /// service.
    #[must_use]
    pub fn with_semantic_library_service(
        mut self,
        semantic_library: SemanticLibraryService,
    ) -> Self {
        self.semantic_library = SemanticLibraryComposition::fixed(semantic_library);
        self
    }

    /// Resolves the composed semantic-library capability.
    ///
    /// Desktop composition is deferred: a device-local library only exists once
    /// managed components (task 0178) report an installed data root and an
    /// active model, because only then are its roots and immutable embedding
    /// identity known backend-authoritatively. Until then the capability is
    /// explicitly unavailable rather than rooted at an invented path.
    async fn semantic_library(&self) -> Arc<SemanticLibraryService> {
        self.semantic_library
            .resolve(&self.semantic_components)
            .await
    }

    /// Reports effective consent for the verified active folder.
    ///
    /// # Errors
    ///
    /// Returns an authority, authorization, workspace, lock, or persistence
    /// failure.
    pub async fn semantic_library_folder_status(
        &self,
        access: &SemanticAccessContext,
        context: SemanticFolderContext,
    ) -> Result<SemanticFolderStatus, SemanticLibraryError> {
        let status = self
            .semantic_library()
            .await
            .folder_status(access, &context)?;
        self.ensure_active_semantic_folder(&context).await?;
        Ok(status)
    }

    /// Creates an enrolment disclosure for the verified active folder.
    ///
    /// # Errors
    ///
    /// Returns an authority, authorization, workspace, lock, or persistence
    /// failure.
    pub async fn semantic_library_preview_enrolment(
        &self,
        access: &SemanticAccessContext,
        context: SemanticFolderContext,
        recursive: bool,
    ) -> Result<SemanticEnrolmentPreview, SemanticLibraryError> {
        let library = self.semantic_library().await;
        library.ensure_operation_allowed(access, SemanticLibraryOperation::PreviewEnrolment)?;
        self.ensure_active_semantic_folder(&context).await?;
        library.preview_enrolment(access, context, recursive)
    }

    /// Confirms one live enrolment disclosure for the still-active folder.
    ///
    /// # Errors
    ///
    /// Returns an authority, authorization, workspace, stale-revision,
    /// stale-confirmation, lock, or persistence failure.
    pub async fn semantic_library_confirm_enrolment(
        &self,
        access: &SemanticAccessContext,
        confirmation_id: &str,
        expected_revision: u64,
        context: SemanticFolderContext,
    ) -> Result<SemanticLibraryStatus, SemanticLibraryError> {
        let library = self.semantic_library().await;
        library.ensure_operation_allowed(access, SemanticLibraryOperation::Enrol)?;
        self.ensure_active_semantic_folder(&context).await?;
        library.confirm_enrolment(access, confirmation_id, expected_revision, &context)
    }

    /// Creates an authoritative destructive exclusion plan.
    ///
    /// # Errors
    ///
    /// Returns an authority, authorization, workspace, consent-state,
    /// stale-revision, lock, or persistence failure.
    pub async fn semantic_library_plan_exclusion(
        &self,
        access: &SemanticAccessContext,
        context: SemanticFolderContext,
        expected_revision: u64,
    ) -> Result<SemanticExclusionPlan, SemanticLibraryError> {
        let library = self.semantic_library().await;
        library.ensure_operation_allowed(access, SemanticLibraryOperation::PlanExclusion)?;
        self.ensure_active_semantic_folder(&context).await?;
        library.plan_exclusion(access, context, expected_revision)
    }

    /// Confirms one exclusion plan for the still-active folder.
    ///
    /// # Errors
    ///
    /// Returns an authority, authorization, workspace, stale-revision,
    /// stale-confirmation, lock, or persistence failure.
    pub async fn semantic_library_confirm_exclusion(
        &self,
        access: &SemanticAccessContext,
        confirmation_id: &str,
        expected_revision: u64,
        context: SemanticFolderContext,
    ) -> Result<SemanticLibraryStatus, SemanticLibraryError> {
        let library = self.semantic_library().await;
        library.ensure_operation_allowed(access, SemanticLibraryOperation::ConfirmExclusion)?;
        self.ensure_active_semantic_folder(&context).await?;
        library.confirm_exclusion(access, confirmation_id, expected_revision, &context)
    }

    /// Resumes one authoritative cleanup plan.
    ///
    /// # Errors
    ///
    /// Returns an authority, authorization, not-found, stale-revision, lock,
    /// or persistence failure.
    pub async fn semantic_library_resume_cleanup(
        &self,
        access: &SemanticAccessContext,
        plan_id: fm_semantic_library::DeletionPlanId,
        expected_revision: u64,
    ) -> Result<SemanticLibraryStatus, SemanticLibraryError> {
        self.semantic_library()
            .await
            .resume_cleanup(access, plan_id, expected_revision)
    }

    /// Pauses ingestion while preserving consent and indexed data.
    ///
    /// # Errors
    ///
    /// Returns an authority, authorization, stale-revision, lock, or
    /// persistence failure.
    pub async fn semantic_library_pause(
        &self,
        access: &SemanticAccessContext,
        expected_revision: u64,
    ) -> Result<SemanticLibraryStatus, SemanticLibraryError> {
        self.semantic_library()
            .await
            .pause(access, expected_revision)
    }

    /// Resumes semantic ingestion.
    ///
    /// # Errors
    ///
    /// Returns an authority, authorization, stale-revision, lock, or
    /// persistence failure.
    pub async fn semantic_library_resume(
        &self,
        access: &SemanticAccessContext,
        expected_revision: u64,
    ) -> Result<SemanticLibraryStatus, SemanticLibraryError> {
        self.semantic_library()
            .await
            .resume(access, expected_revision)
    }

    /// Replaces fixed, safe eligibility overrides for an attached root.
    ///
    /// # Errors
    ///
    /// Returns an authority, authorization, workspace, unsafe-override,
    /// not-found, stale-revision, lock, or persistence failure.
    pub async fn semantic_library_update_eligibility_overrides(
        &self,
        access: &SemanticAccessContext,
        root_id: fm_semantic_library::RootId,
        workspace_id: fm_domain::WorkspaceId,
        expected_revision: u64,
        overrides: std::collections::BTreeMap<
            fm_semantic_library::EligibilityReason,
            fm_semantic_library::EligibilityOverride,
        >,
    ) -> Result<SemanticLibraryStatus, SemanticLibraryError> {
        let library = self.semantic_library().await;
        library.ensure_operation_allowed(
            access,
            SemanticLibraryOperation::UpdateEligibilityOverrides,
        )?;
        self.workspaces
            .load(workspace_id)
            .await
            .map_err(|_| SemanticLibraryError::WorkspaceRequired)?;
        library.update_eligibility_overrides(
            access,
            root_id,
            workspace_id,
            expected_revision,
            overrides,
        )
    }

    /// Records that an enrolled semantic root is temporarily unreachable.
    ///
    /// This is an internal reconciliation capability for watchers and the
    /// incremental ingestion scheduler of task 0182, not a user mutation:
    /// consent and every indexed generation survive untouched.
    ///
    /// # Errors
    ///
    /// Returns an authorization, not-found, lock, or persistence failure.
    pub async fn semantic_library_mark_root_unavailable(
        &self,
        access: &SemanticAccessContext,
        root_id: fm_semantic_library::RootId,
        reason: fm_semantic_library::RootUnavailabilityReason,
    ) -> Result<SemanticLibraryStatus, SemanticLibraryError> {
        self.semantic_library()
            .await
            .mark_root_unavailable(access, root_id, reason)
    }

    /// Applies provider observations to an enrolled semantic root, following a
    /// move and restoring availability only when stable identity proves it.
    ///
    /// This is the only way a quarantined root becomes available again. The
    /// observations must come from a provider capability that exposes a
    /// verified stable entry and volume identity; an observation without one
    /// leaves the root exactly as unavailable as it was.
    ///
    /// # Errors
    ///
    /// Returns an authorization, invalid-request, lock, or persistence failure.
    pub async fn semantic_library_observe_root_identity(
        &self,
        access: &SemanticAccessContext,
        root_id: fm_semantic_library::RootId,
        observations: &[fm_semantic_library::ObservedRootIdentity],
    ) -> Result<fm_semantic_library::RootMoveResolution, SemanticLibraryError> {
        self.semantic_library()
            .await
            .observe_root_identity(access, root_id, observations)
    }

    /// Commits one complete successful reconciliation of a semantic root.
    ///
    /// # Errors
    ///
    /// Returns an authorization, invalid-request, lock, or persistence failure.
    pub async fn semantic_library_complete_reconciliation(
        &self,
        access: &SemanticAccessContext,
        root_id: fm_semantic_library::RootId,
        observed_occurrences: &std::collections::BTreeSet<fm_semantic_library::OccurrenceId>,
    ) -> Result<u64, SemanticLibraryError> {
        self.semantic_library()
            .await
            .complete_reconciliation(access, root_id, observed_occurrences)
    }

    /// Returns provider-neutral worker feed decisions and curated eligibility
    /// verdicts for host-enumerated candidates.
    ///
    /// # Errors
    ///
    /// Returns an authorization, invalid-request, not-found, lock, or
    /// persistence failure.
    pub async fn semantic_library_worker_feed_plan(
        &self,
        access: &SemanticAccessContext,
        candidates: &[SemanticFeedCandidate],
    ) -> Result<SemanticWorkerFeedPlan, SemanticLibraryError> {
        self.semantic_library()
            .await
            .worker_feed_plan(access, candidates)
    }

    /// Records that an enrolled semantic root could not be reached at its own
    /// location.
    ///
    /// This direction is safe without identity proof because it only ever
    /// *reduces* what the library will do: consent, evidence, and generations
    /// survive, ingestion stops, and source links show as unavailable. The
    /// opposite direction is not symmetric and is deliberately absent —
    /// restoring availability requires
    /// [`semantic_library_observe_root_identity`](Self::semantic_library_observe_root_identity)
    /// and a provider-verified stable identity, because a readable path is not
    /// proof that it is still the same directory.
    ///
    /// Absence is never deletion — only
    /// [`semantic_library_complete_reconciliation`](Self::semantic_library_complete_reconciliation)
    /// may remove missing documents. Scheduling a periodic identity-proving
    /// crawl at the policy cadence remains task 0182's responsibility.
    pub async fn semantic_library_quarantine_unreachable_root(
        &self,
        location: &fm_domain::Location,
    ) {
        let library = self.semantic_library().await;
        let access = SemanticAccessContext::Host;
        let Ok(Some(root_id)) = library.enrolled_root_at(&access, location) else {
            return;
        };
        let _ = library.mark_root_unavailable(
            &access,
            root_id,
            fm_semantic_library::RootUnavailabilityReason::Missing,
        );
    }

    async fn ensure_active_semantic_folder(
        &self,
        context: &SemanticFolderContext,
    ) -> Result<(), SemanticLibraryError> {
        let workspace = self
            .workspaces
            .load(context.workspace_id)
            .await
            .map_err(|_| SemanticLibraryError::WorkspaceRequired)?;
        let active = workspace
            .panes
            .iter()
            .find(|pane| pane.id == workspace.active_pane_id)
            .and_then(|pane| pane.tabs.iter().find(|tab| tab.id == pane.active_tab_id))
            .map(|tab| &tab.location);
        if active == Some(&context.location) {
            Ok(())
        } else {
            Err(SemanticLibraryError::WorkspaceRequired)
        }
    }

    /// Reports semantic component authority and lifecycle operations.
    pub async fn semantic_component_capabilities(&self) -> SemanticComponentCapabilities {
        if self.runtime == RuntimeKindDto::BrowserServer {
            return SemanticComponentCapabilities::administrator_provisioned();
        }
        self.semantic_components.capabilities().await
    }

    /// Reports semantic component lifecycle and disk-use state.
    pub async fn semantic_component_status(
        &self,
    ) -> Result<SemanticComponentStatus, SemanticComponentError> {
        self.semantic_components.status().await
    }

    /// Replaces the inert default with a host-provided component capability.
    #[must_use]
    pub fn with_semantic_component_capability(
        mut self,
        capability: Arc<dyn SemanticComponentCapability>,
    ) -> Self {
        self.semantic_components = SemanticComponentService::new(capability);
        self
    }

    /// Returns catalog-backed profiles and exact model revisions.
    pub async fn semantic_component_catalog_profiles(
        &self,
    ) -> Result<Vec<SemanticModelProfile>, SemanticComponentError> {
        self.semantic_components.catalog_profiles().await
    }

    /// Creates a complete component installation disclosure.
    pub async fn semantic_component_installation_offer(
        &self,
        profile: SemanticProfile,
    ) -> Result<SemanticInstallationOffer, SemanticComponentError> {
        self.ensure_semantic_component_mutation(
            SemanticComponentOperation::CreateInstallationOffer,
        )?;
        self.semantic_components.installation_offer(profile).await
    }

    /// Installs or enables exactly one explicitly accepted offer.
    pub async fn semantic_component_install_or_enable(
        &self,
        consent: SemanticInstallationConsent,
    ) -> Result<SemanticInstallReceipt, SemanticComponentError> {
        self.ensure_semantic_component_mutation(SemanticComponentOperation::InstallOrEnable)?;
        self.semantic_components.install_or_enable(consent).await
    }

    /// Applies the newest compatible signed worker patch, if available.
    pub async fn semantic_component_install_compatible_worker_patch(
        &self,
        request: SemanticWorkerPatchRequest,
    ) -> Result<Option<SemanticInstallReceipt>, SemanticComponentError> {
        self.ensure_semantic_component_mutation(SemanticComponentOperation::InstallWorkerPatch)?;
        self.semantic_components
            .install_compatible_worker_patch(request)
            .await
    }

    /// Pauses semantic indexing without uninstalling components.
    pub async fn semantic_component_pause_indexing(&self) -> Result<(), SemanticComponentError> {
        self.ensure_semantic_component_mutation(SemanticComponentOperation::PauseIndexing)?;
        self.semantic_components.pause_indexing().await
    }

    /// Resumes explicitly paused semantic indexing.
    pub async fn semantic_component_resume_indexing(&self) -> Result<(), SemanticComponentError> {
        self.ensure_semantic_component_mutation(SemanticComponentOperation::ResumeIndexing)?;
        self.semantic_components.resume_indexing().await
    }

    /// Removes every record derived from one semantic enrolment.
    pub async fn semantic_component_remove_index(
        &self,
        request: RemoveSemanticIndexRequest,
    ) -> Result<SemanticIndexRemovalReceipt, SemanticComponentError> {
        self.ensure_semantic_component_mutation(SemanticComponentOperation::RemoveIndex)?;
        self.semantic_components.remove_index(request).await
    }

    /// Inventories enrolment-derived data and returns an opaque confirmation plan.
    pub async fn semantic_component_plan_index_removal(
        &self,
        enrolment_id: String,
    ) -> Result<SemanticIndexRemovalPlan, SemanticComponentError> {
        self.ensure_semantic_component_mutation(SemanticComponentOperation::RemoveIndex)?;
        self.semantic_components
            .plan_index_removal(enrolment_id)
            .await
    }

    /// Confirms one live authoritative enrolment-removal plan.
    pub async fn semantic_component_confirm_index_removal(
        &self,
        confirmation: SemanticIndexRemovalConfirmation,
    ) -> Result<SemanticIndexRemovalReceipt, SemanticComponentError> {
        self.ensure_semantic_component_mutation(SemanticComponentOperation::RemoveIndex)?;
        self.semantic_components
            .confirm_index_removal(confirmation)
            .await
    }

    /// Moves semantic data through pause-copy-verify-switch.
    pub async fn semantic_component_move_data(
        &self,
        destination: PathBuf,
    ) -> Result<SemanticDataMoveReceipt, SemanticComponentError> {
        self.ensure_semantic_component_mutation(SemanticComponentOperation::MoveData)?;
        self.semantic_components.move_data(destination).await
    }

    /// Uninstalls semantic components using an explicit index decision.
    pub async fn semantic_component_uninstall(
        &self,
        index_decision: SemanticIndexRetentionDecision,
    ) -> Result<SemanticUninstallReceipt, SemanticComponentError> {
        self.ensure_semantic_component_mutation(SemanticComponentOperation::UninstallComponents)?;
        self.semantic_components
            .uninstall_components(index_decision)
            .await
    }

    /// Validates an expert local model and returns a confirmation-gated migration.
    pub async fn semantic_component_import_local_model(
        &self,
        request: SemanticLocalModelImportRequest,
        profile: SemanticProfile,
        estimate: SemanticReindexEstimate,
    ) -> Result<SemanticModelMigrationPlan, SemanticComponentError> {
        self.ensure_semantic_component_mutation(SemanticComponentOperation::ImportLocalModel)?;
        self.semantic_components
            .import_local_model(request, profile, estimate)
            .await
    }

    /// Plans migration to a signed catalog model resolution.
    pub async fn semantic_component_plan_model_migration(
        &self,
        profile: SemanticProfile,
        estimate: SemanticReindexEstimate,
    ) -> Result<SemanticModelMigrationPlan, SemanticComponentError> {
        self.ensure_semantic_component_mutation(SemanticComponentOperation::PlanModelMigration)?;
        self.semantic_components
            .plan_model_migration(profile, estimate)
            .await
    }

    /// Confirms and begins a model migration.
    pub async fn semantic_component_confirm_model_migration(
        &self,
        confirmation: SemanticModelMigrationConfirmation,
    ) -> Result<SemanticModelMigrationProgress, SemanticComponentError> {
        self.ensure_semantic_component_mutation(SemanticComponentOperation::ConfirmModelMigration)?;
        self.semantic_components
            .confirm_model_migration(confirmation)
            .await
    }

    /// Persists a resumable model migration checkpoint.
    pub async fn semantic_component_checkpoint_model_migration(
        &self,
        checkpoint: SemanticModelMigrationCheckpoint,
    ) -> Result<SemanticModelMigrationProgress, SemanticComponentError> {
        self.ensure_semantic_component_mutation(
            SemanticComponentOperation::CheckpointModelMigration,
        )?;
        self.semantic_components
            .checkpoint_model_migration(checkpoint)
            .await
    }

    /// Activates a fully reindexed model migration target.
    pub async fn semantic_component_complete_model_migration(
        &self,
        migration_id: SemanticModelMigrationId,
    ) -> Result<SemanticModelSelection, SemanticComponentError> {
        self.ensure_semantic_component_mutation(
            SemanticComponentOperation::CompleteModelMigration,
        )?;
        self.semantic_components
            .complete_model_migration(migration_id)
            .await
    }

    fn ensure_semantic_component_mutation(
        &self,
        operation: SemanticComponentOperation,
    ) -> Result<(), SemanticComponentError> {
        if self.runtime == RuntimeKindDto::BrowserServer {
            return Err(SemanticComponentError::AuthorityDenied {
                authority:
                    crate::semantic_components::SemanticComponentAuthority::AdministratorProvisioned,
                operation,
            });
        }
        Ok(())
    }

    /// Reports the optional semantic capability's current health.
    pub async fn semantic_health(&self) -> Result<SemanticHealth, SemanticError> {
        self.semantic.health().await
    }

    /// Replaces the unavailable default with a host-provided semantic capability.
    #[must_use]
    pub fn with_semantic_capability(mut self, capability: Arc<dyn SemanticCapability>) -> Self {
        let semantic = SemanticService::new(capability);
        self.search_comparison.set_semantic(semantic.clone());
        self.semantic_indexing.set_semantic(semantic.clone());
        self.semantic = semantic;
        self
    }

    /// Reconciles one explicitly enrolled root into the active semantic worker.
    ///
    /// This is opt-in application orchestration for a trusted host command. It
    /// is never triggered by ordinary HTTP or library-enrolment methods.
    pub async fn semantic_reconcile_enrolled_root(
        &self,
        access: &SemanticAccessContext,
        root_id: fm_semantic_library::RootId,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<SemanticIndexingReport, SemanticIndexingError> {
        self.semantic_indexing
            .reconcile(self.semantic_library().await, access, root_id, cancellation)
            .await
    }

    /// Replaces the unavailable default with a worker-backed summary capability.
    #[must_use]
    pub fn with_document_summary_capability(
        mut self,
        capability: Arc<dyn DocumentSummaryCapability>,
    ) -> Self {
        self.document_summaries = DocumentSummaryCoordinator::new(capability);
        self
    }

    /// Replaces the unavailable default with a worker-backed Ask retriever.
    #[must_use]
    pub fn with_rag_retrieval_capability(
        mut self,
        capability: Arc<dyn RagRetrievalCapability>,
    ) -> Self {
        self.rag = RagCoordinator::new(capability);
        self
    }

    /// Prepares bounded key passages and the disclosure required before generation.
    pub async fn preview_document_summary(
        &self,
        access: &SemanticAccessContext,
        request: fm_transport_dto::PreviewDocumentSummaryRequestDto,
    ) -> Result<fm_transport_dto::DocumentSummaryPreviewDto, ApplicationError> {
        let worker_request = self
            .resolve_document_summary_request(access, request.target, request.input_token_budget)
            .await?;
        let cancellation = tokio_util::sync::CancellationToken::new();
        self.document_summaries
            .preview(
                worker_request,
                request.profile_id,
                &self.llm_profiles,
                &cancellation,
            )
            .await
            .map(preview_to_dto)
            .map_err(summary_error_to_application)
    }

    /// Generates and publishes a summary after revalidating the preview fingerprint.
    pub async fn generate_document_summary(
        &self,
        access: &SemanticAccessContext,
        request: fm_transport_dto::GenerateDocumentSummaryRequestDto,
    ) -> Result<fm_transport_dto::DocumentSummaryDto, ApplicationError> {
        let worker_request = self
            .resolve_document_summary_request(access, request.target, request.input_token_budget)
            .await?;
        let cancellation = tokio_util::sync::CancellationToken::new();
        self.document_summaries
            .generate(
                GenerateDocumentSummary {
                    request: worker_request,
                    expected_selection_fingerprint: request.expected_selection_fingerprint,
                    profile_id: request.profile_id,
                },
                &self.llm_profiles,
                &cancellation,
            )
            .await
            .map(|summary| summary_to_dto(summary, false))
            .map_err(summary_error_to_application)
    }

    /// Returns the current generated summary, including stale-source state.
    pub async fn get_document_summary(
        &self,
        access: &SemanticAccessContext,
        request: fm_transport_dto::GetDocumentSummaryRequestDto,
    ) -> Result<Option<fm_transport_dto::DocumentSummaryDto>, ApplicationError> {
        let worker_request = self
            .resolve_document_summary_request(access, request.target, 1)
            .await?;
        self.document_summaries
            .current(&worker_request)
            .await
            .map(|summary| summary.map(|(stored, stale)| summary_to_dto(stored, stale)))
            .map_err(summary_error_to_application)
    }

    async fn resolve_document_summary_request(
        &self,
        access: &SemanticAccessContext,
        target: fm_transport_dto::DocumentSummaryTargetDto,
        input_token_budget: u32,
    ) -> Result<fm_semantic_worker::document_summary::PrepareDocumentSummary, ApplicationError>
    {
        const MAX_SUMMARY_INPUT_TOKENS: u32 = 32_768;
        if input_token_budget == 0 || input_token_budget > MAX_SUMMARY_INPUT_TOKENS {
            return Err(ApplicationError::InvalidRequest(
                "summary input token budget is outside the supported range".into(),
            ));
        }
        let library = self.semantic_library().await;
        let resolved = library
            .resolve_summary_document(
                access,
                target.workspace_id.into(),
                target.entry_id.into(),
                &target.location.into(),
            )
            .map_err(|error| match error {
                SemanticLibraryError::Unavailable => ApplicationError::ProviderUnavailable,
                SemanticLibraryError::AuthorityDenied { .. } => ApplicationError::PermissionDenied,
                SemanticLibraryError::NotFound | SemanticLibraryError::NotEnrolled => {
                    ApplicationError::NotFound
                }
                _ => ApplicationError::Internal,
            })?
            .ok_or(ApplicationError::NotFound)?;
        Ok(
            fm_semantic_worker::document_summary::PrepareDocumentSummary {
                tenant_id: resolved.tenant_id,
                library_id: resolved.library_id,
                document_id: resolved.document_id,
                input_token_budget: usize::try_from(input_token_budget)
                    .map_err(|_| ApplicationError::InvalidRequest("invalid token budget".into()))?,
            },
        )
    }

    /// Retrieves inspectable evidence before any generation request.
    pub async fn preview_rag(
        &self,
        access: &SemanticAccessContext,
        request: PreviewRagRequestDto,
    ) -> Result<RagPreviewDto, ApplicationError> {
        let scope = request.scope.clone();
        let authorized = self
            .resolve_rag_request(access, request.question, &scope)
            .await?;
        self.rag
            .preview(
                authorized,
                request.profile_id,
                &self.llm_profiles,
                &tokio_util::sync::CancellationToken::new(),
            )
            .await
            .map(|preview| rag_preview_to_dto(preview, scope))
            .map_err(rag_error_to_application)
    }

    /// Generates one read-only answer after revalidating inspected evidence.
    pub async fn generate_rag_answer(
        &self,
        access: &SemanticAccessContext,
        request: GenerateRagAnswerRequestDto,
    ) -> Result<GenerateRagAnswerResponseDto, ApplicationError> {
        let scope = request.scope.clone();
        let authorized = self
            .resolve_rag_request(access, request.question.clone(), &scope)
            .await?;
        let tenant_id = authorized.retrieval.filters.tenant_id.clone();
        let internal_scope = authorized.scope.clone();
        let conversation_id = request.conversation_id.unwrap_or_else(Uuid::new_v4);
        let existing = self
            .rag_ephemeral
            .lock()
            .map_err(|_| ApplicationError::Internal)?
            .get(&conversation_id)
            .cloned();
        let history = if let Some(conversation) = &existing {
            if conversation.tenant_id != tenant_id {
                return Err(ApplicationError::PermissionDenied);
            }
            if conversation.profile_id != request.profile_id
                || conversation.scope != internal_scope
                || conversation.model_knowledge_allowed != request.allow_model_knowledge
            {
                return Err(ApplicationError::InvalidRequest(
                    "conversation profile, scope, and knowledge mode cannot change".into(),
                ));
            }
            conversation
                .turns
                .iter()
                .map(|turn| RagHistoryTurn {
                    question: turn.question.clone(),
                    answer: turn.answer.text.clone(),
                })
                .collect()
        } else {
            Vec::new()
        };
        let events = self
            .rag
            .generate(
                GenerateRagAnswer {
                    authorized,
                    expected_retrieval_fingerprint: request.expected_retrieval_fingerprint,
                    profile_id: request.profile_id,
                    allow_model_knowledge: request.allow_model_knowledge,
                    history,
                },
                &self.llm_profiles,
                &tokio_util::sync::CancellationToken::new(),
            )
            .await
            .map_err(rag_error_to_application)?;
        let answer = events.iter().find_map(|event| match event {
            RagAnswerEvent::Done { answer } => Some(answer.clone()),
            RagAnswerEvent::Retrieval { .. } | RagAnswerEvent::Token { .. } => None,
        });
        if let Some(answer) = answer {
            let mut conversation = existing.unwrap_or(SavedRagConversation {
                id: conversation_id,
                tenant_id,
                profile_id: request.profile_id,
                scope: internal_scope,
                model_knowledge_allowed: request.allow_model_knowledge,
                turns: Vec::new(),
                storage_bytes: 0,
            });
            conversation.turns.push(SavedRagTurn {
                question: request.question,
                answer,
            });
            self.rag_ephemeral
                .lock()
                .map_err(|_| ApplicationError::Internal)?
                .insert(conversation_id, conversation);
        }
        Ok(GenerateRagAnswerResponseDto {
            conversation_id,
            events: events_to_dto(events, &scope),
        })
    }

    /// Persists an explicitly saved, server-authored conversation.
    pub fn save_rag_conversation(
        &self,
        access: &SemanticAccessContext,
        request: SaveRagConversationRequestDto,
    ) -> Result<SavedRagConversationDto, ApplicationError> {
        let tenant_id = access
            .tenant_id()
            .map_err(|_| ApplicationError::PermissionDenied)?;
        let conversation = self
            .rag_ephemeral
            .lock()
            .map_err(|_| ApplicationError::Internal)?
            .get(&request.conversation_id)
            .filter(|conversation| conversation.tenant_id == tenant_id)
            .cloned()
            .ok_or(ApplicationError::NotFound)?;
        let store = RagConversationStore::open(&self.rag_conversation_path)
            .map_err(rag_error_to_application)?;
        store
            .save(conversation)
            .map(|saved| saved_to_dto(saved, request.workspace_id))
            .map_err(rag_error_to_application)
    }

    /// Lists explicitly saved conversations inside the caller's tenant.
    pub fn list_saved_rag_conversations(
        &self,
        access: &SemanticAccessContext,
        workspace_id: Uuid,
    ) -> Result<Vec<SavedRagConversationDto>, ApplicationError> {
        let tenant_id = access
            .tenant_id()
            .map_err(|_| ApplicationError::PermissionDenied)?;
        RagConversationStore::open(&self.rag_conversation_path)
            .map_err(rag_error_to_application)?
            .list(&tenant_id)
            .map(|saved| {
                saved
                    .into_iter()
                    .map(|conversation| saved_to_dto(conversation, workspace_id))
                    .collect()
            })
            .map_err(rag_error_to_application)
    }

    /// Deletes one caller-owned saved conversation.
    pub fn delete_rag_conversation(
        &self,
        access: &SemanticAccessContext,
        request: DeleteRagConversationRequestDto,
    ) -> Result<(), ApplicationError> {
        let tenant_id = access
            .tenant_id()
            .map_err(|_| ApplicationError::PermissionDenied)?;
        let removed = RagConversationStore::open(&self.rag_conversation_path)
            .map_err(rag_error_to_application)?
            .delete(&tenant_id, request.conversation_id)
            .map_err(rag_error_to_application)?;
        if removed {
            Ok(())
        } else {
            Err(ApplicationError::NotFound)
        }
    }

    /// Resolves a citation against current workspace authorization and availability.
    pub async fn resolve_rag_citation(
        &self,
        access: &SemanticAccessContext,
        request: ResolveRagCitationRequestDto,
    ) -> Result<ResolvedRagCitationDto, ApplicationError> {
        let occurrence = self
            .semantic_library()
            .await
            .resolve_occurrence(access, request.workspace_id.into(), &request.source_id)
            .map_err(|error| match error {
                SemanticLibraryError::Unavailable => ApplicationError::ProviderUnavailable,
                SemanticLibraryError::AuthorityDenied { .. } => ApplicationError::PermissionDenied,
                _ => ApplicationError::InvalidRequest(error.to_string()),
            })?
            .ok_or(ApplicationError::NotFound)?;
        Ok(ResolvedRagCitationDto {
            entry_id: occurrence.entry_id.into_inner(),
            location: occurrence.location.into(),
            available: occurrence.available,
        })
    }

    async fn resolve_rag_request(
        &self,
        access: &SemanticAccessContext,
        question: String,
        scope: &RagScopeDto,
    ) -> Result<AuthorizedRagRequest, ApplicationError> {
        let selection = match scope.kind {
            RagScopeKindDto::EntireLibrary => RagScopeSelection::EntireLibrary,
            RagScopeKindDto::SelectedFiles => {
                if scope.selected_files.is_empty()
                    || scope
                        .selected_files
                        .iter()
                        .any(|target| target.workspace_id != scope.workspace_id)
                {
                    return Err(ApplicationError::InvalidRequest(
                        "selected-file Ask scope is invalid".into(),
                    ));
                }
                RagScopeSelection::SelectedFiles(
                    scope
                        .selected_files
                        .iter()
                        .map(|target| (target.entry_id.into(), target.location.clone().into()))
                        .collect(),
                )
            }
            RagScopeKindDto::CurrentFolder => RagScopeSelection::CurrentFolder(
                scope
                    .folder
                    .clone()
                    .ok_or_else(|| {
                        ApplicationError::InvalidRequest(
                            "current-folder Ask scope requires a folder".into(),
                        )
                    })?
                    .into(),
            ),
            RagScopeKindDto::SemanticResults => {
                if scope.semantic_source_ids.is_empty() {
                    return Err(ApplicationError::InvalidRequest(
                        "semantic-result Ask scope is empty".into(),
                    ));
                }
                RagScopeSelection::SemanticResults(scope.semantic_source_ids.clone())
            }
            RagScopeKindDto::EnrolledRoots => {
                if scope.enrolled_root_ids.is_empty() {
                    return Err(ApplicationError::InvalidRequest(
                        "enrolled-root Ask scope is empty".into(),
                    ));
                }
                RagScopeSelection::EnrolledRoots(
                    scope
                        .enrolled_root_ids
                        .iter()
                        .map(|value| crate::semantic_library::parse_semantic_root_id(value))
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|_| ApplicationError::InvalidRequest("invalid root id".into()))?,
                )
            }
        };
        let resolved = self
            .semantic_library()
            .await
            .resolve_rag_scope(
                access,
                scope.workspace_id.into(),
                &selection,
                question.clone(),
                fm_semantic_worker::rag_retrieval::RagRetrievalPolicy::default_ask(),
            )
            .map_err(|error| match error {
                SemanticLibraryError::Unavailable => ApplicationError::ProviderUnavailable,
                SemanticLibraryError::AuthorityDenied { .. } => ApplicationError::PermissionDenied,
                _ => ApplicationError::InvalidRequest(error.to_string()),
            })?;
        if resolved
            .retrieval
            .source_restriction
            .allowed_source_ids
            .is_empty()
        {
            return Err(ApplicationError::NotFound);
        }
        let indexed = resolved.eligible.saturating_sub(resolved.unavailable);
        Ok(AuthorizedRagRequest {
            question,
            scope: scope_from_dto(scope),
            retrieval: resolved.retrieval,
            coverage: RagCoverage {
                eligible: resolved.eligible,
                indexed,
                unavailable: resolved.unavailable,
                ..RagCoverage::default()
            },
            display: RagSourceDisplay {
                titles: resolved.titles,
            },
        })
    }

    /// Streams a provider-neutral document into the semantic capability.
    pub async fn semantic_ingest(
        &self,
        ingestion: DocumentIngestion,
    ) -> Result<SemanticJobId, SemanticError> {
        self.semantic.ingest(ingestion).await
    }

    /// Executes a scoped semantic query.
    pub async fn semantic_query(
        &self,
        query: SemanticQuery,
    ) -> Result<Vec<SemanticSearchResult>, SemanticError> {
        self.semantic.query(query).await
    }

    /// Reads one scoped semantic ingestion job.
    pub async fn semantic_ingestion_job(
        &self,
        scope: SemanticScope,
        job_id: SemanticJobId,
    ) -> Result<SemanticIngestionJob, SemanticError> {
        self.semantic.ingestion_job(scope, job_id).await
    }

    /// Reads a scoped snapshot of semantic progress events.
    pub async fn semantic_events(
        &self,
        scope: SemanticScope,
    ) -> Result<Vec<SemanticProgressEvent>, SemanticError> {
        self.semantic.events(scope).await
    }

    /// Requests cancellation of an opaque semantic operation.
    pub async fn semantic_cancel(
        &self,
        operation_id: SemanticOperationId,
    ) -> Result<bool, SemanticError> {
        self.semantic.cancel(operation_id).await
    }

    /// Requests bounded graceful shutdown of the semantic capability.
    pub async fn semantic_shutdown(&self, grace: Duration) -> Result<(), SemanticError> {
        self.semantic.shutdown(grace).await
    }

    /// Generates (or reuses a cached) downscaled preview for an image or
    /// CBZ/CBR comic archive entry (task 0134). `size` must be `"small"`,
    /// `"medium"` or `"large"`. Every unsupported/oversized/undecodable
    /// input is reported as [`ApplicationError::NotFound`], matching
    /// [`Self::file_icon`]'s convention - the frontend falls back to the
    /// generic type icon rather than treating it as a hard error.
    pub async fn thumbnail(&self, uri: &str, size: &str) -> Result<Vec<u8>, ApplicationError> {
        let location = Location::parse(uri)
            .map_err(|error| ApplicationError::InvalidRequest(format!("invalid `uri`: {error}")))?;
        let size = fm_metadata::ThumbnailSize::parse(size).ok_or_else(|| {
            ApplicationError::InvalidRequest(format!(
                "invalid `size`: must be `small`, `medium` or `large`, got {size:?}"
            ))
        })?;
        let thumbnail = self
            .thumbnails
            .thumbnail(&self.providers, &location, size)
            .await?;
        Ok(thumbnail.bytes)
    }

    /// Starts a semantic operation, deduplicating retries by idempotency key.
    pub fn start_operation(
        &self,
        request: StartOperationRequestDto,
        idempotency_key: Option<String>,
    ) -> Result<OperationDto, ApplicationError> {
        self.operations.start(request, idempotency_key)
    }

    /// Lists active and retained historical operation snapshots.
    #[must_use]
    pub fn list_operations(&self) -> Vec<OperationDto> {
        self.operations.list()
    }

    /// Returns a bounded page of active and retained historical operations.
    #[must_use]
    pub fn list_operation_page(
        &self,
        offset: u64,
        limit: u16,
    ) -> fm_transport_dto::OperationPageDto {
        self.operations.page(offset, limit)
    }

    /// Gets one operation snapshot.
    pub fn get_operation(&self, id: OperationId) -> Result<OperationDto, ApplicationError> {
        self.operations.get(id)
    }

    /// Starts a guarded operation-engine job that reverses a completed operation.
    pub fn undo_operation(&self, id: OperationId) -> Result<OperationDto, ApplicationError> {
        self.operations.undo(id)
    }

    /// Requests cancellation of an operation.
    ///
    /// Searches, comparisons, checksum jobs and duplicate scans are not
    /// registered with the mutation [`Scheduler`], so an unknown id is
    /// retried against each read-only engine in turn (all share their
    /// `operation_id` with their own id, see [`Self::start_search`],
    /// [`Self::start_comparison`], [`Self::start_checksums`] and
    /// [`Self::start_duplicate_scan`]) before giving up.
    pub fn cancel_operation(&self, id: OperationId) -> Result<(), ApplicationError> {
        match self.operations.cancel(id) {
            Ok(()) => Ok(()),
            Err(SchedulerError::UnknownOperation(_)) => self
                .search_comparison
                .cancel_search(id.into_inner())
                .or_else(|_| {
                    self.search_comparison
                        .cancel_comparison(id.into_inner())
                        .map_err(|_| ())
                })
                .or_else(|()| {
                    self.checksums
                        .cancel_checksums(id.into_inner())
                        .map_err(|_| ())
                })
                .or_else(|()| {
                    self.checksums
                        .cancel_duplicate_scan(id.into_inner())
                        .map_err(|_| ())
                })
                .map_err(|()| ApplicationError::NotFound),
            Err(error) => Err(map_scheduler_error(error)),
        }
    }

    /// Forces move's copy/delete fallback for deterministic integration tests.
    #[doc(hidden)]
    pub fn force_cross_volume_moves_for_tests(&self, force: bool) {
        self.operations.force_cross_volume_moves_for_tests(force);
    }

    /// Pauses a running operation.
    pub fn pause_operation(&self, id: OperationId) -> Result<(), ApplicationError> {
        self.operations.pause(id)
    }

    /// Resumes a paused operation.
    pub fn resume_operation(&self, id: OperationId) -> Result<(), ApplicationError> {
        self.operations.resume(id)
    }

    /// Applies a reserved conflict decision through the shared operation service.
    pub fn resolve_operation_conflict(
        &self,
        id: OperationId,
        request: ResolveOperationConflictRequestDto,
    ) -> Result<(), ApplicationError> {
        if request.resolution == ConflictResolutionDto::CancelOperation {
            return self.cancel_operation(id);
        }
        self.operations.resolve_conflict(id, request)
    }

    /// Lists every registered action (spec §18).
    #[must_use]
    pub fn list_actions(&self) -> Vec<ActionDescriptorDto> {
        self.action_invoker.list(&self.plugin_manager)
    }

    /// Lists discovered plugins, retaining malformed manifests as disabled records.
    #[must_use]
    pub fn list_plugins(&self) -> Vec<PluginDescriptorDto> {
        self.plugin_manager.list_plugins()
    }

    /// Overrides the bundled (read-only, shipped-with-the-app) plugin directory that
    /// construction set up as `$CARGO_MANIFEST_DIR/../../plugins` - a path baked in at compile
    /// time on whichever machine built the binary, meaningless once that binary runs anywhere
    /// else. Real hosts only learn their actual bundled-resources location once the app has
    /// finished initializing (Tauri's `resource_dir()`, resolved from a running `AppHandle`),
    /// so this exists to be called once, right after that resolves, rather than requiring the
    /// location up front. `fm-server` and every test that never calls this keep the
    /// compile-time default, which is exactly right for their own dev-source-checkout context.
    pub fn set_bundled_plugins_directory(&mut self, directory: std::path::PathBuf) {
        self.plugin_manager.set_bundled_plugins_directory(directory);
    }

    /// Reads one asset referenced by an enabled plugin's icon theme (task 0095), rejecting any
    /// path that is not exactly one of the theme's declared icon definitions and any path that
    /// escapes the plugin's own directory.
    pub fn plugin_icon_theme_asset(
        &self,
        plugin_id: &str,
        asset_path: &str,
    ) -> Result<String, ApplicationError> {
        self.plugin_manager
            .plugin_icon_theme_asset(plugin_id, asset_path)
    }

    /// Returns the bounded diagnostic log retained for one plugin (spec §19.4).
    pub fn plugin_logs(&self, plugin_id: &str) -> Result<Vec<PluginLogEntryDto>, ApplicationError> {
        self.plugin_manager.plugin_logs(plugin_id)
    }

    /// Persists a plugin enablement decision after confirming its manifest is valid.
    pub fn set_plugin_enabled(
        &self,
        plugin_id: String,
        enabled: bool,
    ) -> Result<(), ApplicationError> {
        self.plugin_manager.set_plugin_enabled(plugin_id, enabled)
    }

    /// Invokes a registered action, re-validating its context requirements
    /// server-side and delegating file-mutating actions to the operation
    /// engine (spec §18). Never panics: an unknown or unavailable action is
    /// reported as a typed [`ApplicationError`].
    pub fn invoke_action(
        &self,
        action_id: String,
        request: InvokeActionRequestDto,
        idempotency_key: Option<String>,
    ) -> Result<ActionResultDto, ApplicationError> {
        self.action_invoker.invoke(
            action_id,
            request,
            idempotency_key,
            &self.plugin_manager,
            &self.operations,
        )
    }

    /// Returns the current application-wide settings.
    pub fn get_settings(&self) -> SettingsDto {
        let settings = self
            .settings
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        settings_to_dto(settings)
    }

    /// Atomically persists and returns a complete settings replacement.
    pub fn update_settings(&self, settings: SettingsDto) -> Result<SettingsDto, ApplicationError> {
        let settings = settings_from_dto(settings);
        self.settings_store
            .save(&settings)
            .map_err(|_| ApplicationError::Internal)?;
        let mut current = self
            .settings
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        *current = settings;
        Ok(settings_to_dto(current.clone()))
    }

    /// Returns the shared backend event bus used by both host adapters.
    #[must_use]
    pub fn event_bus(&self) -> EventBus {
        self.events.clone()
    }

    /// Re-publishes unresolved conflicts when a transport reconnects.
    pub fn republish_pending_operation_conflicts(&self) {
        self.operations.republish_pending_conflicts();
    }

    /// Supplies an archive credential to the backend-session cache.
    ///
    /// The secret is passed directly to the provider and is never added to operation history or
    /// an event payload.
    pub fn cache_archive_password(
        &self,
        request: fm_transport_dto::ArchiveCredentialRequestDto,
    ) -> Result<(), ApplicationError> {
        self.archive_provider
            .cache_password(&Location::from(request.location), request.password)
            .map_err(ApplicationError::from)
    }

    /// Lists one page of a directory.
    ///
    /// Listing deliberately reports *nothing* to the semantic library. A
    /// successful listing proves only that a path can be read, and a path is
    /// not an identity: after a directory is deleted and another one is
    /// created at the same place, listing it succeeds exactly as before. A
    /// quarantined root must therefore stay quarantined until a provider
    /// capability supplies a verified stable entry and volume identity, which
    /// the directory APIs do not yet expose. Task 0182 owns that scheduled
    /// reconciliation and calls
    /// [`semantic_library_observe_root_identity`](Self::semantic_library_observe_root_identity)
    /// with real observations.
    pub async fn list_directory(
        &self,
        request: ListDirectoryRequest,
    ) -> Result<DirectorySnapshotDto, ApplicationError> {
        let snapshot = self.directories.list(request).await?;
        Ok(self.enrich_snapshot(snapshot).await)
    }

    /// Lists the immediate child directories of a location, for the directory-tree sidebar (task
    /// 0139). Not bound to a pane, unlike [`list_directory`](Self::list_directory).
    pub async fn list_directory_children(
        &self,
        request: fm_transport_dto::ListDirectoryChildrenRequest,
    ) -> Result<Vec<fm_transport_dto::EntrySummaryDto>, ApplicationError> {
        let location = request.location.into();
        let entries = self
            .directories
            .list_children(&location, request.show_hidden)
            .await?;
        Ok(entries.into_iter().map(Into::into).collect())
    }

    /// Refreshes a directory using the same options as a listing.
    pub async fn refresh_directory(
        &self,
        request: ListDirectoryRequest,
    ) -> Result<DirectorySnapshotDto, ApplicationError> {
        let snapshot = self.directories.refresh(request).await?;
        Ok(self.enrich_snapshot(snapshot).await)
    }

    /// Navigates a pane and lists its first page.
    pub async fn navigate_pane(
        &self,
        request: NavigateRequest,
    ) -> Result<DirectorySnapshotDto, ApplicationError> {
        let snapshot = self.directories.navigate(request).await?;
        Ok(self.enrich_snapshot(snapshot).await)
    }

    /// Marks whether a pane is currently in the foreground, so a
    /// poll-tracked directory watch can poll less often while backgrounded
    /// (task 0109).
    pub async fn set_pane_activity(
        &self,
        request: SetPaneActivityRequest,
    ) -> Result<(), ApplicationError> {
        self.directories
            .set_pane_activity(PaneId::from(request.pane_id), request.active)
            .await
    }

    /// Converts a domain snapshot to its wire DTO, attaching the backing
    /// volume's total/available capacity (task 0096) when the platform
    /// adapter and location support it.
    async fn enrich_snapshot(&self, snapshot: DirectorySnapshot) -> DirectorySnapshotDto {
        let volume_capacity = volume_capacity(&self.platform, &snapshot.location).await;
        DirectorySnapshotDto {
            volume_capacity,
            ..DirectorySnapshotDto::from(snapshot)
        }
    }

    /// Fetches detailed metadata for one entry.
    pub async fn get_entry_metadata(
        &self,
        request: EntryMetadataRequest,
    ) -> Result<EntryMetadata, ApplicationError> {
        self.directories.metadata(request).await
    }

    /// Reads one bounded chunk of a file's raw bytes, for the in-app large
    /// file viewer (task 0088).
    pub async fn read_file_range(
        &self,
        request: ReadFileRangeRequestDto,
    ) -> Result<ReadFileRangeResponseDto, ApplicationError> {
        content_streaming::read_file_range(&self.providers, request).await
    }

    /// Opens a bounded, provider-neutral semantic DOCX preview session.
    pub async fn open_docx_preview(
        &self,
        request: fm_transport_dto::OpenDocxPreviewRequestDto,
    ) -> Result<fm_transport_dto::OpenDocxPreviewResponseDto, ApplicationError> {
        self.docx_preview.open(request).await
    }

    /// Reads one bounded embedded image from a DOCX preview session.
    pub async fn read_docx_preview_resource(
        &self,
        request: fm_transport_dto::ReadDocxPreviewResourceRequestDto,
    ) -> Result<fm_transport_dto::ReadDocxPreviewResourceResponseDto, ApplicationError> {
        self.docx_preview.read_resource(request).await
    }

    /// Cancels and releases a DOCX preview session.
    pub async fn close_docx_preview(
        &self,
        request: fm_transport_dto::DocxPreviewSessionRequestDto,
    ) -> Result<(), ApplicationError> {
        self.docx_preview.close(request).await
    }

    /// Opens a bounded, provider-neutral rendered PowerPoint preview session.
    pub async fn open_pptx_preview(
        &self,
        request: fm_transport_dto::OpenPptxPreviewRequestDto,
    ) -> Result<fm_transport_dto::OpenPptxPreviewResponseDto, ApplicationError> {
        self.pptx_preview.open(request).await
    }

    /// Reads one bounded byte range from a rendered PowerPoint PDF.
    pub async fn read_pptx_preview_pdf(
        &self,
        request: fm_transport_dto::ReadPptxPreviewPdfRequestDto,
    ) -> Result<fm_transport_dto::ReadFileRangeResponseDto, ApplicationError> {
        self.pptx_preview.read_pdf(request).await
    }

    /// Cancels and releases a PPTX preview session.
    pub async fn close_pptx_preview(
        &self,
        request: fm_transport_dto::PptxPreviewSessionRequestDto,
    ) -> Result<(), ApplicationError> {
        self.pptx_preview.close(request).await
    }

    /// Opens a bounded, provider-neutral read-only structured-data session.
    pub async fn open_structured_view(
        &self,
        request: fm_transport_dto::OpenStructuredViewRequestDto,
    ) -> Result<fm_transport_dto::OpenStructuredViewResponseDto, ApplicationError> {
        self.structured_view.open(request).await
    }

    /// Returns incremental indexing progress for a structured-data session.
    pub async fn structured_view_status(
        &self,
        request: fm_transport_dto::StructuredViewSessionRequestDto,
    ) -> Result<fm_transport_dto::StructuredViewStatusDto, ApplicationError> {
        self.structured_view.status(request).await
    }

    /// Applies delimiter/header overrides without reopening the source.
    pub async fn update_structured_view(
        &self,
        request: fm_transport_dto::UpdateStructuredViewRequestDto,
    ) -> Result<fm_transport_dto::OpenStructuredViewResponseDto, ApplicationError> {
        self.structured_view.update(request).await
    }

    /// Reads one bounded logical-record page from a structured-data session.
    pub async fn read_structured_rows(
        &self,
        request: fm_transport_dto::ReadStructuredRowsRequestDto,
    ) -> Result<fm_transport_dto::ReadStructuredRowsResponseDto, ApplicationError> {
        self.structured_view.read_rows(request).await
    }

    /// Reads one bounded raw JSON window and its token spans.
    pub async fn read_structured_json_window(
        &self,
        request: fm_transport_dto::ReadStructuredJsonWindowRequestDto,
    ) -> Result<fm_transport_dto::ReadStructuredJsonWindowResponseDto, ApplicationError> {
        self.structured_view.read_json_window(request).await
    }

    /// Searches indexed table records with a bounded continuation cursor.
    pub async fn search_structured_rows(
        &self,
        request: fm_transport_dto::SearchStructuredRowsRequestDto,
    ) -> Result<fm_transport_dto::SearchStructuredRowsResponseDto, ApplicationError> {
        self.structured_view.search_rows(request).await
    }

    /// Cancels indexing and drops every cache/checkpoint owned by the session.
    pub async fn close_structured_view(
        &self,
        request: fm_transport_dto::StructuredViewSessionRequestDto,
    ) -> Result<(), ApplicationError> {
        self.structured_view.close(request).await
    }

    /// Loads a complete text file only when it fits the bounded editor budget.
    pub async fn load_editable_file(
        &self,
        request: fm_transport_dto::LoadEditableFileRequestDto,
    ) -> Result<fm_transport_dto::LoadEditableFileResponseDto, ApplicationError> {
        self.editor.load(request).await
    }

    /// Safely replaces editable content through a sibling temporary file and optimistic revision.
    pub async fn save_editable_file(
        &self,
        request: fm_transport_dto::SaveEditableFileRequestDto,
    ) -> Result<fm_transport_dto::SaveEditableFileResponseDto, ApplicationError> {
        self.editor.save(request).await
    }

    /// Searches a single file's content for a substring or regex, for the
    /// in-app large file viewer (task 0088). Only requires
    /// [`ProviderCapabilities::READ`], so it works for every provider.
    pub async fn search_in_file(
        &self,
        request: SearchInFileRequestDto,
    ) -> Result<SearchInFileResponseDto, ApplicationError> {
        content_streaming::search_in_file(&self.providers, request).await
    }

    /// Recursively sums a directory's total size (task 0071), for the Total Commander-style
    /// "press a key on a folder to see how much space it consumes" behaviour. Provider-agnostic -
    /// works for any location whose provider reports `ProviderCapabilities::LIST`.
    pub async fn calculate_folder_size(
        &self,
        request: fm_transport_dto::CalculateFolderSizeRequestDto,
    ) -> Result<fm_transport_dto::CalculateFolderSizeResponseDto, ApplicationError> {
        crate::folder_size::calculate_folder_size(&self.providers, request.location.into()).await
    }

    /// Summarizes an archive through its virtual root, reusing the provider-neutral directory
    /// walker for entry counts and uncompressed bytes.
    pub async fn archive_summary(
        &self,
        request: ArchiveSummaryRequestDto,
    ) -> Result<ArchiveSummaryResponseDto, ApplicationError> {
        crate::archive_summary::calculate_archive_summary(
            &self.providers,
            &self.archive_provider,
            request.location.into(),
        )
        .await
    }

    /// Builds a bounded hierarchical disk-usage tree for one local directory on a blocking
    /// worker, leaving the async host runtime responsive while the scan traverses. Delegates to
    /// [`DiskUsageCoordinator`] so the scan is cancellable and its `scan_id` is rejected if
    /// already running.
    pub async fn scan_disk_usage(
        &self,
        request: fm_transport_dto::ScanDiskUsageRequestDto,
    ) -> Result<fm_transport_dto::ScanDiskUsageResponseDto, ApplicationError> {
        self.disk_usage.scan_disk_usage(request).await
    }

    /// Runs one event-driven disk-usage job. The host owns task spawning because Tauri and Axum
    /// enter different async runtimes; progress, completion, and failure remain transport-neutral.
    pub async fn run_disk_usage_job(&self, request: fm_transport_dto::ScanDiskUsageRequestDto) {
        let workspace_id = request.workspace_id;
        let scan_id = request.scan_id;
        if let Err(error) = self.scan_disk_usage(request).await {
            let error = error.into_dto(Uuid::new_v4());
            let code = match serde_json::to_value(error.code) {
                Ok(serde_json::Value::String(code)) => code,
                _ => "internal".to_owned(),
            };
            self.events.publish(
                EventAudience::Workspace(workspace_id.into()),
                BackendEventPayload::DiskUsageFailed {
                    scan_id,
                    code,
                    message: error.message,
                },
            );
        }
    }

    /// Cancels a disk-usage scan idempotently, including when cancellation reaches the service just
    /// before the matching scan registration.
    pub fn cancel_disk_usage(&self, scan_id: Uuid) -> Result<(), ApplicationError> {
        self.disk_usage.cancel_disk_usage(scan_id)
    }

    /// Scans a `.app` bundle's well-known related-file locations (task 0148:
    /// `Application Support`, `Caches`, `Preferences`, `Saved Application
    /// State`, `LaunchAgents`, `Logs`), for the user to review before
    /// anything is deleted. Read-only - dispatched directly to the platform
    /// adapter like `core.revealInSystemFileManager`/`core.openTerminal`
    /// (task 0061), never through the operation engine. Deletion itself
    /// reuses the existing `start_operation`/Trash path once the caller has
    /// picked which discovered candidates to remove, alongside the bundle.
    pub fn discover_application_uninstall_candidates(
        &self,
        request: DiscoverApplicationUninstallCandidatesRequestDto,
    ) -> Result<DiscoverApplicationUninstallCandidatesResponseDto, ApplicationError> {
        let bundle_location: Location = request.location.into();
        let bundle_path = bundle_location.to_native_path().map_err(|error| {
            ApplicationError::InvalidRequest(format!("invalid `location`: {error}"))
        })?;
        let plan = self
            .platform
            .plan_application_uninstall(&bundle_path)
            .map_err(|error| match error {
                fm_platform::PlatformError::NotFound { .. } => ApplicationError::NotFound,
                other => map_platform_error(&ActionId::new("core.uninstallApplication"), other),
            })?;
        let related_files = plan
            .related_files
            .into_iter()
            .map(|candidate| {
                let location = Location::from_native_path(&candidate.path).map_err(|error| {
                    ApplicationError::PlatformOperationFailed(error.to_string())
                })?;
                Ok(ApplicationUninstallCandidateDto {
                    location: location.into(),
                    size_bytes: candidate.size_bytes,
                    removable: candidate.removable,
                })
            })
            .collect::<Result<Vec<_>, ApplicationError>>()?;
        Ok(DiscoverApplicationUninstallCandidatesResponseDto {
            bundle_identifier: plan.bundle_identifier,
            product_name: plan.product_name,
            related_files,
        })
    }

    /// Removes `request.location`'s pinned Dock icon, if it has one (task 0148 follow-up), so
    /// confirming an uninstall doesn't leave a Dock icon pointing at a now-trashed bundle. Called
    /// once the user confirms the uninstall checklist, alongside (not instead of) the Trash
    /// operation. An adapter that doesn't implement this (`PlatformError::Unsupported`) reports
    /// `removed: false` rather than an error - this is best-effort cleanup, not a required step.
    pub fn remove_application_dock_icon(
        &self,
        request: RemoveApplicationDockIconRequestDto,
    ) -> Result<RemoveApplicationDockIconResponseDto, ApplicationError> {
        let bundle_location: Location = request.location.into();
        let bundle_path = bundle_location.to_native_path().map_err(|error| {
            ApplicationError::InvalidRequest(format!("invalid `location`: {error}"))
        })?;
        let removed = match self.platform.remove_application_dock_icon(&bundle_path) {
            Ok(removed) => removed,
            Err(fm_platform::PlatformError::Unsupported { .. }) => false,
            Err(other) => {
                return Err(map_platform_error(
                    &ActionId::new("core.uninstallApplication"),
                    other,
                ));
            }
        };
        Ok(RemoveApplicationDockIconResponseDto { removed })
    }

    /// Fetches a file's git commit history for the Alt+Space metadata panel's history section
    /// (task 0135). Never errors: an empty commit list means the file has no history to show
    /// (non-local provider, outside a git working tree, or not yet committed).
    pub async fn git_file_history(
        &self,
        request: GetFileGitHistoryRequestDto,
    ) -> GetFileGitHistoryResponseDto {
        let location: Location = request.location.into();
        let commits = self
            .directories
            .git_history(&location)
            .await
            .into_iter()
            .map(Into::into)
            .collect();
        GetFileGitHistoryResponseDto { commits }
    }

    /// Starts a cancellable recursive filename search over one or more
    /// roots, streaming matches to `request.workspace_id` over the event
    /// bus as they are found (spec §24, task 0068).
    pub async fn start_search(
        &self,
        request: StartSearchRequestDto,
    ) -> Result<StartSearchResponseDto, ApplicationError> {
        let semantic_library = self.semantic_library().await;
        let vocabulary_store =
            crate::semantic_vocabulary::VocabularyStore::open(&self.semantic_vocabulary_path)
                .map_err(crate::semantic_vocabulary_mapping::vocabulary_error)?;
        self.search_comparison
            .start_search(request, Some(&semantic_library), Some(&vocabulary_store))
            .await
    }

    /// Cancels a running search, stopping its traversal promptly.
    pub fn cancel_search(&self, search_id: Uuid) -> Result<(), ApplicationError> {
        self.search_comparison.cancel_search(search_id)
    }

    /// Starts a new cancellable directory comparison, streaming compared
    /// entries to `request.workspace_id` over the event bus as they are
    /// found (spec §16 milestone 5, task 0075).
    pub fn start_comparison(
        &self,
        request: StartComparisonRequestDto,
    ) -> Result<StartComparisonResponseDto, ApplicationError> {
        self.search_comparison.start_comparison(request)
    }

    /// Cancels a running comparison, stopping its traversal promptly.
    pub fn cancel_comparison(&self, comparison_id: Uuid) -> Result<(), ApplicationError> {
        self.search_comparison.cancel_comparison(comparison_id)
    }

    /// Returns a bounded page of a comparison's results, optionally
    /// restricted to non-identical entries (spec §16 milestone 5: "can be
    /// filtered to differences only").
    pub fn get_comparison_page(
        &self,
        comparison_id: Uuid,
        offset: u64,
        limit: u16,
        differences_only: bool,
    ) -> Result<ComparisonPageDto, ApplicationError> {
        self.search_comparison
            .comparison_page(comparison_id, offset, limit, differences_only)
    }

    /// Proposes a sync plan from a comparison's current results. Never
    /// touches a filesystem (spec §35): it only reads the comparison's
    /// already-computed results.
    pub fn generate_sync_plan(
        &self,
        comparison_id: Uuid,
        request: GenerateSyncPlanRequestDto,
    ) -> Result<SyncPlanDto, ApplicationError> {
        self.search_comparison
            .generate_sync_plan(comparison_id, request)
    }

    /// Applies a (possibly user-edited) sync plan: every non-`skip` row
    /// starts one ordinary `copy` or `trash` operation through the existing
    /// operation engine, with the normal conflict, progress and
    /// cancellation semantics (spec §35: nothing runs without this explicit,
    /// reviewed call).
    pub fn apply_sync_plan(
        &self,
        comparison_id: Uuid,
        request: ApplySyncPlanRequestDto,
    ) -> Result<ApplySyncPlanResponseDto, ApplicationError> {
        self.search_comparison
            .apply_sync_plan(comparison_id, request, &self.operations)
    }

    /// Starts a cancellable checksum job over a selection, streaming results
    /// to `request.workspace_id` over the event bus (spec §18
    /// `core.calculateChecksum`, task 0077).
    ///
    /// Rejected up front when any entry's provider does not advertise
    /// [`fm_vfs::ProviderCapabilities::CHECKSUM`] (spec §6).
    pub fn start_checksums(
        &self,
        request: StartChecksumRequestDto,
    ) -> Result<StartChecksumResponseDto, ApplicationError> {
        self.checksums.start_checksums(request)
    }

    /// Cancels a running checksum job.
    pub fn cancel_checksums(&self, job_id: Uuid) -> Result<(), ApplicationError> {
        self.checksums.cancel_checksums(job_id)
    }

    /// Returns a bounded page of a checksum job's results.
    pub fn get_checksum_page(
        &self,
        job_id: Uuid,
        offset: u64,
        limit: u16,
    ) -> Result<ChecksumPageDto, ApplicationError> {
        self.checksums.get_checksum_page(job_id, offset, limit)
    }

    /// Renders a job's results as coreutils-compatible checksum-file text.
    ///
    /// Returns the text rather than writing a file: saving goes through the
    /// caller's normal write path, so this never becomes a second,
    /// unaudited way to create a file (spec §35).
    pub fn render_checksum_file(
        &self,
        job_id: Uuid,
        request: RenderChecksumFileRequestDto,
    ) -> Result<ChecksumFileDto, ApplicationError> {
        self.checksums.render_checksum_file(job_id, request)
    }

    /// Writes a job's results to a checksum file through the provider's
    /// normal `WRITE` path (task 0077).
    ///
    /// Deliberately server-side rather than a host-native save dialog: this
    /// keeps every file this application creates on one audited,
    /// capability-gated path (spec §35), and makes saving behave identically
    /// under the Axum and Tauri hosts.
    ///
    /// # Errors
    ///
    /// Returns [`ApplicationError::NotFound`] for an unknown job, and an
    /// invalid-request error if the destination's provider cannot be
    /// resolved, lacks `WRITE`, or already holds a file and `overwrite` is
    /// false.
    pub async fn save_checksum_file(
        &self,
        job_id: Uuid,
        request: fm_transport_dto::SaveChecksumFileRequestDto,
    ) -> Result<fm_transport_dto::SaveChecksumFileResponseDto, ApplicationError> {
        self.checksums
            .save_checksum_file(job_id, request, &self.providers)
            .await
    }

    /// Verifies a job's computed digests against an existing checksum file,
    /// reporting per-entry match, mismatch or missing.
    pub fn verify_checksum_file(
        &self,
        job_id: Uuid,
        request: VerifyChecksumFileRequestDto,
    ) -> Result<VerificationReportDto, ApplicationError> {
        self.checksums.verify_checksum_file(job_id, request)
    }

    /// Starts a cancellable duplicate scan across one or more roots, using
    /// the staged size -> partial-hash -> full-hash strategy (task 0077).
    pub fn start_duplicate_scan(
        &self,
        request: StartDuplicateScanRequestDto,
    ) -> Result<StartDuplicateScanResponseDto, ApplicationError> {
        self.checksums.start_duplicate_scan(request)
    }

    /// Cancels a running duplicate scan.
    pub fn cancel_duplicate_scan(&self, scan_id: Uuid) -> Result<(), ApplicationError> {
        self.checksums.cancel_duplicate_scan(scan_id)
    }

    /// Returns a bounded page of a duplicate scan's grouped results.
    pub fn get_duplicate_page(
        &self,
        scan_id: Uuid,
        offset: u64,
        limit: u16,
    ) -> Result<DuplicatePageDto, ApplicationError> {
        self.checksums.get_duplicate_page(scan_id, offset, limit)
    }

    /// Reports which capabilities are available for the current runtime and
    /// platform, so the frontend can respond to capabilities rather than
    /// detecting operating systems itself (spec §21).
    pub fn runtime_capabilities(&self) -> RuntimeCapabilitiesDto {
        runtime_capabilities_dto(self.runtime, self.platform.capabilities())
    }

    /// Reports runtime capabilities including the active semantic component
    /// authority and executable-download policy.
    pub async fn runtime_capabilities_with_semantic_components(&self) -> RuntimeCapabilitiesDto {
        let semantic = self.semantic_component_capabilities_dto().await;
        runtime_capabilities_dto_with_semantic(
            self.runtime,
            self.platform.capabilities(),
            semantic.authority,
            semantic.runtime_executable_download,
        )
    }

    /// Returns the active platform adapter's PNG icon for one sample entry.
    /// The adapter owns extension-level caching; this service deliberately
    /// adds no second cache layer (task 0091).
    pub fn file_icon(&self, uri: &str) -> Result<Vec<u8>, ApplicationError> {
        platform_mapping::read_file_icon(&self.platform, uri)
    }

    /// Reads an entry's Finder tags (task 0136). Missing capability support
    /// or a vanished entry are both reported as [`ApplicationError::NotFound`]
    /// so a lazy per-entry frontend loader can treat them as "no tags"
    /// rather than a failure.
    pub fn finder_tags(&self, uri: &str) -> Result<FinderTagsDto, ApplicationError> {
        platform_mapping::read_finder_tags(&self.platform, uri)
    }

    /// Replaces an entry's complete set of Finder tags (task 0136),
    /// returning the persisted set back (mirrors [`Self::update_settings`]'s
    /// get/put symmetry).
    pub fn set_finder_tags(
        &self,
        uri: &str,
        request: FinderTagsDto,
    ) -> Result<FinderTagsDto, ApplicationError> {
        platform_mapping::write_finder_tags(&self.platform, uri, request)
    }

    /// Reads an entry's Spotlight comment (task 0136). Same graceful
    /// missing-capability/missing-entry handling as [`Self::finder_tags`].
    pub fn spotlight_comment(&self, uri: &str) -> Result<SpotlightCommentDto, ApplicationError> {
        platform_mapping::read_spotlight_comment(&self.platform, uri)
    }

    /// Sets or clears an entry's Spotlight comment (task 0136), returning
    /// the persisted value back.
    pub fn set_spotlight_comment(
        &self,
        uri: &str,
        request: SpotlightCommentDto,
    ) -> Result<SpotlightCommentDto, ApplicationError> {
        platform_mapping::write_spotlight_comment(&self.platform, uri, request)
    }

    /// Installs the native menu bar from `spec` (task 0133), a thin
    /// passthrough to the platform adapter. `on_action` is invoked whenever
    /// the user clicks a `NativeMenuItem::Action` item, with that item's
    /// action-registry id, so the caller can dispatch it exactly like an
    /// `invoke_action` call from the keyboard.
    pub fn install_native_menu(
        &self,
        spec: &fm_domain::NativeMenuSpec,
        on_action: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Result<(), ApplicationError> {
        platform_mapping::install_native_menu(&self.platform, spec, on_action)
    }

    /// Runs the workspace startup lifecycle (spec §5.3.7): selects an
    /// explicitly requested workspace, otherwise the last-active one,
    /// otherwise creates a default.
    pub async fn start_workspace(
        &self,
        requested_workspace_id: Option<Uuid>,
    ) -> Result<WorkspaceDto, ApplicationError> {
        let workspace = self
            .workspaces
            .start(requested_workspace_id.map(Into::into))
            .await?;
        Ok(workspace.into())
    }

    /// Lists every stored workspace as a lightweight summary (spec §5.3.12
    /// `listWorkspaces`).
    pub async fn list_workspaces(&self) -> Result<Vec<WorkspaceSummaryDto>, ApplicationError> {
        let summaries = self.workspaces.list().await?;
        Ok(summaries.into_iter().map(Into::into).collect())
    }

    /// Loads a single workspace by id (spec §5.3.12 `getWorkspace`).
    pub async fn get_workspace(&self, id: Uuid) -> Result<WorkspaceDto, ApplicationError> {
        let workspace = self.workspaces.load(id.into()).await?;
        Ok(workspace.into())
    }

    /// Creates and persists a new workspace (spec §5.3.12 `createWorkspace`).
    pub async fn create_workspace(
        &self,
        name: Option<String>,
    ) -> Result<WorkspaceDto, ApplicationError> {
        let workspace = self.workspaces.create(name).await?;
        Ok(workspace.into())
    }

    /// Deletes a workspace (spec §5.3.12 `deleteWorkspace`).
    ///
    /// Semantic detachment runs *before* the repository delete and must
    /// succeed. The ordering is deliberate: detaching only removes a
    /// workspace's authorization references from globally enrolled roots, so if
    /// the repository delete then fails the workspace survives with a strictly
    /// narrower semantic scope — never with dangling scopes naming a workspace
    /// that no longer exists. Narrowing can only deny access, and re-enrolling
    /// the folder from the surviving workspace restores it; the reverse
    /// ordering would leave revoked-workspace scopes behind whenever
    /// detachment failed. Detachment is idempotent, so a retried delete is
    /// safe. When no semantic capability is configured, or the library is a
    /// read-only administrator-provisioned server library, detachment is a
    /// no-op and workspace deletion is unaffected.
    /// Deletes a workspace and then drops the semantic references that named
    /// it (spec §5.3.12 `deleteWorkspace`).
    ///
    /// The order is deliberate. The workspace repository is authoritative, and
    /// it and the semantic library are two independent stores that cannot
    /// commit atomically; sequencing the semantic mutation first would revoke
    /// the references of a workspace that a stale revision, a missing
    /// workspace, or a repository I/O failure then left alive. Deleting first
    /// means a semantic failure can only leave *extra* references behind, and
    /// those are unreachable because every semantic call validates that the
    /// workspace still exists. The detachment is idempotent and stays queued,
    /// so the next semantic operation completes it.
    pub async fn delete_workspace(
        &self,
        id: Uuid,
        expected_revision: Option<u64>,
    ) -> Result<(), ApplicationError> {
        self.workspaces.delete(id.into(), expected_revision).await?;
        let _ = self.semantic_library().await.detach_workspace(id.into());
        Ok(())
    }

    /// Selects an existing workspace as the last-active workspace (spec
    /// §5.3.12 `openWorkspace`).
    pub async fn open_workspace(&self, id: Uuid) -> Result<WorkspaceDto, ApplicationError> {
        let workspace = self.workspaces.open(id.into()).await?;
        Ok(workspace.into())
    }

    /// Forks a new ephemeral (per-window) workspace from `source_id`'s current shape,
    /// or the hardcoded default shape if `None` (ephemeral per-window workspaces spec
    /// follow-up).
    pub async fn fork_workspace(
        &self,
        source_id: Option<Uuid>,
    ) -> Result<WorkspaceDto, ApplicationError> {
        let workspace = self.workspaces.fork(source_id.map(Into::into)).await?;
        Ok(workspace.into())
    }

    /// Writes an ephemeral workspace's current shape back into `target_id`, or - if omitted -
    /// the named workspace it was forked from, creating one if it was seeded from the hardcoded
    /// default (ephemeral per-window workspaces spec follow-up). Returns the target named
    /// workspace, not the ephemeral one.
    pub async fn resync_workspace(
        &self,
        ephemeral_id: Uuid,
        target_id: Option<Uuid>,
    ) -> Result<WorkspaceDto, ApplicationError> {
        let workspace = self
            .workspaces
            .resync(ephemeral_id.into(), target_id.map(Into::into))
            .await?;
        Ok(workspace.into())
    }

    /// Applies a semantic workspace mutation command (spec §5.3.9, §5.3.12
    /// `applyWorkspaceCommand`).
    pub async fn apply_workspace_command(
        &self,
        command: WorkspaceCommandDto,
    ) -> Result<WorkspaceDto, ApplicationError> {
        let workspace = self.workspaces.apply_command(command.into()).await?;
        Ok(workspace.into())
    }

    /// Returns safe defaults for every supported generation provider.
    pub fn list_llm_profile_presets(&self) -> Vec<LlmProfilePresetDto> {
        LlmProfileService::presets()
            .into_iter()
            .map(preset_to_profile_dto)
            .collect()
    }

    /// Lists saved generation profiles without exposing credential IDs.
    pub fn list_llm_profiles(&self) -> Result<Vec<LlmProfileDto>, ApplicationError> {
        self.llm_profiles
            .list()?
            .into_iter()
            .map(profile_to_dto)
            .collect::<Result<_, _>>()
            .map_err(Into::into)
    }

    /// Creates a named generation profile and stores its optional key in the
    /// protected credential service.
    pub async fn create_llm_profile(
        &self,
        request: SaveLlmProfileRequestDto,
    ) -> Result<LlmProfileDto, ApplicationError> {
        profile_to_dto(self.llm_profiles.create(draft_from_dto(request)).await?).map_err(Into::into)
    }

    /// Updates a generation profile, preserving its credential when no new
    /// write-only key is supplied.
    pub async fn update_llm_profile(
        &self,
        id: Uuid,
        request: SaveLlmProfileRequestDto,
    ) -> Result<LlmProfileDto, ApplicationError> {
        profile_to_dto(
            self.llm_profiles
                .update(id, draft_from_dto(request))
                .await?,
        )
        .map_err(Into::into)
    }

    /// Deletes a profile with an explicit credential-retention choice.
    pub async fn delete_llm_profile(
        &self,
        id: Uuid,
        request: DeleteLlmProfileRequestDto,
    ) -> Result<(), ApplicationError> {
        self.llm_profiles
            .delete(id, disposition_from_dto(request.credential_disposition))
            .await
            .map_err(Into::into)
    }

    /// Persists a credential-free clone of a generation profile.
    pub fn clone_llm_profile(&self, id: Uuid) -> Result<LlmProfileDto, ApplicationError> {
        profile_to_dto(self.llm_profiles.clone_profile(id)?).map_err(Into::into)
    }

    /// Exports only non-secret reusable configuration.
    pub fn export_llm_profile(&self, id: Uuid) -> Result<LlmProfileExportDto, ApplicationError> {
        Ok(profile_to_export_dto(self.llm_profiles.export_profile(id)?))
    }

    /// Activates a profile, recording explicit consent for its normalized
    /// cloud host when required.
    pub fn activate_llm_profile(
        &self,
        id: Uuid,
        consent: bool,
    ) -> Result<LlmProfileDto, ApplicationError> {
        profile_to_dto(self.llm_profiles.activate(id, consent)?).map_err(Into::into)
    }

    /// Runs the bounded synthetic compatibility probe and returns only
    /// normalized, content-free diagnostics.
    pub async fn test_llm_profile(
        &self,
        id: Uuid,
    ) -> Result<LlmProfileTestResultDto, ApplicationError> {
        let cancellation = tokio_util::sync::CancellationToken::new();
        Ok(test_result_to_dto(
            self.llm_profiles.test(id, &cancellation).await?,
        ))
    }

    /// Lists every stored connection profile with its current runtime status
    /// (spec §16 `GET /api/v1/connections`, task 0103).
    pub async fn list_connections(&self) -> Result<Vec<ConnectionDto>, ApplicationError> {
        self.connections.list_connections().await
    }

    /// Loads a single connection profile with its current runtime status
    /// (spec §16 `GET /api/v1/connections/{connectionId}`, task 0103).
    pub async fn get_connection(&self, id: Uuid) -> Result<ConnectionDto, ApplicationError> {
        self.connections.get_connection(id).await
    }

    /// Creates and persists a new connection profile (spec §16
    /// `POST /api/v1/connections`, task 0103).
    pub async fn create_connection(
        &self,
        request: CreateConnectionRequestDto,
    ) -> Result<ConnectionDto, ApplicationError> {
        self.connections.create_connection(request).await
    }

    /// Updates an existing connection profile, optionally replacing its
    /// stored credential (spec §16 `PUT /api/v1/connections/{connectionId}`,
    /// task 0103).
    pub async fn update_connection(
        &self,
        id: Uuid,
        request: UpdateConnectionRequestDto,
    ) -> Result<ConnectionDto, ApplicationError> {
        self.connections.update_connection(id, request).await
    }

    /// Deletes a connection profile and its stored credential, if any (spec
    /// §16 `DELETE /api/v1/connections/{connectionId}`, task 0103).
    pub async fn delete_connection(&self, id: Uuid) -> Result<(), ApplicationError> {
        self.connections.delete_connection(id).await
    }

    /// Attempts to connect (spec §16
    /// `POST /api/v1/connections/{connectionId}/connect`, task 0103).
    pub async fn connect_connection(&self, id: Uuid) -> Result<ConnectionDto, ApplicationError> {
        self.connections.connect_connection(id).await
    }

    /// Marks a connection as disconnected (spec §16
    /// `POST /api/v1/connections/{connectionId}/disconnect`, task 0103).
    pub async fn disconnect_connection(&self, id: Uuid) -> Result<ConnectionDto, ApplicationError> {
        self.connections.disconnect_connection(id).await
    }

    /// Checks whether a connection's configuration and credential are
    /// currently usable, without changing its tracked status (spec §16
    /// `POST /api/v1/connections/{connectionId}/test`, task 0103).
    pub async fn test_connection(&self, id: Uuid) -> Result<ConnectionDto, ApplicationError> {
        self.connections.test_connection(id).await
    }

    /// Probes an SSH connection's currently presented host key without
    /// authenticating (task 0104, spec §6.4's mandatory explicit
    /// confirmation flow).
    pub async fn probe_ssh_host_key(
        &self,
        id: Uuid,
    ) -> Result<fm_transport_dto::HostKeyProbeDto, ApplicationError> {
        self.connections.probe_ssh_host_key(id).await
    }

    /// Accepts (persists) a host-key fingerprint for an SSH connection (task
    /// 0104, spec §6.4) - the only path that ever writes to the known-hosts
    /// store, and only after re-probing to confirm the host is still
    /// presenting exactly the fingerprint being accepted (defense against
    /// confirming a stale or attacker-supplied value passed by a caller).
    ///
    /// A connection configured with
    /// [`fm_connections::HostKeyPolicy::RequireKnownHost`] refuses to
    /// establish first-time trust through this call (it only ever succeeds
    /// once a fingerprint is already known by some other means); it may
    /// still be used to re-confirm a changed key that was previously known.
    pub async fn accept_ssh_host_key(
        &self,
        id: Uuid,
        fingerprint: String,
    ) -> Result<(), ApplicationError> {
        self.connections.accept_ssh_host_key(id, fingerprint).await
    }

    /// Begins a OneDrive OAuth authorization attempt for a saved connection
    /// (task 0110, spec §19): binds the loopback callback listener and
    /// returns an attempt id plus the Microsoft authorization URL for the
    /// caller (the frontend adapter) to open in the system browser, while
    /// the callback wait, token exchange and credential persistence run in
    /// the background. See `onedrive::OneDriveAuthorizationService` for the
    /// full state machine.
    pub async fn begin_onedrive_authorization(
        &self,
        connection_id: Uuid,
    ) -> Result<fm_transport_dto::BeginOneDriveAuthorizationResponseDto, ApplicationError> {
        self.onedrive.begin_authorization(connection_id).await
    }

    /// Polls a OneDrive authorization attempt's current status (task 0110).
    pub async fn onedrive_authorization_attempt(
        &self,
        attempt_id: Uuid,
    ) -> Result<fm_transport_dto::OneDriveAuthorizationAttemptDto, ApplicationError> {
        self.onedrive.attempt_status(attempt_id).await
    }

    /// Cancels a pending OneDrive authorization attempt (task 0110).
    /// Idempotent for an attempt that has already reached a terminal state.
    pub async fn cancel_onedrive_authorization(
        &self,
        attempt_id: Uuid,
    ) -> Result<fm_transport_dto::OneDriveAuthorizationAttemptDto, ApplicationError> {
        self.onedrive.cancel_authorization(attempt_id).await
    }

    /// Opens an interactive remote shell channel on an SSH connection for
    /// the embedded terminal drawer (task 0105, extending task 0126),
    /// starting in `remote_path` if given.
    ///
    /// Reuses the same pooled SSH session an open SFTP browse for
    /// `connection_id` already established rather than dialing again, and
    /// reports [`ApplicationError::InvalidRequest`] (not a silent local
    /// fallback) if `connection_id` does not name an SSH connection.
    pub async fn open_remote_shell(
        &self,
        connection_id: Uuid,
        remote_path: Option<&str>,
        term: &str,
        cols: u16,
        rows: u16,
    ) -> Result<fm_ssh::RemoteShellChannel, ApplicationError> {
        self.remote_terminals
            .open_shell(connection_id, remote_path, term, cols, rows)
            .await
    }
}

impl From<WorkspaceSummary> for WorkspaceSummaryDto {
    fn from(summary: WorkspaceSummary) -> Self {
        Self {
            id: summary.id.into(),
            name: summary.name,
            updated_at: summary.updated_at,
            revision: summary.revision,
            ephemeral: summary.ephemeral,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::io::Write;
    use std::path::Path;

    use fm_events::{SessionId, SubscriptionEvent};
    use fm_operations::Operation;
    use fm_transport_dto::{
        LoadEditableFileRequestDto, OperationConflictPolicyDto, OperationKindDto,
        OperationStateDto, PlatformKindDto, SaveEditableFileRequestDto,
    };

    use fm_platform::PlatformCapabilities;

    use crate::content_streaming::MAX_RANGE_LENGTH;
    use crate::file_editor::MAX_EDITABLE_FILE_BYTES;
    use crate::platform_mapping::detect_platform;

    fn service() -> (tempfile::TempDir, FileManagerService) {
        let dir = tempfile::tempdir().expect("must create a temp dir");
        let service = FileManagerService::new(
            RuntimeKindDto::BrowserServer,
            dir.path(),
            dir.path().join("settings"),
        );
        (dir, service)
    }

    #[test]
    fn enabled_plugin_actions_are_projected_into_the_shared_action_registry() {
        let (directory, service) = service();
        let plugin = directory.path().join("settings/plugins/copy-path");
        std::fs::create_dir_all(&plugin).expect("plugin directory");
        std::fs::write(
            plugin.join("plugin.toml"),
            "id='example.copy-path'\nname='Copy Path'\nversion='1'\napi_version='1'\ndescription='Copies a path'\nentrypoint='plugin.lua'\n[contributions]\nactions=true",
        )
        .expect("manifest");
        std::fs::write(
            plugin.join("plugin.lua"),
            "return { actions = function() return {{ id = 'example.copy-path.copy', title = 'Copy Path', description = 'Copies the selected path' }} end }",
        )
        .expect("script");
        service
            .set_plugin_enabled("example.copy-path".to_owned(), true)
            .expect("enable plugin");

        let action = service
            .list_actions()
            .into_iter()
            .find(|action| action.id == "example.copy-path.copy")
            .expect("plugin action");

        assert_eq!(action.title, "Copy Path");
        assert!(matches!(
            action.source,
            fm_transport_dto::ActionSourceDto::Plugin { .. }
        ));
    }

    fn write_copy_markdown_plugin(directory: &std::path::Path, clipboard_write: bool) {
        std::fs::create_dir_all(directory).expect("plugin directory");
        std::fs::write(
            directory.join("plugin.toml"),
            format!(
                "id='example.copy-markdown'\nname='Copy Markdown'\nversion='1'\napi_version='1'\ndescription='Copies a markdown link'\nentrypoint='plugin.lua'\n[permissions]\nselected_entry_metadata=true\nclipboard_write={clipboard_write}\n[contributions]\nactions=true"
            ),
        )
        .expect("manifest");
        std::fs::write(
            directory.join("plugin.lua"),
            "return { actions = function() return {{ id = 'example.copy-markdown.copy', title = 'Copy Markdown', description = 'Copies a markdown link', requires_single_selection = true }} end, invoke = function(action_id) local entries = host.selected_entry_metadata() host.clipboard_write('[' .. entries[1].name .. '](' .. entries[1].uri .. ')') end }",
        )
        .expect("script");
    }

    #[test]
    fn plugin_action_requiring_single_selection_reports_that_context_requirement() {
        let (directory, service) = service();
        write_copy_markdown_plugin(
            &directory.path().join("settings/plugins/copy-markdown"),
            true,
        );
        service
            .set_plugin_enabled("example.copy-markdown".to_owned(), true)
            .expect("enable plugin");

        let action = service
            .list_actions()
            .into_iter()
            .find(|action| action.id == "example.copy-markdown.copy")
            .expect("plugin action");

        assert!(action.context_requirements.requires_single_selection);
    }

    #[tokio::test]
    async fn invoke_action_runs_a_plugin_action_and_publishes_a_clipboard_notification() {
        let (directory, service) = service();
        write_copy_markdown_plugin(
            &directory.path().join("settings/plugins/copy-markdown"),
            true,
        );
        service
            .set_plugin_enabled("example.copy-markdown".to_owned(), true)
            .expect("enable plugin");
        // `None` (rather than `Some(0)`) skips backlog replay, since this test
        // only cares about the notification `invoke_action` publishes below and
        // `set_plugin_enabled` above now also publishes a `plugin.changed` event.
        let mut events = service
            .event_bus()
            .subscribe(SessionId::new("test"), [], None);

        let result = service
            .invoke_action(
                "example.copy-markdown.copy".to_owned(),
                InvokeActionRequestDto {
                    parameters: Some(serde_json::json!({
                        "selectedEntries": [
                            { "name": "report.pdf", "uri": "file:///Users/erik/Documents/report.pdf" }
                        ]
                    })),
                    context: fm_transport_dto::ActionInvocationContextDto {
                        selected_entry_ids: vec![uuid::Uuid::new_v4()],
                        ..Default::default()
                    },
                },
                None,
            )
            .expect("plugin action must be invoked");

        assert!(result.invoked);
        assert_eq!(
            result.clipboard_text.as_deref(),
            Some("[report.pdf](file:///Users/erik/Documents/report.pdf)")
        );

        let event = events.recv().await.expect("notification event");
        assert!(matches!(
            event,
            SubscriptionEvent::Event(envelope)
                if matches!(
                    envelope.payload,
                    BackendEventPayload::NotificationCreated {
                        notification: NotificationPayload {
                            level: NotificationLevelPayload::Info,
                            ..
                        }
                    }
                )
        ));
    }

    #[test]
    fn invoke_action_reports_a_visible_error_when_a_plugin_action_lacks_the_clipboard_write_permission()
     {
        let (directory, service) = service();
        write_copy_markdown_plugin(
            &directory.path().join("settings/plugins/copy-markdown"),
            false,
        );
        service
            .set_plugin_enabled("example.copy-markdown".to_owned(), true)
            .expect("enable plugin");

        let error = service
            .invoke_action(
                "example.copy-markdown.copy".to_owned(),
                InvokeActionRequestDto {
                    parameters: Some(serde_json::json!({
                        "selectedEntries": [
                            { "name": "report.pdf", "uri": "file:///Users/erik/Documents/report.pdf" }
                        ]
                    })),
                    context: fm_transport_dto::ActionInvocationContextDto {
                        selected_entry_ids: vec![uuid::Uuid::new_v4()],
                        ..Default::default()
                    },
                },
                None,
            )
            .expect_err("clipboard write must be denied without the permission");

        assert_eq!(
            error.code(),
            fm_transport_dto::ApplicationErrorCode::InvalidRequest
        );
        assert!(error.to_string().contains("permission denied"));
    }

    #[test]
    fn invoke_action_reports_unavailable_when_the_plugin_action_context_requirement_is_not_met() {
        let (directory, service) = service();
        write_copy_markdown_plugin(
            &directory.path().join("settings/plugins/copy-markdown"),
            true,
        );
        service
            .set_plugin_enabled("example.copy-markdown".to_owned(), true)
            .expect("enable plugin");

        let error = service
            .invoke_action(
                "example.copy-markdown.copy".to_owned(),
                InvokeActionRequestDto::default(),
                None,
            )
            .expect_err("action requires exactly one selected entry");

        assert_eq!(
            error.code(),
            fm_transport_dto::ApplicationErrorCode::ActionUnavailable
        );
    }

    #[test]
    fn enabled_plugin_columns_are_exposed_as_declarative_descriptors() {
        let (directory, service) = service();
        let plugin = directory.path().join("settings/plugins/file-age");
        std::fs::create_dir_all(&plugin).expect("plugin directory");
        std::fs::write(
            plugin.join("plugin.toml"),
            "id='example.file-age'\nname='File Age'\nversion='1'\napi_version='1'\ndescription='Shows file age'\nentrypoint='plugin.lua'\n[contributions]\ncolumns=true",
        )
        .expect("manifest");
        std::fs::write(
            plugin.join("plugin.lua"),
            "return { columns = function() return {{ id = 'sample.fileAge', title = 'Age' }} end }",
        )
        .expect("script");
        service
            .set_plugin_enabled("example.file-age".to_owned(), true)
            .expect("enable plugin");

        let plugin = service
            .list_plugins()
            .into_iter()
            .find(|plugin| plugin.id == "example.file-age")
            .expect("plugin descriptor");

        assert!(plugin.enabled);
        assert_eq!(plugin.columns.len(), 1);
        assert_eq!(plugin.columns[0].id, "sample.fileAge");
        assert_eq!(plugin.columns[0].title, "Age");
    }

    #[test]
    fn listed_plugins_project_declared_permissions_and_mark_ungranted_ones_denied() {
        let (directory, service) = service();
        let plugin = directory.path().join("settings/plugins/copy-markdown");
        write_copy_markdown_plugin(&plugin, true);
        service
            .set_plugin_enabled("example.copy-markdown".to_owned(), true)
            .expect("enable plugin");

        let descriptor = service
            .list_plugins()
            .into_iter()
            .find(|plugin| plugin.id == "example.copy-markdown")
            .expect("plugin descriptor");

        assert!(descriptor.permissions.selected_entry_metadata);
        assert!(descriptor.permissions.clipboard_write);
        assert!(
            !descriptor.permissions.clipboard_read,
            "clipboard_read was never granted"
        );
        assert!(
            !descriptor.permissions.notifications,
            "notifications was never granted"
        );
        assert!(descriptor.permissions.filesystem_read.is_empty());
    }

    #[test]
    fn an_invalid_manifest_is_listed_with_its_validation_diagnostic() {
        let (directory, service) = service();
        let plugin = directory.path().join("settings/plugins/broken");
        std::fs::create_dir_all(&plugin).expect("plugin directory");
        std::fs::write(plugin.join("plugin.toml"), "id=''\n").expect("malformed manifest");

        let descriptor = service
            .list_plugins()
            .into_iter()
            .find(|plugin| plugin.id == "broken")
            .expect("invalid plugin is still listed");

        assert!(!descriptor.enabled);
        assert!(descriptor.diagnostic.is_some());
    }

    #[test]
    fn plugin_logs_reports_not_found_for_an_undiscovered_plugin() {
        let (_directory, service) = service();

        let error = service
            .plugin_logs("unknown.plugin")
            .expect_err("unknown plugin must be reported as not found");

        assert_eq!(
            error.code(),
            fm_transport_dto::ApplicationErrorCode::NotFound
        );
    }

    #[test]
    fn plugin_logs_returns_the_bounded_diagnostic_log_after_a_failure() {
        let (directory, service) = service();
        let plugin = directory.path().join("settings/plugins/copy-path");
        std::fs::create_dir_all(&plugin).expect("plugin directory");
        std::fs::write(
            plugin.join("plugin.toml"),
            "id='example.copy-path'\nname='Copy Path'\nversion='1'\napi_version='1'\ndescription='Copies a path'\nentrypoint='plugin.lua'\n[contributions]\nactions=true",
        )
        .expect("manifest");
        std::fs::write(
            plugin.join("plugin.lua"),
            "return { actions = function() error('boom') end }",
        )
        .expect("script");
        service
            .set_plugin_enabled("example.copy-path".to_owned(), true)
            .expect("enable plugin");

        // Triggers the runtime failure that is recorded into the bounded log.
        let _ = service.list_actions();

        let logs = service
            .plugin_logs("example.copy-path")
            .expect("plugin is discovered");

        assert_eq!(logs.len(), 1);
        assert!(logs[0].message.contains("boom"));
    }

    #[tokio::test]
    async fn enabling_a_plugin_publishes_a_plugin_changed_event() {
        let (directory, service) = service();
        let plugin = directory.path().join("settings/plugins/copy-path");
        std::fs::create_dir_all(&plugin).expect("plugin directory");
        std::fs::write(
            plugin.join("plugin.toml"),
            "id='example.copy-path'\nname='Copy Path'\nversion='1'\napi_version='1'\ndescription='Copies a path'\nentrypoint='plugin.lua'\n[contributions]\nactions=true",
        )
        .expect("manifest");
        std::fs::write(
            plugin.join("plugin.lua"),
            "return { actions = function() return {} end }",
        )
        .expect("script");
        let mut events = service
            .event_bus()
            .subscribe(SessionId::new("test"), [], Some(0));

        service
            .set_plugin_enabled("example.copy-path".to_owned(), true)
            .expect("enable plugin");

        let event = events.recv().await.expect("plugin.changed event");
        let SubscriptionEvent::Event(envelope) = event else {
            panic!("expected an event envelope");
        };
        let BackendEventPayload::PluginChanged { plugin } = envelope.payload else {
            panic!("expected a PluginChanged payload");
        };
        assert_eq!(plugin.id.as_str(), "example.copy-path");
        assert_eq!(plugin.name, "Copy Path");
        assert!(plugin.enabled);
    }

    #[test]
    fn restarted_service_restores_inflight_history_as_interrupted() {
        let directory = tempfile::tempdir().expect("must create a temp dir");
        let settings_directory = directory.path().join("settings");
        std::fs::create_dir_all(&settings_directory).expect("must create settings directory");
        let mut operation = Operation::new(
            fm_operations::OperationKind::Copy,
            vec![],
            None,
            fm_operations::ConflictPolicy::Ask,
        );
        operation
            .transition(fm_operations::OperationState::Planning)
            .expect("queued operation starts planning");
        std::fs::write(
            settings_directory.join(crate::operation_history::OPERATION_HISTORY_FILE_NAME),
            serde_json::to_vec(&vec![operation]).expect("history serializes"),
        )
        .expect("must write persisted history");

        let service = FileManagerService::new(
            RuntimeKindDto::BrowserServer,
            directory.path(),
            &settings_directory,
        );
        let page = service.list_operation_page(0, 50);

        assert_eq!(page.total, 1);
        assert_eq!(page.operations[0].state, OperationStateDto::Interrupted);
        assert_eq!(
            page.operations[0].result_summary.as_deref(),
            Some("Interrupted after 0 items; it was not resumed.")
        );
    }

    #[test]
    fn runtime_capabilities_report_the_configured_runtime_kind() {
        let (_dir, service) = service();
        assert_eq!(
            service.runtime_capabilities().runtime,
            RuntimeKindDto::BrowserServer
        );

        let dir = tempfile::tempdir().expect("must create a temp dir");
        let service = FileManagerService::new(
            RuntimeKindDto::Tauri,
            dir.path(),
            dir.path().join("settings"),
        );
        assert_eq!(
            service.runtime_capabilities().runtime,
            RuntimeKindDto::Tauri
        );
    }

    #[tokio::test]
    async fn corrupt_settings_surface_a_global_warning_notification() {
        let directory = tempfile::tempdir().expect("must create a temp dir");
        let settings_directory = directory.path().join("settings");
        std::fs::create_dir_all(&settings_directory).expect("create settings directory");
        std::fs::write(
            settings_directory.join(fm_settings::SETTINGS_FILE_NAME),
            "{broken",
        )
        .expect("write corrupt settings");
        let service = FileManagerService::new(
            RuntimeKindDto::BrowserServer,
            directory.path().join("workspaces"),
            settings_directory,
        );
        let mut events = service
            .event_bus()
            .subscribe(SessionId::new("test"), [], Some(0));

        let event = events.recv().await.expect("warning event");
        assert!(matches!(
            event,
            SubscriptionEvent::Event(envelope)
                if matches!(
                    envelope.payload,
                    BackendEventPayload::NotificationCreated {
                        notification: NotificationPayload {
                            level: NotificationLevelPayload::Warning,
                            ..
                        }
                    }
                )
        ));
    }

    #[test]
    fn runtime_capabilities_report_no_unimplemented_natives() {
        let (_dir, service) = service();
        let capabilities = service.runtime_capabilities();

        assert!(!capabilities.native_menus);
        assert!(!capabilities.native_file_icons);
        assert!(!capabilities.native_thumbnails);
        assert!(!capabilities.native_drag_out);
        assert!(!capabilities.system_trash);
        assert!(!capabilities.reveal_in_system_file_manager);
        assert!(!capabilities.open_terminal);
        assert!(capabilities.plugins);
        assert!(!capabilities.server_administration);
        assert!(capabilities.clipboard);
    }

    fn location_dto_for(path: &std::path::Path) -> fm_transport_dto::LocationDto {
        Location::from_native_path(path)
            .expect("path must convert to a location")
            .into()
    }

    fn write_xlsx_fixture(path: &Path) {
        let file = std::fs::File::create(path).expect("create XLSX fixture");
        let mut archive = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        let entries = [
            (
                "[Content_Types].xml",
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
<Override PartName="/xl/worksheets/sheet2.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
</Types>"#,
            ),
            (
                "_rels/.rels",
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
</Relationships>"#,
            ),
            (
                "xl/workbook.xml",
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<sheets><sheet name="Summary" sheetId="1" r:id="rId1"/><sheet name="Details" sheetId="2" r:id="rId2"/></sheets>
</workbook>"#,
            ),
            (
                "xl/_rels/workbook.xml.rels",
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/>
</Relationships>"#,
            ),
            (
                "xl/worksheets/sheet1.xml",
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><dimension ref="A1:F2"/><sheetData>
<row r="1"><c r="A1" t="inlineStr"><is><t>Label</t></is></c><c r="B1"><v>42</v></c><c r="C1" t="b"><v>1</v></c><c r="D1" t="e"><v>#DIV/0!</v></c><c r="E1" t="d"><v>2026-09-01T18:30:00Z</v></c><c r="F1"><f>B1*2</f><v>84</v></c></row>
<row r="2"><c r="A2" t="inlineStr"><is><t>Second</t></is></c></row>
</sheetData></worksheet>"#,
            ),
            (
                "xl/worksheets/sheet2.xml",
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><dimension ref="A1:A3"/><sheetData>
<row r="1"><c r="A1" t="inlineStr"><is><t>Other sheet</t></is></c></row>
<row r="3"><c r="A3" t="inlineStr"><is><t>Sparse row</t></is></c></row>
</sheetData></worksheet>"#,
            ),
        ];
        for (name, data) in entries {
            archive.start_file(name, options).expect("start XLSX entry");
            archive
                .write_all(data.as_bytes())
                .expect("write XLSX entry");
        }
        archive.finish().expect("finish XLSX fixture");
    }

    #[tokio::test]
    async fn read_file_range_reads_the_requested_bytes_at_an_offset() {
        let (dir, service) = service();
        let target = dir.path().join("report.txt");
        std::fs::write(&target, b"0123456789").expect("write fixture file");

        let response = service
            .read_file_range(ReadFileRangeRequestDto {
                location: location_dto_for(&target),
                offset: 4,
                length: 3,
            })
            .await
            .expect("range read must succeed");

        assert_eq!(response.data, b"456");
        assert_eq!(response.offset, 4);
        assert_eq!(response.length, 3);
        assert!(!response.eof);
        assert_eq!(response.probably_binary, None);
    }

    #[tokio::test]
    async fn structured_csv_session_returns_quoted_newlines_as_one_logical_row() {
        let (dir, service) = service();
        let target = dir.path().join("quoted.csv");
        std::fs::write(&target, b"name,notes\nAda,\"one\ntwo\"\nGrace,three\n")
            .expect("write CSV fixture");

        let opened = service
            .open_structured_view(fm_transport_dto::OpenStructuredViewRequestDto {
                location: location_dto_for(&target),
                format: fm_transport_dto::StructuredViewFormatDto::Csv,
                delimiter: None,
                header_mode: fm_transport_dto::StructuredHeaderModeDto::FirstRow,
            })
            .await
            .expect("open structured view");

        assert_eq!(opened.headers, ["name", "notes"]);
        assert_eq!(opened.rows[0].cells, ["Ada", "one\ntwo"]);
        assert_eq!(opened.rows[1].cells, ["Grace", "three"]);
        assert!(opened.rows.len() <= 200);
    }

    #[tokio::test]
    async fn structured_excel_opens_multiple_sheets_with_typed_cells_and_cached_formulas() {
        let (dir, service) = service();
        let target = dir.path().join("typed.xlsx");
        write_xlsx_fixture(&target);

        let opened = service
            .open_structured_view(fm_transport_dto::OpenStructuredViewRequestDto {
                location: location_dto_for(&target),
                format: fm_transport_dto::StructuredViewFormatDto::Excel,
                delimiter: None,
                header_mode: fm_transport_dto::StructuredHeaderModeDto::None,
            })
            .await
            .expect("open bounded workbook");

        assert_eq!(opened.kind, fm_transport_dto::StructuredViewKindDto::Table);
        assert_eq!(opened.selected_sheet.as_deref(), Some("Summary"));
        assert_eq!(
            opened
                .sheets
                .iter()
                .map(|sheet| sheet.name.as_str())
                .collect::<Vec<_>>(),
            ["Summary", "Details"]
        );
        assert_eq!(
            opened.rows[0].cells,
            [
                "Label",
                "42",
                "true",
                "#DIV/0!",
                "2026-09-01T18:30:00Z",
                "84"
            ]
        );
        assert_eq!(
            opened.rows[0].cell_details[5].formula.as_deref(),
            Some("B1*2")
        );
        assert_eq!(
            opened.rows[0].cell_details[3].value_type,
            fm_transport_dto::StructuredCellValueTypeDto::Error
        );
        assert_eq!(
            opened.rows[0].cell_details[4].value_type,
            fm_transport_dto::StructuredCellValueTypeDto::DateTime
        );
    }

    #[tokio::test]
    async fn structured_excel_switches_sheets_and_pages_sparse_rows_without_dense_session_state() {
        let (dir, service) = service();
        let target = dir.path().join("sparse.xlsx");
        write_xlsx_fixture(&target);
        let opened = service
            .open_structured_view(fm_transport_dto::OpenStructuredViewRequestDto {
                location: location_dto_for(&target),
                format: fm_transport_dto::StructuredViewFormatDto::Excel,
                delimiter: None,
                header_mode: fm_transport_dto::StructuredHeaderModeDto::None,
            })
            .await
            .expect("open bounded workbook");

        let selected = service
            .update_structured_view(fm_transport_dto::UpdateStructuredViewRequestDto {
                session_id: opened.session_id,
                delimiter: None,
                header_mode: None,
                selected_sheet: Some("Details".to_owned()),
            })
            .await
            .expect("select worksheet");
        assert_eq!(selected.selected_sheet.as_deref(), Some("Details"));
        assert_eq!(selected.total_rows, Some(3));
        assert_eq!(selected.rows[0].cells, ["Other sheet"]);
        assert!(selected.rows[1].cells.is_empty());
        assert_eq!(selected.rows[2].cells, ["Sparse row"]);

        let page = service
            .read_structured_rows(fm_transport_dto::ReadStructuredRowsRequestDto {
                session_id: opened.session_id,
                start_row: 1,
                count: 2,
            })
            .await
            .expect("read sparse page");
        assert_eq!(page.rows.len(), 2);
        assert!(page.rows[0].cells.is_empty());
        assert_eq!(page.rows[1].cells, ["Sparse row"]);
    }

    #[tokio::test]
    async fn structured_excel_falls_back_with_the_specific_source_or_parse_limit() {
        let (dir, service) = service();
        let oversized = dir.path().join("oversized.xls");
        let file = std::fs::File::create(&oversized).expect("create oversized workbook");
        file.set_len(crate::structured_view::WORKBOOK_SOURCE_BYTES + 1)
            .expect("size oversized workbook");
        let source_fallback = service
            .open_structured_view(fm_transport_dto::OpenStructuredViewRequestDto {
                location: location_dto_for(&oversized),
                format: fm_transport_dto::StructuredViewFormatDto::Excel,
                delimiter: None,
                header_mode: fm_transport_dto::StructuredHeaderModeDto::None,
            })
            .await
            .expect("open oversized workbook as fallback");
        assert_eq!(
            source_fallback.kind,
            fm_transport_dto::StructuredViewKindDto::ExternalFallback
        );
        assert!(
            source_fallback
                .warning
                .as_deref()
                .is_some_and(|warning| warning.contains("source") && warning.contains("limit"))
        );

        let corrupt = dir.path().join("corrupt.xlsb");
        std::fs::write(&corrupt, b"not a workbook").expect("write corrupt workbook");
        let parse_fallback = service
            .open_structured_view(fm_transport_dto::OpenStructuredViewRequestDto {
                location: location_dto_for(&corrupt),
                format: fm_transport_dto::StructuredViewFormatDto::Excel,
                delimiter: None,
                header_mode: fm_transport_dto::StructuredHeaderModeDto::None,
            })
            .await
            .expect("open corrupt workbook as fallback");
        assert!(
            parse_fallback
                .warning
                .as_deref()
                .is_some_and(|warning| warning.contains("corrupt or unsupported"))
        );
    }

    #[tokio::test]
    async fn structured_excel_revision_change_invalidates_sheet_access_and_close_releases_state() {
        let (dir, service) = service();
        let target = dir.path().join("revision.xlsx");
        write_xlsx_fixture(&target);
        let opened = service
            .open_structured_view(fm_transport_dto::OpenStructuredViewRequestDto {
                location: location_dto_for(&target),
                format: fm_transport_dto::StructuredViewFormatDto::Excel,
                delimiter: None,
                header_mode: fm_transport_dto::StructuredHeaderModeDto::None,
            })
            .await
            .expect("open bounded workbook");
        std::fs::OpenOptions::new()
            .append(true)
            .open(&target)
            .expect("open workbook for revision change")
            .write_all(b"changed")
            .expect("change workbook revision");

        assert!(matches!(
            service
                .update_structured_view(fm_transport_dto::UpdateStructuredViewRequestDto {
                    session_id: opened.session_id,
                    delimiter: None,
                    header_mode: None,
                    selected_sheet: Some("Details".to_owned()),
                })
                .await,
            Err(ApplicationError::FileRevisionConflict { .. })
        ));
        service
            .close_structured_view(fm_transport_dto::StructuredViewSessionRequestDto {
                session_id: opened.session_id,
            })
            .await
            .expect("close invalidated workbook");
        assert!(matches!(
            service
                .read_structured_rows(fm_transport_dto::ReadStructuredRowsRequestDto {
                    session_id: opened.session_id,
                    start_row: 0,
                    count: 1,
                })
                .await,
            Err(ApplicationError::NotFound)
        ));
    }

    #[tokio::test]
    async fn structured_csv_supports_bom_dialect_and_header_overrides_without_reopen() {
        let (dir, service) = service();
        let target = dir.path().join("dialect.csv");
        std::fs::write(&target, b"\xef\xbb\xbfname;city\nAda;London\n").expect("write CSV fixture");
        let opened = service
            .open_structured_view(fm_transport_dto::OpenStructuredViewRequestDto {
                location: location_dto_for(&target),
                format: fm_transport_dto::StructuredViewFormatDto::Csv,
                delimiter: Some(",".to_owned()),
                header_mode: fm_transport_dto::StructuredHeaderModeDto::FirstRow,
            })
            .await
            .expect("open with deliberately wrong delimiter");
        assert_eq!(opened.headers, ["name;city"]);

        let corrected = service
            .update_structured_view(fm_transport_dto::UpdateStructuredViewRequestDto {
                session_id: opened.session_id,
                delimiter: Some(";".to_owned()),
                header_mode: Some(fm_transport_dto::StructuredHeaderModeDto::None),
                selected_sheet: None,
            })
            .await
            .expect("correct options in the same session");
        assert!(corrected.headers.is_empty());
        assert_eq!(corrected.rows[0].cells, ["name", "city"]);
        assert_eq!(corrected.rows[1].cells, ["Ada", "London"]);
    }

    #[tokio::test]
    async fn structured_json_window_is_bounded_and_valid_when_offset_splits_utf8_and_string_state()
    {
        let (dir, service) = service();
        let target = dir.path().join("minified.json");
        let bytes = r#"{"prefix":"aaaaaaaaaaaaaaaa","city":"Zürich","ok":true}"#.as_bytes();
        std::fs::write(&target, bytes).expect("write JSON fixture");
        let opened = service
            .open_structured_view(fm_transport_dto::OpenStructuredViewRequestDto {
                location: location_dto_for(&target),
                format: fm_transport_dto::StructuredViewFormatDto::Json,
                delimiter: None,
                header_mode: fm_transport_dto::StructuredHeaderModeDto::None,
            })
            .await
            .expect("open JSON");
        let split = bytes
            .windows("ü".len())
            .position(|candidate| candidate == "ü".as_bytes())
            .expect("multibyte fixture") as u64
            + 1;
        let window = service
            .read_structured_json_window(fm_transport_dto::ReadStructuredJsonWindowRequestDto {
                session_id: opened.session_id,
                offset: split,
                length: 24,
            })
            .await
            .expect("read aligned JSON window");
        assert!(window.data.len() <= 24);
        assert!(std::str::from_utf8(&window.data).is_ok());
        assert!(
            window
                .tokens
                .iter()
                .any(|token| token.kind == fm_transport_dto::JsonTokenKindDto::String)
        );
    }

    #[tokio::test]
    async fn structured_session_invalidates_on_revision_change_and_close_cleans_it_up() {
        let (dir, service) = service();
        let target = dir.path().join("revision.csv");
        std::fs::write(&target, b"a,b\n1,2\n").expect("write CSV fixture");
        let opened = service
            .open_structured_view(fm_transport_dto::OpenStructuredViewRequestDto {
                location: location_dto_for(&target),
                format: fm_transport_dto::StructuredViewFormatDto::Csv,
                delimiter: None,
                header_mode: fm_transport_dto::StructuredHeaderModeDto::FirstRow,
            })
            .await
            .expect("open CSV");
        std::fs::write(&target, b"a,b\n9,8\nchanged\n").expect("change source");
        assert!(matches!(
            service
                .structured_view_status(fm_transport_dto::StructuredViewSessionRequestDto {
                    session_id: opened.session_id,
                })
                .await,
            Err(ApplicationError::FileRevisionConflict { .. })
        ));
        service
            .close_structured_view(fm_transport_dto::StructuredViewSessionRequestDto {
                session_id: opened.session_id,
            })
            .await
            .expect("close invalidated session");
        assert!(matches!(
            service
                .structured_view_status(fm_transport_dto::StructuredViewSessionRequestDto {
                    session_id: opened.session_id,
                })
                .await,
            Err(ApplicationError::NotFound)
        ));
    }

    #[tokio::test]
    async fn generated_large_csv_never_returns_an_unbounded_initial_row_set() {
        let (dir, service) = service();
        let target = dir.path().join("large.csv");
        let mut fixture = String::from("id,value\n");
        for index in 0..100_000 {
            use std::fmt::Write as _;
            writeln!(&mut fixture, "{index},value-{index}").expect("append fixture row");
        }
        std::fs::write(&target, fixture).expect("write generated CSV fixture");
        let opened = service
            .open_structured_view(fm_transport_dto::OpenStructuredViewRequestDto {
                location: location_dto_for(&target),
                format: fm_transport_dto::StructuredViewFormatDto::Csv,
                delimiter: None,
                header_mode: fm_transport_dto::StructuredHeaderModeDto::FirstRow,
            })
            .await
            .expect("open large CSV");
        assert_eq!(opened.rows.len(), 200);
        assert!(opened.total_rows.is_none());
        assert!(!opened.indexing_complete);
        service
            .close_structured_view(fm_transport_dto::StructuredViewSessionRequestDto {
                session_id: opened.session_id,
            })
            .await
            .expect("cancel and release session");
    }

    #[tokio::test]
    async fn read_file_range_reports_eof_and_sniffs_binary_only_at_offset_zero() {
        let (dir, service) = service();
        let target = dir.path().join("short.bin");
        std::fs::write(&target, [b'a', b'b', 0, b'c']).expect("write fixture file");

        let first_chunk = service
            .read_file_range(ReadFileRangeRequestDto {
                location: location_dto_for(&target),
                offset: 0,
                length: 1000,
            })
            .await
            .expect("range read must succeed");
        assert_eq!(first_chunk.data, [b'a', b'b', 0, b'c']);
        assert!(first_chunk.eof);
        assert_eq!(first_chunk.probably_binary, Some(true));

        let later_chunk = service
            .read_file_range(ReadFileRangeRequestDto {
                location: location_dto_for(&target),
                offset: 2,
                length: 2,
            })
            .await
            .expect("range read must succeed");
        assert_eq!(later_chunk.probably_binary, None);
    }

    #[tokio::test]
    async fn read_file_range_rejects_a_zero_or_oversized_length() {
        let (dir, service) = service();
        let target = dir.path().join("report.txt");
        std::fs::write(&target, b"contents").expect("write fixture file");

        assert!(matches!(
            service
                .read_file_range(ReadFileRangeRequestDto {
                    location: location_dto_for(&target),
                    offset: 0,
                    length: 0,
                })
                .await,
            Err(ApplicationError::InvalidRequest(_))
        ));
        assert!(matches!(
            service
                .read_file_range(ReadFileRangeRequestDto {
                    location: location_dto_for(&target),
                    offset: 0,
                    length: MAX_RANGE_LENGTH + 1,
                })
                .await,
            Err(ApplicationError::InvalidRequest(_))
        ));
    }

    #[tokio::test]
    async fn editable_file_save_uses_revision_and_preserves_external_changes() {
        let (dir, service) = service();
        let target = dir.path().join("note.json");
        std::fs::write(&target, b"{\"value\":1}").expect("write fixture file");
        let location = location_dto_for(&target);
        let loaded = service
            .load_editable_file(LoadEditableFileRequestDto {
                location: location.clone(),
            })
            .await
            .expect("editable load must succeed");
        assert_eq!(loaded.content, "{\"value\":1}");

        std::fs::write(&target, b"external").expect("simulate external edit");
        let result = service
            .save_editable_file(SaveEditableFileRequestDto {
                location,
                destination: None,
                content: "editor".to_owned(),
                expected_revision: loaded.revision,
                overwrite_conflict: false,
            })
            .await;
        assert!(matches!(
            result,
            Err(ApplicationError::FileRevisionConflict { .. })
        ));
        assert_eq!(
            std::fs::read_to_string(&target).expect("read target"),
            "external"
        );
    }

    #[tokio::test]
    async fn editable_file_explicit_overwrite_is_reported_and_audited() {
        let (dir, service) = service();
        let target = dir.path().join("note.txt");
        std::fs::write(&target, b"one").expect("write fixture file");
        let location = location_dto_for(&target);
        let loaded = service
            .load_editable_file(LoadEditableFileRequestDto {
                location: location.clone(),
            })
            .await
            .expect("load");
        std::fs::write(&target, b"two").expect("external edit");
        let saved = service
            .save_editable_file(SaveEditableFileRequestDto {
                location,
                destination: None,
                content: "three".to_owned(),
                expected_revision: loaded.revision,
                overwrite_conflict: true,
            })
            .await
            .expect("explicit overwrite");
        assert!(saved.overwrote_conflict);
        assert_eq!(
            std::fs::read_to_string(&target).expect("read target"),
            "three"
        );
        let audit =
            std::fs::read_to_string(dir.path().join("settings/audit.jsonl")).expect("audit log");
        assert!(audit.contains("note.txt"));
    }

    #[tokio::test]
    async fn editable_file_load_rejects_binary_and_oversized_files() {
        let (dir, service) = service();
        let binary = dir.path().join("binary.txt");
        std::fs::write(&binary, [1, 0, 2]).expect("write binary fixture");
        assert!(
            service
                .load_editable_file(LoadEditableFileRequestDto {
                    location: location_dto_for(&binary)
                })
                .await
                .is_err()
        );
        let large = dir.path().join("large.txt");
        std::fs::File::create(&large)
            .expect("create large fixture")
            .set_len(MAX_EDITABLE_FILE_BYTES + 1)
            .expect("size fixture");
        assert!(matches!(
            service
                .load_editable_file(LoadEditableFileRequestDto {
                    location: location_dto_for(&large)
                })
                .await,
            Err(ApplicationError::InvalidRequest(_))
        ));
    }

    #[tokio::test]
    async fn editable_file_save_as_creates_a_sibling_without_changing_the_source() {
        let (dir, service) = service();
        let source = dir.path().join("source.txt");
        let destination = dir.path().join("copy.txt");
        std::fs::write(&source, b"source").expect("write fixture");
        let location = location_dto_for(&source);
        let loaded = service
            .load_editable_file(LoadEditableFileRequestDto {
                location: location.clone(),
            })
            .await
            .expect("load");
        service
            .save_editable_file(SaveEditableFileRequestDto {
                location,
                destination: Some(location_dto_for(&destination)),
                content: "copy".to_owned(),
                expected_revision: loaded.revision,
                overwrite_conflict: false,
            })
            .await
            .expect("save as");
        assert_eq!(std::fs::read_to_string(source).expect("source"), "source");
        assert_eq!(
            std::fs::read_to_string(destination).expect("destination"),
            "copy"
        );
    }

    #[tokio::test]
    async fn search_in_file_finds_substring_matches_across_lines() {
        let (dir, service) = service();
        let target = dir.path().join("log.txt");
        std::fs::write(
            &target,
            b"first line\nsecond ERROR line\nthird error line\n",
        )
        .expect("write fixture file");

        let response = service
            .search_in_file(SearchInFileRequestDto {
                location: location_dto_for(&target),
                query: "error".to_owned(),
                regex: false,
                case_sensitive: false,
                whole_word: false,
            })
            .await
            .expect("search must succeed");

        assert_eq!(response.matches.len(), 2);
        assert_eq!(response.matches[0].line_number, 2);
        assert_eq!(response.matches[1].line_number, 3);
        assert!(!response.truncated);
    }

    #[tokio::test]
    async fn search_in_file_whole_word_excludes_matches_inside_a_larger_word() {
        let (dir, service) = service();
        let target = dir.path().join("log.txt");
        std::fs::write(&target, b"cat concatenate cats\n").expect("write fixture file");

        let response = service
            .search_in_file(SearchInFileRequestDto {
                location: location_dto_for(&target),
                query: "cat".to_owned(),
                regex: false,
                case_sensitive: false,
                whole_word: true,
            })
            .await
            .expect("search must succeed");

        assert_eq!(response.matches.len(), 1);
        assert_eq!(response.matches[0].offset, 0);
    }

    #[tokio::test]
    async fn search_in_file_rejects_an_invalid_regex() {
        let (dir, service) = service();
        let target = dir.path().join("log.txt");
        std::fs::write(&target, b"contents").expect("write fixture file");

        assert!(matches!(
            service
                .search_in_file(SearchInFileRequestDto {
                    location: location_dto_for(&target),
                    query: "(unclosed".to_owned(),
                    regex: true,
                    case_sensitive: false,
                    whole_word: false,
                })
                .await,
            Err(ApplicationError::InvalidRequest(_))
        ));
    }

    #[tokio::test]
    async fn calculate_folder_size_sums_nested_files_recursively() {
        let (dir, service) = service();
        // A dedicated subdirectory, isolated from whatever `service()` itself writes into the
        // temp dir's root (e.g. its settings file), so the walk only ever sees this test's fixtures.
        let root = dir.path().join("root");
        std::fs::create_dir(&root).expect("create root dir");
        std::fs::write(root.join("top.txt"), [0_u8; 10]).expect("write top-level fixture");
        let nested = root.join("nested");
        std::fs::create_dir(&nested).expect("create nested dir");
        std::fs::write(nested.join("a.txt"), [0_u8; 20]).expect("write nested fixture a");
        std::fs::write(nested.join("b.txt"), [0_u8; 5]).expect("write nested fixture b");
        let deeper = nested.join("deeper");
        std::fs::create_dir(&deeper).expect("create deeper dir");
        std::fs::write(deeper.join("c.txt"), [0_u8; 7]).expect("write deeper fixture c");

        let response = service
            .calculate_folder_size(fm_transport_dto::CalculateFolderSizeRequestDto {
                location: location_dto_for(&root),
            })
            .await
            .expect("calculate_folder_size must succeed");

        assert_eq!(response.total_bytes, 10 + 20 + 5 + 7);
        assert_eq!(response.file_count, 4);
    }

    #[tokio::test]
    async fn calculate_folder_size_reports_not_found_for_a_missing_directory() {
        let (dir, service) = service();
        let missing = dir.path().join("does-not-exist");

        let result = service
            .calculate_folder_size(fm_transport_dto::CalculateFolderSizeRequestDto {
                location: location_dto_for(&missing),
            })
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn archive_summary_counts_nested_zip_tar_and_seven_zip_entries() {
        let (dir, service) = service();
        let contents = [
            ("top.txt", b"top".as_slice()),
            ("docs/a.txt", b"alpha"),
            ("docs/nested/b.txt", b"bravo!"),
        ];

        let zip_path = dir.path().join("summary.zip");
        let zip_file = std::fs::File::create(&zip_path).expect("create zip fixture");
        let mut zip_writer = zip::ZipWriter::new(zip_file);
        for (name, bytes) in contents {
            zip_writer
                .start_file(
                    name,
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated),
                )
                .expect("start zip entry");
            zip_writer.write_all(bytes).expect("write zip entry");
        }
        zip_writer.finish().expect("finish zip fixture");

        let tar_path = dir.path().join("summary.tar");
        let tar_file = std::fs::File::create(&tar_path).expect("create tar fixture");
        let mut tar_writer = tar::Builder::new(tar_file);
        for (name, bytes) in contents {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar_writer
                .append_data(&mut header, name, bytes)
                .expect("write tar entry");
        }
        tar_writer.finish().expect("finish tar fixture");

        let seven_path = dir.path().join("summary.7z");
        let mut seven_writer =
            sevenz_rust2::ArchiveWriter::create(&seven_path).expect("create 7z fixture");
        for (name, bytes) in contents {
            seven_writer
                .push_archive_entry(sevenz_rust2::ArchiveEntry::new_file(name), Some(bytes))
                .expect("write 7z entry");
        }
        seven_writer.finish().expect("finish 7z fixture");

        for (path, format, compressed_size_known) in [
            (&zip_path, "zip", true),
            (&tar_path, "tar", false),
            (&seven_path, "7z", true),
        ] {
            let summary = service
                .archive_summary(ArchiveSummaryRequestDto {
                    location: location_dto_for(path),
                })
                .await
                .expect("archive summary must succeed");
            assert_eq!(summary.format, format);
            assert_eq!(summary.file_count, 3);
            assert_eq!(summary.directory_count, 2);
            assert_eq!(summary.uncompressed_size, 14);
            assert_eq!(summary.compressed_size.is_some(), compressed_size_known);
        }
    }

    #[tokio::test]
    async fn disk_usage_scan_builds_a_hierarchy_and_deduplicates_hardlinks() {
        let (dir, service) = service();
        let root = dir.path().join("usage");
        let nested = root.join("nested");
        std::fs::create_dir_all(&nested).expect("create fixture directories");
        let original = root.join("shared.bin");
        std::fs::write(&original, [7_u8; 13]).expect("write fixture file");
        std::fs::hard_link(&original, nested.join("shared-too.bin")).expect("create hardlink");
        std::fs::write(nested.join("unique.txt"), [3_u8; 5]).expect("write unique fixture");

        let response = service
            .scan_disk_usage(fm_transport_dto::ScanDiskUsageRequestDto {
                workspace_id: Uuid::new_v4(),
                scan_id: Uuid::new_v4(),
                location: location_dto_for(&root),
                expand_root: false,
            })
            .await
            .expect("disk-usage scan must succeed");

        assert_eq!(
            response.root.kind,
            fm_transport_dto::DiskUsageNodeKindDto::Directory
        );
        fn file_total(node: &fm_transport_dto::DiskUsageNodeDto) -> u64 {
            if node.kind == fm_transport_dto::DiskUsageNodeKindDto::Directory {
                node.children.iter().map(file_total).sum()
            } else {
                node.logical_bytes
            }
        }
        fn child_totals_fit(node: &fm_transport_dto::DiskUsageNodeDto) -> bool {
            let logical_children = node
                .children
                .iter()
                .map(|child| child.logical_bytes)
                .sum::<u64>();
            let physical_children = node
                .children
                .iter()
                .map(|child| child.physical_bytes)
                .sum::<u64>();
            logical_children <= node.logical_bytes
                && physical_children <= node.physical_bytes
                && node.children.iter().all(child_totals_fit)
        }

        assert_eq!(file_total(&response.root), 18);
        assert!(response.root.logical_bytes >= 18);
        assert!(child_totals_fit(&response.root));
        assert_eq!(response.root.children.len(), 2);
        assert!(
            response
                .root
                .children
                .iter()
                .any(|child| child.name == "nested" && !child.children.is_empty())
        );
    }

    #[tokio::test]
    async fn disk_usage_scan_keeps_sizes_beyond_the_display_depth_cap() {
        let (dir, service) = service();
        let root = dir.path().join("deep-usage");
        let mut left = root.join("left");
        let mut right = root.join("right");
        for level in 0..14 {
            left = left.join(format!("level-{level}"));
            right = right.join(format!("level-{level}"));
            std::fs::create_dir_all(&left).expect("create left fixture directory");
            std::fs::create_dir_all(&right).expect("create right fixture directory");
        }
        let original = root.join("shallow.bin");
        std::fs::write(&original, [9_u8; 17]).expect("write shallow fixture");
        std::fs::hard_link(&original, left.join("deep.bin")).expect("create first deep hardlink");
        std::fs::hard_link(&original, right.join("deep-too.bin"))
            .expect("create second deep hardlink");
        fn deduplicated_logical_size(
            path: &Path,
            seen_files: &mut HashSet<fm_checksum::FileIdentity>,
        ) -> u64 {
            let metadata = std::fs::symlink_metadata(path).expect("fixture metadata");
            if metadata.is_dir() {
                metadata.len()
                    + std::fs::read_dir(path)
                        .expect("read fixture directory")
                        .map(|entry| {
                            deduplicated_logical_size(
                                &entry.expect("fixture entry").path(),
                                seen_files,
                            )
                        })
                        .sum::<u64>()
            } else if fm_checksum::FileIdentity::of_path(path)
                .is_some_and(|identity| !seen_files.insert(identity))
            {
                0
            } else {
                metadata.len()
            }
        }
        let expected_logical =
            deduplicated_logical_size(&root, &mut HashSet::<fm_checksum::FileIdentity>::new());

        let response = service
            .scan_disk_usage(fm_transport_dto::ScanDiskUsageRequestDto {
                workspace_id: Uuid::new_v4(),
                scan_id: Uuid::new_v4(),
                location: location_dto_for(&root),
                expand_root: false,
            })
            .await
            .expect("deep disk-usage scan must succeed");

        assert_eq!(response.root.logical_bytes, expected_logical);
        fn child_totals_fit(node: &fm_transport_dto::DiskUsageNodeDto) -> bool {
            node.children
                .iter()
                .map(|child| child.logical_bytes)
                .sum::<u64>()
                <= node.logical_bytes
                && node
                    .children
                    .iter()
                    .map(|child| child.physical_bytes)
                    .sum::<u64>()
                    <= node.physical_bytes
                && node.children.iter().all(child_totals_fit)
        }
        assert!(child_totals_fit(&response.root));
    }

    #[tokio::test]
    async fn disk_usage_scan_bounds_wide_directory_responses() {
        let (dir, service) = service();
        let root = dir.path().join("wide-usage");
        std::fs::create_dir(&root).expect("create fixture directory");
        for index in 0..2055 {
            std::fs::write(root.join(format!("file-{index:04}.bin")), [1_u8])
                .expect("write fixture file");
        }

        let response = service
            .scan_disk_usage(fm_transport_dto::ScanDiskUsageRequestDto {
                workspace_id: Uuid::new_v4(),
                scan_id: Uuid::new_v4(),
                location: location_dto_for(&root),
                expand_root: false,
            })
            .await
            .expect("wide disk-usage scan must succeed");

        assert_eq!(response.root.children.len(), 2048);
        assert!(
            response
                .root
                .children
                .iter()
                .any(|child| child.name == "Small files (8)")
        );
        assert_eq!(
            response
                .root
                .children
                .iter()
                .map(|child| child.logical_bytes)
                .sum::<u64>(),
            2055
        );
    }

    #[tokio::test]
    async fn disk_usage_scan_streams_workspace_scoped_ordered_snapshots() {
        let dir = tempfile::tempdir().expect("must create a temp dir");
        let events = EventBus::default();
        let service = FileManagerService::with_event_bus(
            RuntimeKindDto::BrowserServer,
            dir.path(),
            dir.path().join("settings"),
            events.clone(),
        );
        let root = dir.path().join("progressive-usage");
        std::fs::create_dir(&root).expect("create fixture directory");
        for name in ["charlie.bin", "alpha.bin", "bravo.bin"] {
            std::fs::write(root.join(name), [1_u8; 8]).expect("write fixture file");
        }
        let workspace_id = Uuid::new_v4();
        let scan_id = Uuid::new_v4();
        let mut subscription = events.subscribe(
            SessionId::new("disk-usage-test"),
            [workspace_id.into()],
            None,
        );

        let response = service
            .scan_disk_usage(fm_transport_dto::ScanDiskUsageRequestDto {
                workspace_id,
                scan_id,
                location: location_dto_for(&root),
                expand_root: false,
            })
            .await
            .expect("disk-usage scan must succeed");

        let mut snapshots = Vec::new();
        let mut saw_finalizing = false;
        let mut latest_progress_names = Vec::new();
        loop {
            let SubscriptionEvent::Event(envelope) = subscription
                .recv()
                .await
                .expect("disk-usage progress event")
            else {
                panic!("expected an event envelope");
            };
            assert_eq!(envelope.workspace_id, Some(workspace_id.into()));
            let (event_scan_id, root, is_complete) = match envelope.payload {
                BackendEventPayload::DiskUsageProgress {
                    scan_id,
                    root,
                    is_complete,
                    ..
                } => (scan_id, root, is_complete),
                BackendEventPayload::DiskUsageFinalizing {
                    scan_id: finalizing_scan_id,
                    scanned_entries,
                } => {
                    assert_eq!(finalizing_scan_id, scan_id);
                    assert_eq!(scanned_entries, 3);
                    assert_eq!(
                        latest_progress_names,
                        ["alpha.bin", "bravo.bin", "charlie.bin"]
                    );
                    saw_finalizing = true;
                    continue;
                }
                _ => continue,
            };
            assert_eq!(event_scan_id, scan_id);
            latest_progress_names = root
                .children
                .iter()
                .map(|child| child.name.clone())
                .collect();
            snapshots.push((root, is_complete));
            if is_complete {
                break;
            }
        }

        assert_eq!(snapshots[0].0.children.len(), 1);
        assert!(!snapshots[0].1);
        assert!(saw_finalizing);
        assert!(snapshots.last().expect("final snapshot").1);
        let final_names = snapshots
            .last()
            .expect("final snapshot")
            .0
            .children
            .iter()
            .map(|child| child.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(final_names, ["alpha.bin", "bravo.bin", "charlie.bin"]);
        assert_eq!(snapshots.last().expect("final snapshot").0, {
            crate::disk_usage::event_node(&response.root)
        });
    }

    #[tokio::test]
    async fn disk_usage_scan_collapses_heavy_directories_and_can_expand_its_root() {
        let (dir, service) = service();
        let root = dir.path().join(".git");
        let nested = root.join("node_modules");
        std::fs::create_dir_all(&nested).expect("create fixture directories");
        std::fs::write(root.join("config"), [2_u8; 7]).expect("write root fixture");
        std::fs::write(nested.join("package.bin"), [3_u8; 11]).expect("write nested fixture");

        let collapsed = service
            .scan_disk_usage(fm_transport_dto::ScanDiskUsageRequestDto {
                workspace_id: Uuid::new_v4(),
                scan_id: Uuid::new_v4(),
                location: location_dto_for(&root),
                expand_root: false,
            })
            .await
            .expect("collapsed disk-usage scan must succeed");
        assert!(collapsed.root.collapsed);
        assert!(collapsed.root.children.is_empty());
        assert!(collapsed.root.logical_bytes >= 18);

        let expanded = service
            .scan_disk_usage(fm_transport_dto::ScanDiskUsageRequestDto {
                workspace_id: Uuid::new_v4(),
                scan_id: Uuid::new_v4(),
                location: location_dto_for(&root),
                expand_root: true,
            })
            .await
            .expect("expanded disk-usage scan must succeed");
        assert!(!expanded.root.collapsed);
        let nested = expanded
            .root
            .children
            .iter()
            .find(|child| child.name == "node_modules")
            .expect("nested heavy directory");
        assert!(nested.collapsed);
        assert!(nested.children.is_empty());
        assert!(nested.logical_bytes >= 11);
        assert_eq!(expanded.root.logical_bytes, collapsed.root.logical_bytes);
        assert_eq!(expanded.root.physical_bytes, collapsed.root.physical_bytes);
    }

    #[tokio::test]
    async fn empty_disk_usage_scan_emits_only_its_final_snapshot() {
        let dir = tempfile::tempdir().expect("must create a temp dir");
        let events = EventBus::default();
        let service = FileManagerService::with_event_bus(
            RuntimeKindDto::BrowserServer,
            dir.path(),
            dir.path().join("settings"),
            events.clone(),
        );
        let root = dir.path().join("empty-usage");
        std::fs::create_dir(&root).expect("create empty fixture directory");
        let workspace_id = Uuid::new_v4();
        let scan_id = Uuid::new_v4();
        let mut subscription = events.subscribe(
            SessionId::new("empty-disk-usage-test"),
            [workspace_id.into()],
            None,
        );

        service
            .scan_disk_usage(fm_transport_dto::ScanDiskUsageRequestDto {
                workspace_id,
                scan_id,
                location: location_dto_for(&root),
                expand_root: false,
            })
            .await
            .expect("empty disk-usage scan must succeed");

        let SubscriptionEvent::Event(envelope) =
            subscription.recv().await.expect("final disk-usage event")
        else {
            panic!("expected an event envelope");
        };
        assert!(matches!(
            envelope.payload,
            BackendEventPayload::DiskUsageProgress {
                scan_id: event_scan_id,
                is_complete: true,
                ..
            } if event_scan_id == scan_id
        ));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(25), subscription.recv())
                .await
                .is_err()
        );
    }

    /// A platform adapter test double reporting a hand-picked, non-uniform
    /// set of capabilities, so `runtime_capabilities` tests can distinguish
    /// "derives from the adapter" from "always reports every flag the same
    /// way" - a fixture where every flag were true or every flag were false
    /// would pass even if the mapping from bit to DTO field were wrong.
    #[derive(Debug, Clone, Copy)]
    struct StubPlatformAdapter;

    impl fm_platform::PlatformAdapter for StubPlatformAdapter {
        fn capabilities(&self) -> PlatformCapabilities {
            PlatformCapabilities::TRASH
                | PlatformCapabilities::OPEN_TERMINAL
                | PlatformCapabilities::NATIVE_DRAG_OUT
        }
    }

    #[test]
    fn runtime_capabilities_are_derived_from_the_injected_platform_adapter() {
        let dir = tempfile::tempdir().expect("must create a temp dir");
        let service = FileManagerService::with_platform_adapter(
            RuntimeKindDto::Tauri,
            dir.path(),
            dir.path().join("settings"),
            EventBus::default(),
            Arc::new(StubPlatformAdapter),
        );
        let capabilities = service.runtime_capabilities();

        assert!(capabilities.system_trash);
        assert!(capabilities.open_terminal);
        assert!(capabilities.native_drag_out);
        assert!(!capabilities.native_menus);
        assert!(!capabilities.native_file_icons);
        assert!(!capabilities.native_thumbnails);
        assert!(!capabilities.reveal_in_system_file_manager);
    }

    /// A platform adapter test double reporting a fixed volume capacity
    /// (task 0096), so `list_directory` tests can distinguish "the service
    /// actually calls into the adapter and forwards its result" from a
    /// coincidentally-passing empty default.
    #[derive(Debug, Clone, Copy)]
    struct VolumeCapacityPlatformAdapter {
        capabilities: PlatformCapabilities,
    }

    impl fm_platform::PlatformAdapter for VolumeCapacityPlatformAdapter {
        fn capabilities(&self) -> PlatformCapabilities {
            self.capabilities
        }

        fn volume_capacity(
            &self,
            _path: &Path,
        ) -> Result<fm_platform::VolumeCapacity, fm_platform::PlatformError> {
            Ok(fm_platform::VolumeCapacity {
                total_bytes: 1_000_000_000_000,
                available_bytes: 616_040_000_000,
            })
        }
    }

    fn list_directory_request(location: Location) -> ListDirectoryRequest {
        ListDirectoryRequest {
            workspace_id: fm_domain::WorkspaceId::new().into(),
            pane_id: fm_domain::PaneId::new().into(),
            request_id: Uuid::new_v4(),
            location: location.into(),
            continuation_token: None,
            sort: Vec::new(),
            show_hidden: false,
            folders_first: true,
            show_git_status: false,
        }
    }

    #[tokio::test]
    async fn list_directory_attaches_volume_capacity_when_the_platform_adapter_supports_it() {
        let dir = tempfile::tempdir().expect("must create a temp dir");
        let service = FileManagerService::with_platform_adapter(
            RuntimeKindDto::BrowserServer,
            dir.path(),
            dir.path().join("settings"),
            EventBus::default(),
            Arc::new(VolumeCapacityPlatformAdapter {
                capabilities: PlatformCapabilities::VOLUME_CAPACITY,
            }),
        );
        let location = Location::from_native_path(dir.path()).expect("native path location");

        let snapshot = service
            .list_directory(list_directory_request(location))
            .await
            .expect("list directory");

        let capacity = snapshot
            .volume_capacity
            .expect("volume capacity must be attached");
        assert_eq!(capacity.total_bytes, 1_000_000_000_000);
        assert_eq!(capacity.available_bytes, 616_040_000_000);
    }

    #[tokio::test]
    async fn list_directory_omits_volume_capacity_when_the_adapter_lacks_the_capability() {
        let dir = tempfile::tempdir().expect("must create a temp dir");
        let service = FileManagerService::with_platform_adapter(
            RuntimeKindDto::BrowserServer,
            dir.path(),
            dir.path().join("settings"),
            EventBus::default(),
            Arc::new(VolumeCapacityPlatformAdapter {
                capabilities: PlatformCapabilities::empty(),
            }),
        );
        let location = Location::from_native_path(dir.path()).expect("native path location");

        let snapshot = service
            .list_directory(list_directory_request(location))
            .await
            .expect("list directory");

        assert!(snapshot.volume_capacity.is_none());
    }

    #[tokio::test]
    async fn list_directory_omits_volume_capacity_for_a_non_local_location() {
        let dir = tempfile::tempdir().expect("must create a temp dir");
        let service = FileManagerService::with_platform_adapter(
            RuntimeKindDto::BrowserServer,
            dir.path(),
            dir.path().join("settings"),
            EventBus::default(),
            Arc::new(VolumeCapacityPlatformAdapter {
                capabilities: PlatformCapabilities::VOLUME_CAPACITY,
            }),
        );
        // A search location has no backing native path, so capacity lookup must
        // degrade gracefully rather than erroring the whole listing.
        let location = Location::new(
            fm_domain::ProviderId::new("search"),
            "search://local/example-search",
        );

        let capacity = volume_capacity(&service.platform, &location).await;

        assert!(capacity.is_none());
    }

    #[tokio::test]
    async fn list_directory_children_returns_only_directories() {
        let dir = tempfile::tempdir().expect("must create a temp dir");
        let content_root = dir.path().join("content");
        std::fs::create_dir(&content_root).expect("create content root");
        std::fs::create_dir(content_root.join("child")).expect("create child dir");
        std::fs::write(content_root.join("file.txt"), b"contents").expect("create file");
        let service = FileManagerService::new(
            RuntimeKindDto::BrowserServer,
            dir.path().join("workspaces"),
            dir.path().join("settings"),
        );
        let location = Location::from_native_path(&content_root).expect("native path location");

        let children = service
            .list_directory_children(fm_transport_dto::ListDirectoryChildrenRequest {
                location: location.into(),
                show_hidden: false,
            })
            .await
            .expect("list directory children");

        assert_eq!(children.len(), 1);
        assert_eq!(children[0].name, "child");
    }

    /// A platform adapter test double reporting a fixed set of mounted
    /// volumes (task 0144), so `volumes` tests can distinguish "the service
    /// actually calls into the adapter and forwards its result" from a
    /// coincidentally-passing empty default.
    #[derive(Debug, Clone, Copy)]
    struct MountedVolumesPlatformAdapter {
        capabilities: PlatformCapabilities,
    }

    impl fm_platform::PlatformAdapter for MountedVolumesPlatformAdapter {
        fn capabilities(&self) -> PlatformCapabilities {
            self.capabilities
        }

        fn mounted_volumes(
            &self,
        ) -> Result<Vec<fm_platform::MountedVolume>, fm_platform::PlatformError> {
            // Platform-native root: `from_native_path` rejects `/` on Windows (no drive letter).
            #[cfg(windows)]
            let mount_point = PathBuf::from(r"C:\");
            #[cfg(not(windows))]
            let mount_point = PathBuf::from("/");

            Ok(vec![fm_platform::MountedVolume {
                name: "Macintosh HD".to_owned(),
                mount_point,
            }])
        }
    }

    #[tokio::test]
    async fn volumes_are_surfaced_when_the_platform_adapter_supports_it() {
        let dir = tempfile::tempdir().expect("must create a temp dir");
        let service = FileManagerService::with_platform_adapter(
            RuntimeKindDto::BrowserServer,
            dir.path(),
            dir.path().join("settings"),
            EventBus::default(),
            Arc::new(MountedVolumesPlatformAdapter {
                capabilities: PlatformCapabilities::MOUNTED_VOLUMES,
            }),
        );

        let volumes = service.volumes().await.expect("volumes discovered");

        assert_eq!(volumes.len(), 1);
        assert_eq!(volumes[0].name, "Macintosh HD");
        assert_eq!(volumes[0].location.provider_id, "local");
    }

    #[tokio::test]
    async fn volumes_are_empty_when_the_adapter_lacks_the_capability() {
        let dir = tempfile::tempdir().expect("must create a temp dir");
        let service = FileManagerService::with_platform_adapter(
            RuntimeKindDto::BrowserServer,
            dir.path(),
            dir.path().join("settings"),
            EventBus::default(),
            Arc::new(MountedVolumesPlatformAdapter {
                capabilities: PlatformCapabilities::empty(),
            }),
        );

        let volumes = service.volumes().await.expect("volumes discovered");

        assert!(volumes.is_empty());
    }

    /// A platform adapter test double that records every call it receives
    /// (task 0061), so `invoke_action`'s platform dispatch can be asserted
    /// end to end: which path was passed (verifying `Location`
    /// parsing/round-tripping never mangles spaces, quotes or Unicode
    /// instead of shell-interpolating a string), and what terminal command
    /// override was forwarded from settings.
    struct RecordingPlatformAdapter {
        capabilities: PlatformCapabilities,
        opened: Mutex<Vec<PathBuf>>,
        opened_with_chooser: Mutex<Vec<PathBuf>>,
        quick_looked: Mutex<Vec<PathBuf>>,
        revealed: Mutex<Vec<PathBuf>>,
        terminals: Mutex<Vec<(PathBuf, Option<String>)>>,
        edited: Mutex<Vec<(PathBuf, Option<String>)>>,
        trashed: Mutex<Vec<PathBuf>>,
        open_error: Mutex<Option<fm_platform::PlatformError>>,
        trash_error: Mutex<Option<fm_platform::PlatformError>>,
        installed_menus: Mutex<Vec<fm_domain::NativeMenuSpec>>,
        install_native_menu_error: Mutex<Option<fm_platform::PlatformError>>,
        uninstall_plan: Mutex<
            Option<Result<fm_platform::ApplicationUninstallPlan, fm_platform::PlatformError>>,
        >,
        dock_icon_removal: Mutex<Option<Result<bool, fm_platform::PlatformError>>>,
    }

    impl RecordingPlatformAdapter {
        fn new(capabilities: PlatformCapabilities) -> Self {
            Self {
                capabilities,
                opened: Mutex::new(Vec::new()),
                opened_with_chooser: Mutex::new(Vec::new()),
                quick_looked: Mutex::new(Vec::new()),
                revealed: Mutex::new(Vec::new()),
                terminals: Mutex::new(Vec::new()),
                edited: Mutex::new(Vec::new()),
                trashed: Mutex::new(Vec::new()),
                open_error: Mutex::new(None),
                trash_error: Mutex::new(None),
                installed_menus: Mutex::new(Vec::new()),
                install_native_menu_error: Mutex::new(None),
                uninstall_plan: Mutex::new(None),
                dock_icon_removal: Mutex::new(None),
            }
        }

        fn set_next_uninstall_plan(
            &self,
            result: Result<fm_platform::ApplicationUninstallPlan, fm_platform::PlatformError>,
        ) {
            *self
                .uninstall_plan
                .lock()
                .expect("lock must not be poisoned") = Some(result);
        }

        fn set_next_dock_icon_removal(&self, result: Result<bool, fm_platform::PlatformError>) {
            *self
                .dock_icon_removal
                .lock()
                .expect("lock must not be poisoned") = Some(result);
        }

        fn fail_next_open_with(&self, error: fm_platform::PlatformError) {
            *self.open_error.lock().expect("lock must not be poisoned") = Some(error);
        }

        fn fail_next_trash_with(&self, error: fm_platform::PlatformError) {
            *self.trash_error.lock().expect("lock must not be poisoned") = Some(error);
        }

        fn fail_next_install_native_menu_with(&self, error: fm_platform::PlatformError) {
            *self
                .install_native_menu_error
                .lock()
                .expect("lock must not be poisoned") = Some(error);
        }
    }

    impl fm_platform::PlatformAdapter for RecordingPlatformAdapter {
        fn capabilities(&self) -> PlatformCapabilities {
            self.capabilities
        }

        fn open_with_default_application(
            &self,
            path: &Path,
        ) -> Result<(), fm_platform::PlatformError> {
            if let Some(error) = self
                .open_error
                .lock()
                .expect("lock must not be poisoned")
                .take()
            {
                return Err(error);
            }
            self.opened
                .lock()
                .expect("lock must not be poisoned")
                .push(path.to_path_buf());
            Ok(())
        }

        fn reveal_in_file_manager(&self, path: &Path) -> Result<(), fm_platform::PlatformError> {
            self.revealed
                .lock()
                .expect("lock must not be poisoned")
                .push(path.to_path_buf());
            Ok(())
        }

        fn trash(&self, path: &Path) -> Result<(), fm_platform::PlatformError> {
            if let Some(error) = self
                .trash_error
                .lock()
                .expect("lock must not be poisoned")
                .take()
            {
                return Err(error);
            }
            self.trashed
                .lock()
                .expect("lock must not be poisoned")
                .push(path.to_path_buf());
            Ok(())
        }

        fn open_terminal(
            &self,
            path: &Path,
            command_override: Option<&str>,
        ) -> Result<(), fm_platform::PlatformError> {
            self.terminals
                .lock()
                .expect("lock must not be poisoned")
                .push((path.to_path_buf(), command_override.map(str::to_owned)));
            Ok(())
        }

        fn open_in_text_editor(
            &self,
            path: &Path,
            command_override: Option<&str>,
        ) -> Result<(), fm_platform::PlatformError> {
            self.edited
                .lock()
                .expect("lock must not be poisoned")
                .push((path.to_path_buf(), command_override.map(str::to_owned)));
            Ok(())
        }

        fn open_with_chooser(&self, path: &Path) -> Result<(), fm_platform::PlatformError> {
            self.opened_with_chooser
                .lock()
                .expect("lock must not be poisoned")
                .push(path.to_path_buf());
            Ok(())
        }

        fn quick_look(&self, path: &Path) -> Result<(), fm_platform::PlatformError> {
            self.quick_looked
                .lock()
                .expect("lock must not be poisoned")
                .push(path.to_path_buf());
            Ok(())
        }

        fn install_native_menu(
            &self,
            spec: &fm_domain::NativeMenuSpec,
            on_action: Arc<dyn Fn(String) + Send + Sync>,
        ) -> Result<(), fm_platform::PlatformError> {
            if let Some(error) = self
                .install_native_menu_error
                .lock()
                .expect("lock must not be poisoned")
                .take()
            {
                return Err(error);
            }
            self.installed_menus
                .lock()
                .expect("lock must not be poisoned")
                .push(spec.clone());
            // Exercises the wiring end to end: a real caller's `on_action`
            // would forward this to the frontend over a Tauri `Channel`.
            on_action("recorded-action-id".to_owned());
            Ok(())
        }

        fn plan_application_uninstall(
            &self,
            bundle_path: &Path,
        ) -> Result<fm_platform::ApplicationUninstallPlan, fm_platform::PlatformError> {
            self.uninstall_plan
                .lock()
                .expect("lock must not be poisoned")
                .take()
                .unwrap_or_else(|| {
                    Ok(fm_platform::ApplicationUninstallPlan {
                        bundle_identifier: None,
                        product_name: bundle_path
                            .file_stem()
                            .map(|stem| stem.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                        related_files: Vec::new(),
                    })
                })
        }

        fn remove_application_dock_icon(
            &self,
            _bundle_path: &Path,
        ) -> Result<bool, fm_platform::PlatformError> {
            self.dock_icon_removal
                .lock()
                .expect("lock must not be poisoned")
                .take()
                .unwrap_or(Ok(false))
        }
    }

    /// Builds a service backed by a [`RecordingPlatformAdapter`] reporting
    /// every platform capability task 0061/0043 cares about, and returns the
    /// adapter (still owned via a second `Arc`) so tests can inspect what it
    /// recorded.
    fn service_with_recording_adapter() -> (
        tempfile::TempDir,
        FileManagerService,
        Arc<RecordingPlatformAdapter>,
    ) {
        let dir = tempfile::tempdir().expect("must create a temp dir");
        let adapter = Arc::new(RecordingPlatformAdapter::new(
            PlatformCapabilities::OPEN_WITH_DEFAULT_APPLICATION
                | PlatformCapabilities::REVEAL_IN_FILE_MANAGER
                | PlatformCapabilities::OPEN_TERMINAL
                | PlatformCapabilities::TRASH
                | PlatformCapabilities::QUICK_LOOK,
        ));
        let service = FileManagerService::with_platform_adapter(
            RuntimeKindDto::Tauri,
            dir.path(),
            dir.path().join("settings"),
            EventBus::default(),
            adapter.clone(),
        );
        (dir, service, adapter)
    }

    fn single_selection_context() -> fm_transport_dto::ActionInvocationContextDto {
        fm_transport_dto::ActionInvocationContextDto {
            selected_entry_ids: vec![uuid::Uuid::new_v4()],
            ..Default::default()
        }
    }

    #[test]
    fn invoke_action_opens_the_uri_parameters_path_with_the_default_application() {
        let (dir, service, adapter) = service_with_recording_adapter();
        // Spaces, single/double quotes and non-ASCII must round-trip exactly:
        // this proves the dispatch parses the URI via `Location` rather than
        // building a shell command string.
        let target = dir.path().join("with spaces & 'quotes' café.txt");
        std::fs::write(&target, b"contents").expect("write fixture file");
        let uri = Location::from_native_path(&target)
            .expect("path must convert to a location")
            .uri;

        let result = service
            .invoke_action(
                "core.open".to_owned(),
                InvokeActionRequestDto {
                    parameters: Some(serde_json::json!({ "uri": uri })),
                    context: single_selection_context(),
                },
                None,
            )
            .expect("core.open must dispatch to the platform adapter");

        assert!(result.invoked);
        assert!(result.operation_id.is_none());
        assert_eq!(adapter.opened.lock().unwrap().as_slice(), [target]);
    }

    #[test]
    fn invoke_action_reveals_the_uri_parameters_path() {
        let (dir, service, adapter) = service_with_recording_adapter();
        let target = dir.path().join("report.pdf");
        let uri = Location::from_native_path(&target)
            .expect("path must convert to a location")
            .uri;

        service
            .invoke_action(
                "core.revealInSystemFileManager".to_owned(),
                InvokeActionRequestDto {
                    parameters: Some(serde_json::json!({ "uri": uri })),
                    context: single_selection_context(),
                },
                None,
            )
            .expect("core.revealInSystemFileManager must dispatch to the platform adapter");

        assert_eq!(adapter.revealed.lock().unwrap().as_slice(), [target]);
    }

    #[test]
    fn invoke_action_open_terminal_forwards_the_configured_terminal_command_override() {
        let (dir, service, adapter) = service_with_recording_adapter();
        let mut settings = service.get_settings();
        settings.terminal_command = Some("alacritty".to_owned());
        service
            .update_settings(settings)
            .expect("settings update must succeed");
        let uri = Location::from_native_path(dir.path())
            .expect("path must convert to a location")
            .uri;

        service
            .invoke_action(
                "core.openTerminal".to_owned(),
                InvokeActionRequestDto {
                    parameters: Some(serde_json::json!({ "uri": uri })),
                    context: fm_transport_dto::ActionInvocationContextDto::default(),
                },
                None,
            )
            .expect("core.openTerminal must dispatch to the platform adapter");

        assert_eq!(
            adapter.terminals.lock().unwrap().as_slice(),
            [(dir.path().to_path_buf(), Some("alacritty".to_owned()))]
        );
    }

    #[test]
    fn invoke_action_views_the_uri_parameters_path_with_the_default_application() {
        // core.view (task 0087) is a documented stopgap that dispatches
        // exactly like core.open until a real in-app viewer (task 0088) exists.
        let (dir, service, adapter) = service_with_recording_adapter();
        let target = dir.path().join("report.pdf");
        let uri = Location::from_native_path(&target)
            .expect("path must convert to a location")
            .uri;

        service
            .invoke_action(
                "core.view".to_owned(),
                InvokeActionRequestDto {
                    parameters: Some(serde_json::json!({ "uri": uri })),
                    context: single_selection_context(),
                },
                None,
            )
            .expect("core.view must dispatch to the platform adapter");

        assert_eq!(adapter.opened.lock().unwrap().as_slice(), [target]);
    }

    #[test]
    fn invoke_action_open_with_shows_the_chooser_not_the_default_application() {
        // core.openWith (task 0061 follow-up) dispatches to the platform
        // adapter's distinct open_with_chooser method, not the same
        // open_with_default_application path as core.open/core.view.
        let (dir, service, adapter) = service_with_recording_adapter();
        let target = dir.path().join("report.pdf");
        let uri = Location::from_native_path(&target)
            .expect("path must convert to a location")
            .uri;

        service
            .invoke_action(
                "core.openWith".to_owned(),
                InvokeActionRequestDto {
                    parameters: Some(serde_json::json!({ "uri": uri })),
                    context: single_selection_context(),
                },
                None,
            )
            .expect("core.openWith must dispatch to the platform adapter");

        assert_eq!(
            adapter.opened_with_chooser.lock().unwrap().as_slice(),
            [target]
        );
        assert!(
            adapter.opened.lock().unwrap().is_empty(),
            "core.openWith must not dispatch through open_with_default_application"
        );
    }

    #[test]
    fn invoke_action_quick_looks_the_native_unicode_path() {
        let (dir, service, adapter) = service_with_recording_adapter();
        let target = dir.path().join("résumé 'final'.pdf");
        let uri = Location::from_native_path(&target)
            .expect("path must convert to a location")
            .uri;

        service
            .invoke_action(
                "core.quickLook".to_owned(),
                InvokeActionRequestDto {
                    parameters: Some(serde_json::json!({ "uri": uri })),
                    context: single_selection_context(),
                },
                None,
            )
            .expect("core.quickLook must dispatch to the platform adapter");

        assert_eq!(
            adapter.quick_looked.lock().unwrap().as_slice(),
            [target],
            "the native path must be preserved without shell interpolation"
        );
    }

    #[test]
    fn invoke_action_quick_look_rejects_non_local_locations() {
        let (_dir, service, adapter) = service_with_recording_adapter();

        let error = service
            .invoke_action(
                "core.quickLook".to_owned(),
                InvokeActionRequestDto {
                    parameters: Some(serde_json::json!({
                        "uri": format!("sftp://{}/report.pdf", uuid::Uuid::new_v4())
                    })),
                    context: single_selection_context(),
                },
                None,
            )
            .expect_err("Quick Look must not materialize remote files");

        assert_eq!(
            error,
            ApplicationError::ActionUnavailable(ActionId::new("core.quickLook"))
        );
        assert!(adapter.quick_looked.lock().unwrap().is_empty());
    }

    #[test]
    fn invoke_action_edit_opens_the_uri_parameters_path_in_a_text_editor() {
        let (dir, service, adapter) = service_with_recording_adapter();
        let target = dir.path().join("notes.txt");
        let uri = Location::from_native_path(&target)
            .expect("path must convert to a location")
            .uri;

        service
            .invoke_action(
                "core.edit".to_owned(),
                InvokeActionRequestDto {
                    parameters: Some(serde_json::json!({ "uri": uri })),
                    context: single_selection_context(),
                },
                None,
            )
            .expect("core.edit must dispatch to the platform adapter");

        assert_eq!(adapter.edited.lock().unwrap().as_slice(), [(target, None)]);
    }

    #[test]
    fn invoke_action_edit_forwards_the_configured_editor_command_override() {
        let (dir, service, adapter) = service_with_recording_adapter();
        let mut settings = service.get_settings();
        settings.editor_command = Some("code --wait".to_owned());
        service
            .update_settings(settings)
            .expect("settings update must succeed");
        let target = dir.path().join("notes.txt");
        let uri = Location::from_native_path(&target)
            .expect("path must convert to a location")
            .uri;

        service
            .invoke_action(
                "core.edit".to_owned(),
                InvokeActionRequestDto {
                    parameters: Some(serde_json::json!({ "uri": uri })),
                    context: single_selection_context(),
                },
                None,
            )
            .expect("core.edit must dispatch to the platform adapter");

        assert_eq!(
            adapter.edited.lock().unwrap().as_slice(),
            [(target, Some("code --wait".to_owned()))]
        );
    }

    /// Builds a `Trash` request targeting the given native paths.
    fn trash_request(paths: &[&std::path::Path]) -> StartOperationRequestDto {
        StartOperationRequestDto {
            operation_type: OperationKindDto::Trash,
            sources: paths
                .iter()
                .map(|path| {
                    Location::from_native_path(path)
                        .expect("path must convert to a location")
                        .into()
                })
                .collect(),
            destination: None,
            destinations: vec![],
            conflict_policy: OperationConflictPolicyDto::Ask,
            name: None,
            archive_format: None,
            archive_compression_level: None,
            create_intermediate_directories: false,
            symlink_policy: Default::default(),
            permanent_delete_confirmed: false,
            override_read_only: false,
        }
    }

    struct RestorableTrashAdapter {
        directory: PathBuf,
    }

    impl fm_platform::PlatformAdapter for RestorableTrashAdapter {
        fn capabilities(&self) -> PlatformCapabilities {
            PlatformCapabilities::TRASH
        }

        fn trash_with_restore_location(
            &self,
            path: &Path,
        ) -> Result<Option<PathBuf>, fm_platform::PlatformError> {
            std::fs::create_dir_all(&self.directory).map_err(|_| {
                fm_platform::PlatformError::Io {
                    message: "could not create test trash directory".into(),
                }
            })?;
            let destination = self.directory.join(path.file_name().ok_or_else(|| {
                fm_platform::PlatformError::Io {
                    message: "trash path has no file name".into(),
                }
            })?);
            std::fs::rename(path, &destination).map_err(|_| fm_platform::PlatformError::Io {
                message: "could not move test item to trash".into(),
            })?;
            Ok(Some(destination))
        }
    }

    async fn wait_for_terminal_operation(
        service: &FileManagerService,
        id: fm_domain::OperationId,
    ) -> OperationDto {
        loop {
            let operation = service.get_operation(id).expect("operation must exist");
            if matches!(
                operation.state,
                OperationStateDto::Completed
                    | OperationStateDto::CompletedWithWarnings
                    | OperationStateDto::Failed
                    | OperationStateDto::Cancelled
            ) {
                return operation;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }

    #[tokio::test]
    async fn start_operation_trash_dispatches_every_source_to_the_platform_adapter() {
        let (dir, service, adapter) = service_with_recording_adapter();
        let first = dir.path().join("trash-me.txt");
        let second = dir.path().join("trash-me-too.txt");
        std::fs::write(&first, b"1").expect("write first fixture");
        std::fs::create_dir(&second).expect("create second fixture");
        std::fs::write(second.join("nested.txt"), b"2345").expect("write nested fixture");

        let started = service
            .start_operation(trash_request(&[&first, &second]), None)
            .expect("trash must be accepted when TRASH capability is available");
        let result = wait_for_terminal_operation(&service, started.id.into()).await;

        assert_eq!(result.state, OperationStateDto::Completed);
        assert_eq!(result.progress.total_items, Some(2));
        assert_eq!(result.progress.completed_items, 2);
        assert_eq!(result.progress.total_bytes, Some(5));
        assert_eq!(result.progress.completed_bytes, 5);
        assert_eq!(adapter.trashed.lock().unwrap().as_slice(), [first, second]);
        // Trashing never routes through a `FileSystemProvider::remove` call,
        // so the fixtures are untouched by this test double; only the real
        // macOS adapter test (`fm-platform-macos`) exercises an actual move.
    }

    #[tokio::test]
    async fn trash_undo_restores_a_directory_with_read_only_descendants() {
        let dir = tempfile::tempdir().expect("must create a temp dir");
        let source = dir.path().join("trash-me");
        let file = source.join(".git/objects/pack/index.idx");
        std::fs::create_dir_all(file.parent().expect("fixture file must have a parent"))
            .expect("create fixture tree");
        std::fs::write(&file, b"content").expect("write fixture");
        let mut permissions = std::fs::metadata(&file)
            .expect("read fixture metadata")
            .permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&file, permissions).expect("make fixture read-only");
        let service = FileManagerService::with_platform_adapter(
            RuntimeKindDto::Tauri,
            dir.path().join("workspaces"),
            dir.path().join("settings"),
            EventBus::default(),
            Arc::new(RestorableTrashAdapter {
                directory: dir.path().join("trash"),
            }),
        );

        let started = service
            .start_operation(trash_request(&[&source]), None)
            .expect("trash must be accepted");
        let completed = wait_for_terminal_operation(&service, started.id.into()).await;
        assert!(completed.undo.available);
        assert!(!source.exists());

        let undo = service
            .undo_operation(started.id.into())
            .expect("trash undo must be accepted");
        let undone = wait_for_terminal_operation(&service, undo.id.into()).await;
        assert_eq!(undone.state, OperationStateDto::Completed);
        assert_eq!(std::fs::read(&file).expect("restored fixture"), b"content");
        #[cfg(windows)]
        {
            let mut permissions = std::fs::metadata(&file)
                .expect("read restored fixture metadata")
                .permissions();
            // On Windows this clears the read-only file attribute; the Unix warning is inapplicable.
            #[allow(clippy::permissions_set_readonly_false)]
            permissions.set_readonly(false);
            std::fs::set_permissions(file, permissions).expect("make restored fixture writable");
        }
    }

    #[test]
    fn start_operation_trash_is_rejected_when_the_platform_reports_no_trash_capability() {
        let dir = tempfile::tempdir().expect("must create a temp dir");
        let file = dir.path().join("trash-me.txt");
        std::fs::write(&file, b"content").expect("write fixture");
        // `FileManagerService::new` defaults to `FallbackPlatformAdapter`,
        // which reports no capabilities at all (browser/server mode).
        let service = FileManagerService::new(
            RuntimeKindDto::BrowserServer,
            dir.path().join("workspaces"),
            dir.path().join("settings"),
        );

        let error = service
            .start_operation(trash_request(&[&file]), None)
            .expect_err("trash must be rejected without the TRASH capability");

        assert_eq!(
            error.code(),
            fm_transport_dto::ApplicationErrorCode::PlatformOperationFailed
        );
        assert!(file.exists(), "no attempt to move the file must be made");
    }

    #[tokio::test]
    async fn start_operation_trash_reports_completed_with_warnings_on_a_platform_failure() {
        let (dir, service, adapter) = service_with_recording_adapter();
        let file = dir.path().join("stubborn.txt");
        std::fs::write(&file, b"content").expect("write fixture");
        adapter.fail_next_trash_with(fm_platform::PlatformError::Io {
            message: "permission denied".to_owned(),
        });

        let started = service
            .start_operation(trash_request(&[&file]), None)
            .expect("trash must be accepted when TRASH capability is available");
        let result = wait_for_terminal_operation(&service, started.id.into()).await;

        assert_eq!(result.state, OperationStateDto::CompletedWithWarnings);
    }

    #[test]
    fn invoke_action_rejects_a_missing_uri_parameter_as_invalid_request() {
        let (_dir, service, _adapter) = service_with_recording_adapter();

        let error = service
            .invoke_action(
                "core.open".to_owned(),
                InvokeActionRequestDto {
                    parameters: None,
                    context: single_selection_context(),
                },
                None,
            )
            .expect_err("a missing uri parameter must be rejected");

        assert_eq!(
            error.code(),
            fm_transport_dto::ApplicationErrorCode::InvalidRequest
        );
    }

    #[test]
    fn invoke_action_rejects_a_malformed_uri_as_invalid_request() {
        let (_dir, service, _adapter) = service_with_recording_adapter();

        let error = service
            .invoke_action(
                "core.open".to_owned(),
                InvokeActionRequestDto {
                    parameters: Some(serde_json::json!({ "uri": "not a valid uri" })),
                    context: single_selection_context(),
                },
                None,
            )
            .expect_err("a malformed uri must be rejected");

        assert_eq!(
            error.code(),
            fm_transport_dto::ApplicationErrorCode::InvalidRequest
        );
    }

    #[test]
    fn invoke_action_maps_a_genuine_platform_failure_to_a_user_readable_platform_operation_failed_error()
     {
        let (dir, service, adapter) = service_with_recording_adapter();
        adapter.fail_next_open_with(fm_platform::PlatformError::Io {
            message: "no default application is registered for .xyz files".to_owned(),
        });
        let uri = Location::from_native_path(&dir.path().join("mystery.xyz"))
            .expect("path must convert to a location")
            .uri;

        let error = service
            .invoke_action(
                "core.open".to_owned(),
                InvokeActionRequestDto {
                    parameters: Some(serde_json::json!({ "uri": uri })),
                    context: single_selection_context(),
                },
                None,
            )
            .expect_err("a genuine platform failure must be reported, not swallowed");

        assert_eq!(
            error.code(),
            fm_transport_dto::ApplicationErrorCode::PlatformOperationFailed
        );
        assert!(
            error
                .to_string()
                .contains("no default application is registered for .xyz files")
        );
    }

    /// `install_native_menu` (task 0133) is a thin passthrough: it forwards
    /// the spec and callback unchanged to the platform adapter, and maps
    /// whatever failure the adapter reports to a user-readable
    /// `PlatformOperationFailed` error rather than swallowing it.
    #[test]
    fn install_native_menu_forwards_the_spec_and_maps_adapter_failures() {
        let (_dir, service, adapter) = service_with_recording_adapter();
        let spec = fm_domain::NativeMenuSpec {
            menus: vec![fm_domain::NativeMenu {
                title: "File".to_owned(),
                items: vec![fm_domain::NativeMenuItem::Action {
                    id: "core.newWindow".to_owned(),
                    title: "New Window".to_owned(),
                    shortcut: None,
                    enabled: true,
                    checked: false,
                }],
            }],
        };
        let received_ids = Arc::new(Mutex::new(Vec::new()));
        let received_ids_clone = Arc::clone(&received_ids);
        let on_action: Arc<dyn Fn(String) + Send + Sync> =
            Arc::new(move |id| received_ids_clone.lock().unwrap().push(id));

        service
            .install_native_menu(&spec, Arc::clone(&on_action))
            .expect("the recording adapter always succeeds by default");

        assert_eq!(
            adapter.installed_menus.lock().unwrap().as_slice(),
            std::slice::from_ref(&spec)
        );
        assert_eq!(
            received_ids.lock().unwrap().as_slice(),
            &["recorded-action-id".to_owned()]
        );

        adapter.fail_next_install_native_menu_with(fm_platform::PlatformError::Unsupported {
            capability: PlatformCapabilities::NATIVE_MENUS,
        });
        let error = service
            .install_native_menu(&spec, on_action)
            .expect_err("an adapter failure must be reported, not swallowed");
        assert_eq!(
            error.code(),
            fm_transport_dto::ApplicationErrorCode::PlatformOperationFailed
        );
    }

    /// Task 0148: discovery is a plain platform dispatch, like
    /// `core.revealInSystemFileManager`/`core.openTerminal` - it forwards the
    /// bundle's native path unchanged and translates whatever the adapter
    /// found into the DTOs the frontend's review checklist renders.
    #[test]
    fn discover_application_uninstall_candidates_returns_the_planned_bundle_identity_and_related_files()
     {
        let (dir, service, adapter) = service_with_recording_adapter();
        let bundle_path = dir.path().join("Widget.app");
        let related_path = dir.path().join("Widget-support");
        adapter.set_next_uninstall_plan(Ok(fm_platform::ApplicationUninstallPlan {
            bundle_identifier: Some("com.example.Widget".to_owned()),
            product_name: "Widget".to_owned(),
            related_files: vec![fm_platform::UninstallCandidate {
                path: related_path.clone(),
                size_bytes: 4096,
                removable: true,
            }],
        }));
        let location = Location::from_native_path(&bundle_path).expect("native path location");

        let response = service
            .discover_application_uninstall_candidates(
                DiscoverApplicationUninstallCandidatesRequestDto {
                    location: location.into(),
                },
            )
            .expect("discovery must succeed");

        assert_eq!(
            response.bundle_identifier.as_deref(),
            Some("com.example.Widget")
        );
        assert_eq!(response.product_name, "Widget");
        assert_eq!(response.related_files.len(), 1);
        let candidate = &response.related_files[0];
        assert_eq!(candidate.size_bytes, 4096);
        assert!(candidate.removable);
        let candidate_location: Location = candidate.location.clone().into();
        assert_eq!(
            candidate_location.to_native_path().expect("native path"),
            related_path
        );
    }

    #[test]
    fn discover_application_uninstall_candidates_reports_a_missing_bundle_as_not_found() {
        let (dir, service, adapter) = service_with_recording_adapter();
        adapter.set_next_uninstall_plan(Err(fm_platform::PlatformError::NotFound {
            path: dir.path().join("Missing.app").display().to_string(),
        }));
        let location = Location::from_native_path(&dir.path().join("Missing.app"))
            .expect("native path location");

        let error = service
            .discover_application_uninstall_candidates(
                DiscoverApplicationUninstallCandidatesRequestDto {
                    location: location.into(),
                },
            )
            .expect_err("a missing bundle must not be reported as a generic platform failure");

        assert_eq!(error, ApplicationError::NotFound);
    }

    #[test]
    fn discover_application_uninstall_candidates_maps_unsupported_to_action_unavailable() {
        let (dir, service, adapter) = service_with_recording_adapter();
        adapter.set_next_uninstall_plan(Err(fm_platform::PlatformError::Unsupported {
            capability: PlatformCapabilities::APPLICATION_UNINSTALL,
        }));
        let location = Location::from_native_path(&dir.path().join("Widget.app"))
            .expect("native path location");

        let error = service
            .discover_application_uninstall_candidates(
                DiscoverApplicationUninstallCandidatesRequestDto {
                    location: location.into(),
                },
            )
            .expect_err("an unsupported capability must be reported as action-unavailable");

        assert_eq!(
            error,
            ApplicationError::ActionUnavailable(ActionId::new("core.uninstallApplication"))
        );
    }

    #[test]
    fn discover_application_uninstall_candidates_rejects_a_non_local_location() {
        let (_dir, service, _adapter) = service_with_recording_adapter();
        let location = Location::new(
            fm_domain::ProviderId::new("search"),
            "search://local/example-search",
        );

        let error = service
            .discover_application_uninstall_candidates(
                DiscoverApplicationUninstallCandidatesRequestDto {
                    location: location.into(),
                },
            )
            .expect_err("a non-local location has no native path to scan");

        assert!(matches!(error, ApplicationError::InvalidRequest(_)));
    }

    #[test]
    fn remove_application_dock_icon_reports_whether_a_pinned_icon_was_found() {
        let (dir, service, adapter) = service_with_recording_adapter();
        adapter.set_next_dock_icon_removal(Ok(true));
        let location =
            Location::from_native_path(&dir.path().join("Widget.app")).expect("native path");

        let response = service
            .remove_application_dock_icon(RemoveApplicationDockIconRequestDto {
                location: location.into(),
            })
            .expect("removal must succeed");

        assert!(response.removed);
    }

    #[test]
    fn remove_application_dock_icon_treats_unsupported_as_not_removed_rather_than_an_error() {
        let (dir, service, adapter) = service_with_recording_adapter();
        adapter.set_next_dock_icon_removal(Err(fm_platform::PlatformError::Unsupported {
            capability: PlatformCapabilities::APPLICATION_UNINSTALL,
        }));
        let location =
            Location::from_native_path(&dir.path().join("Widget.app")).expect("native path");

        let response = service
            .remove_application_dock_icon(RemoveApplicationDockIconRequestDto {
                location: location.into(),
            })
            .expect("an unsupported adapter must degrade to removed: false, not an error");

        assert!(!response.removed);
    }

    #[test]
    fn remove_application_dock_icon_surfaces_a_genuine_platform_failure() {
        let (dir, service, adapter) = service_with_recording_adapter();
        adapter.set_next_dock_icon_removal(Err(fm_platform::PlatformError::Io {
            message: "failed to update Dock preferences".to_owned(),
        }));
        let location =
            Location::from_native_path(&dir.path().join("Widget.app")).expect("native path");

        let error = service
            .remove_application_dock_icon(RemoveApplicationDockIconRequestDto {
                location: location.into(),
            })
            .expect_err("a genuine I/O failure must be reported, not swallowed");

        assert_eq!(
            error.code(),
            fm_transport_dto::ApplicationErrorCode::PlatformOperationFailed
        );
    }

    #[test]
    fn invoke_action_reveal_and_terminal_are_unavailable_in_browser_server_mode() {
        let (_dir, service) = service();
        let context = single_selection_context();

        let reveal_error = service
            .invoke_action(
                "core.revealInSystemFileManager".to_owned(),
                InvokeActionRequestDto {
                    parameters: Some(serde_json::json!({ "uri": "file:///tmp/report.pdf" })),
                    context: context.clone(),
                },
                None,
            )
            .expect_err("reveal has no native access in browser/server mode");
        assert_eq!(
            reveal_error.code(),
            fm_transport_dto::ApplicationErrorCode::ActionUnavailable
        );

        let terminal_error = service
            .invoke_action(
                "core.openTerminal".to_owned(),
                InvokeActionRequestDto {
                    parameters: Some(serde_json::json!({ "uri": "file:///tmp" })),
                    context: fm_transport_dto::ActionInvocationContextDto::default(),
                },
                None,
            )
            .expect_err("openTerminal has no native access in browser/server mode");
        assert_eq!(
            terminal_error.code(),
            fm_transport_dto::ApplicationErrorCode::ActionUnavailable
        );

        for action_id in ["core.view", "core.edit"] {
            let error = service
                .invoke_action(
                    action_id.to_owned(),
                    InvokeActionRequestDto {
                        parameters: Some(serde_json::json!({ "uri": "file:///tmp/report.pdf" })),
                        context: context.clone(),
                    },
                    None,
                )
                .expect_err("view/edit have no native access in browser/server mode");
            assert_eq!(
                error.code(),
                fm_transport_dto::ApplicationErrorCode::ActionUnavailable
            );
        }
    }

    #[test]
    fn list_actions_includes_every_core_and_reserved_action_id() {
        let (_dir, service) = service();
        let ids: Vec<String> = service
            .list_actions()
            .into_iter()
            .map(|action| action.id)
            .collect();

        for expected in [
            "core.copy",
            "core.rename",
            "core.selectAll",
            "core.open",
            "core.paste",
            "core.refresh",
            "core.openTerminal",
        ] {
            assert!(ids.iter().any(|id| id == expected), "missing {expected}");
        }
    }

    #[test]
    fn invoke_action_reports_an_unknown_action_without_panicking() {
        let (_dir, service) = service();
        let error = service
            .invoke_action(
                "does.not.exist".to_owned(),
                InvokeActionRequestDto::default(),
                None,
            )
            .expect_err("an unregistered action must be reported, not panic");
        assert_eq!(
            error,
            ApplicationError::ActionNotFound(fm_domain::ActionId::new("does.not.exist"))
        );
    }

    #[test]
    fn invoke_action_reports_unavailable_for_a_feature_without_a_backend_implementation() {
        let (_dir, service) = service();
        let error = service
            .invoke_action(
                "core.open".to_owned(),
                InvokeActionRequestDto::default(),
                None,
            )
            .expect_err("core.open has no backend feature yet");
        assert_eq!(
            error,
            ApplicationError::ActionUnavailable(fm_domain::ActionId::new("core.open"))
        );
    }

    #[test]
    fn invoke_action_reports_unavailable_when_context_requirements_are_not_met() {
        let (_dir, service) = service();
        let error = service
            .invoke_action(
                "core.rename".to_owned(),
                InvokeActionRequestDto::default(),
                None,
            )
            .expect_err("rename requires exactly one selected entry");
        assert_eq!(
            error,
            ApplicationError::ActionUnavailable(fm_domain::ActionId::new("core.rename"))
        );
    }

    #[test]
    fn invoke_action_returns_invoked_without_an_operation_for_non_mutating_actions() {
        let (_dir, service) = service();
        let result = service
            .invoke_action(
                "core.selectAll".to_owned(),
                InvokeActionRequestDto::default(),
                None,
            )
            .expect("core.selectAll has no context requirements");
        assert_eq!(result.action_id, "core.selectAll");
        assert!(result.invoked);
        assert!(result.operation_id.is_none());
    }

    #[tokio::test]
    async fn invoke_action_delegates_create_directory_to_the_operation_engine() {
        let (dir, service) = service();
        let parent = dir.path().join("parent");
        std::fs::create_dir_all(&parent).expect("must create parent directory");
        let destination = fm_transport_dto::LocationDto {
            provider_id: "local".to_owned(),
            uri: Location::from_native_path(&parent)
                .expect("path must convert to a location")
                .uri,
        };
        let parameters = serde_json::to_value(StartOperationRequestDto {
            operation_type: OperationKindDto::CreateDirectory,
            sources: Vec::new(),
            destination: Some(destination),
            destinations: vec![],
            conflict_policy: OperationConflictPolicyDto::Ask,
            name: Some("child".to_owned()),
            archive_format: None,
            archive_compression_level: None,
            create_intermediate_directories: false,
            symlink_policy: fm_transport_dto::SymlinkPolicyDto::default(),
            permanent_delete_confirmed: false,
            override_read_only: false,
        })
        .expect("must serialize the operation request");

        let result = service
            .invoke_action(
                "core.createDirectory".to_owned(),
                InvokeActionRequestDto {
                    parameters: Some(parameters),
                    context: fm_transport_dto::ActionInvocationContextDto::default(),
                },
                None,
            )
            .expect("createDirectory has no context requirements");

        assert!(result.invoked);
        let operation_id = result
            .operation_id
            .expect("a mutating action must return an operation id");
        let operation = loop {
            let current = service
                .get_operation(OperationId::from(operation_id))
                .expect("the started operation must be retrievable");
            if matches!(
                current.state,
                OperationStateDto::Completed | OperationStateDto::Failed
            ) {
                break current;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        };
        assert_eq!(operation.state, OperationStateDto::Completed);
        assert!(parent.join("child").is_dir());
    }

    #[test]
    fn invoke_action_reports_invalid_request_when_mutating_action_parameters_are_missing() {
        let (_dir, service) = service();
        let error = service
            .invoke_action(
                "core.createDirectory".to_owned(),
                InvokeActionRequestDto::default(),
                None,
            )
            .expect_err("createDirectory requires parameters");
        assert_eq!(
            error.code(),
            fm_transport_dto::ApplicationErrorCode::InvalidRequest
        );
    }

    #[test]
    fn detect_platform_matches_the_compiled_target() {
        let expected = match std::env::consts::OS {
            "macos" => PlatformKindDto::Macos,
            "windows" => PlatformKindDto::Windows,
            "linux" => PlatformKindDto::Linux,
            _ => PlatformKindDto::Unknown,
        };
        assert_eq!(detect_platform(), expected);
    }

    #[tokio::test]
    async fn create_list_open_and_delete_workspace_round_trip_through_dtos() {
        let (_dir, service) = service();

        let created = service
            .create_workspace(Some("Photos".to_owned()))
            .await
            .expect("create must succeed");
        assert_eq!(created.name, "Photos");

        let summaries = service.list_workspaces().await.expect("list must succeed");
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].id, created.id);

        let opened = service
            .open_workspace(created.id)
            .await
            .expect("open must succeed");
        assert_eq!(opened.id, created.id);

        service
            .delete_workspace(created.id, Some(created.revision))
            .await
            .expect("delete must succeed");
        assert!(service.list_workspaces().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn apply_workspace_command_reports_a_stale_revision_conflict() {
        let (_dir, service) = service();
        let created = service
            .create_workspace(None)
            .await
            .expect("create must succeed");

        let command = fm_transport_dto::WorkspaceCommandDto::RenameWorkspace {
            workspace_id: created.id,
            name: "Renamed".to_owned(),
            expected_revision: created.revision + 1,
        };

        let error = service
            .apply_workspace_command(command)
            .await
            .expect_err("a stale revision must be rejected");

        assert!(matches!(
            error,
            ApplicationError::WorkspaceRevisionConflict { .. }
        ));
    }
}
