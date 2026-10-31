import type {
  ApplySyncPlanRequest,
  ApplySyncPlanResult,
  ChecksumAlgorithm,
  ChecksumFile,
  ChecksumPage,
  ComparisonPage,
  DuplicatePage,
  EntrySummary,
  FileDiffOptions,
  FileDiffResult,
  FindLinkReplacementsRequest,
  FindLinkReplacementsResult,
  GenerateSyncPlanRequest,
  LinkDoctorPage,
  LinkOptions,
  LinkOptionsRequest,
  Location,
  MusicDuplicatePage,
  Operation,
  OperationId,
  ResolveConflictRequest,
  SaveChecksumFileRequest,
  SavedChecksumFile,
  SnapshotJobPage,
  StartChecksumRequest,
  StartChecksumResult,
  StartComparisonRequest,
  StartComparisonResult,
  StartDuplicateScanRequest,
  StartDuplicateScanResult,
  StartLinkDoctorScanRequest,
  StartLinkDoctorScanResult,
  StartMusicDuplicateScanRequest,
  StartMusicDuplicateScanResult,
  StartOperationRequest,
  StartSearchRequest,
  StartSearchResult,
  StartSnapshotJobRequest,
  StartSnapshotJobResult,
  SyncPlan,
  VerificationReport,
} from '../../models';

/**
 * The operation engine: jobs, search, comparison, sync, checksums and links.
 */
export interface OperationClient {
  /** Applies a (possibly user-edited) sync plan through the operation engine. */
  applySyncPlan(
    comparisonId: string,
    request: ApplySyncPlanRequest,
    signal?: AbortSignal,
  ): Promise<ApplySyncPlanResult>;

  cancelChecksums(jobId: string, signal?: AbortSignal): Promise<void>;

  cancelComparison(comparisonId: string, signal?: AbortSignal): Promise<void>;

  cancelDuplicateScan(scanId: string, signal?: AbortSignal): Promise<void>;

  cancelLinkDoctorScan(scanId: string, signal?: AbortSignal): Promise<void>;

  cancelMusicDuplicateScan(scanId: string, signal?: AbortSignal): Promise<void>;

  cancelOperation(operationId: OperationId, signal?: AbortSignal): Promise<void>;

  cancelSearch(searchId: string, signal?: AbortSignal): Promise<void>;

  cancelSnapshotJob(jobId: string, signal?: AbortSignal): Promise<void>;

  /**
   * Diffs two files read-only: aligned rows with intra-line highlights for text, the first
   * differing offset for binary files, and a size notice for files over the limit (task 0256).
   */
  diffFiles(
    left: Location,
    right: Location,
    options?: FileDiffOptions,
    signal?: AbortSignal,
  ): Promise<FileDiffResult>;

  /** Searches for files that could replace a missing link target. */
  findLinkReplacements(
    request: FindLinkReplacementsRequest,
    signal?: AbortSignal,
  ): Promise<FindLinkReplacementsResult>;

  /**
   * Forgets the remembered view (sort, columns, mode) of a folder so it opens with the defaults
   * again (task 0270). Idempotent.
   */
  forgetFolderView(location: Location, signal?: AbortSignal): Promise<void>;

  /** Proposes a sync plan from a comparison's current results; never mutates anything. */
  generateSyncPlan(
    comparisonId: string,
    request: GenerateSyncPlanRequest,
    signal?: AbortSignal,
  ): Promise<SyncPlan>;

  /** Pages through a checksum job's streamed results. */
  getChecksums(
    jobId: string,
    options?: { offset?: number; limit?: number },
    signal?: AbortSignal,
  ): Promise<ChecksumPage>;

  /** Pages through a comparison's streamed results, optionally differences-only. */
  getComparison(
    comparisonId: string,
    options?: { offset?: number; limit?: number; differencesOnly?: boolean },
    signal?: AbortSignal,
  ): Promise<ComparisonPage>;

  /** Pages through a duplicate scan's grouped results. */
  getDuplicateScan(
    scanId: string,
    options?: { offset?: number; limit?: number },
    signal?: AbortSignal,
  ): Promise<DuplicatePage>;

