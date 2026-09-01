//! Wire types for both hosts (task 0007).
//!
//! DTOs are converted explicitly to and from `fm-domain` types; they are never
//! reused as internal domain models (specification §3 rule 5). Keeping them in
//! one crate is what lets the Tauri commands and the REST endpoints stay
//! byte-for-byte compatible.

pub mod action;
pub mod application_uninstall;
pub mod checksum;
pub mod comparison;
pub mod connection;
pub mod diagnostics;
pub mod disk_usage;
pub mod entry;
pub mod error;
pub mod files;
pub mod finder_tags;
pub mod health;
pub mod location;
pub mod operation;
pub mod plugin;
pub mod redaction;
pub mod requests;
pub mod runtime;
pub mod search;
pub mod settings;
pub mod snapshot;
pub mod system_location;
pub mod workspace;
pub mod workspace_command;

pub use action::{
    ActionContextRequirementsDto, ActionDescriptorDto, ActionInvocationContextDto, ActionResultDto,
    ActionSourceDto, InvokeActionRequestDto, KeyChordDto,
};
pub use application_uninstall::{
    ApplicationUninstallCandidateDto, DiscoverApplicationUninstallCandidatesRequestDto,
    DiscoverApplicationUninstallCandidatesResponseDto, RemoveApplicationDockIconRequestDto,
    RemoveApplicationDockIconResponseDto,
};
pub use checksum::{
    ChecksumAlgorithmDto, ChecksumEntryDto, ChecksumFileDto, ChecksumPageDto, DuplicateGroupDto,
    DuplicatePageDto, DuplicateStatsDto, HardlinkClusterDto, RenderChecksumFileRequestDto,
    SaveChecksumFileRequestDto, SaveChecksumFileResponseDto, StartChecksumRequestDto,
    StartChecksumResponseDto, StartDuplicateScanRequestDto, StartDuplicateScanResponseDto,
    VerificationReportDto, VerificationResultDto, VerificationStatusDto,
    VerifyChecksumFileRequestDto,
};
pub use comparison::{
    ApplySyncPlanRequestDto, ApplySyncPlanResponseDto, ComparisonCriteriaDto, ComparisonEntryDto,
    ComparisonEntrySideDto, ComparisonPageDto, ComparisonStatusDto, GenerateSyncPlanRequestDto,
    StartComparisonRequestDto, StartComparisonResponseDto, SyncActionDto, SyncModeDto, SyncPlanDto,
    SyncPlanItemDto,
};
pub use connection::{
    AcceptSshHostKeyRequestDto, BeginOneDriveAuthorizationResponseDto, ConnectionConfigurationDto,
    ConnectionDto, ConnectionKindDto, ConnectionSecretInputDto, ConnectionStatusDto,
    CreateConnectionRequestDto, FtpConnectionConfigurationDto, HostKeyPolicyDto, HostKeyProbeDto,
    OneDriveAuthorizationAttemptDto, OneDriveAuthorizationErrorCodeDto,
    OneDriveAuthorizationStatusDto, OneDriveConnectionConfigurationDto, OneDriveDriveTypeDto,
    S3ConnectionConfigurationDto, SmbConnectionConfigurationDto, SshAuthenticationMethodDto,
    SshConnectionConfigurationDto, UpdateConnectionRequestDto, WebDavAuthenticationSchemeDto,
    WebDavConnectionConfigurationDto,
};
pub use diagnostics::{
    ConnectionStateDto, DiagnosticErrorDto, DiagnosticsDto, OperationQueueStatusDto,
    PluginStatusDto,
};
pub use disk_usage::{
    DiskUsageNodeDto, DiskUsageNodeKindDto, DiskUsageUnreadableEntryDto,
    DiskUsageUnreadableReasonDto, ScanDiskUsageRequestDto, ScanDiskUsageResponseDto,
};
pub use entry::{
    ArchiveInfoDto, EntryKindDto, EntryMetadataDto, EntrySummaryDto, ImageDimensionsDto,
    MediaMetadataDto, OwnershipInfoDto, PermissionsInfoDto,
};
pub use error::{ApplicationErrorCode, ApplicationErrorDto};
pub use files::{
    ArchiveCredentialRequestDto, ArchiveSummaryRequestDto, ArchiveSummaryResponseDto,
    CalculateFolderSizeRequestDto, CalculateFolderSizeResponseDto, DocxPreviewResourceDto,
    DocxPreviewSessionRequestDto, GetFileGitHistoryRequestDto, GetFileGitHistoryResponseDto,
    GitLogEntryDto, JsonTokenKindDto, JsonTokenSpanDto, LoadEditableFileRequestDto,
    LoadEditableFileResponseDto, OpenDocxPreviewRequestDto, OpenDocxPreviewResponseDto,
    OpenPptxPreviewRequestDto, OpenPptxPreviewResponseDto, OpenStructuredViewRequestDto,
    OpenStructuredViewResponseDto, PptxPreviewSessionRequestDto, ReadDocxPreviewResourceRequestDto,
    ReadDocxPreviewResourceResponseDto, ReadFileRangeRequestDto, ReadFileRangeResponseDto,
    ReadPptxPreviewPdfRequestDto, ReadStructuredJsonWindowRequestDto,
    ReadStructuredJsonWindowResponseDto, ReadStructuredRowsRequestDto,
    ReadStructuredRowsResponseDto, SaveEditableFileRequestDto, SaveEditableFileResponseDto,
    SearchInFileMatchDto, SearchInFileRequestDto, SearchInFileResponseDto,
    SearchStructuredRowsRequestDto, SearchStructuredRowsResponseDto, StructuredCellDto,
    StructuredCellValueTypeDto, StructuredHeaderModeDto, StructuredRowDto, StructuredSheetDto,
    StructuredViewFormatDto, StructuredViewKindDto, StructuredViewSessionRequestDto,
    StructuredViewStatusDto, UpdateStructuredViewRequestDto,
};
pub use finder_tags::{FinderTagColorDto, FinderTagDto, FinderTagsDto, SpotlightCommentDto};
pub use health::{HealthDto, HealthStatusDto};
pub use location::LocationDto;
pub use operation::{
    ArchiveFormatDto, ConflictResolutionDto, EntryRefDto, OperationConflictPolicyDto, OperationDto,
    OperationEntryErrorDto, OperationKindDto, OperationPageDto, OperationProgressDto,
    OperationStateDto, OperationUndoDto, ResolveOperationConflictRequestDto,
    StartOperationRequestDto, SymlinkPolicyDto,
};
pub use plugin::{
    PluginColumnDto, PluginDescriptorDto, PluginIconDefinitionDto, PluginIconThemeDto,
    PluginLogEntryDto, PluginPermissionsDto,
};
pub use redaction::{redact, redact_absolute_paths, redact_path};
pub use requests::{
    EntryMetadataRequest, ListDirectoryChildrenRequest, ListDirectoryRequest, NavigateRequest,
    SetPaneActivityRequest,
};
pub use runtime::{PlatformKindDto, RuntimeCapabilitiesDto, RuntimeKindDto};
pub use search::{
    SearchContentPredicateDto, SearchEntryKindDto, SearchExecutionModeDto, SearchGitStatusDto,
    SearchNameModeDto, SearchNamePredicateDto, SearchPredicateKindDto, SearchProviderLimitationDto,
    SearchQueryDto, SearchScopeDto, StartSearchRequestDto, StartSearchResponseDto,
};
pub use settings::{
    ConflictPolicyDto, DateFormatDto, DefaultPaneLayoutDto, FavouriteLocationDto, LanguageDto,
    MultiRenameCaseTransformDto, MultiRenamePresetDto, MultiRenameRulesDto, MultiRenameSequenceDto,
    SavedSearchDto, SettingsDto, SizeFormatDto, ThemeDto,
};
pub use snapshot::{DirectorySnapshotDto, LoadingStateDto, VolumeCapacityDto};
pub use system_location::{SystemLocationDto, SystemLocationKindDto, VolumeDto};
pub use workspace::{
    ColumnConfigurationDto, CreateWorkspaceRequestDto, DirectoryViewConfigurationDto,
    DirectoryViewModeDto, IconSizeDto, NavigationHistoryDto, OperationCentrePreferencesDto,
    PaneStateDto, PersistedFilterDto, SortDescriptorDto, SortDirectionDto, SplitAxisDto,
    TabStateDto, WorkspaceDto, WorkspaceLayoutDto, WorkspaceSummaryDto,
};
pub use workspace_command::{
    DirectoryViewPatchDto, NavigationModeDto, QuickFilterPatchDto, WorkspaceCommandDto,
};
