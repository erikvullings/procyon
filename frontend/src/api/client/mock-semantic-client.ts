import type {
  AcceptSemanticInstallationOfferRequest,
  AnalyzeCorpusThemesRequest,
  AttachSemanticVocabularyRequest,
  CancelKnowledgeAnswerRequest,
  CancelKnowledgeSearchRequest,
  CheckpointSemanticModelMigrationRequest,
  CompleteSemanticModelMigrationRequest,
  ConfirmSemanticEnrolmentRequest,
  ConfirmSemanticExclusionRequest,
  ConfirmSemanticIndexRemovalRequest,
  ConfirmSemanticModelMigrationRequest,
  CorpusThemeSources,
  CorpusThemeStatus,
  CorpusThemesResponse,
  CreateSemanticIndexRemovalPlanRequest,
  CreateSemanticInstallationOfferRequest,
  DeleteLlmProfileRequest,
  DeleteRagConversationRequest,
  DeleteSemanticVocabularyImpact,
  DocumentSummary,
  DocumentSummaryPreview,
  ExecuteKnowledgeSearchRequest,
  GenerateDocumentSummaryRequest,
  GenerateKnowledgeAnswerRequest,
  GenerateRagAnswerRequest,
  GenerateRagAnswerResponse,
  GetCorpusThemeSourcesRequest,
  GetCorpusThemeStatusRequest,
  GetDocumentSummaryRequest,
  GetSemanticFolderStatusRequest,
  ImportSemanticLocalModelRequest,
  InstallSemanticWorkerPatchRequest,
  KnowledgeAnswer,
  KnowledgeCapabilities,
  KnowledgeQueryInterpretation,
  KnowledgeRoot,
  KnowledgeSearchPlan,
  KnowledgeSearchResult,
  KnowledgeSourceLocation,
  ListKnowledgeRootsRequest,
  LlmModelLimits,
  LlmProfile,
  LlmProfileExport,
  LlmProfilePreset,
  LlmProfileTestResult,
  MoveSemanticDataRequest,
  ParseKnowledgeQueryRequest,
  PlanKnowledgeSearchRequest,
  PlanSemanticExclusionRequest,
  PlanSemanticModelMigrationRequest,
  PreviewDocumentSummaryRequest,
  PreviewRagRequest,
  PreviewSemanticEnrolmentRequest,
  RagAnswer,
  RagPreview,
  ResolvedRagCitation,
  ResolveKnowledgeSourceRequest,
  ResolveRagCitationRequest,
  ResumeSemanticCleanupRequest,
  ReviewConceptCandidateRequest,
  SavedRagConversation,
  SaveLlmProfileRequest,
  SaveRagConversationRequest,
  SemanticComponentCapabilities,
  SemanticComponentStatus,
  SemanticDataMoveReceipt,
  SemanticEnrolmentPreview,
  SemanticExclusionPlan,
  SemanticFolderStatus,
  SemanticIndexRemovalPlan,
  SemanticIndexRemovalReceipt,
  SemanticInstallationOffer,
  SemanticInstallReceipt,
  SemanticLibraryCapabilities,
  SemanticLibraryRevisionRequest,
  SemanticLibraryStatus,
  SemanticModelMigrationPlan,
  SemanticModelMigrationProgress,
  SemanticModelProfile,
  SemanticModelSelection,
  SemanticOcrJob,
  SemanticOcrStatus,
  SemanticOcrTarget,
  SemanticUninstallReceipt,
  SemanticVocabulary,
  SemanticWorkerPatchResponse,
  StartSemanticOcrRemediationRequest,
  UninstallSemanticComponentsRequest,
  UpdateGemmaMediaRequest,
  UpdateSemanticEligibilityOverridesRequest,
  WorkspaceId,
} from '../../models';
import type { MockContext } from './mock-context';
import { mockCorpusThemeStatus, mockCorpusThemes } from './mock-corpus-themes';
import {
  buildMockKnowledgeAnswer,
  executeMockKnowledgeSearch,
  mockKnowledgeCapabilities,
  mockKnowledgeRoots,
  mockKnowledgeRouteUnavailable,
  parseMockKnowledgeQuery,
  resolveMockKnowledgeSource,
} from './mock-knowledge-search';
import {
  emptySemanticDiskUse,
  installedSemanticDiskUse,
  MockClientError,
  mockCleanupCategories,
  mockSemanticIdentity,
  mockSemanticInstalledComponents,
  mockSemanticMigrationProgress,
  mockSemanticOffer,
  mockSemanticProfiles,
  mockSummaryVisualCount,
  retainedSemanticDiskUse,
  SEMANTIC_OPERATIONS,
  sameSemanticIndexCounts,
  sameSemanticOcrTarget,
  semanticLocationContains,
} from './mock-mock-shared';
import type { SemanticClient } from './semantic-client';

export class MockSemanticClient implements SemanticClient {
  constructor(private readonly ctx: MockContext) {}

  getSemanticComponentCapabilities(signal?: AbortSignal): Promise<SemanticComponentCapabilities> {
    return this.ctx.perform('getSemanticComponentCapabilities', signal, () =>
      this.ctx.semanticStatus.lifecycle.state === 'unavailable'
        ? {
            authority: 'unavailable',
            operations: [],
            runtimeExecutableDownload: 'unavailable',
          }
        : {
            authority: 'deterministicMock',
            operations: [...SEMANTIC_OPERATIONS],
            runtimeExecutableDownload: 'simulated',
          },
    );
  }

  getSemanticOcrStatus(signal?: AbortSignal): Promise<SemanticOcrStatus> {
    return this.ctx.perform('getSemanticOcrStatus', signal, () => {
      if (this.ctx.semanticOcrAutoAdvance) this.ctx.advanceSemanticOcrJobs();
      return structuredClone(this.ctx.semanticOcrStatus);
    });
  }

  setSemanticOcrConsent(enabled: boolean, signal?: AbortSignal): Promise<SemanticOcrStatus> {
    return this.ctx.perform('setSemanticOcrConsent', signal, () => {
      if (enabled && this.ctx.semanticOcrStatus.availability.state !== 'available') {
        throw new MockClientError('unavailable', 'OCRmyPDF is unavailable');
      }
      this.ctx.semanticOcrStatus = {
        ...this.ctx.semanticOcrStatus,
        enabled,
        replaceOriginal: enabled && this.ctx.semanticOcrStatus.replaceOriginal,
        jobs: enabled
          ? this.ctx.semanticOcrStatus.jobs
          : this.ctx.semanticOcrStatus.jobs.map((job) =>
              job.state === 'queued' || job.state === 'running'
                ? { ...job, state: 'cancelled', updatedAtMs: Date.now() }
                : job,
            ),
      };
      return structuredClone(this.ctx.semanticOcrStatus);
    });
  }

  setSemanticOcrReplaceOriginal(
    enabled: boolean,
    signal?: AbortSignal,
  ): Promise<SemanticOcrStatus> {
    return this.ctx.perform('setSemanticOcrReplaceOriginal', signal, () => {
      if (enabled && !this.ctx.semanticOcrStatus.enabled) {
        throw new MockClientError('disabled', 'OCR remediation is disabled');
      }
      this.ctx.semanticOcrStatus = { ...this.ctx.semanticOcrStatus, replaceOriginal: enabled };
      return structuredClone(this.ctx.semanticOcrStatus);
    });
  }

  startSemanticOcrRemediation(
    request: StartSemanticOcrRemediationRequest,
    signal?: AbortSignal,
  ): Promise<SemanticOcrJob> {
    return this.ctx.perform('startSemanticOcrRemediation', signal, () => {
      if (!this.ctx.semanticOcrStatus.enabled) {
        throw new MockClientError('disabled', 'OCR remediation is disabled');
      }
      if (this.ctx.semanticOcrStatus.availability.state !== 'available') {
        throw new MockClientError('unavailable', 'OCRmyPDF is unavailable');
      }
      const reported = this.ctx.semanticOcrStatus.reportedFiles;
      let targets: readonly SemanticOcrTarget[];
      switch (request.scope) {
        case 'oneFile':
          targets = [request.file];
          break;
        case 'selectedFiles':
          targets = request.files;
          break;
        case 'enrolledRoot':
          targets = reported.filter((target) => target.rootId === request.rootId);
          break;
        case 'allReported':
          targets = reported;
          break;
      }
      if (targets.length === 0) {
        throw new MockClientError('nothingToRemediate', 'No files currently require OCR');
      }
      if (
        targets.some(
          (target) => !reported.some((candidate) => sameSemanticOcrTarget(candidate, target)),
        )
      ) {
        throw new MockClientError(
          'unreportedTarget',
          'The requested file is not currently reported as requiring OCR',
        );
      }
      const unique = targets.filter(
        (target, index) =>
          targets.findIndex((candidate) => sameSemanticOcrTarget(candidate, target)) === index,
      );
      this.ctx.semanticOcrJobSequence += 1;
      const now = Date.now();
      const job: SemanticOcrJob = {
        id: `00000000-0000-4000-8000-${String(this.ctx.semanticOcrJobSequence).padStart(12, '0')}`,
        state: 'queued',
        createdAtMs: now,
        updatedAtMs: now,
        totalFiles: unique.length,
        processedFiles: 0,
        files: [],
        availabilityFailure: null,
      };
      this.ctx.semanticOcrJobTargets.set(job.id, structuredClone(unique));
      this.ctx.semanticOcrStatus = {
        ...this.ctx.semanticOcrStatus,
        jobs: [job, ...this.ctx.semanticOcrStatus.jobs],
      };
      return structuredClone(job);
    });
  }

