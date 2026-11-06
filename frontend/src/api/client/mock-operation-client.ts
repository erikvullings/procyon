import type {
  ApplySyncPlanRequest,
  ApplySyncPlanResult,
  ChecksumAlgorithm,
  ChecksumEntry,
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
  InterruptedTransfers,
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
  TransferConflicts,
  VerificationReport,
  VerificationResult,
} from '../../models';
import type { MockContext } from './mock-context';
import { mockFileDiff } from './mock-file-diff';
import {
  buildMockComparisonEntries,
  buildMockDuplicateGroups,
  collectMatches,
  defaultSyncAction,
  directories,
  fixtureEntry,
  lastSegment,
  MockClientError,
  mockDigest,
} from './mock-shared';
import type { OperationClient } from './operation-client';
import { admitsPath } from './sync-path-filter';

export class MockOperationClient implements OperationClient {
  constructor(private readonly ctx: MockContext) {}

  diffFiles(
    left: Location,
    right: Location,
    options?: FileDiffOptions,
    signal?: AbortSignal,
  ): Promise<FileDiffResult> {
    return this.ctx.perform('diffFiles', signal, () =>
      mockFileDiff(this.ctx.fileContentFor(left.uri), this.ctx.fileContentFor(right.uri), options),
    );
  }

  startOperation(request: StartOperationRequest, signal?: AbortSignal): Promise<Operation> {
    return this.ctx.perform('startOperation', signal, () => {
      this.ctx.operationSequence += 1;
      const operation: Operation = {
        id: `mock-operation-${this.ctx.seed}-${this.ctx.operationSequence}`,
        kind: request.type,
        state: 'running',
        sources: request.sources.map((location) => ({ id: location.uri, location })),
        ...(request.destination === undefined ? {} : { destination: request.destination }),
        progress: { completedItems: 0, completedBytes: 0 },
        conflictPolicy: request.conflictPolicy,
        createdAt: '2026-01-01T00:00:00.000Z',
        startedAt: '2026-01-01T00:00:00.000Z',
      };
      this.ctx.operations.set(operation.id, operation);
      return operation;
    });
  }

  resolveLinkTarget(location: Location, signal?: AbortSignal): Promise<EntrySummary> {
    return this.ctx.perform('resolveLinkTarget', signal, () => {
      const separator = location.uri.lastIndexOf('/');
      const rawParent = location.uri.slice(0, separator);
      const parentUri = rawParent === 'mock://' ? 'mock:///' : rawParent;
      const name = decodeURIComponent(location.uri.slice(separator + 1));
      const fixture = (directories[parentUri] ?? []).find((candidate) => candidate.name === name);
      if (fixture === undefined) {
        throw new MockClientError('notFound', `no mock entry at ${location.uri}`);
      }
      // Mock links have no stored target; they resolve to a file with the link's own name.
      return fixtureEntry(
        parentUri,
        fixture.kind === 'symlink' ? { ...fixture, kind: 'file' } : fixture,
      );
    });
  }

  findTransferConflicts(
    sources: readonly Location[],
    destination: Location,
    signal?: AbortSignal,
  ): Promise<TransferConflicts> {
    return this.ctx.perform('findTransferConflicts', signal, () => {
      const directoryUri =
        destination.uri === 'mock:///' ? destination.uri : destination.uri.replace(/\/+$/, '');
      const existing = new Set((directories[directoryUri] ?? []).map((fixture) => fixture.name));
      const names = sources
        .map((source) => decodeURIComponent(source.uri.split('/').filter(Boolean).at(-1) ?? ''))
        .filter((name) => name !== '' && existing.has(name));
      return { names, complete: true };
    });
  }

  getLinkOptions(request: LinkOptionsRequest, signal?: AbortSignal): Promise<LinkOptions> {
    return this.ctx.perform('getLinkOptions', signal, () => {
      const sameProvider = request.target.providerId === request.destination.providerId;
      const name = request.target.uri.split('/').filter(Boolean).at(-1) ?? 'link';
      return {
        kinds: sameProvider
          ? [
              {
                kind: 'symbolicLink',
                supportsRelative: true,
                requirements: [],
                suggestedName: decodeURIComponent(name),
              },
            ]
          : [],
      };
    });
  }

