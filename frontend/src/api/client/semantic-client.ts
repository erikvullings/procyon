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
  ExportCorpusThemesRequest,
  ExportCorpusThemesResponse,
  FileAssistantEvent,
  FileAssistantRequest,
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
  LlmRole,
  LlmRoles,
  MoveSemanticDataRequest,
  ParseKnowledgeQueryRequest,
  PlanKnowledgeSearchRequest,
  PlanSemanticExclusionRequest,
  PlanSemanticModelMigrationRequest,
  PreviewDocumentSummaryRequest,
  PreviewRagRequest,
  PreviewSemanticEnrolmentRequest,
  RagPreview,
  ResolvedRagCitation,
  ResolveKnowledgeSourceRequest,
  ResolveRagCitationRequest,
  ResumeSemanticCleanupRequest,
  ReviewConceptCandidateRequest,
  SavedRagConversation,
  SaveLlmProfileRequest,
  SaveLlmRoleRequest,
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
  SemanticUninstallReceipt,
  SemanticVocabulary,
  SemanticWorkerPatchResponse,
  StartSemanticOcrRemediationRequest,
  UninstallSemanticComponentsRequest,
  UpdateGemmaMediaRequest,
  UpdateSemanticEligibilityOverridesRequest,
  UpdateSemanticRootWorkspacesRequest,
  WorkspaceId,
} from '../../models';

/**
 * Semantic components, library, vocabularies, OCR, LLM profiles, corpus, RAG and knowledge search.
 */
export interface SemanticClient {
  acceptSemanticComponentInstallationOffer(
    request: AcceptSemanticInstallationOfferRequest,
    signal?: AbortSignal,
  ): Promise<SemanticInstallReceipt>;

  /** Binds a role to an endpoint, model and cloud permission; the result is all role bindings. */
  setLlmRole(role: LlmRole, request: SaveLlmRoleRequest, signal?: AbortSignal): Promise<LlmRoles>;
  clearLlmRole(role: LlmRole, signal?: AbortSignal): Promise<LlmRoles>;
  listLlmRoles(signal?: AbortSignal): Promise<LlmRoles>;
  /** Records consent for the profile's current cloud host, from the endpoint editor. */
  grantLlmProfileConsent(profileId: string, signal?: AbortSignal): Promise<LlmProfile>;
  activateLlmProfile(
    profileId: string,
    consent: boolean,
    signal?: AbortSignal,
  ): Promise<LlmProfile>;

  /** Discovers labelled themes across the authorized chunks of a scope. */
  analyzeCorpusThemes(
    request: AnalyzeCorpusThemesRequest,
    signal?: AbortSignal,
  ): Promise<CorpusThemesResponse>;

  /**
   * Exports the themes of a scope to a file written through the destination provider
   * (Markdown, Word or SKOS). Passages and document titles are included only on request.
   */
  exportCorpusThemes(
    request: ExportCorpusThemesRequest,
    signal?: AbortSignal,
  ): Promise<ExportCorpusThemesResponse>;

  attachSemanticVocabulary(
    request: AttachSemanticVocabularyRequest,
    signal?: AbortSignal,
  ): Promise<SemanticVocabulary>;

  /** Cancels a running or not-yet-started knowledge answer by request id. */
  cancelKnowledgeAnswer(request: CancelKnowledgeAnswerRequest, signal?: AbortSignal): Promise<void>;

  /** Cancels a running or not-yet-started knowledge search by request id. */
  cancelKnowledgeSearch(request: CancelKnowledgeSearchRequest, signal?: AbortSignal): Promise<void>;

  cancelSemanticOcrRemediation(jobId: string, signal?: AbortSignal): Promise<SemanticOcrJob>;

  checkpointSemanticComponentModelMigration(
    request: CheckpointSemanticModelMigrationRequest,
    signal?: AbortSignal,
  ): Promise<SemanticModelMigrationProgress>;

  cloneLlmProfile(profileId: string, signal?: AbortSignal): Promise<LlmProfile>;