  /** Pages through a broken-link scan's problems and reports its progress. */
  getLinkDoctorScan(
    scanId: string,
    options?: { offset?: number; limit?: number },
    signal?: AbortSignal,
  ): Promise<LinkDoctorPage>;

  /** Lists the link kinds that can point at a target from a destination directory (task 0168);
   * an empty list means the pair has no link semantics (remote providers, mixed providers). */
  getLinkOptions(request: LinkOptionsRequest, signal?: AbortSignal): Promise<LinkOptions>;

  /** Pages through a music duplicate scan's groups and reports its progress. */
  getMusicDuplicateScan(
    scanId: string,
    options?: { offset?: number; limit?: number },
    signal?: AbortSignal,
  ): Promise<MusicDuplicatePage>;

  /** Pages through a snapshot job's differences and reports its progress. */
  getSnapshotJob(
    jobId: string,
    options?: { offset?: number; limit?: number },
    signal?: AbortSignal,
  ): Promise<SnapshotJobPage>;

  listOperations(signal?: AbortSignal): Promise<Operation[]>;

  pauseOperation(operationId: OperationId, signal?: AbortSignal): Promise<void>;

  /** Renders a job's results as coreutils-compatible checksum-file text. */
  renderChecksumFile(
    jobId: string,
    algorithm: ChecksumAlgorithm,
    signal?: AbortSignal,
  ): Promise<ChecksumFile>;

  resolveConflict(request: ResolveConflictRequest, signal?: AbortSignal): Promise<void>;

  /** Resolves a symbolic link to the entry it points at; other entries resolve to themselves. */
  resolveLinkTarget(location: Location, signal?: AbortSignal): Promise<EntrySummary>;

  resumeOperation(operationId: OperationId, signal?: AbortSignal): Promise<void>;

  /**
   * Writes a job's results to a checksum file at `request.destination`.
   *
   * Server-side by design: both hosts create files through the provider's
   * audited `WRITE` path rather than a host-specific save dialog (spec §35).
   */
  saveChecksumFile(
    jobId: string,
    request: SaveChecksumFileRequest,
    signal?: AbortSignal,
  ): Promise<SavedChecksumFile>;

  /** Starts a cancellable checksum job over a selection (spec §18, task 0077). */
  startChecksums(request: StartChecksumRequest, signal?: AbortSignal): Promise<StartChecksumResult>;

  /** Starts a cancellable directory comparison (spec §16 milestone 5, task 0075). */
  startComparison(
    request: StartComparisonRequest,
    signal?: AbortSignal,
  ): Promise<StartComparisonResult>;

  /** Starts a cancellable duplicate scan across one or more roots. */
  startDuplicateScan(
    request: StartDuplicateScanRequest,
    signal?: AbortSignal,
  ): Promise<StartDuplicateScanResult>;

  /** Starts a cancellable broken-link scan (task 0266). */
  startLinkDoctorScan(
    request: StartLinkDoctorScanRequest,
    signal?: AbortSignal,
  ): Promise<StartLinkDoctorScanResult>;

  /** Starts a cancellable duplicate and near-duplicate music scan (task 0265). */
  startMusicDuplicateScan(
    request: StartMusicDuplicateScanRequest,
    signal?: AbortSignal,
  ): Promise<StartMusicDuplicateScanResult>;

  startOperation(request: StartOperationRequest, signal?: AbortSignal): Promise<Operation>;

  startSearch(request: StartSearchRequest, signal?: AbortSignal): Promise<StartSearchResult>;

  /** Starts a cancellable folder snapshot create or compare job (task 0268). */
  startSnapshotJob(
    request: StartSnapshotJobRequest,
    signal?: AbortSignal,
  ): Promise<StartSnapshotJobResult>;

  undoOperation(operationId: OperationId, signal?: AbortSignal): Promise<Operation>;

  /** Verifies a job's digests against an existing checksum file's text. */
  verifyChecksumFile(
    jobId: string,
    content: string,
    signal?: AbortSignal,
  ): Promise<VerificationReport>;
}