  cancelSemanticOcrRemediation(jobId: string, signal?: AbortSignal): Promise<SemanticOcrJob> {
    return this.ctx.perform('cancelSemanticOcrRemediation', signal, () => {
      const job = this.ctx.semanticOcrStatus.jobs.find((candidate) => candidate.id === jobId);
      if (job === undefined) throw new MockClientError('notFound', 'OCR job was not found');
      const cancelled: SemanticOcrJob =
        job.state === 'queued' || job.state === 'running'
          ? { ...job, state: 'cancelled', updatedAtMs: Date.now() }
          : job;
      this.ctx.semanticOcrJobTargets.delete(jobId);
      this.ctx.semanticOcrStatus = {
        ...this.ctx.semanticOcrStatus,
        jobs: this.ctx.semanticOcrStatus.jobs.map((candidate) =>
          candidate.id === jobId ? cancelled : candidate,
        ),
      };
      return structuredClone(cancelled);
    });
  }

  getSemanticComponentStatus(signal?: AbortSignal): Promise<SemanticComponentStatus> {
    return this.ctx.perform('getSemanticComponentStatus', signal, () =>
      structuredClone(this.ctx.semanticStatus),
    );
  }

  listSemanticComponentProfiles(signal?: AbortSignal): Promise<SemanticModelProfile[]> {
    return this.ctx.perform('listSemanticComponentProfiles', signal, () => {
      this.ctx.requireSemanticAvailable();
      return mockSemanticProfiles();
    });
  }

  getGemmaLibrarySetup(
    signal?: AbortSignal,
  ): Promise<import('../../models/semantic-components').GemmaLibrarySetup | null> {
    return this.ctx.perform('getGemmaLibrarySetup', signal, () => null);
  }

  initializeGemmaLibrary(
    _request: import('../../models/semantic-components').InitializeGemmaLibraryRequest,
    signal?: AbortSignal,
  ): Promise<void> {
    return this.ctx.perform('initializeGemmaLibrary', signal, () => {
      throw new MockClientError(
        'unavailable',
        'The mock host does not include a signed Gemma package.',
      );
    });
  }

  createSemanticComponentInstallationOffer(
    request: CreateSemanticInstallationOfferRequest,
    signal?: AbortSignal,
  ): Promise<SemanticInstallationOffer> {
    return this.ctx.perform('createSemanticComponentInstallationOffer', signal, () => {
      this.ctx.requireSemanticAvailable();
      this.ctx.semanticOfferSequence += 1;
      const offerId = `mock-offer-${this.ctx.semanticOfferSequence}`;
      const offer = mockSemanticOffer(offerId, request.profile);
      this.ctx.semanticOffers.set(offerId, offer);
      this.ctx.semanticStatus = {
        ...this.ctx.semanticStatus,
        lifecycle: { state: 'offered', offerId },
      };
      return structuredClone(offer);
    });
  }

  acceptSemanticComponentInstallationOffer(
    request: AcceptSemanticInstallationOfferRequest,
    signal?: AbortSignal,
  ): Promise<SemanticInstallReceipt> {
    return this.ctx.perform('acceptSemanticComponentInstallationOffer', signal, () => {
      this.ctx.requireSemanticAvailable();
      const offer = this.ctx.semanticOffers.get(request.offerId);
      if (offer === undefined) {
        throw new MockClientError('consentRequired', 'A reviewed installation offer is required');
      }
      this.ctx.semanticOffers.delete(request.offerId);
      this.ctx.semanticStatus = {
        lifecycle: { state: 'installedEnabled' },
        dataRoot: offer.dataRoot,
        activeModel: { profile: offer.profile, identity: offer.resolvedModel },
        migration: null,
        components: mockSemanticInstalledComponents(),
        diskUse: installedSemanticDiskUse(),
      };
      return {
        installedArtifactIds: offer.components.map(({ artifactId }) => artifactId),
      };
    });
  }

  pauseSemanticComponentIndexing(signal?: AbortSignal): Promise<void> {
    return this.ctx.perform('pauseSemanticComponentIndexing', signal, () => {
      this.ctx.requireSemanticLifecycle('installedEnabled');
      this.ctx.semanticStatus = { ...this.ctx.semanticStatus, lifecycle: { state: 'paused' } };
    });
  }

  resumeSemanticComponentIndexing(signal?: AbortSignal): Promise<void> {
    return this.ctx.perform('resumeSemanticComponentIndexing', signal, () => {
      this.ctx.requireSemanticLifecycle('paused');
      this.ctx.semanticStatus = {
        ...this.ctx.semanticStatus,
        lifecycle: { state: 'installedEnabled' },
      };
    });
  }

  createSemanticComponentIndexRemovalPlan(
    request: CreateSemanticIndexRemovalPlanRequest,
    signal?: AbortSignal,
  ): Promise<SemanticIndexRemovalPlan> {
    return this.ctx.perform('createSemanticComponentIndexRemovalPlan', signal, () => {
      this.ctx.requireSemanticAvailable();
      if (!/^[A-Za-z0-9._-]+$/u.test(request.enrolmentId)) {
        throw new MockClientError('invalidEnrolment', 'The enrolment identifier is invalid');
      }
      const expected = this.ctx.semanticEnrolmentInventory.get(request.enrolmentId);
      if (expected === undefined) {
        throw new MockClientError('invalidEnrolment', 'The enrolment identifier is unknown');
      }
      this.ctx.semanticIndexRemovalSequence += 1;
      const plan: SemanticIndexRemovalPlan = {
        planId: `mock-index-removal-${this.ctx.semanticIndexRemovalSequence}`,
        enrolmentId: request.enrolmentId,
        expected: structuredClone(expected),
      };
      this.ctx.semanticIndexRemovalPlans.set(plan.planId, plan);
      return structuredClone(plan);
    });
  }

  confirmSemanticComponentIndexRemoval(
    request: ConfirmSemanticIndexRemovalRequest,
    signal?: AbortSignal,
  ): Promise<SemanticIndexRemovalReceipt> {
    return this.ctx.perform('confirmSemanticComponentIndexRemoval', signal, () => {
      this.ctx.requireSemanticAvailable();
      const plan = this.ctx.semanticIndexRemovalPlans.get(request.planId);
      if (plan === undefined) {
        throw new MockClientError('indexRemoval', 'The index removal plan is stale or unknown');
      }
      this.ctx.semanticIndexRemovalPlans.delete(request.planId);
      const inventory = this.ctx.semanticEnrolmentInventory.get(plan.enrolmentId);
      if (inventory === undefined || !sameSemanticIndexCounts(inventory, plan.expected)) {
        throw new MockClientError('indexRemoval', 'The index removal plan is stale or unknown');
      }
      this.ctx.semanticEnrolmentInventory.delete(plan.enrolmentId);
      for (const [planId, candidate] of this.ctx.semanticIndexRemovalPlans) {
        if (candidate.enrolmentId === plan.enrolmentId) {
          this.ctx.semanticIndexRemovalPlans.delete(planId);
        }
      }
      return {
        enrolmentId: plan.enrolmentId,
        deleted: structuredClone(inventory),
        conversationEvidenceDeleted: true,
      };
    });
  }

  moveSemanticComponentData(
    request: MoveSemanticDataRequest,
    signal?: AbortSignal,
  ): Promise<SemanticDataMoveReceipt> {
    return this.ctx.perform('moveSemanticComponentData', signal, () => {
      this.ctx.requireSemanticAvailable();
      if (request.destination.trim().length === 0) {
        throw new MockClientError('dataMigration', 'A destination is required');
      }
      const source = this.ctx.semanticStatus.dataRoot ?? 'mock/semantic';
      this.ctx.semanticStatus = { ...this.ctx.semanticStatus, dataRoot: request.destination };
      return {
        source,
        destination: request.destination,
        verifiedBytes: this.ctx.semanticStatus.diskUse.totalBytes,
        verifiedFileCount: this.ctx.semanticStatus.components.length,
      };
    });
  }