  listOperations(signal?: AbortSignal): Promise<Operation[]> {
    return this.ctx.perform('listOperations', signal, () =>
      [...this.ctx.operations.values()].map((operation) => structuredClone(operation)),
    );
  }

  listInterruptedTransfers(signal?: AbortSignal): Promise<InterruptedTransfers> {
    return this.ctx.perform('listInterruptedTransfers', signal, () => ({
      transfers: [...this.ctx.interruptedTransfers.values()].map((transfer) =>
        structuredClone(transfer),
      ),
    }));
  }

  restartInterruptedTransfer(operationId: OperationId, signal?: AbortSignal): Promise<Operation> {
    return this.ctx.perform('restartInterruptedTransfer', signal, () => {
      const transfer = this.ctx.interruptedTransfers.get(operationId);
      if (transfer === undefined) {
        throw new MockClientError('notFound', `No interrupted transfer ${operationId}`);
      }
      this.ctx.interruptedTransfers.delete(operationId);
      this.ctx.operationSequence += 1;
      const restarted: Operation = {
        id: `mock-operation-${this.ctx.seed}-${this.ctx.operationSequence}`,
        kind: transfer.kind,
        state: 'queued',
        sources: transfer.sources.map((location: Location) => ({ id: location.uri, location })),
        ...(transfer.destination === undefined || transfer.destination === null
          ? {}
          : { destination: transfer.destination }),
        progress: { completedItems: 0, completedBytes: 0 },
        conflictPolicy: transfer.committedItems + transfer.unconfirmedItems > 0 ? 'skip' : 'ask',
        createdAt: new Date().toISOString(),
        errors: [],
      };
      this.ctx.operations.set(restarted.id, restarted);
      return structuredClone(restarted);
    });
  }

  discardInterruptedTransfer(operationId: OperationId, signal?: AbortSignal): Promise<void> {
    return this.ctx.perform('discardInterruptedTransfer', signal, () => {
      if (!this.ctx.interruptedTransfers.delete(operationId)) {
        throw new MockClientError('notFound', `No interrupted transfer ${operationId}`);
      }
    });
  }

  cancelOperation(operationId: OperationId, signal?: AbortSignal): Promise<void> {
    return this.ctx.perform('cancelOperation', signal, () => {
      const operation = this.ctx.requireOperation(operationId);
      this.ctx.operations.set(operationId, { ...operation, state: 'cancelled' });
    });
  }

  undoOperation(operationId: OperationId, signal?: AbortSignal): Promise<Operation> {
    return this.ctx.perform('undoOperation', signal, () => {
      const original = this.ctx.requireOperation(operationId);
      if (original.undo?.available !== true) {
        throw new MockClientError(
          original.undo?.reasonCode ?? 'undoUnsafe',
          original.undo?.reason ?? 'This operation cannot be undone.',
          { reason: original.undo?.reasonCode ?? 'undoUnsafe' },
        );
      }
      this.ctx.operationSequence += 1;
      const undo: Operation = {
        id: `mock-operation-${this.ctx.seed}-${this.ctx.operationSequence}`,
        kind: 'undo',
        state: 'running',
        sources: original.sources,
        progress: { completedItems: 0, completedBytes: 0 },
        conflictPolicy: 'ask',
        createdAt: '2026-01-01T00:00:00.000Z',
        startedAt: '2026-01-01T00:00:00.000Z',
        undo: {
          available: false,
          reason: 'Undo operations cannot themselves be undone.',
          reasonCode: 'undoOfUndo',
        },
        undoOf: operationId,
      };
      this.ctx.operations.set(operationId, {
        ...original,
        undo: {
          available: false,
          reason: 'Undo is already in progress for this operation.',
          reasonCode: 'undoInProgress',
          operationId: undo.id,
        },
      });
      this.ctx.operations.set(undo.id, undo);
      return undo;
    });
  }

  pauseOperation(operationId: OperationId, signal?: AbortSignal): Promise<void> {
    return this.ctx.perform('pauseOperation', signal, () => {
      const operation = this.ctx.requireOperation(operationId);
      this.ctx.operations.set(operationId, { ...operation, state: 'paused' });
    });
  }

