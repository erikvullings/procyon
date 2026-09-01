import type { ApplicationUninstallCandidateDto } from '../api/generated/models/applicationUninstallCandidateDto';
import type { ArchiveSummaryResponseDto } from '../api/generated/models/archiveSummaryResponseDto';
import type { CalculateFolderSizeResponseDto } from '../api/generated/models/calculateFolderSizeResponseDto';
import type { ComparisonPageDto } from '../api/generated/models/comparisonPageDto';
import type { DiagnosticsDto } from '../api/generated/models/diagnosticsDto';
import type { DiscoverApplicationUninstallCandidatesResponseDto } from '../api/generated/models/discoverApplicationUninstallCandidatesResponseDto';
import type { DocxPreviewSessionRequestDto } from '../api/generated/models/docxPreviewSessionRequestDto';
import type { GetFileGitHistoryResponseDto } from '../api/generated/models/getFileGitHistoryResponseDto';
import type { GitLogEntryDto } from '../api/generated/models/gitLogEntryDto';
import type { JsonTokenSpanDto } from '../api/generated/models/jsonTokenSpanDto';
import type { LoadEditableFileResponseDto } from '../api/generated/models/loadEditableFileResponseDto';
import type { OpenDocxPreviewRequestDto } from '../api/generated/models/openDocxPreviewRequestDto';
import type { OpenDocxPreviewResponseDto } from '../api/generated/models/openDocxPreviewResponseDto';
import type { OpenPptxPreviewRequestDto } from '../api/generated/models/openPptxPreviewRequestDto';
import type { OpenPptxPreviewResponseDto } from '../api/generated/models/openPptxPreviewResponseDto';
import type { OpenStructuredViewRequestDto } from '../api/generated/models/openStructuredViewRequestDto';
import type { OpenStructuredViewResponseDto } from '../api/generated/models/openStructuredViewResponseDto';
import type { PptxPreviewSessionRequestDto } from '../api/generated/models/pptxPreviewSessionRequestDto';
import type { ReadDocxPreviewResourceRequestDto } from '../api/generated/models/readDocxPreviewResourceRequestDto';
import type { ReadDocxPreviewResourceResponseDto } from '../api/generated/models/readDocxPreviewResourceResponseDto';
import type { ReadFileRangeResponseDto } from '../api/generated/models/readFileRangeResponseDto';
import type { ReadPptxPreviewPdfRequestDto } from '../api/generated/models/readPptxPreviewPdfRequestDto';
import type { ReadStructuredJsonWindowRequestDto } from '../api/generated/models/readStructuredJsonWindowRequestDto';
import type { ReadStructuredJsonWindowResponseDto } from '../api/generated/models/readStructuredJsonWindowResponseDto';
import type { ReadStructuredRowsRequestDto } from '../api/generated/models/readStructuredRowsRequestDto';
import type { ReadStructuredRowsResponseDto } from '../api/generated/models/readStructuredRowsResponseDto';
import type { RemoveApplicationDockIconResponseDto } from '../api/generated/models/removeApplicationDockIconResponseDto';
import type { SaveEditableFileResponseDto } from '../api/generated/models/saveEditableFileResponseDto';
import type { SearchInFileMatchDto } from '../api/generated/models/searchInFileMatchDto';
import type { SearchInFileResponseDto } from '../api/generated/models/searchInFileResponseDto';
import type { SearchStructuredRowsRequestDto } from '../api/generated/models/searchStructuredRowsRequestDto';
import type { SearchStructuredRowsResponseDto } from '../api/generated/models/searchStructuredRowsResponseDto';
import type { SortDescriptorDto } from '../api/generated/models/sortDescriptorDto';
import type { StructuredRowDto } from '../api/generated/models/structuredRowDto';
import type { StructuredViewSessionRequestDto } from '../api/generated/models/structuredViewSessionRequestDto';
import type { StructuredViewStatusDto } from '../api/generated/models/structuredViewStatusDto';
import type { SyncPlanDto } from '../api/generated/models/syncPlanDto';
import type { UpdateStructuredViewRequestDto } from '../api/generated/models/updateStructuredViewRequestDto';
import type { ActionInvocationContext } from './action';
import type { ChecksumAlgorithm } from './checksum';
import {
  type ComparisonCriteria,
  type ComparisonEntry,
  comparisonEntryFromDto,
  type SyncMode,
  type SyncPlanItem,
  syncPlanItemFromDto,
} from './comparison';
import type { ActionId, EntryId, OperationId, PaneId } from './ids';
import type { Location } from './location';
import type { ConflictPolicy, OperationKind } from './operation';
import type { SearchExecutionMode, SearchProviderLimitation, SearchQuery } from './search';