  uninstallSemanticComponents(
    request: UninstallSemanticComponentsRequest,
    signal?: AbortSignal,
  ): Promise<SemanticUninstallReceipt> {
    return this.ctx.perform('uninstallSemanticComponents', signal, () => {
      this.ctx.requireSemanticAvailable();
      const removedComponentCount = this.ctx.semanticStatus.components.length;
      this.ctx.semanticStatus = {
        ...this.ctx.semanticStatus,
        lifecycle: { state: 'uninstalled', indexDecision: request.indexDecision },
        components: [],
        migration: null,
        ...(request.indexDecision === 'delete'
          ? { activeModel: null, diskUse: emptySemanticDiskUse() }
          : { diskUse: retainedSemanticDiskUse() }),
      };
      return { indexDecision: request.indexDecision, removedComponentCount };
    });
  }

  installSemanticComponentWorkerPatch(
    _request: InstallSemanticWorkerPatchRequest,
    signal?: AbortSignal,
  ): Promise<SemanticWorkerPatchResponse> {
    return this.ctx.perform('installSemanticComponentWorkerPatch', signal, () => {
      this.ctx.requireSemanticAvailable();
      return { receipt: null };
    });
  }

  importSemanticComponentLocalModel(
    request: ImportSemanticLocalModelRequest,
    signal?: AbortSignal,
  ): Promise<SemanticModelMigrationPlan> {
    return this.ctx.perform('importSemanticComponentLocalModel', signal, () => {
      this.ctx.requireSemanticAvailable();
      if (
        request.sourcePath.trim().length === 0 ||
        /^[A-Za-z][A-Za-z0-9+.-]*:\/\//u.test(request.sourcePath.trim()) ||
        request.modelId.trim().length === 0 ||
        request.upstreamRevision.trim().length === 0 ||
        request.licenseSpdx.trim().length === 0 ||
        request.licenseNotice.trim().length === 0 ||
        request.tokenizer.trim().length === 0 ||
        request.dimensions <= 0 ||
        request.normalization == null ||
        request.runtimeComponentId.trim().length === 0 ||
        request.runtimeVersionRequirement.trim().length === 0 ||
        request.languageCoverage.length === 0 ||
        request.languageCoverage.some((language) => language.trim().length === 0) ||
        request.estimatedDiskBytes <= 0 ||
        request.estimatedRamBytes <= 0
      ) {
        throw new MockClientError(
          'invalidLocalModelMetadata',
          'Complete local model metadata is required',
        );
      }
      return this.ctx.createSemanticMigrationPlan(
        request.profile,
        {
          modelId: request.modelId,
          revision: request.upstreamRevision,
        },
        request.estimate,
      );
    });
  }

  planSemanticComponentModelMigration(
    request: PlanSemanticModelMigrationRequest,
    signal?: AbortSignal,
  ): Promise<SemanticModelMigrationPlan> {
    return this.ctx.perform('planSemanticComponentModelMigration', signal, () => {
      this.ctx.requireSemanticAvailable();
      return this.ctx.createSemanticMigrationPlan(
        request.profile,
        mockSemanticIdentity(request.profile),
        request.estimate,
      );
    });
  }

  confirmSemanticComponentModelMigration(
    request: ConfirmSemanticModelMigrationRequest,
    signal?: AbortSignal,
  ): Promise<SemanticModelMigrationProgress> {
    return this.ctx.perform('confirmSemanticComponentModelMigration', signal, () => {
      this.ctx.requireSemanticAvailable();
      const plan = this.ctx.semanticMigrationPlan(request.migrationId);
      const progress = mockSemanticMigrationProgress(
        plan.migrationId,
        0,
        plan.estimate,
        plan.target,
        null,
      );
      this.ctx.semanticStatus = {
        ...this.ctx.semanticStatus,
        lifecycle: { state: 'migrating', progress },
        migration: progress,
      };
      return structuredClone(progress);
    });
  }

  checkpointSemanticComponentModelMigration(
    request: CheckpointSemanticModelMigrationRequest,
    signal?: AbortSignal,
  ): Promise<SemanticModelMigrationProgress> {
    return this.ctx.perform('checkpointSemanticComponentModelMigration', signal, () => {
      this.ctx.requireSemanticAvailable();
      const plan = this.ctx.semanticMigrationPlan(request.migrationId);
      const previous = this.ctx.semanticStatus.migration;
      if (
        previous == null ||
        previous.migrationId !== request.migrationId ||
        request.completedDocuments < previous.completedDocuments ||
        request.completedDocuments > plan.estimate.documents
      ) {
        throw new MockClientError('invalidMigrationProgress', 'Migration progress is invalid');
      }
      const progress = mockSemanticMigrationProgress(
        plan.migrationId,
        request.completedDocuments,
        plan.estimate,
        plan.target,
        request.resumeCursor ?? null,
      );
      this.ctx.semanticStatus = {
        ...this.ctx.semanticStatus,
        lifecycle: { state: 'migrating', progress },
        migration: progress,
      };
      return structuredClone(progress);
    });
  }

  completeSemanticComponentModelMigration(
    request: CompleteSemanticModelMigrationRequest,
    signal?: AbortSignal,
  ): Promise<SemanticModelSelection> {
    return this.ctx.perform('completeSemanticComponentModelMigration', signal, () => {
      this.ctx.requireSemanticAvailable();
      const plan = this.ctx.semanticMigrationPlan(request.migrationId);
      if (
        this.ctx.semanticStatus.migration?.migrationId !== request.migrationId ||
        this.ctx.semanticStatus.migration.completedDocuments < plan.estimate.documents
      ) {
        throw new MockClientError('invalidMigrationProgress', 'Migration is not complete');
      }
      this.ctx.semanticMigrationPlans.delete(request.migrationId);
      this.ctx.semanticStatus = {
        ...this.ctx.semanticStatus,
        lifecycle: { state: 'installedEnabled' },
        activeModel: plan.target,
        migration: null,
      };
      return structuredClone(plan.target);
    });
  }

  getSemanticLibraryCapabilities(signal?: AbortSignal): Promise<SemanticLibraryCapabilities> {
    return this.ctx.perform('getSemanticLibraryCapabilities', signal, () => ({
      authority: 'deterministicMock',
      operations: [
        'viewStatus',
        'viewFolderStatus',
        'previewEnrolment',
        'enrol',
        'planExclusion',
        'confirmExclusion',
        'resumeCleanup',
        'pause',
        'resume',
        'updateEligibilityOverrides',
        ...(this.ctx.semanticLibrary.gemmaMedia ? (['updateGemmaMedia'] as const) : []),
      ],
    }));
  }

  getSemanticLibraryStatus(signal?: AbortSignal): Promise<SemanticLibraryStatus> {
    return this.ctx.perform('getSemanticLibraryStatus', signal, () =>
      structuredClone(this.ctx.semanticLibrary),
    );
  }

  updateGemmaMedia(
    request: UpdateGemmaMediaRequest,
    signal?: AbortSignal,
  ): Promise<SemanticLibraryStatus> {
    return this.ctx.perform('updateGemmaMedia', signal, () => {
      this.ctx.requireSemanticLibraryRevision(request.policyRevision);
      const previous = this.ctx.semanticLibrary.gemmaMedia;
      if (!previous) throw new MockClientError('unavailable', 'A Gemma library is required');
      if (
        !request.confirmRemoval &&
        ((previous.images && !request.images) ||
          (previous.audio && !request.audio) ||
          (previous.video && !request.video))
      ) {
        throw new MockClientError('staleConfirmation', 'Removing media requires confirmation');
      }
      this.ctx.semanticLibrary.gemmaMedia = {
        images: request.images,
        audio: request.audio,
        video: request.video,
      };
      this.ctx.advanceSemanticLibraryRevision();
      return structuredClone(this.ctx.semanticLibrary);
    });
  }

  listSemanticVocabularies(signal?: AbortSignal): Promise<readonly SemanticVocabulary[]> {
    return this.ctx.perform('listSemanticVocabularies', signal, () =>
      structuredClone(this.ctx.semanticVocabularies),
    );
  }

  importSemanticVocabulary(skosJson: string, signal?: AbortSignal): Promise<SemanticVocabulary> {
    return this.ctx.perform('importSemanticVocabulary', signal, () => {
      const parsed = JSON.parse(skosJson) as {
        id: string;
        name: string;
        concepts?: SemanticVocabulary['concepts'];
      };
      const vocabulary: SemanticVocabulary = {
        id: parsed.id,
        name: parsed.name,
        concepts: parsed.concepts ?? [],
        workspaceIds: [],
        rootIds: [],
        reviewQueue: [],
        revision: 0,
      };
      this.ctx.semanticVocabularies.push(vocabulary);
      return structuredClone(vocabulary);
    });
  }

  exportSemanticVocabulary(vocabularyId: string, signal?: AbortSignal): Promise<string> {
    return this.ctx.perform('exportSemanticVocabulary', signal, () => {
      const vocabulary = this.ctx.semanticVocabularies.find(({ id }) => id === vocabularyId);
      if (!vocabulary) throw new MockClientError('notFound', 'Vocabulary not found');
      return JSON.stringify({ format: 'procyon-skos-1', ...vocabulary });
    });
  }