  completeSemanticComponentModelMigration(
    request: CompleteSemanticModelMigrationRequest,
    signal?: AbortSignal,
  ): Promise<SemanticModelSelection>;

  confirmSemanticComponentIndexRemoval(
    request: ConfirmSemanticIndexRemovalRequest,
    signal?: AbortSignal,
  ): Promise<SemanticIndexRemovalReceipt>;

  confirmSemanticComponentModelMigration(
    request: ConfirmSemanticModelMigrationRequest,
    signal?: AbortSignal,
  ): Promise<SemanticModelMigrationProgress>;

  confirmSemanticEnrolment(
    request: ConfirmSemanticEnrolmentRequest,
    signal?: AbortSignal,
  ): Promise<SemanticLibraryStatus>;

  confirmSemanticExclusion(
    request: ConfirmSemanticExclusionRequest,
    signal?: AbortSignal,
  ): Promise<SemanticLibraryStatus>;

  createLlmProfile(request: SaveLlmProfileRequest, signal?: AbortSignal): Promise<LlmProfile>;

  createSemanticComponentIndexRemovalPlan(
    request: CreateSemanticIndexRemovalPlanRequest,
    signal?: AbortSignal,
  ): Promise<SemanticIndexRemovalPlan>;

  createSemanticComponentInstallationOffer(
    request: CreateSemanticInstallationOfferRequest,
    signal?: AbortSignal,
  ): Promise<SemanticInstallationOffer>;

  deleteLlmProfile(
    profileId: string,
    request: DeleteLlmProfileRequest,
    signal?: AbortSignal,
  ): Promise<void>;

  deleteRagConversation(request: DeleteRagConversationRequest, signal?: AbortSignal): Promise<void>;

  deleteSemanticVocabulary(
    vocabularyId: string,
    confirmAffected: boolean,
    signal?: AbortSignal,
  ): Promise<DeleteSemanticVocabularyImpact>;

  discoverLlmProfileDraftModelLimits(
    request: SaveLlmProfileRequest,
    signal?: AbortSignal,
  ): Promise<LlmModelLimits>;

  discoverLlmProfileDraftModels(
    request: SaveLlmProfileRequest,
    signal?: AbortSignal,
  ): Promise<string[]>;

  discoverLlmProfileModels(profileId: string, signal?: AbortSignal): Promise<string[]>;

  /**
   * Executes one search-only knowledge retrieval. Aborting `signal` must also
   * cancel the backend search identified by `request.requestId`, so both hosts
   * stop the same work rather than only dropping the response.
   */
  executeKnowledgeSearch(
    request: ExecuteKnowledgeSearchRequest,
    signal?: AbortSignal,
  ): Promise<KnowledgeSearchResult>;

  exportLlmProfile(profileId: string, signal?: AbortSignal): Promise<LlmProfileExport>;

  exportSemanticVocabulary(vocabularyId: string, signal?: AbortSignal): Promise<string>;

  generateDocumentSummary(
    request: GenerateDocumentSummaryRequest,
    signal?: AbortSignal,
  ): Promise<DocumentSummary>;

  /**
   * Generates one optional answer over an evidence set an earlier successful
   * search already displayed (task 0207).
   *
   * This never retrieves: the evidence is addressed by the fingerprint that
   * search returned, and a host that no longer retains it must fail with the
   * typed `knowledgeEvidenceRefreshRequired` code rather than searching again.
   * Aborting `signal` must also cancel the backend generation identified by
   * `request.requestId`, exactly as `executeKnowledgeSearch` does.
   */
  generateKnowledgeAnswer(
    request: GenerateKnowledgeAnswerRequest,
    signal?: AbortSignal,
  ): Promise<KnowledgeAnswer>;

  generateRagAnswer(
    request: GenerateRagAnswerRequest,
    signal?: AbortSignal,
  ): Promise<GenerateRagAnswerResponse>;

  /** Lists the authorized sources contributing to one theme, to scope a follow-up search. */
  getCorpusThemeSources(
    request: GetCorpusThemeSourcesRequest,
    signal?: AbortSignal,
  ): Promise<CorpusThemeSources>;