  resumeOperation(operationId: OperationId, signal?: AbortSignal): Promise<void> {
    return this.ctx.perform('resumeOperation', signal, () => {
      const operation = this.ctx.requireOperation(operationId);
      this.ctx.operations.set(operationId, { ...operation, state: 'running' });
    });
  }

  resolveConflict(request: ResolveConflictRequest, signal?: AbortSignal): Promise<void> {
    return this.ctx.perform('resolveConflict', signal, () => {
      const operation = this.ctx.requireOperation(request.operationId);
      this.ctx.operations.set(request.operationId, {
        ...operation,
        ...(request.resolution === 'cancelOperation'
          ? { state: 'cancelled' as const }
          : request.resolution === 'confirm'
            ? { state: 'running' as const }
            : { conflictPolicy: request.resolution, state: 'running' as const }),
      });
    });
  }

  startSearch(request: StartSearchRequest, signal?: AbortSignal): Promise<StartSearchResult> {
    return this.ctx.perform('startSearch', signal, () => {
      this.ctx.searchSequence += 1;
      const searchId = `mock-search-${this.ctx.seed}-${this.ctx.searchSequence}`;
      const location: Location = { providerId: 'local', uri: `search://local/${searchId}` };
      const roots = request.structuredQuery?.scope.locations ?? request.roots;
      const filenameQuery = request.structuredQuery?.name?.pattern ?? request.query;
      const semanticQuery = request.structuredQuery?.semantic?.query;
      const contentQuery =
        request.structuredQuery?.content?.query ?? semanticQuery ?? request.contentQuery;
      const semanticMode = request.structuredQuery?.mode === 'semantic';
      const entries = roots.flatMap((root) =>
        collectMatches(
          root.uri,
          filenameQuery,
          contentQuery,
          request.structuredQuery?.scope.showHidden ?? request.showHidden ?? true,
          (uri) => this.ctx.fileContentFor(uri),
          request.structuredQuery,
        ),
      );
      this.ctx.searches.set(searchId, { cancelled: false, entries });
      // Deferred with a macrotask (rather than a microtask) so it always runs
      // after this method's own promise has resolved and the caller has
      // recorded `searchId`, avoiding a race against the resultsBatch handler
      // matching events by searchId.
      setTimeout(() => {
        if (this.ctx.searches.get(searchId)?.cancelled ?? true) return;
        this.ctx.eventSequence += 1;
        this.ctx.emit({
          eventId: this.ctx.eventSequence,
          timestamp: '2026-01-01T00:00:00.000Z',
          workspaceId: request.workspaceId,
          payload: {
            type: 'search.resultsBatch',
            searchId,
            entries,
            isComplete: true,
            warningsCount: 0,
            executionMode: semanticMode ? 'semantic' : 'liveRecursive',
          },
        });
      }, 0);
      const unsupported = [
        ...(request.structuredQuery?.gitStatuses.length ? (['gitStatus'] as const) : []),
        ...(request.structuredQuery?.tags.length ? (['tags'] as const) : []),
        ...(Object.keys(request.structuredQuery?.metadata ?? {}).length
          ? (['metadata'] as const)
          : []),
      ];
      return {
        searchId,
        location,
        limitations:
          unsupported.length === 0
            ? []
            : [{ providerId: 'local', unevaluatedPredicates: unsupported }],
        executionMode: semanticMode ? 'semantic' : 'liveRecursive',
        ...(semanticMode
          ? {
              semanticResults: entries.map((entry, index) => ({
                entryId: entry.id,
                location: entry.location,
                score: 1 - index * 0.05,
                bestEvidence: {
                  recordId: `mock-record-${index}`,
                  sourceId: entry.id,
                  score: 1 - index * 0.05,
                  chunkKind: 'chunk',
                  excerpt: contentQuery ?? '',
                  provenanceJson: JSON.stringify({
                    kind: 'exact',
                    value: { kind: 'textLines', startLine: 1, endLine: 3 },
                  }),
                  indexedContentHash: `mock-hash-${entry.id}`,
                  generation: 1,
                  available: true,
                  stale: false,
                  generated: false,
                  sourcePosition: 0,
                },
                additionalEvidence: [],
                additionalSourceIds: [],
              })),
            }
          : {}),
        ...(semanticMode
          ? {
              semanticCoverage: {
                eligible: entries.length,
                indexed: entries.length,
                stale: 0,
                pending: 0,
                excluded: 0,
                skipped: 0,
                failed: 0,
                unavailable: 0,
                partial: false,
              },
            }
          : {}),
      };
    });
  }