  attachSemanticVocabulary(
    request: AttachSemanticVocabularyRequest,
    signal?: AbortSignal,
  ): Promise<SemanticVocabulary> {
    return this.ctx.perform('attachSemanticVocabulary', signal, () => {
      const index = this.ctx.semanticVocabularies.findIndex(
        ({ id }) => id === request.vocabularyId,
      );
      const current = this.ctx.semanticVocabularies[index];
      if (!current) throw new MockClientError('notFound', 'Vocabulary not found');
      const updated: SemanticVocabulary = {
        ...current,
        workspaceIds: request.workspaceId
          ? [...new Set([...current.workspaceIds, request.workspaceId])]
          : current.workspaceIds,
        rootIds: request.rootId
          ? [...new Set([...current.rootIds, request.rootId])]
          : current.rootIds,
        revision: current.revision + 1,
      };
      this.ctx.semanticVocabularies[index] = updated;
      return structuredClone(updated);
    });
  }

  reviewSemanticConceptCandidate(
    request: ReviewConceptCandidateRequest,
    signal?: AbortSignal,
  ): Promise<SemanticVocabulary> {
    return this.ctx.perform('reviewSemanticConceptCandidate', signal, () => {
      const index = this.ctx.semanticVocabularies.findIndex(
        ({ id }) => id === request.vocabularyId,
      );
      const current = this.ctx.semanticVocabularies[index];
      if (!current) throw new MockClientError('notFound', 'Vocabulary not found');
      const updated: SemanticVocabulary = {
        ...current,
        reviewQueue: current.reviewQueue.map((candidate) =>
          candidate.id === request.candidateId
            ? { ...candidate, status: request.action === 'reject' ? 'rejected' : 'accepted' }
            : candidate,
        ),
        revision: current.revision + 1,
      };
      this.ctx.semanticVocabularies[index] = updated;
      return structuredClone(updated);
    });
  }

  deleteSemanticVocabulary(
    vocabularyId: string,
    confirmAffected: boolean,
    signal?: AbortSignal,
  ): Promise<DeleteSemanticVocabularyImpact> {
    return this.ctx.perform('deleteSemanticVocabulary', signal, () => {
      const vocabulary = this.ctx.semanticVocabularies.find(({ id }) => id === vocabularyId);
      if (!vocabulary) throw new MockClientError('notFound', 'Vocabulary not found');
      const requiresConfirmation =
        vocabulary.workspaceIds.length > 0 || vocabulary.rootIds.length > 0;
      const deleted = confirmAffected || !requiresConfirmation;
      if (deleted) {
        this.ctx.semanticVocabularies = this.ctx.semanticVocabularies.filter(
          ({ id }) => id !== vocabularyId,
        );
      }
      return {
        vocabularyId,
        affectedWorkspaceIds: vocabulary.workspaceIds,
        affectedRootIds: vocabulary.rootIds,
        requiresConfirmation,
        deleted,
      };
    });
  }

  getSemanticFolderStatus(
    request: GetSemanticFolderStatusRequest,
    signal?: AbortSignal,
  ): Promise<SemanticFolderStatus> {
    return this.ctx.perform('getSemanticFolderStatus', signal, () => {
      this.ctx.requireActiveSemanticFolder(request.workspaceId, request.location);
      const matchingExclusions = this.ctx.semanticLibrary.roots
        .flatMap((root) =>
          root.exclusions
            .filter((exclusion) => semanticLocationContains(exclusion.location, request.location))
            .map((exclusion) => ({ root, exclusion })),
        )
        .sort(
          (left, right) => left.exclusion.location.uri.length - right.exclusion.location.uri.length,
        );
      const excluded = matchingExclusions.at(-1);
      if (excluded !== undefined) {
        return {
          consent: 'excluded',
          rootId: excluded.root.id,
          exclusionId: excluded.exclusion.id,
          workspaceReferenced: excluded.root.workspaceReferences.includes(request.workspaceId),
          sourceAvailable: excluded.root.availability.state === 'available',
          unavailableReason:
            excluded.root.availability.state === 'temporarilyUnavailable'
              ? excluded.root.availability.reason
              : null,
        };
      }
      const matchingRoots = this.ctx.semanticLibrary.roots
        .filter(
          (root) =>
            root.location.providerId === request.location.providerId &&
            (root.location.uri === request.location.uri ||
              (root.recursive && semanticLocationContains(root.location, request.location))),
        )
        .sort((left, right) => left.location.uri.length - right.location.uri.length);
      const root = matchingRoots.at(-1);
      if (root === undefined) {
        return {
          consent: 'notIncluded',
          rootId: null,
          exclusionId: null,
          workspaceReferenced: false,
          sourceAvailable: true,
          unavailableReason: null,
        };
      }
      return {
        consent:
          root.location.uri === request.location.uri ? 'includedHere' : 'inheritedFromParent',
        rootId: root.id,
        exclusionId: null,
        workspaceReferenced: root.workspaceReferences.includes(request.workspaceId),
        sourceAvailable: root.availability.state === 'available',
        unavailableReason:
          root.availability.state === 'temporarilyUnavailable' ? root.availability.reason : null,
      };
    });
  }

  previewSemanticEnrolment(
    request: PreviewSemanticEnrolmentRequest,
    signal?: AbortSignal,
  ): Promise<SemanticEnrolmentPreview> {
    return this.ctx.perform('previewSemanticEnrolment', signal, () => {
      this.ctx.requireActiveSemanticFolder(request.workspaceId, request.location);
      this.ctx.semanticLibrarySequence += 1;
      const confirmationId = `mock-enrol-confirmation-${this.ctx.semanticLibrarySequence}`;
      this.ctx.semanticEnrolmentPreviews.set(confirmationId, {
        ...structuredClone(request),
        policyRevision: this.ctx.semanticLibrary.revision,
      });
      return {
        confirmationId,
        policyRevision: this.ctx.semanticLibrary.revision,
        location: structuredClone(request.location),
        recursive: request.recursive,
        pdfImagesEnabled: request.pdfImagesEnabled ?? false,
        pdfImagePageBudget: request.pdfImagePageBudget ?? null,
        normalizedExcerptsRetainedLocally: true,
        estimate: {
          completeness: 'partial',
          estimatedFiles: 42,
          estimatedSourceBytes: 4_200,
          estimatedExtractedBytes: 1_200,
          estimatedVectorBytes: 800,
          estimatedAdditionalLocalBytes: 2_500,
          missingModelDownloadBytes: 500,
          skippedReasonCounts: [
            { reason: 'hidden', count: 1 },
            { reason: 'unsupportedMime', count: 1 },
          ],
          exceededBudgets: [],
          unavailableReason: null,
        },
      };
    });
  }

  confirmSemanticEnrolment(
    request: ConfirmSemanticEnrolmentRequest,
    signal?: AbortSignal,
  ): Promise<SemanticLibraryStatus> {
    return this.ctx.perform('confirmSemanticEnrolment', signal, () => {
      this.ctx.requireActiveSemanticFolder(request.workspaceId, request.location);
      this.ctx.requireSemanticLibraryRevision(request.policyRevision);
      const preview = this.ctx.semanticEnrolmentPreviews.get(request.confirmationId);
      if (
        preview === undefined ||
        preview.policyRevision !== request.policyRevision ||
        preview.workspaceId !== request.workspaceId ||
        preview.location.providerId !== request.location.providerId ||
        preview.location.uri !== request.location.uri
      ) {
        throw new MockClientError(
          'staleConfirmation',
          'Semantic library confirmation is stale or invalid',
        );
      }
      this.ctx.semanticEnrolmentPreviews.delete(request.confirmationId);
      const existing = this.ctx.semanticLibrary.roots.find(
        (root) =>
          root.location.providerId === request.location.providerId &&
          root.location.uri === request.location.uri,
      );
      if (existing === undefined) {
        this.ctx.semanticLibrarySequence += 1;
        this.ctx.semanticLibrary.roots.push({
          id: `00000000-0000-0000-0000-${String(this.ctx.semanticLibrarySequence).padStart(12, '0')}`,
          location: structuredClone(request.location),
          recursive: preview.recursive,
          pdfImagesEnabled: preview.pdfImagesEnabled ?? false,
          pdfImagePageBudget: preview.pdfImagePageBudget ?? null,
          stableIdentityVerified: true,
          workspaceReferences: [request.workspaceId],
          eligibilityOverrides: [],
          attachedVocabularyIds: [],
          eligibilityReasonCounts: [
            { reason: 'hidden', count: 1 },
            { reason: 'unsupportedMime', count: 1 },
          ],
          ocrRequiredFiles: [],
          availability: { state: 'available' },
          reconciliationGeneration: 0,
          indexedGeneration: 0,
          exclusions: [],
        });
      } else {
        existing.pdfImagesEnabled = preview.pdfImagesEnabled ?? false;
        existing.pdfImagePageBudget = preview.pdfImagePageBudget ?? null;
        if (!existing.workspaceReferences.includes(request.workspaceId)) {
          existing.workspaceReferences.push(request.workspaceId);
        }
      }
      this.ctx.advanceSemanticLibraryRevision();
      return structuredClone(this.ctx.semanticLibrary);
    });
  }