/** Open column sort descriptor shared by workspace views and directory requests. */
export type SortDescriptor = SortDescriptorDto;

/**
 * Requests the entries of a directory (`POST /api/v1/directories/list`),
 * mirroring `fm_transport_dto::ListDirectoryRequest`.
 */
export interface ListDirectoryRequest {
  workspaceId: string;
  paneId: PaneId;
  requestId: string;
  location: Location;
  continuationToken?: string;
  sort?: SortDescriptor[];
  showHidden?: boolean;
  foldersFirst?: boolean;
  showGitStatus?: boolean;
}

/**
 * Requests navigation to a new location (`POST /api/v1/navigation/open`),
 * mirroring `fm_transport_dto::NavigateRequest`.
 */
export interface NavigateRequest {
  workspaceId: string;
  paneId: PaneId;
  requestId: string;
  location: Location;
  sort?: SortDescriptor[];
  showHidden?: boolean;
  foldersFirst?: boolean;
  showGitStatus?: boolean;
}

/**
 * Requests detailed metadata for a single entry
 * (`POST /api/v1/entries/metadata`), mirroring
 * `fm_transport_dto::EntryMetadataRequest`.
 */
export interface EntryMetadataRequest {
  entryId: EntryId;
  location: Location;
}

/**
 * Marks whether a pane is currently in the foreground, so a poll-tracked
 * directory watch (SFTP, FTP, ...) can poll less often while backgrounded
 * (`POST /api/v1/directories/activity`, task 0109), mirroring
 * `fm_transport_dto::SetPaneActivityRequest`.
 */
export interface SetPaneActivityRequest {
  paneId: PaneId;
  active: boolean;
}

/** Supplies an archive password to the backend-session-only credential cache. */
export interface ArchiveCredentialRequest {
  location: Location;
  password: string;
}

/**
 * Starts a mutating operation (spec §17). No backend DTO exists yet
 * (operations land in tasks 0037+); fields mirror the domain `Operation`
 * struct until then.
 */
export interface StartOperationRequest {
  type: OperationKind;
  sources: readonly Location[];
  destination?: Location;
  /**
   * Per-source destinations for a batch `rename` (task 0072 multi-rename), one entry per
   * `sources` item in the same order. Omitted for every other operation kind and for a
   * single-entry rename, which keeps using `destination` instead.
   */
  destinations?: readonly Location[];
  conflictPolicy: ConflictPolicy;
  name?: string;
  archiveFormat?: 'zip' | 'sevenZip';
  archiveCompressionLevel?: number | undefined;
  createIntermediateDirectories?: boolean;
  symlinkPolicy?: 'copyLink' | 'copyTarget';
  permanentDeleteConfirmed?: boolean;
  overrideReadOnly?: boolean;
}

/** Submits the user's decision for a queued conflict (spec §17). */
export type ConflictResolution = 'confirm' | 'skip' | 'overwrite' | 'renameNew' | 'cancelOperation';

export interface ResolveConflictRequest {
  operationId: OperationId;
  resolution: ConflictResolution;
  applyToAllSimilar: boolean;
}