  cancelSearch(searchId: string, signal?: AbortSignal): Promise<void> {
    return this.ctx.perform('cancelSearch', signal, () => {
      const search = this.ctx.searches.get(searchId);
      if (search === undefined) {
        throw new MockClientError('notFound', `No mock search with id ${searchId}`);
      }
      search.cancelled = true;
    });
  }

  startComparison(
    request: StartComparisonRequest,
    signal?: AbortSignal,
  ): Promise<StartComparisonResult> {
    return this.ctx.perform('startComparison', signal, () => {
      this.ctx.comparisonSequence += 1;
      const comparisonId = `mock-comparison-${this.ctx.seed}-${this.ctx.comparisonSequence}`;
      const entries = buildMockComparisonEntries(
        request.left.uri,
        request.right.uri,
        request.criteria,
        request.showHidden ?? false,
      );
      this.ctx.comparisons.set(comparisonId, {
        cancelled: false,
        entries,
        left: request.left,
        right: request.right,
        criteria: request.criteria,
      });
      // Deferred with a macrotask so it always runs after this method's own
      // promise resolves and the caller has recorded `comparisonId`,
      // mirroring `startSearch`'s race-avoidance for its results-batch event.
      setTimeout(() => {
        if (this.ctx.comparisons.get(comparisonId)?.cancelled ?? true) return;
        this.ctx.eventSequence += 1;
        this.ctx.emit({
          eventId: this.ctx.eventSequence,
          timestamp: '2026-01-01T00:00:00.000Z',
          workspaceId: request.workspaceId,
          payload: {
            type: 'comparison.resultsBatch',
            comparisonId,
            entries,
            isComplete: true,
            warningsCount: 0,
          },
        });
      }, 0);
      return { comparisonId };
    });
  }

  getComparison(
    comparisonId: string,
    options?: { offset?: number; limit?: number; differencesOnly?: boolean },
    signal?: AbortSignal,
  ): Promise<ComparisonPage> {
    return this.ctx.perform('getComparison', signal, () => {
      const comparison = this.ctx.requireComparison(comparisonId);
      const offset = options?.offset ?? 0;
      const limit = options?.limit ?? 200;
      const filtered =
        (options?.differencesOnly ?? false)
          ? comparison.entries.filter((entry) => entry.status !== 'identical')
          : comparison.entries;
      return {
        comparisonId,
        left: comparison.left,
        right: comparison.right,
        criteria: comparison.criteria,
        offset,
        limit,
        total: filtered.length,
        entries: filtered.slice(offset, offset + limit),
        isComplete: true,
        warningsCount: 0,
      };
    });
  }

  cancelComparison(comparisonId: string, signal?: AbortSignal): Promise<void> {
    return this.ctx.perform('cancelComparison', signal, () => {
      this.ctx.requireComparison(comparisonId).cancelled = true;
    });
  }

  startChecksums(
    request: StartChecksumRequest,
    signal?: AbortSignal,
  ): Promise<StartChecksumResult> {
    return this.ctx.perform('startChecksums', signal, () => {
      this.ctx.checksumSequence += 1;
      const jobId = `mock-checksum-${this.ctx.seed}-${this.ctx.checksumSequence}`;
      const entries: ChecksumEntry[] = request.entries.map((location) => {
        const content = this.ctx.fileContentFor(location.uri);
        const checksums: Record<string, string> = {};
        for (const algorithm of request.algorithms) {
          checksums[algorithm] = mockDigest(location.uri, algorithm);
        }
        return {
          location,
          relativePath: lastSegment(location.uri),
          size: content.byteLength,
          checksums,
        };
      });
      this.ctx.checksumJobs.set(jobId, {
        cancelled: false,
        entries,
        algorithms: request.algorithms,
      });
      // Deferred with a macrotask for the same reason as `startComparison`:
      // the caller must have recorded `jobId` before the batch arrives.
      setTimeout(() => {
        if (this.ctx.checksumJobs.get(jobId)?.cancelled ?? true) return;
        this.ctx.eventSequence += 1;
        this.ctx.emit({
          eventId: this.ctx.eventSequence,
          timestamp: '2026-01-01T00:00:00.000Z',
          workspaceId: request.workspaceId,
          payload: {
            type: 'checksum.resultsBatch',
            jobId,
            entries,
            isComplete: true,
            isCancelled: false,
          },
        });
      }, 0);
      return { jobId };
    });
  }