  planSemanticExclusion(
    request: PlanSemanticExclusionRequest,
    signal?: AbortSignal,
  ): Promise<SemanticExclusionPlan> {
    return this.ctx.perform('planSemanticExclusion', signal, () => {
      this.ctx.requireActiveOrEnrolledSemanticRoot(request.workspaceId, request.location);
      this.ctx.requireSemanticLibraryRevision(request.policyRevision);
      const root = this.ctx.semanticRootFor(request.location);
      if (root === undefined) {
        throw new MockClientError('notEnrolled', 'The folder is not included');
      }
      if (!root.workspaceReferences.includes(request.workspaceId)) {
        throw new MockClientError('workspaceRequired', 'An active workspace/root is required');
      }
      this.ctx.semanticLibrarySequence += 1;
      const confirmationId = `mock-exclusion-confirmation-${this.ctx.semanticLibrarySequence}`;
      this.ctx.semanticExclusionPlans.set(confirmationId, {
        ...structuredClone(request),
        rootId: root.id,
      });
      return {
        confirmationId,
        policyRevision: request.policyRevision,
        rootId: root.id,
        location: structuredClone(request.location),
        categories: mockCleanupCategories(false),
      };
    });
  }

  confirmSemanticExclusion(
    request: ConfirmSemanticExclusionRequest,
    signal?: AbortSignal,
  ): Promise<SemanticLibraryStatus> {
    return this.ctx.perform('confirmSemanticExclusion', signal, () => {
      this.ctx.requireActiveOrEnrolledSemanticRoot(request.workspaceId, request.location);
      this.ctx.requireSemanticLibraryRevision(request.policyRevision);
      const plan = this.ctx.semanticExclusionPlans.get(request.confirmationId);
      if (
        plan === undefined ||
        plan.policyRevision !== request.policyRevision ||
        plan.workspaceId !== request.workspaceId ||
        plan.location.providerId !== request.location.providerId ||
        plan.location.uri !== request.location.uri
      ) {
        throw new MockClientError(
          'staleConfirmation',
          'Semantic library confirmation is stale or invalid',
        );
      }
      const root = this.ctx.semanticLibrary.roots.find((candidate) => candidate.id === plan.rootId);
      if (root === undefined) throw new MockClientError('notFound', 'Semantic root not found');
      this.ctx.semanticLibrarySequence += 1;
      root.exclusions.push({
        id: `00000000-0000-0000-0001-${String(this.ctx.semanticLibrarySequence).padStart(12, '0')}`,
        location: structuredClone(request.location),
        cleanup: {
          planId: `00000000-0000-0000-0002-${String(this.ctx.semanticLibrarySequence).padStart(12, '0')}`,
          status: 'complete',
          categories: mockCleanupCategories(true),
        },
      });
      this.ctx.semanticExclusionPlans.delete(request.confirmationId);
      this.ctx.advanceSemanticLibraryRevision();
      return structuredClone(this.ctx.semanticLibrary);
    });
  }

  resumeSemanticCleanup(
    request: ResumeSemanticCleanupRequest,
    signal?: AbortSignal,
  ): Promise<SemanticLibraryStatus> {
    return this.ctx.perform('resumeSemanticCleanup', signal, () => {
      this.ctx.requireSemanticLibraryRevision(request.policyRevision);
      const cleanup = this.ctx.semanticLibrary.roots
        .flatMap((root) => root.exclusions)
        .map((exclusion) => exclusion.cleanup)
        .find((candidate) => candidate.planId === request.planId);
      if (cleanup === undefined) throw new MockClientError('notFound', 'Cleanup plan not found');
      cleanup.status = 'complete';
      cleanup.categories = mockCleanupCategories(true);
      this.ctx.advanceSemanticLibraryRevision();
      return structuredClone(this.ctx.semanticLibrary);
    });
  }

  pauseSemanticLibrary(
    request: SemanticLibraryRevisionRequest,
    signal?: AbortSignal,
  ): Promise<SemanticLibraryStatus> {
    return this.ctx.perform('pauseSemanticLibrary', signal, () => {
      this.ctx.requireSemanticLibraryRevision(request.policyRevision);
      this.ctx.semanticLibrary.paused = true;
      this.ctx.advanceSemanticLibraryRevision();
      return structuredClone(this.ctx.semanticLibrary);
    });
  }

  resumeSemanticLibrary(
    request: SemanticLibraryRevisionRequest,
    signal?: AbortSignal,
  ): Promise<SemanticLibraryStatus> {
    return this.ctx.perform('resumeSemanticLibrary', signal, () => {
      this.ctx.requireSemanticLibraryRevision(request.policyRevision);
      this.ctx.semanticLibrary.paused = false;
      this.ctx.advanceSemanticLibraryRevision();
      return structuredClone(this.ctx.semanticLibrary);
    });
  }

  updateSemanticEligibilityOverrides(
    request: UpdateSemanticEligibilityOverridesRequest,
    signal?: AbortSignal,
  ): Promise<SemanticLibraryStatus> {
    return this.ctx.perform('updateSemanticEligibilityOverrides', signal, () => {
      this.ctx.requireSemanticLibraryRevision(request.policyRevision);
      const root = this.ctx.semanticLibrary.roots.find(
        (candidate) => candidate.id === request.rootId,
      );
      if (root === undefined) throw new MockClientError('notFound', 'Semantic root not found');
      if (!root.workspaceReferences.includes(request.workspaceId)) {
        throw new MockClientError('workspaceRequired', 'An active workspace/root is required');
      }
      const safeReasons = new Set([
        'hidden',
        'system',
        'applicationOrPackageBundle',
        'dependencyDirectory',
        'buildDirectory',
        'cacheDirectory',
        'gitIgnored',
      ]);
      if (
        request.overrides.some(
          (override) => override.action === 'include' && !safeReasons.has(override.reason),
        )
      ) {
        throw new MockClientError(
          'unsafeEligibilityOverride',
          'This eligibility reason cannot be overridden',
        );
      }
      root.eligibilityOverrides = structuredClone(request.overrides);
      this.ctx.advanceSemanticLibraryRevision();
      return structuredClone(this.ctx.semanticLibrary);
    });
  }

  listLlmProfilePresets(signal?: AbortSignal): Promise<LlmProfilePreset[]> {
    return this.ctx.perform('listLlmProfilePresets', signal, () => {
      const defaults: Pick<LlmProfilePreset, 'model' | 'advanced' | 'capabilities'> = {
        model: '',
        advanced: {
          contextWindow: 8192,
          maximumAnswerTokens: 1024,
          temperature: 0.2,
          timeoutSeconds: 30,
          tlsPolicy: 'requireValidCertificate' as const,
          customHeaders: {},
        },
        capabilities: ['chatCompletions', 'modelDiscovery'],
      };
      return [
        {
          ...defaults,
          name: 'Ollama',
          preset: 'ollama',
          baseUrl: 'http://127.0.0.1:11434',
          redactFilenames: false,
        },
        {
          ...defaults,
          name: 'LM Studio',
          preset: 'lmStudio',
          baseUrl: 'http://127.0.0.1:1234',
          redactFilenames: false,
        },
        {
          ...defaults,
          name: 'vLLM',
          preset: 'vllm',
          baseUrl: 'http://127.0.0.1:8000',
          redactFilenames: false,
        },
        {
          ...defaults,
          name: 'SGLang',
          preset: 'sglang',
          baseUrl: 'http://127.0.0.1:30000',
          redactFilenames: false,
        },
        {
          ...defaults,
          name: 'OMLX',
          preset: 'omlx',
          baseUrl: 'http://127.0.0.1:8080',
          redactFilenames: false,
        },
        {
          ...defaults,
          name: 'mlx-serve',
          preset: 'mlxServe',
          baseUrl: 'http://127.0.0.1:11234',
          redactFilenames: false,
        },
        {
          ...defaults,
          name: 'OpenAI-compatible',
          preset: 'openAiCompatible',
          baseUrl: 'https://api.openai.com',
          redactFilenames: true,
        },
        {
          ...defaults,
          name: 'Azure OpenAI',
          preset: 'azureOpenAi',
          baseUrl: 'https://example.openai.azure.com',
          apiVersion: '2024-10-21',
          redactFilenames: true,
        },
      ];
    });
  }