/** Invokes a registered action (spec §18). */
export interface InvokeActionRequest {
  actionId: ActionId;
  parameters?: unknown;
  context: ActionInvocationContext;
}

/**
 * Starts a recursive, cancellable search (filename and/or content,
 * `POST /api/v1/search`, task 0068/0089), mirroring `fm_transport_dto::StartSearchRequestDto`.
 */
export interface StartSearchRequest {
  query: string;
  contentQuery?: string | undefined;
  contentRegex?: boolean;
  contentCaseSensitive?: boolean;
  contentWholeWord?: boolean;
  recurse?: boolean;
  showHidden?: boolean;
  roots: readonly Location[];
  workspaceId: string;
  structuredQuery?: SearchQuery;
}

/**
 * Identifies a started search and its `search://local/{searchId}` virtual
 * result location, mirroring `fm_transport_dto::StartSearchResponseDto`.
 */
export interface StartSearchResult {
  searchId: string;
  location: Location;
  limitations: readonly SearchProviderLimitation[];
  executionMode: SearchExecutionMode;
}

/**
 * Requests a byte range from a single file (`POST /api/v1/files/range`,
 * task 0088), mirroring `fm_transport_dto::ReadFileRangeRequestDto`.
 */
export interface ReadFileRangeRequest {
  location: Location;
  offset: number;
  length: number;
}

/**
 * One chunk of a file's content, mirroring
 * `fm_transport_dto::ReadFileRangeResponseDto`. Field shapes match the wire
 * DTO exactly, so no separate mapper is needed.
 */
export type FileRangeChunk = ReadFileRangeResponseDto;

/** Provider-neutral bounded semantic DOCX preview session contracts (task 0171). */
export type OpenDocxPreviewRequest = OpenDocxPreviewRequestDto;
export type DocxPreview = OpenDocxPreviewResponseDto;
export type DocxPreviewResourceDescriptor = OpenDocxPreviewResponseDto['resources'][number];
export type DocxPreviewSessionRequest = DocxPreviewSessionRequestDto;
export type ReadDocxPreviewResourceRequest = ReadDocxPreviewResourceRequestDto;
export type DocxPreviewResource = ReadDocxPreviewResourceResponseDto;

/** Provider-neutral bounded rendered PowerPoint preview session contracts (task 0173). */
export type OpenPptxPreviewRequest = OpenPptxPreviewRequestDto;
export type PptxPreview = OpenPptxPreviewResponseDto;
export type PptxPreviewSessionRequest = PptxPreviewSessionRequestDto;
export type ReadPptxPreviewPdfRequest = ReadPptxPreviewPdfRequestDto;

export interface LoadEditableFileRequest {
  location: Location;
}
export type EditableFile = LoadEditableFileResponseDto;
export interface SaveEditableFileRequest {
  location: Location;
  destination?: Location;
  content: string;
  expectedRevision: string;
  overwriteConflict: boolean;
}
export type EditableFileSave = SaveEditableFileResponseDto;

/**
 * Searches a single file's content for a substring or regex
 * (`POST /api/v1/files/search`, task 0088), mirroring
 * `fm_transport_dto::SearchInFileRequestDto`.
 */
export interface SearchInFileRequest {
  location: Location;
  query: string;
  regex: boolean;
  caseSensitive: boolean;
  wholeWord: boolean;
}

/** One match found by a {@link SearchInFileRequest}. */
export type SearchInFileMatch = SearchInFileMatchDto;

/** The result of a {@link SearchInFileRequest}. */
export type SearchInFileResult = SearchInFileResponseDto;