  getChecksums(
    jobId: string,
    options?: { offset?: number; limit?: number },
    signal?: AbortSignal,
  ): Promise<ChecksumPage> {
    return this.ctx.perform('getChecksums', signal, () => {
      const job = this.ctx.requireChecksumJob(jobId);
      const offset = options?.offset ?? 0;
      const limit = options?.limit ?? 200;
      return {
        jobId,
        algorithms: job.algorithms,
        offset,
        limit,
        total: job.entries.length,
        totalEntries: job.entries.length,
        entries: job.entries.slice(offset, offset + limit),
        isComplete: true,
        isCancelled: job.cancelled,
        hasMore: offset + limit < job.entries.length,
      };
    });
  }

  cancelChecksums(jobId: string, signal?: AbortSignal): Promise<void> {
    return this.ctx.perform('cancelChecksums', signal, () => {
      this.ctx.requireChecksumJob(jobId).cancelled = true;
    });
  }

  renderChecksumFile(
    jobId: string,
    algorithm: ChecksumAlgorithm,
    signal?: AbortSignal,
  ): Promise<ChecksumFile> {
    return this.ctx.perform('renderChecksumFile', signal, () => {
      const job = this.ctx.requireChecksumJob(jobId);
      const lines = job.entries
        .filter((entry) => entry.checksums[algorithm] !== undefined)
        .map((entry) => `${entry.checksums[algorithm]}  ${entry.relativePath}`);
      return {
        suggestedName: `checksums.${algorithm}`,
        content: `# ${algorithm}\n${lines.join('\n')}\n`,
      };
    });
  }

  saveChecksumFile(
    jobId: string,
    request: SaveChecksumFileRequest,
    signal?: AbortSignal,
  ): Promise<SavedChecksumFile> {
    return this.ctx.perform('saveChecksumFile', signal, () => {
      const job = this.ctx.requireChecksumJob(jobId);
      const lines = job.entries
        .filter((entry) => entry.checksums[request.algorithm] !== undefined)
        .map((entry) => `${entry.checksums[request.algorithm]}  ${entry.relativePath}`);
      const content = `# ${request.algorithm}\n${lines.join('\n')}\n`;
      // The mock has no real filesystem; recording the bytes it would have
      // written is enough to exercise the UI's save flow.
      this.ctx.fileContents.set(request.destination.uri, new TextEncoder().encode(content));
      return { location: request.destination, bytesWritten: content.length };
    });
  }

  verifyChecksumFile(
    jobId: string,
    content: string,
    signal?: AbortSignal,
  ): Promise<VerificationReport> {
    return this.ctx.perform('verifyChecksumFile', signal, () => {
      const job = this.ctx.requireChecksumJob(jobId);
      const results: VerificationResult[] = [];
      let matched = 0;
      let mismatched = 0;
      let missing = 0;
      for (const line of content.split('\n')) {
        const trimmed = line.trim();
        if (trimmed === '' || trimmed.startsWith('#')) continue;
        const [digest, ...rest] = trimmed.split(/ {2}| \*/);
        const path = rest.join('  ');
        if (digest === undefined || path === '') continue;
        const entry = job.entries.find((candidate) => candidate.relativePath === path);
        const actual = entry === undefined ? undefined : Object.values(entry.checksums)[0];
        if (actual === undefined) {
          missing += 1;
          results.push({ path, status: 'missing' });
        } else if (actual.toLowerCase() === digest.toLowerCase()) {
          matched += 1;
          results.push({ path, status: 'match' });
        } else {
          mismatched += 1;
          results.push({ path, status: 'mismatch', expected: digest, actual });
        }
      }
      return { jobId, results, matched, mismatched, missing };
    });
  }