  listLlmProfiles(signal?: AbortSignal): Promise<LlmProfile[]> {
    return this.ctx.perform('listLlmProfiles', signal, () =>
      [...this.ctx.llmProfiles.values()]
        .sort(
          (left, right) =>
            Number(right.id === this.ctx.activeLlmProfileId) -
            Number(left.id === this.ctx.activeLlmProfileId),
        )
        .map((profile) => structuredClone(profile)),
    );
  }

  createLlmProfile(request: SaveLlmProfileRequest, signal?: AbortSignal): Promise<LlmProfile> {
    return this.ctx.perform('createLlmProfile', signal, () => {
      this.ctx.llmProfileSequence += 1;
      const host = new URL(request.baseUrl).hostname.toLowerCase();
      const profile: LlmProfile = {
        ...structuredClone(request),
        id: `00000000-0000-4000-8000-${String(this.ctx.llmProfileSequence).padStart(12, '0')}`,
        hasCredential: request.credential != null,
        locality:
          host === 'localhost' || host === '127.0.0.1' || host === '::1' ? 'loopback' : 'cloud',
        consentedHost: null,
      };
      this.ctx.llmProfiles.set(profile.id, profile);
      this.ctx.activeLlmProfileId ??= profile.id;
      return structuredClone(profile);
    });
  }

  updateLlmProfile(
    profileId: string,
    request: SaveLlmProfileRequest,
    signal?: AbortSignal,
  ): Promise<LlmProfile> {
    return this.ctx.perform('updateLlmProfile', signal, () => {
      const current = this.ctx.requireLlmProfile(profileId);
      const host = new URL(request.baseUrl).hostname.toLowerCase();
      const endpoint = new URL(request.baseUrl).href.replace(/\/$/u, '');
      const oldEndpoint = new URL(current.baseUrl).href.replace(/\/$/u, '');
      const profile: LlmProfile = {
        ...structuredClone(request),
        id: profileId,
        hasCredential: request.credential != null || current.hasCredential,
        locality:
          host === 'localhost' || host === '127.0.0.1' || host === '::1' ? 'loopback' : 'cloud',
        consentedHost: endpoint === oldEndpoint ? (current.consentedHost ?? null) : null,
      };
      this.ctx.llmProfiles.set(profileId, profile);
      return structuredClone(profile);
    });
  }

  deleteLlmProfile(
    profileId: string,
    _request: DeleteLlmProfileRequest,
    signal?: AbortSignal,
  ): Promise<void> {
    return this.ctx.perform('deleteLlmProfile', signal, () => {
      this.ctx.requireLlmProfile(profileId);
      this.ctx.llmProfiles.delete(profileId);
      if (this.ctx.activeLlmProfileId === profileId) {
        this.ctx.activeLlmProfileId = this.ctx.llmProfiles.keys().next().value;
      }
    });
  }

  cloneLlmProfile(profileId: string, signal?: AbortSignal): Promise<LlmProfile> {
    return this.ctx.perform('cloneLlmProfile', signal, () => {
      const source = this.ctx.requireLlmProfile(profileId);
      this.ctx.llmProfileSequence += 1;
      const clone: LlmProfile = {
        ...structuredClone(source),
        id: `00000000-0000-4000-8000-${String(this.ctx.llmProfileSequence).padStart(12, '0')}`,
        name: `${source.name} copy`,
        hasCredential: false,
        consentedHost: null,
      };
      this.ctx.llmProfiles.set(clone.id, clone);
      return structuredClone(clone);
    });
  }

  exportLlmProfile(profileId: string, signal?: AbortSignal): Promise<LlmProfileExport> {
    return this.ctx.perform('exportLlmProfile', signal, () => {
      const profile = this.ctx.requireLlmProfile(profileId);
      return structuredClone({
        name: profile.name,
        preset: profile.preset,
        baseUrl: profile.baseUrl,
        deployment: profile.deployment ?? null,
        apiVersion: profile.apiVersion ?? null,
        model: profile.model,
        advanced: profile.advanced,
        capabilities: profile.capabilities,
        redactFilenames: profile.redactFilenames,
      });
    });
  }

  activateLlmProfile(
    profileId: string,
    consent: boolean,
    signal?: AbortSignal,
  ): Promise<LlmProfile> {
    return this.ctx.perform('activateLlmProfile', signal, () => {
      const profile = this.ctx.requireLlmProfile(profileId);
      const endpoint = new URL(profile.baseUrl).href.replace(/\/$/u, '');
      if (profile.locality === 'cloud' && !consent && profile.consentedHost !== endpoint) {
        throw new MockClientError('invalidRequest', 'Cloud consent is required', {
          reason: 'cloudConsentRequired',
          host: new URL(profile.baseUrl).host,
        });
      }
      const activated: LlmProfile = {
        ...profile,
        consentedHost: profile.locality === 'cloud' ? endpoint : null,
      };
      this.ctx.llmProfiles.set(profileId, activated);
      this.ctx.activeLlmProfileId = profileId;
      return structuredClone(activated);
    });
  }

  testLlmProfile(profileId: string, signal?: AbortSignal): Promise<LlmProfileTestResult> {
    return this.ctx.perform('testLlmProfile', signal, () => {
      const profile = this.ctx.requireLlmProfile(profileId);
      const success = profile.model.trim().length > 0;
      const availableModels =
        profile.capabilities.includes('modelDiscovery') && profile.preset !== 'azureOpenAi'
          ? [profile.model, 'model-b']
          : null;
      return {
        profileId,
        provider: profile.preset,
        locality: profile.locality,
        success,
        category: success ? null : 'modelUnavailable',
        durationMs: 1,
        modelAvailable: success,
        availableModels,
        capabilities: [...profile.capabilities],
      };
    });
  }

  discoverLlmProfileModels(profileId: string, signal?: AbortSignal): Promise<string[]> {
    return this.ctx.perform('discoverLlmProfileModels', signal, () => {
      const profile = this.ctx.requireLlmProfile(profileId);
      return profile.capabilities.includes('modelDiscovery') && profile.preset !== 'azureOpenAi'
        ? [profile.model, 'model-b']
        : [];
    });
  }

  discoverLlmProfileDraftModels(
    request: SaveLlmProfileRequest,
    signal?: AbortSignal,
  ): Promise<string[]> {
    return this.ctx.perform('discoverLlmProfileDraftModels', signal, () =>
      request.capabilities.includes('modelDiscovery') && request.preset !== 'azureOpenAi'
        ? ['model-a', 'model-b']
        : [],
    );
  }

  discoverLlmProfileDraftModelLimits(
    request: SaveLlmProfileRequest,
    signal?: AbortSignal,
  ): Promise<LlmModelLimits> {
    return this.ctx.perform('discoverLlmProfileDraftModelLimits', signal, () => ({
      contextWindow: request.model === 'model-a' ? 131_072 : null,
      maximumAnswerTokens: request.model === 'model-a' ? 8_192 : null,
      contextWindowCap: 1_048_576,
      maximumAnswerTokensCap: 16_384,
    }));
  }

  previewDocumentSummary(
    request: PreviewDocumentSummaryRequest,
    signal?: AbortSignal,
  ): Promise<DocumentSummaryPreview> {
    return this.ctx.perform('previewDocumentSummary', signal, () => {
      const profile =
        request.profileId == null ? undefined : this.ctx.requireLlmProfile(request.profileId);
      const visualCount = mockSummaryVisualCount(request.target.location.uri);
      const imageInputAvailable = profile?.preset === 'ollama' && visualCount > 0;
      return {
        selectionFingerprint: `mock-summary-${request.target.entryId}`,
        representativeTokens: Math.min(request.inputTokenBudget, 72),
        sourceTokens: request.target.location.uri.includes('large-report') ? 12_288 : 72,
        textPreview: 'Overview\n\nA representative passage from the selected document.',
        textPreviewTruncated: request.target.location.uri.includes('large-report'),
        selectionMode:
          request.inputTokenBudget > 4_096 && !request.target.location.uri.includes('large-report')
            ? 'fullDocument'
            : 'representativePassages',
        sourcePassCount: request.target.location.uri.includes('large-report') ? 2 : 1,
        effectiveContextWindow: profile?.advanced.contextWindow ?? null,
        providerLimitVerified: profile?.preset === 'ollama',
        imageInputAvailable,
        includedImageCount: request.includeImages && imageInputAvailable ? visualCount : 0,
        omittedImageCount: 0,
        keyPassages: [
          {
            label: 'S1',
            chunkId: `mock-chunk-${request.target.entryId}`,
            content: 'Overview\n\nA representative passage from the selected document.',
            sectionPath: ['Overview'],
            provenance: '{"kind":"textLines","start_line":1,"end_line":3}',
            clusterPopulation: 1,
            weight: 1,
            structuralAnchor: true,
          },
        ],
        profile:
          profile === undefined
            ? null
            : {
                profileId: profile.id,
                profileName: profile.name,
                modelId: profile.model,
                locality: profile.locality,
              },
        reusedSelection: this.ctx.documentSummaries.has(request.target.entryId),
      };
    });
  }