/** Provider-neutral read-only structured-data viewer session contracts (task 0100). */
export type OpenStructuredViewRequest = OpenStructuredViewRequestDto;
export type StructuredView = OpenStructuredViewResponseDto;
export type StructuredViewSessionRequest = StructuredViewSessionRequestDto;
export type StructuredViewStatus = StructuredViewStatusDto;
export type UpdateStructuredViewRequest = UpdateStructuredViewRequestDto;
export type ReadStructuredRowsRequest = ReadStructuredRowsRequestDto;
export type StructuredRows = ReadStructuredRowsResponseDto;
export type ReadStructuredJsonWindowRequest = ReadStructuredJsonWindowRequestDto;
export type StructuredJsonWindow = ReadStructuredJsonWindowResponseDto;
export type SearchStructuredRowsRequest = SearchStructuredRowsRequestDto;
export type StructuredRowSearch = SearchStructuredRowsResponseDto;
export type StructuredRow = StructuredRowDto;
export type JsonTokenSpan = JsonTokenSpanDto;

/**
 * Requests a directory's recursive total size (`POST /api/v1/directories/size`, task 0071's
 * Total Commander-style "press a key on a folder to see how much space it consumes" behaviour),
 * mirroring `fm_transport_dto::CalculateFolderSizeRequestDto`.
 */
export interface CalculateFolderSizeRequest {
  location: Location;
}

/** The result of a {@link CalculateFolderSizeRequest}. */
export type CalculateFolderSizeResult = CalculateFolderSizeResponseDto;

/** Requests a recursively computed summary for a local archive file. */
export interface ArchiveSummaryRequest {
  location: Location;
}

/** Content-derived format and recursive archive entry totals. */
export type ArchiveSummaryResult = ArchiveSummaryResponseDto;

/**
 * Requests discovery of a `.app` bundle's related files across the well-known macOS locations
 * (`POST /api/v1/applications/uninstall/discover`, task 0148's uninstall review checklist),
 * mirroring `fm_transport_dto::DiscoverApplicationUninstallCandidatesRequestDto`. Read-only:
 * nothing is deleted by this call.
 */
export interface DiscoverApplicationUninstallCandidatesRequest {
  location: Location;
}

/** The result of a {@link DiscoverApplicationUninstallCandidatesRequest}. */
export type DiscoverApplicationUninstallCandidatesResult =
  DiscoverApplicationUninstallCandidatesResponseDto;

/** One file or folder discovered under a well-known macOS location that appears to belong to the
 * application being uninstalled (task 0148's uninstall review checklist). */
export type ApplicationUninstallCandidate = ApplicationUninstallCandidateDto;

/**
 * Requests removal of a `.app` bundle's pinned Dock icon, if it has one
 * (`POST /api/v1/applications/uninstall/remove-dock-icon`, task 0148 follow-up), mirroring
 * `fm_transport_dto::RemoveApplicationDockIconRequestDto`. Called once the user confirms an
 * uninstall, alongside (not instead of) moving the bundle to the Trash.
 */
export interface RemoveApplicationDockIconRequest {
  location: Location;
}

/** The result of a {@link RemoveApplicationDockIconRequest}: whether a pinned icon was found and
 * removed. `false` is a normal outcome, not a failure - it just means there was none to remove. */
export type RemoveApplicationDockIconResult = RemoveApplicationDockIconResponseDto;

/**
 * The result of `getDiagnostics()` (`GET /api/v1/diagnostics`, spec §30), mirroring
 * `fm_transport_dto::DiagnosticsDto`. `diagnosticsFromDto` (features/diagnostics/diagnostics.ts)
 * still does its own defensive parsing on top of this, since the DTO is also the input to that
 * conversion from untyped JSON in older call sites.
 */
export type DiagnosticsResult = DiagnosticsDto;

/**
 * Requests a file's git commit history (`POST /api/v1/files/git-history`), for the Alt+Space
 * metadata panel's history section (task 0135), mirroring
 * `fm_transport_dto::GetFileGitHistoryRequestDto`.
 */
export interface GitFileHistoryRequest {
  location: Location;
}

/** One commit touching a file, newest first. */
export type GitLogEntry = GitLogEntryDto;

/** The result of a {@link GitFileHistoryRequest}: empty when the file has no history to show. */
export type GitFileHistoryResult = GetFileGitHistoryResponseDto;