  /** Reports whether themes are meaningful for a scope and its current index revision. */
  getCorpusThemeStatus(
    request: GetCorpusThemeStatusRequest,
    signal?: AbortSignal,
  ): Promise<CorpusThemeStatus>;

  getDocumentSummary(
    request: GetDocumentSummaryRequest,
    signal?: AbortSignal,
  ): Promise<DocumentSummary | null>;

  getGemmaLibrarySetup(
    signal?: AbortSignal,
  ): Promise<import('../../models/semantic-components').GemmaLibrarySetup | null>;

  /**
   * Reports full-text, semantic and answer-generation availability
   * independently (task 0206, ADR 0012). Knowledge search stays usable when
   * `answerGeneration` is `false`; hosts must never gate search on it.
   */
  getKnowledgeCapabilities(signal?: AbortSignal): Promise<KnowledgeCapabilities>;

  getSemanticComponentCapabilities(signal?: AbortSignal): Promise<SemanticComponentCapabilities>;

  getSemanticComponentStatus(signal?: AbortSignal): Promise<SemanticComponentStatus>;

  getSemanticFolderStatus(
    request: GetSemanticFolderStatusRequest,
    signal?: AbortSignal,
  ): Promise<SemanticFolderStatus>;

  getSemanticLibraryCapabilities(signal?: AbortSignal): Promise<SemanticLibraryCapabilities>;

  getSemanticLibraryStatus(signal?: AbortSignal): Promise<SemanticLibraryStatus>;

  getSemanticOcrStatus(signal?: AbortSignal): Promise<SemanticOcrStatus>;

  importSemanticComponentLocalModel(
    request: ImportSemanticLocalModelRequest,
    signal?: AbortSignal,
  ): Promise<SemanticModelMigrationPlan>;

  importSemanticVocabulary(skosJson: string, signal?: AbortSignal): Promise<SemanticVocabulary>;

  initializeGemmaLibrary(
    request: import('../../models/semantic-components').InitializeGemmaLibraryRequest,
    signal?: AbortSignal,
  ): Promise<void>;

  installSemanticComponentWorkerPatch(
    request: InstallSemanticWorkerPatchRequest,
    signal?: AbortSignal,
  ): Promise<SemanticWorkerPatchResponse>;

  /** Lists the indexed roots a knowledge search may be scoped to. */
  listKnowledgeRoots(
    request: ListKnowledgeRootsRequest,
    signal?: AbortSignal,
  ): Promise<KnowledgeRoot[]>;

  listLlmProfilePresets(signal?: AbortSignal): Promise<LlmProfilePreset[]>;

  listLlmProfiles(signal?: AbortSignal): Promise<LlmProfile[]>;

  /** Streams the next reply: `onEvent` gets fragments, then one `done` or `failed`. */
  streamFileAssistantReply(
    request: FileAssistantRequest,
    onEvent: (event: FileAssistantEvent) => void,
    signal?: AbortSignal,
  ): Promise<void>;

  /** Stops a running file-assistant reply; events that follow are not delivered to the caller. */
  cancelFileAssistantReply(requestId: string): Promise<void>;

  listSavedRagConversations(
    workspaceId: WorkspaceId,
    signal?: AbortSignal,
  ): Promise<SavedRagConversation[]>;

  listSemanticComponentProfiles(signal?: AbortSignal): Promise<SemanticModelProfile[]>;

  listSemanticVocabularies(signal?: AbortSignal): Promise<readonly SemanticVocabulary[]>;

  moveSemanticComponentData(
    request: MoveSemanticDataRequest,
    signal?: AbortSignal,
  ): Promise<SemanticDataMoveReceipt>;

  /**
   * Interprets composer or DSL text deterministically. The text is never sent
   * to an LLM, and parsing never resolves authorization.
   */
  parseKnowledgeQuery(
    request: ParseKnowledgeQueryRequest,
    signal?: AbortSignal,
  ): Promise<KnowledgeQueryInterpretation>;