  generateDocumentSummary(
    request: GenerateDocumentSummaryRequest,
    signal?: AbortSignal,
  ): Promise<DocumentSummary> {
    return this.ctx.perform('generateDocumentSummary', signal, () => {
      const profile = this.ctx.requireLlmProfile(request.profileId);
      const expected = `mock-summary-${request.target.entryId}`;
      if (request.expectedSelectionFingerprint !== expected) {
        throw new MockClientError('invalidRequest', 'Document summary confirmation is stale', {
          reason: 'summaryConfirmationStale',
        });
      }
      const summary: DocumentSummary = {
        recordId: `mock-generated-${request.target.entryId}`,
        sourceGeneration: 1,
        profileId: profile.id,
        modelId: profile.model,
        supportingChunkIds: [`mock-chunk-${request.target.entryId}`],
        supportingWeights: [1],
        createdAtMs: Date.now(),
        brief: 'A concise document summary.',
        full: 'A fuller summary grounded in the selected document evidence.',
        stale: false,
      };
      this.ctx.documentSummaries.set(request.target.entryId, summary);
      return structuredClone(summary);
    });
  }

  getDocumentSummary(
    request: GetDocumentSummaryRequest,
    signal?: AbortSignal,
  ): Promise<DocumentSummary | null> {
    return this.ctx.perform('getDocumentSummary', signal, () => {
      const summary = this.ctx.documentSummaries.get(request.target.entryId);
      return summary === undefined ? null : structuredClone(summary);
    });
  }

  getCorpusThemeStatus(
    _request: GetCorpusThemeStatusRequest,
    signal?: AbortSignal,
  ): Promise<CorpusThemeStatus> {
    return this.ctx.perform('getCorpusThemeStatus', signal, () => mockCorpusThemeStatus());
  }

  analyzeCorpusThemes(
    _request: AnalyzeCorpusThemesRequest,
    signal?: AbortSignal,
  ): Promise<CorpusThemesResponse> {
    return this.ctx.perform('analyzeCorpusThemes', signal, () => ({
      outcome: 'ready',
      modelId: 'mock-embedding-model',
      themes: mockCorpusThemes(),
    }));
  }

  getCorpusThemeSources(
    request: GetCorpusThemeSourcesRequest,
    signal?: AbortSignal,
  ): Promise<CorpusThemeSources> {
    return this.ctx.perform('getCorpusThemeSources', signal, () => {
      const themes = mockCorpusThemes();
      if (request.fingerprint !== themes.fingerprint) {
        throw new MockClientError('invalidRequest', 'the corpus themes are out of date', {
          reason: 'corpusThemesStale',
        });
      }
      const sourceIds = themes.documentMixtures
        .filter((mixture) =>
          mixture.shares.some((share) => (share.theme ?? null) === (request.theme ?? null)),
        )
        .map((mixture) => mixture.sourceId);
      return { sourceIds, truncated: false };
    });
  }

  previewRag(request: PreviewRagRequest, signal?: AbortSignal): Promise<RagPreview> {
    return this.ctx.perform('previewRag', signal, () => {
      const requestedStrategy = request.retrievalStrategy ?? 'singleQuery';
      const plannedQueries =
        requestedStrategy === 'multiQuery'
          ? [
              request.question,
              `${request.question} key facts`,
              `${request.question} supporting details`,
            ]
          : [request.question];
      return {
        appliedStrategy: requestedStrategy,
        coverage: {
          eligible: 3,
          indexed: 3,
          pending: 0,
          failed: 0,
          excluded: 0,
          stale: 0,
          unavailable: 0,
        },
        evidence: [
          {
            available: true,
            excerpt: `Mock indexed evidence relevant to "${request.question}".`,
            generated: false,
            label: 'C1',
            sourceId: 'mock-source-1',
            provenance: 'section 1',
            score: 0.91,
            sectionPath: ['Overview'],
            stale: false,
            title: 'Example indexed document',
          },
        ],
        evidenceTokens: 18,
        fallbackReason: null,
        fusionVersion: requestedStrategy === 'multiQuery' ? 'reciprocal-rank-fusion/1' : null,
        insufficient: false,
        locality: 'loopback',
        plannedQueries,
        plannerVersion: requestedStrategy === 'multiQuery' ? 'grounded-rag-query-planner/1' : null,
        profileId: request.profileId,
        profileName: 'Local mock profile',
        requestedStrategy,
        retrievalFingerprint: `mock-rag-${request.profileId}-${requestedStrategy}-${request.question}`,
        scope: structuredClone(request.scope),
      };
    });
  }

  generateRagAnswer(
    request: GenerateRagAnswerRequest,
    signal?: AbortSignal,
  ): Promise<GenerateRagAnswerResponse> {
    return this.ctx.perform('generateRagAnswer', signal, () => {
      const requestedStrategy = request.retrievalStrategy ?? 'singleQuery';
      const expectedFingerprint = `mock-rag-${request.profileId}-${requestedStrategy}-${request.question}`;
      if (request.expectedRetrievalFingerprint !== expectedFingerprint) {
        throw new MockClientError(
          'knowledgeEvidenceRefreshRequired',
          'Ask evidence confirmation is stale',
        );
      }
      const preview: RagPreview = {
        appliedStrategy: requestedStrategy,
        coverage: {
          eligible: 3,
          indexed: 3,
          pending: 0,
          failed: 0,
          excluded: 0,
          stale: 0,
          unavailable: 0,
        },
        evidence: [
          {
            available: true,
            excerpt: `Mock indexed evidence relevant to "${request.question}".`,
            generated: false,
            label: 'C1',
            sourceId: 'mock-source-1',
            provenance: 'section 1',
            score: 0.91,
            sectionPath: ['Overview'],
            stale: false,
            title: 'Example indexed document',
          },
        ],
        evidenceTokens: 18,
        fallbackReason: null,
        fusionVersion: requestedStrategy === 'multiQuery' ? 'reciprocal-rank-fusion/1' : null,
        insufficient: false,
        locality: 'loopback',
        plannedQueries:
          requestedStrategy === 'multiQuery'
            ? [
                request.question,
                `${request.question} key facts`,
                `${request.question} supporting details`,
              ]
            : [request.question],
        plannerVersion: requestedStrategy === 'multiQuery' ? 'grounded-rag-query-planner/1' : null,
        profileId: request.profileId,
        profileName: 'Local mock profile',
        requestedStrategy,
        retrievalFingerprint: expectedFingerprint,
        scope: structuredClone(request.scope),
      };
      const answer: RagAnswer = {
        text: 'This answer is grounded in the selected local evidence [C1].',
        citations: [
          {
            generated: false,
            label: 'C1',
            provenance: 'section 1',
            sourceId: 'mock-source-1',
            stale: false,
            unavailable: false,
          },
        ],
        modelKnowledgeAllowed: request.allowModelKnowledge,
      };
      const conversationId =
        request.conversationId ?? `rag-${this.ctx.ephemeralRagConversations.size + 1}`;
      const existing = this.ctx.ephemeralRagConversations.get(conversationId);
      if (
        existing !== undefined &&
        (existing.profileId !== request.profileId ||
          existing.scope.kind !== request.scope.kind ||
          existing.modelKnowledgeAllowed !== request.allowModelKnowledge ||
          existing.retrievalStrategy !== requestedStrategy)
      ) {
        throw new MockClientError(
          'invalidRequest',
          'Conversation profile, scope, and knowledge mode cannot change',
          { reason: 'conversationImmutable' },
        );
      }
      this.ctx.ephemeralRagConversations.set(conversationId, {
        id: conversationId,
        modelKnowledgeAllowed: request.allowModelKnowledge,
        profileId: request.profileId,
        retrievalStrategy: requestedStrategy,
        scope: structuredClone(request.scope),
        storageBytes: 0,
        turns: [...(existing?.turns ?? []), { question: request.question, answer }],
      });
      return {
        conversationId,
        events: [
          { type: 'retrieval', preview },
          { type: 'token', text: answer.text },
          { type: 'done', answer },
        ],
      };
    });
  }

  saveRagConversation(
    request: SaveRagConversationRequest,
    signal?: AbortSignal,
  ): Promise<SavedRagConversation> {
    return this.ctx.perform('saveRagConversation', signal, () => {
      const conversation = this.ctx.ephemeralRagConversations.get(request.conversationId);
      if (conversation === undefined || conversation.scope.workspaceId !== request.workspaceId) {
        throw new MockClientError('notFound', 'Ask conversation not found');
      }
      const saved = {
        ...structuredClone(conversation),
        storageBytes: JSON.stringify(conversation).length,
      };
      this.ctx.ragConversations.set(saved.id, saved);
      return structuredClone(saved);
    });
  }