  startDuplicateScan(
    request: StartDuplicateScanRequest,
    signal?: AbortSignal,
  ): Promise<StartDuplicateScanResult> {
    return this.ctx.perform('startDuplicateScan', signal, () => {
      this.ctx.duplicateScanSequence += 1;
      const scanId = `mock-duplicate-scan-${this.ctx.seed}-${this.ctx.duplicateScanSequence}`;
      const groups = buildMockDuplicateGroups(request.roots);
      this.ctx.duplicateScans.set(scanId, { cancelled: false, groups, roots: request.roots });
      setTimeout(() => {
        if (this.ctx.duplicateScans.get(scanId)?.cancelled ?? true) return;
        this.ctx.eventSequence += 1;
        this.ctx.emit({
          eventId: this.ctx.eventSequence,
          timestamp: '2026-01-01T00:00:00.000Z',
          workspaceId: request.workspaceId,
          payload: {
            type: 'duplicates.resultsReady',
            scanId,
            groups,
            isCancelled: false,
            warningsCount: 0,
          },
        });
      }, 0);
      return { scanId };
    });
  }

  getDuplicateScan(
    scanId: string,
    options?: { offset?: number; limit?: number },
    signal?: AbortSignal,
  ): Promise<DuplicatePage> {
    return this.ctx.perform('getDuplicateScan', signal, () => {
      const scan = this.ctx.requireDuplicateScan(scanId);
      const offset = options?.offset ?? 0;
      const limit = options?.limit ?? 200;
      const fullyHashed = scan.groups.reduce(
        (total, group) => total + group.distinctLocations.length + group.hardlinkClusters.length,
        0,
      );
      return {
        scanId,
        roots: scan.roots,
        offset,
        limit,
        total: scan.groups.length,
        groups: scan.groups.slice(offset, offset + limit),
        isComplete: true,
        isCancelled: scan.cancelled,
        hasMore: offset + limit < scan.groups.length,
        stats: {
          candidates: fullyHashed + 2,
          sizeSurvivors: fullyHashed,
          partiallyHashed: fullyHashed,
          fullyHashed,
          bytesHashed: scan.groups.reduce(
            (total, group) => total + group.size * group.distinctLocations.length,
            0,
          ),
          failed: 0,
        },
        warningsCount: 0,
      };
    });
  }

  cancelDuplicateScan(scanId: string, signal?: AbortSignal): Promise<void> {
    return this.ctx.perform('cancelDuplicateScan', signal, () => {
      this.ctx.requireDuplicateScan(scanId).cancelled = true;
    });
  }

  startMusicDuplicateScan(
    request: StartMusicDuplicateScanRequest,
    signal?: AbortSignal,
  ): Promise<StartMusicDuplicateScanResult> {
    return this.ctx.perform('startMusicDuplicateScan', signal, () => {
      const scanId = `mock-music-scan-${this.ctx.seed}-${this.ctx.musicScans.size + 1}`;
      this.ctx.musicScans.set(scanId, { roots: request.roots, cancelled: false });
      return { scanId };
    });
  }

  getMusicDuplicateScan(
    scanId: string,
    options?: { offset?: number; limit?: number },
    signal?: AbortSignal,
  ): Promise<MusicDuplicatePage> {
    return this.ctx.perform('getMusicDuplicateScan', signal, () => {
      const scan = this.ctx.musicScans.get(scanId);
      if (scan === undefined) throw new MockClientError('notFound', 'Unknown music scan');
      const root = scan.roots[0]?.uri.replace(/\/$/, '') ?? 'file:///music';
      const file = (name: string, lossless: boolean, bitrate: number, size: number) => ({
        location: { providerId: scan.roots[0]?.providerId ?? 'local', uri: `${root}/${name}` },
        size,
        format: lossless ? 'flac' : 'mpeg',
        lossless,
        bitrateKbps: bitrate,
        durationSeconds: 215,
        artist: 'Example Artist',
        title: 'Example Song',
      });
      const groups = [
        {
          confidence: 'likely' as const,
          files: [
            file('song.flac', true, 900, 30_000_000),
            file('song.mp3', false, 192, 5_000_000),
          ],
          keepIndex: 0,
          reclaimableBytes: 5_000_000,
        },
      ];
      const offset = options?.offset ?? 0;
      const limit = options?.limit ?? 200;
      return {
        scanId,
        roots: scan.roots.map((location) => ({ ...location })),
        phase: 'done' as const,
        filesFound: 2,
        filesProcessed: 2,
        unreadableTags: 0,
        offset,
        limit,
        total: groups.length,
        groups: groups.slice(offset, offset + limit),
        isComplete: true,
        isCancelled: scan.cancelled,
        hasMore: false,
      };
    });
  }