  pauseSemanticComponentIndexing(signal?: AbortSignal): Promise<void>;

  pauseSemanticLibrary(
    request: SemanticLibraryRevisionRequest,
    signal?: AbortSignal,
  ): Promise<SemanticLibraryStatus>;

  /** Builds the deterministic retrieval plan without executing it. */
  planKnowledgeSearch(
    request: PlanKnowledgeSearchRequest,
    signal?: AbortSignal,
  ): Promise<KnowledgeSearchPlan>;

  planSemanticComponentModelMigration(
    request: PlanSemanticModelMigrationRequest,
    signal?: AbortSignal,
  ): Promise<SemanticModelMigrationPlan>;

  planSemanticExclusion(
    request: PlanSemanticExclusionRequest,
    signal?: AbortSignal,
  ): Promise<SemanticExclusionPlan>;

  previewDocumentSummary(
    request: PreviewDocumentSummaryRequest,
    signal?: AbortSignal,
  ): Promise<DocumentSummaryPreview>;

  previewRag(request: PreviewRagRequest, signal?: AbortSignal): Promise<RagPreview>;

  previewSemanticEnrolment(
    request: PreviewSemanticEnrolmentRequest,
    signal?: AbortSignal,
  ): Promise<SemanticEnrolmentPreview>;

  /** Resolves one opaque evidence source into an exact navigable location. */
  resolveKnowledgeSource(
    request: ResolveKnowledgeSourceRequest,
    signal?: AbortSignal,
  ): Promise<KnowledgeSourceLocation>;

  resolveRagCitation(
    request: ResolveRagCitationRequest,
    signal?: AbortSignal,
  ): Promise<ResolvedRagCitation>;

  resumeSemanticCleanup(
    request: ResumeSemanticCleanupRequest,
    signal?: AbortSignal,
  ): Promise<SemanticLibraryStatus>;

  resumeSemanticComponentIndexing(signal?: AbortSignal): Promise<void>;

  resumeSemanticLibrary(
    request: SemanticLibraryRevisionRequest,
    signal?: AbortSignal,
  ): Promise<SemanticLibraryStatus>;

  reviewSemanticConceptCandidate(
    request: ReviewConceptCandidateRequest,
    signal?: AbortSignal,
  ): Promise<SemanticVocabulary>;

  saveRagConversation(
    request: SaveRagConversationRequest,
    signal?: AbortSignal,
  ): Promise<SavedRagConversation>;

  setSemanticOcrConsent(enabled: boolean, signal?: AbortSignal): Promise<SemanticOcrStatus>;

  setSemanticOcrReplaceOriginal(enabled: boolean, signal?: AbortSignal): Promise<SemanticOcrStatus>;

  startSemanticOcrRemediation(
    request: StartSemanticOcrRemediationRequest,
    signal?: AbortSignal,
  ): Promise<SemanticOcrJob>;

  testLlmProfile(profileId: string, signal?: AbortSignal): Promise<LlmProfileTestResult>;

  uninstallSemanticComponents(
    request: UninstallSemanticComponentsRequest,
    signal?: AbortSignal,
  ): Promise<SemanticUninstallReceipt>;

  updateGemmaMedia(
    request: UpdateGemmaMediaRequest,
    signal?: AbortSignal,
  ): Promise<SemanticLibraryStatus>;

  updateLlmProfile(
    profileId: string,
    request: SaveLlmProfileRequest,
    signal?: AbortSignal,
  ): Promise<LlmProfile>;

  updateSemanticEligibilityOverrides(
    request: UpdateSemanticEligibilityOverridesRequest,
    signal?: AbortSignal,
  ): Promise<SemanticLibraryStatus>;

  /** Chooses the workspaces a folder is available in; none selected means all workspaces. */
  updateSemanticRootWorkspaces(
    request: UpdateSemanticRootWorkspacesRequest,
    signal?: AbortSignal,
  ): Promise<SemanticLibraryStatus>;
}