  listSavedRagConversations(
    workspaceId: WorkspaceId,
    signal?: AbortSignal,
  ): Promise<SavedRagConversation[]> {
    return this.ctx.perform('listSavedRagConversations', signal, () =>
      [...this.ctx.ragConversations.values()]
        .filter((conversation) => conversation.scope.workspaceId === workspaceId)
        .map((conversation) => structuredClone(conversation)),
    );
  }

  deleteRagConversation(
    request: DeleteRagConversationRequest,
    signal?: AbortSignal,
  ): Promise<void> {
    return this.ctx.perform('deleteRagConversation', signal, () => {
      const conversation = this.ctx.ragConversations.get(request.conversationId);
      if (conversation === undefined) {
        throw new MockClientError('notFound', 'Ask conversation not found');
      }
      this.ctx.ragConversations.delete(request.conversationId);
    });
  }

  resolveRagCitation(
    request: ResolveRagCitationRequest,
    signal?: AbortSignal,
  ): Promise<ResolvedRagCitation> {
    return this.ctx.perform('resolveRagCitation', signal, () => {
      if (request.sourceId !== 'mock-source-1') {
        throw new MockClientError('notFound', 'Citation not found');
      }
      return {
        entryId: '11111111-1111-4111-8111-111111111111',
        location: { providerId: 'local', uri: 'file:///documents/report.txt' },
        available: true,
      };
    });
  }

  /**
   * Reports full-text retrieval, no query embeddings, and answer generation
   * only when this host has a saved generation profile (task 0207). Search
   * never depends on the answer capability.
   */
  getKnowledgeCapabilities(signal?: AbortSignal): Promise<KnowledgeCapabilities> {
    return this.ctx.perform('getKnowledgeCapabilities', signal, () =>
      mockKnowledgeCapabilities(this.ctx.llmProfiles.size > 0),
    );
  }

  listKnowledgeRoots(
    request: ListKnowledgeRootsRequest,
    signal?: AbortSignal,
  ): Promise<KnowledgeRoot[]> {
    return this.ctx.perform('listKnowledgeRoots', signal, () => {
      if (request.workspaceId.length === 0) {
        throw new MockClientError('workspaceRequired', 'Knowledge roots require a workspace');
      }
      return mockKnowledgeRoots();
    });
  }

  parseKnowledgeQuery(
    request: ParseKnowledgeQueryRequest,
    signal?: AbortSignal,
  ): Promise<KnowledgeQueryInterpretation> {
    return this.ctx.perform('parseKnowledgeQuery', signal, () =>
      parseMockKnowledgeQuery(request.text),
    );
  }

  planKnowledgeSearch(
    request: PlanKnowledgeSearchRequest,
    signal?: AbortSignal,
  ): Promise<KnowledgeSearchPlan> {
    return this.ctx.perform('planKnowledgeSearch', signal, () => this.ctx.knowledgePlan(request));
  }

  /**
   * Runs the deterministic no-LLM search. Cancellation is honoured both ways:
   * an aborted `signal` rejects, and a `cancelKnowledgeSearch` for the same
   * `requestId` (which is how the Tauri host cancels) makes the in-flight
   * search reject with the same `AbortError`.
   *
   * An already-aborted signal rejects before anything else is inspected, so a
   * cancelled request never reports a capability or validation failure the
   * caller did not actually wait for - the same order the desktop host uses.
   */
  async executeKnowledgeSearch(
    request: ExecuteKnowledgeSearchRequest,
    signal?: AbortSignal,
  ): Promise<KnowledgeSearchResult> {
    if (signal?.aborted === true) {
      throw new DOMException('The operation was aborted.', 'AbortError');
    }
    if (mockKnowledgeRouteUnavailable(request.mode ?? 'hybrid')) {
      throw new MockClientError('unavailable', 'Semantic retrieval is unavailable');
    }
    const controller = new AbortController();
    const forward = (): void => controller.abort();
    signal?.addEventListener('abort', forward, { once: true });
    this.ctx.knowledgeSearches.set(request.requestId, controller);
    try {
      const result = await this.ctx.perform('executeKnowledgeSearch', controller.signal, () =>
        executeMockKnowledgeSearch(
          request.requestId,
          this.ctx.knowledgePlan(request),
          this.ctx.llmProfiles.size > 0,
        ),
      );
      if (controller.signal.aborted) {
        throw new DOMException('The operation was aborted.', 'AbortError');
      }
      // Retaining the displayed set is the only bridge to an optional answer:
      // answering later reads this, never a fresh retrieval (task 0207).
      this.ctx.knowledgeEvidence.record(result.evidenceFingerprint, {
        workspaceId: request.scope.workspaceId,
        evidence: result.evidence.map((row) => structuredClone(row)),
      });
      return result;
    } finally {
      signal?.removeEventListener('abort', forward);
      this.ctx.knowledgeSearches.delete(request.requestId);
    }
  }

  /**
   * Cancellation is applied immediately rather than behind the simulated
   * latency, because a cancel that arrives after the work it cancels would
   * never be observable - the desktop host registers the same `requestId` with
   * its coordinator before the search starts.
   */
  cancelKnowledgeSearch(
    request: CancelKnowledgeSearchRequest,
    signal?: AbortSignal,
  ): Promise<void> {
    this.ctx.knowledgeSearches.get(request.requestId)?.abort();
    this.ctx.knowledgeSearches.delete(request.requestId);
    return this.ctx.perform('cancelKnowledgeSearch', signal, () => undefined);
  }

  resolveKnowledgeSource(
    request: ResolveKnowledgeSourceRequest,
    signal?: AbortSignal,
  ): Promise<KnowledgeSourceLocation> {
    return this.ctx.perform('resolveKnowledgeSource', signal, () => {
      if (request.workspaceId.length === 0) {
        throw new MockClientError('workspaceRequired', 'Knowledge sources require a workspace');
      }
      const resolved = resolveMockKnowledgeSource(request.sourceId);
      if (resolved === undefined) {
        throw new MockClientError('notFound', 'Knowledge source not found');
      }
      return resolved;
    });
  }

  /**
   * Answers from an evidence set an earlier search already displayed (task
   * 0207). Nothing here retrieves: an unknown, evicted, or differently
   * authorized fingerprint is refused with `knowledgeEvidenceRefreshRequired`,
   * so the user must run Search again rather than have retrieval reappear.
   *
   * Cancellation is honoured both ways, exactly like a knowledge search: an
   * aborted `signal` rejects, and a `cancelKnowledgeAnswer` for the same
   * `requestId` (which is how the desktop host cancels) makes the in-flight
   * generation reject with the same `AbortError`.
   */
  async generateKnowledgeAnswer(
    request: GenerateKnowledgeAnswerRequest,
    signal?: AbortSignal,
  ): Promise<KnowledgeAnswer> {
    if (signal?.aborted === true) {
      throw new DOMException('The operation was aborted.', 'AbortError');
    }
    const controller = new AbortController();
    const forward = (): void => controller.abort();
    signal?.addEventListener('abort', forward, { once: true });
    this.ctx.knowledgeAnswers.set(request.requestId, controller);
    try {
      const answer = await this.ctx.perform('generateKnowledgeAnswer', controller.signal, () => {
        const retained = this.ctx.knowledgeEvidence.get(
          request.evidenceFingerprint,
          request.workspaceId,
        );
        if (retained === undefined) {
          throw new MockClientError(
            'knowledgeEvidenceRefreshRequired',
            'The inspected evidence set is no longer available; search again to answer.',
          );
        }
        const profile = this.ctx.llmProfiles.get(request.profileId);
        if (profile === undefined) {
          throw new MockClientError('unavailable', 'No generation profile is configured');
        }
        return buildMockKnowledgeAnswer({
          requestId: request.requestId,
          evidenceFingerprint: request.evidenceFingerprint,
          profileId: profile.id,
          profileName: profile.name,
          locality: profile.locality,
          allowModelKnowledge: request.allowModelKnowledge ?? false,
          evidence: retained.evidence,
        });
      });
      if (controller.signal.aborted) {
        throw new DOMException('The operation was aborted.', 'AbortError');
      }
      return answer;
    } finally {
      signal?.removeEventListener('abort', forward);
      this.ctx.knowledgeAnswers.delete(request.requestId);
    }
  }

  /**
   * Cancellation is applied immediately rather than behind the simulated
   * latency, mirroring {@link cancelKnowledgeSearch}: the desktop host
   * registers the same `requestId` before generation starts.
   */
  cancelKnowledgeAnswer(
    request: CancelKnowledgeAnswerRequest,
    signal?: AbortSignal,
  ): Promise<void> {
    this.ctx.knowledgeAnswers.get(request.requestId)?.abort();
    this.ctx.knowledgeAnswers.delete(request.requestId);
    return this.ctx.perform('cancelKnowledgeAnswer', signal, () => undefined);
  }
}