  cancelMusicDuplicateScan(scanId: string, signal?: AbortSignal): Promise<void> {
    return this.ctx.perform('cancelMusicDuplicateScan', signal, () => {
      const scan = this.ctx.musicScans.get(scanId);
      if (scan === undefined) throw new MockClientError('notFound', 'Unknown music scan');
      scan.cancelled = true;
    });
  }

  forgetFolderView(_location: Location, signal?: AbortSignal): Promise<void> {
    return this.ctx.perform('forgetFolderView', signal, () => undefined);
  }

  startSnapshotJob(
    request: StartSnapshotJobRequest,
    signal?: AbortSignal,
  ): Promise<StartSnapshotJobResult> {
    return this.ctx.perform('startSnapshotJob', signal, () => {
      const jobId = `mock-snapshot-${this.ctx.seed}-${this.ctx.snapshotJobs.size + 1}`;
      this.ctx.snapshotJobs.set(jobId, request);
      return { jobId };
    });
  }

  getSnapshotJob(
    jobId: string,
    options?: { offset?: number; limit?: number },
    signal?: AbortSignal,
  ): Promise<SnapshotJobPage> {
    return this.ctx.perform('getSnapshotJob', signal, () => {
      const request = this.ctx.snapshotJobs.get(jobId);
      if (request === undefined) throw new MockClientError('notFound', 'Unknown snapshot job');
      const create = request.type === 'create';
      const root = create ? request.root : request.target;
      const entries = create
        ? []
        : [
            {
              kind: 'changed' as const,
              path: 'docs/notes.txt',
              location: {
                providerId: root.providerId,
                uri: `${root.uri.replace(/\/$/, '')}/docs/notes.txt`,
              },
              snapshotSize: 10,
              currentSize: 12,
              isDirectory: false,
            },
          ];
      const offset = options?.offset ?? 0;
      const limit = options?.limit ?? 200;
      return {
        jobId,
        kind: create ? ('create' as const) : ('compare' as const),
        phase: 'done' as const,
        entriesFound: 3,
        filesProcessed: 3,
        bytesHashed: 0,
        hasChecksums: create ? request.withChecksums : true,
        ...(create ? { snapshot: request.destination } : {}),
        root,
        counts: { added: 0, removed: 0, changed: entries.length, corrupted: 0, unchanged: 2 },
        offset,
        limit,
        total: entries.length,
        entries: entries.slice(offset, offset + limit),
        isComplete: true,
        isCancelled: false,
        hasMore: false,
      };
    });
  }

  cancelSnapshotJob(jobId: string, signal?: AbortSignal): Promise<void> {
    return this.ctx.perform('cancelSnapshotJob', signal, () => {
      if (!this.ctx.snapshotJobs.has(jobId))
        throw new MockClientError('notFound', 'Unknown snapshot job');
    });
  }

  startLinkDoctorScan(
    request: StartLinkDoctorScanRequest,
    signal?: AbortSignal,
  ): Promise<StartLinkDoctorScanResult> {
    return this.ctx.perform('startLinkDoctorScan', signal, () => {
      const scanId = `mock-link-scan-${this.ctx.seed}-${this.ctx.linkScans.size + 1}`;
      this.ctx.linkScans.set(scanId, { roots: request.roots, cancelled: false });
      return { scanId };
    });
  }