/**
 * Starts a recursive, cancellable directory comparison
 * (`POST /api/v1/comparisons`, task 0075), mirroring
 * `fm_transport_dto::StartComparisonRequestDto`.
 */
export interface StartComparisonRequest {
  workspaceId: string;
  left: Location;
  right: Location;
  criteria: ComparisonCriteria;
  showHidden?: boolean;
}

/** Identifies a started comparison, mirroring `fm_transport_dto::StartComparisonResponseDto`. */
export interface StartComparisonResult {
  comparisonId: string;
}

/**
 * A bounded, optionally differences-only page of a comparison's results
 * (`GET /api/v1/comparisons/{comparisonId}`), mirroring
 * `fm_transport_dto::ComparisonPageDto`.
 */
export interface ComparisonPage {
  comparisonId: string;
  left: Location;
  right: Location;
  criteria: ComparisonCriteria;
  offset: number;
  limit: number;
  total: number;
  entries: ComparisonEntry[];
  isComplete: boolean;
  warningsCount: number;
}

/** Converts the wire DTO into the frontend model. */
export function comparisonPageFromDto(dto: ComparisonPageDto): ComparisonPage {
  return {
    comparisonId: dto.comparisonId,
    left: dto.left,
    right: dto.right,
    criteria: dto.criteria,
    offset: dto.offset,
    limit: dto.limit,
    total: dto.total,
    entries: dto.entries.map(comparisonEntryFromDto),
    isComplete: dto.isComplete,
    warningsCount: dto.warningsCount,
  };
}

/**
 * Starts a cancellable checksum job over a selection
 * (`POST /api/v1/checksums`, task 0077), mirroring
 * `fm_transport_dto::StartChecksumRequestDto`.
 */
export interface StartChecksumRequest {
  workspaceId: string;
  entries: Location[];
  algorithms: ChecksumAlgorithm[];
}

/** Identifies a started checksum job. */
export interface StartChecksumResult {
  jobId: string;
}

/**
 * Writes a job's results to a checksum file on disk
 * (`POST /api/v1/checksums/{jobId}/save`, task 0077). Saving goes through the
 * backend's provider `WRITE` path rather than a host-native save dialog, so
 * both hosts create files by the same audited route (spec §35).
 */
export interface SaveChecksumFileRequest {
  destination: Location;
  algorithm: ChecksumAlgorithm;
  overwrite?: boolean;
}

/**
 * Starts a cancellable duplicate scan across one or more roots
 * (`POST /api/v1/duplicate-scans`, task 0077).
 */
export interface StartDuplicateScanRequest {
  workspaceId: string;
  roots: Location[];
  showHidden?: boolean;
  includeEmptyFiles?: boolean;
}

/** Identifies a started duplicate scan. */
export interface StartDuplicateScanResult {
  scanId: string;
}

/**
 * Proposes a sync plan from a comparison's current results
 * (`POST /api/v1/comparisons/{comparisonId}/sync-plan`, task 0075).
 */
export interface GenerateSyncPlanRequest {
  mode: SyncMode;
}

/** A proposed sync plan, mirroring `fm_transport_dto::SyncPlanDto`. */
export interface SyncPlan {
  comparisonId: string;
  items: SyncPlanItem[];
}

/** Converts the wire DTO into the frontend model. */
export function syncPlanFromDto(dto: SyncPlanDto): SyncPlan {
  return {
    comparisonId: dto.comparisonId,
    items: dto.items.map(syncPlanItemFromDto),
  };
}

/**
 * Applies a (possibly user-edited) sync plan
 * (`POST /api/v1/comparisons/{comparisonId}/apply-sync-plan`, task 0075).
 */
export interface ApplySyncPlanRequest {
  items: readonly SyncPlanItem[];
}

/** The operations started by applying a sync plan. */
export interface ApplySyncPlanResult {
  operationIds: readonly OperationId[];
}