  getLinkDoctorScan(
    scanId: string,
    options?: { offset?: number; limit?: number },
    signal?: AbortSignal,
  ): Promise<LinkDoctorPage> {
    return this.ctx.perform('getLinkDoctorScan', signal, () => {
      const scan = this.ctx.linkScans.get(scanId);
      if (scan === undefined) throw new MockClientError('notFound', 'Unknown link scan');
      const root = scan.roots[0]?.uri.replace(/\/$/, '') ?? 'file:///docs';
      const providerId = scan.roots[0]?.providerId ?? 'local';
      const problems = [
        {
          kind: 'missingDocumentLink' as const,
          source: { providerId, uri: `${root}/readme.md` },
          target: 'docs/missing.md',
          resolved: { providerId, uri: `${root}/docs/missing.md` },
          line: 3,
        },
      ];
      const offset = options?.offset ?? 0;
      const limit = options?.limit ?? 200;
      return {
        scanId,
        roots: scan.roots.map((location) => ({ ...location })),
        entriesScanned: 12,
        linksChecked: 7,
        offset,
        limit,
        total: problems.length,
        problems: problems.slice(offset, offset + limit),
        isComplete: true,
        isCancelled: scan.cancelled,
        hasMore: false,
      };
    });
  }

  cancelLinkDoctorScan(scanId: string, signal?: AbortSignal): Promise<void> {
    return this.ctx.perform('cancelLinkDoctorScan', signal, () => {
      const scan = this.ctx.linkScans.get(scanId);
      if (scan === undefined) throw new MockClientError('notFound', 'Unknown link scan');
      scan.cancelled = true;
    });
  }

  findLinkReplacements(
    request: FindLinkReplacementsRequest,
    signal?: AbortSignal,
  ): Promise<FindLinkReplacementsResult> {
    return this.ctx.perform('findLinkReplacements', signal, () => {
      const root = request.roots[0];
      return {
        candidates:
          root === undefined
            ? []
            : [
                {
                  location: {
                    providerId: root.providerId,
                    uri: `${root.uri.replace(/\/$/, '')}/moved/${request.targetName}`,
                  },
                  size: request.expectedSize ?? 100,
                  sameSize: true,
                  exactName: true,
                },
              ],
        truncated: false,
      };
    });
  }

  generateSyncPlan(
    comparisonId: string,
    request: GenerateSyncPlanRequest,
    signal?: AbortSignal,
  ): Promise<SyncPlan> {
    return this.ctx.perform('generateSyncPlan', signal, () => {
      const comparison = this.ctx.requireComparison(comparisonId);
      const items = comparison.entries
        .filter((entry) => entry.status !== 'identical')
        .filter((entry) => admitsPath(entry.relativePath, request.include, request.exclude))
        .map((entry) => ({
          relativePath: entry.relativePath,
          status: entry.status,
          action: defaultSyncAction(entry.status, request.mode),
          ...(entry.left === undefined ? {} : { left: entry.left }),
          ...(entry.right === undefined ? {} : { right: entry.right }),
        }));
      return { comparisonId, items };
    });
  }

  applySyncPlan(
    comparisonId: string,
    request: ApplySyncPlanRequest,
    signal?: AbortSignal,
  ): Promise<ApplySyncPlanResult> {
    return this.ctx.perform('applySyncPlan', signal, () => {
      const comparison = this.ctx.requireComparison(comparisonId);
      const operationIds: OperationId[] = [];
      for (const item of request.items) {
        if (item.action === 'skip') continue;
        this.ctx.operationSequence += 1;
        const operationId = `mock-operation-${this.ctx.seed}-${this.ctx.operationSequence}`;
        const isDelete = item.action === 'deleteLeft' || item.action === 'deleteRight';
        const sourceRoot =
          item.action === 'copyRightToLeft' || item.action === 'deleteRight'
            ? comparison.right
            : comparison.left;
        const source: Location = {
          providerId: sourceRoot.providerId,
          uri: `${sourceRoot.uri}/${item.relativePath}`,
        };
        this.ctx.operations.set(operationId, {
          id: operationId,
          kind: isDelete ? 'delete' : 'copy',
          state: 'completed',
          sources: [{ id: source.uri, location: source }],
          progress: { completedItems: 1, completedBytes: 0 },
          conflictPolicy: request.conflictPolicy ?? 'overwrite',
          createdAt: '2026-01-01T00:00:00.000Z',
          startedAt: '2026-01-01T00:00:00.000Z',
        });
        operationIds.push(operationId);
      }
      return { operationIds };
    });
  }
}
