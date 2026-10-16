import m, { type FactoryComponent, type Vnode } from 'mithril';

import type { FileManagerClient } from '../../api/client/file-manager-client';
import { t } from '../../i18n';
import type {
  KnowledgeRoot,
  Location,
  SemanticDeletionCategory,
  SemanticEligibilityOverride,
  SemanticEligibilityReason,
  SemanticEnrolmentPreview,
  SemanticExclusionPlan,
  SemanticFolderConsent,
  SemanticFolderStatus,
  SemanticLibraryCapabilities,
  SemanticLibraryStatus,
  SemanticRootStatus,
  WorkspaceId,
} from '../../models';

export interface SemanticLibraryManagementAttrs {
  readonly client: FileManagerClient;
  readonly workspaceId?: WorkspaceId;
  readonly location?: Location;
}

export interface SemanticFolderEnrolmentPromptAttrs {
  readonly client: FileManagerClient;
  readonly workspaceId: WorkspaceId;
  readonly location: Location;
  readonly onEnrolled: () => void;
  readonly onCancel: () => void;
}

type LoadState = 'loading' | 'loaded' | 'error';
type BusyAction =
  | 'preview'
  | 'enrol'
  | 'planExclusion'
  | 'confirmExclusion'
  | 'pause'
  | 'resume'
  | 'cleanup'
  | 'override'
  | 'media';

interface PdfImageConsentOption {
  readonly enabled: boolean;
  readonly pageBudget: number | null;
  readonly onChange: (enabled: boolean) => void;
}

const SAFE_OVERRIDE_REASONS = [
  'hidden',
  'system',
  'applicationOrPackageBundle',
  'dependencyDirectory',
  'buildDirectory',
  'cacheDirectory',
  'gitIgnored',
] as const satisfies readonly SemanticEligibilityReason[];

const GEMMA_MEDIA_LABELS = {
  images: 'gemmaImages',
  audio: 'gemmaAudio',
  video: 'gemmaVideo',
} as const;

const CPU_PDF_IMAGE_SECONDS_PER_PAGE = 107;

function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (
    typeof error === 'object' &&
    error !== null &&
    'message' in error &&
    typeof error.message === 'string'
  ) {
    return error.message;
  }
  return t('semanticLibrary', 'unknownError');
}

function formatBytes(bytes: number): string {
  const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB'] as const;
  let value = bytes;
  let unit = 0;
  while (value >= 1_024 && unit < units.length - 1) {
    value /= 1_024;
    unit += 1;
  }
  return `${new Intl.NumberFormat(undefined, { maximumFractionDigits: 1 }).format(value)} ${units[unit]}`;
}

function consentLabel(consent: SemanticFolderConsent): string {
  switch (consent) {
    case 'includedHere':
      return t('semanticLibrary', 'includedHere');
    case 'inheritedFromParent':
      return t('semanticLibrary', 'inheritedFromParent');
    case 'excluded':
      return t('semanticLibrary', 'excluded');
    case 'notIncluded':
      return t('semanticLibrary', 'notIncluded');
  }
}

function reasonLabel(reason: SemanticEligibilityReason): string {
  switch (reason) {
    case 'hidden':
      return t('semanticLibrary', 'reasonHidden');
    case 'system':
      return t('semanticLibrary', 'reasonSystem');
    case 'applicationOrPackageBundle':
      return t('semanticLibrary', 'reasonBundle');
    case 'dependencyDirectory':
      return t('semanticLibrary', 'reasonDependency');
    case 'buildDirectory':
      return t('semanticLibrary', 'reasonBuild');
    case 'cacheDirectory':
      return t('semanticLibrary', 'reasonCache');
    case 'gitIgnored':
      return t('semanticLibrary', 'reasonGitIgnored');
    case 'unsupportedMime':
      return t('semanticLibrary', 'reasonUnsupportedMime');
    case 'oversized':
      return t('semanticLibrary', 'reasonOversized');
    case 'overBudget':
      return t('semanticLibrary', 'reasonOverBudget');
    case 'symlinkOutsideRoot':
      return t('semanticLibrary', 'reasonSymlinkOutside');
    case 'explicitlyExcluded':
      return t('semanticLibrary', 'reasonExplicitlyExcluded');
  }
}

function decodedLocation(uri: string): string {
  try {
    return decodeURIComponent(uri);
  } catch {
    return uri;
  }
}

function deletionCategoryLabel(category: SemanticDeletionCategory): string {
  switch (category) {
    case 'occurrences':
      return t('semanticLibrary', 'categoryOccurrences');
    case 'extractedContent':
      return t('semanticLibrary', 'categoryExtracted');
    case 'summaries':
      return t('semanticLibrary', 'categorySummaries');
    case 'labels':
      return t('semanticLibrary', 'categoryLabels');
    case 'orphanVectors':
      return t('semanticLibrary', 'categoryVectors');
    case 'conversationEvidencePins':
      return t('semanticLibrary', 'categoryConversationPins');
  }
}

function estimateView(
  preview: SemanticEnrolmentPreview,
  busy: BusyAction | undefined,
  onEnrol: () => void,
  onCancel: () => void,
  actionLabel: string,
  pdfImageOption?: PdfImageConsentOption,
): Vnode {
  const estimate = preview.estimate;
  const hasEstimate =
    estimate.completeness !== 'unavailable' &&
    [
      estimate.estimatedFiles,
      estimate.estimatedSourceBytes,
      estimate.estimatedExtractedBytes,
      estimate.estimatedVectorBytes,
      estimate.estimatedAdditionalLocalBytes,
      estimate.missingModelDownloadBytes,
    ].some((value) => value != null);
  const reportedSkips = estimate.skippedReasonCounts.filter(({ count }) => count > 0);
  const budgetLabels = {
    documents: t('semanticLibrary', 'estimatedFiles'),
    sourceBytes: t('semanticLibrary', 'estimatedSourceBytes'),
    extractedBytes: t('semanticLibrary', 'estimatedExtractedBytes'),
    vectorBytes: t('semanticLibrary', 'estimatedVectorBytes'),
  } satisfies Record<SemanticEnrolmentPreview['estimate']['exceededBudgets'][number], string>;
  function estimateRow(
    label: string,
    value: number | null | undefined,
    bytes = false,
  ): Vnode[] | undefined {
    return value == null
      ? undefined
      : [m('dt', label), m('dd', bytes ? formatBytes(value) : String(value))];
  }
  return m(
    'section.fm-semantic-library-plan',
    { 'aria-label': t('semanticLibrary', 'previewInclusion') },
    [
      hasEstimate
        ? [
            m(
              'h6',
              t(
                'semanticLibrary',
                estimate.completeness === 'estimated' ? 'estimate' : 'partialEstimate',
              ),
            ),
            m('dl.fm-semantic-library-values', [
              estimateRow(t('semanticLibrary', 'estimatedFiles'), estimate.estimatedFiles),
              estimateRow(
                t('semanticLibrary', 'estimatedSourceBytes'),
                estimate.estimatedSourceBytes,
                true,
              ),
              estimateRow(
                t('semanticLibrary', 'estimatedExtractedBytes'),
                estimate.estimatedExtractedBytes,
                true,
              ),
              estimateRow(
                t('semanticLibrary', 'estimatedVectorBytes'),
                estimate.estimatedVectorBytes,
                true,
              ),
              estimateRow(
                t('semanticLibrary', 'estimatedLocalBytes'),
                estimate.estimatedAdditionalLocalBytes,
                true,
              ),
              estimateRow(
                t('semanticLibrary', 'missingModelDownload'),
                estimate.missingModelDownloadBytes,
                true,
              ),
            ]),
          ]
        : undefined,
      reportedSkips.length === 0
        ? undefined
        : m(
            'ul.fm-semantic-library-reasons',
            reportedSkips.map((reason) =>
              m('li', { key: reason.reason }, `${reasonLabel(reason.reason)}: ${reason.count}`),
            ),
          ),
      estimate.exceededBudgets.length === 0
        ? undefined
        : m(
            'p',
            { role: 'alert' },
            t('semanticLibrary', 'budgetWarning', {
              budgets: estimate.exceededBudgets.map((budget) => budgetLabels[budget]).join(', '),
            }),
          ),
      pdfImageOption === undefined
        ? undefined
        : m('label.fm-semantic-library-pdf-images', [
            m('input', {
              type: 'checkbox',
              checked: pdfImageOption.enabled,
              disabled: busy !== undefined,
              onchange: (event: Event) =>
                pdfImageOption.onChange((event.currentTarget as HTMLInputElement).checked),
            }),
            t('semanticLibrary', 'indexPdfPageImages'),
          ]),
      pdfImageOption?.enabled && pdfImageOption.pageBudget !== null
        ? m(
            'p.fm-semantic-library-disclosure',
            { role: 'status' },
            t('semanticLibrary', 'cpuPdfImageBudgetWarning', {
              pages: pdfImageOption.pageBudget,
              minutes: Math.ceil((pdfImageOption.pageBudget * CPU_PDF_IMAGE_SECONDS_PER_PAGE) / 60),
            }),
          )
        : undefined,
      m('p.fm-semantic-library-disclosure', t('semanticLibrary', 'normalizedExcerptsDisclosure')),
      m('.fm-semantic-library-actions', [
        m(
          'button.fm-semantic-library-action',
          { type: 'button', disabled: busy !== undefined, onclick: onCancel },
          t('button', 'cancel'),
        ),
        m(
          'button.fm-semantic-library-action',
          { type: 'button', disabled: busy !== undefined, onclick: onEnrol },
          busy === 'enrol' ? t('semanticLibrary', 'working') : actionLabel,
        ),
      ]),
    ],
  );
}

/** Focused consent prompt used by the toolbar assistant for a folder not yet indexed. */
export const SemanticFolderEnrolmentPrompt: FactoryComponent<
  SemanticFolderEnrolmentPromptAttrs
> = () => {
  let preview: SemanticEnrolmentPreview | undefined;
  let busy: 'preview' | 'enrol' | undefined;
  let error: string | undefined;
  let showPdfImageOption = false;
  let pdfImageEnabled = false;
  let pdfImagePageBudget: number | null = null;

  async function load(
    attrs: SemanticFolderEnrolmentPromptAttrs,
    selectedPdfImages?: boolean,
  ): Promise<void> {
    busy = 'preview';
    error = undefined;
    preview = undefined;
    try {
      const component = await attrs.client.getSemanticComponentStatus();
      const setup =
        component.activeModel?.profile === 'embeddingGemma2'
          ? await attrs.client.getGemmaLibrarySetup()
          : null;
      showPdfImageOption = setup?.images === true;
      pdfImagePageBudget = setup?.pdfImagePageBudget ?? null;
      pdfImageEnabled = showPdfImageOption
        ? (selectedPdfImages ?? setup?.imageDevice === 'metal')
        : false;
      preview = await attrs.client.previewSemanticEnrolment({
        workspaceId: attrs.workspaceId,
        location: attrs.location,
        recursive: true,
        pdfImagesEnabled: pdfImageEnabled,
        pdfImagePageBudget: pdfImageEnabled ? pdfImagePageBudget : null,
      });
    } catch (cause) {
      error = errorMessage(cause);
    } finally {
      busy = undefined;
      m.redraw();
    }
  }

  async function enrol(attrs: SemanticFolderEnrolmentPromptAttrs): Promise<void> {
    if (preview === undefined) return;
    busy = 'enrol';
    error = undefined;
    try {
      await attrs.client.confirmSemanticEnrolment({
        confirmationId: preview.confirmationId,
        policyRevision: preview.policyRevision,
        workspaceId: attrs.workspaceId,
        location: attrs.location,
      });
      attrs.onEnrolled();
    } catch (cause) {
      error = errorMessage(cause);
    } finally {
      busy = undefined;
      m.redraw();
    }
  }

  return {
    oninit: ({ attrs }) => void load(attrs),
    view: ({ attrs }) =>
      m('.fm-semantic-folder-prompt', [
        m('p', t('ragAsk', 'includeFolderExplanation')),
        m('p.fm-semantic-library-location', attrs.location.uri),
        busy === 'preview'
          ? m('p', { role: 'status' }, t('semanticLibrary', 'working'))
          : preview === undefined
            ? m(
                'button.fm-semantic-library-action',
                { type: 'button', onclick: () => void load(attrs) },
                t('semanticLibrary', 'retry'),
              )
            : estimateView(
                preview,
                busy,
                () => void enrol(attrs),
                attrs.onCancel,
                t('semanticLibrary', 'confirmInclusion'),
                showPdfImageOption
                  ? {
                      enabled: preview.pdfImagesEnabled ?? false,
                      pageBudget: preview.pdfImagePageBudget ?? null,
                      onChange: (enabled) => void load(attrs, enabled),
                    }
                  : undefined,
              ),
        preview === undefined && busy !== 'preview'
          ? m(
              'button.fm-semantic-library-action',
              { type: 'button', onclick: attrs.onCancel },
              t('button', 'cancel'),
            )
          : undefined,
        error === undefined
          ? undefined
          : m('p.fm-semantic-library-error', { role: 'alert' }, error),
      ]),
  };
};

function exclusionPlanView(
  plan: SemanticExclusionPlan,
  busy: BusyAction | undefined,
  onExclude: () => void,
  onCancel: () => void,
): Vnode {
  return m('section.fm-semantic-library-plan', { 'aria-labelledby': 'semantic-exclusion-plan' }, [
    m('h6#semantic-exclusion-plan', t('semanticLibrary', 'exclusionPlan')),
    m('p.fm-semantic-library-location', plan.location.uri),
    m('p', { role: 'alert' }, t('semanticLibrary', 'exclusionWarning')),
    m(
      'ul.fm-semantic-library-cleanup',
      plan.categories.map((category) =>
        m('li', { key: category.category }, [
          m('span', deletionCategoryLabel(category.category)),
          m('span', String(category.totalItems)),
        ]),
      ),
    ),
    m(
      'button.fm-semantic-library-action',
      { type: 'button', disabled: busy !== undefined, onclick: onCancel },
      t('button', 'cancel'),
    ),
    m(
      'button.fm-semantic-library-action.fm-semantic-library-destructive',
      {
        type: 'button',
        disabled: busy !== undefined,
        onclick: onExclude,
      },
      busy === 'confirmExclusion'
        ? t('semanticLibrary', 'working')
        : t('semanticLibrary', 'excludeAndDelete'),
    ),
  ]);
}

function rootStatusView(
  root: SemanticRootStatus,
  confirmedCompletedSources: number | undefined,
  unverifiedLegacySources: number | undefined,
  lastWorkerDevice: 'metal' | 'cpu' | null | undefined,
  scannedEntries: number | undefined,
  workspaceId: WorkspaceId | undefined,
  busy: BusyAction | undefined,
  canRemove: boolean,
  onRemove: (location: Location) => void,
  onResumeCleanup: (planId: string) => void,
  onOverride: (
    root: SemanticRootStatus,
    reason: SemanticEligibilityReason,
    action: SemanticEligibilityOverride | undefined,
  ) => void,
): Vnode {
  const removed = rootExclusion(root) !== undefined;
  const oversizedCount =
    root.eligibilityReasonCounts.find((reason) => reason.reason === 'oversized')?.count ?? 0;
  return m('li.fm-semantic-library-root', { key: root.id }, [
    m('.fm-semantic-library-root-heading', [
      m('strong.fm-semantic-library-location', root.location.uri),
      removed ? m('span', t('semanticLibrary', 'excluded')) : undefined,
      !removed &&
      canRemove &&
      workspaceId !== undefined &&
      root.workspaceReferences.includes(workspaceId)
        ? m(
            'button.fm-semantic-library-action',
            {
              type: 'button',
              disabled: busy !== undefined,
              onclick: () => onRemove(root.location),
            },
            t('semanticLibrary', 'removeFolder'),
          )
        : undefined,
    ]),
    root.availability.state === 'temporarilyUnavailable'
      ? m('.fm-semantic-library-source-unavailable', { role: 'status' }, [
          m('strong', t('semanticLibrary', 'sourceUnavailable')),
          ` — ${root.availability.reason}`,
        ])
      : undefined,
    root.ocrRequiredFiles.length > 0
      ? m(
          'p.fm-semantic-library-ocr-needed',
          t('semanticLibrary', 'ocrNeededCount', { count: root.ocrRequiredFiles.length }),
        )
      : undefined,
    root.pdfImagesEnabled
      ? m(
          'p.fm-semantic-library-disclosure',
          { role: 'status' },
          root.pdfImagePageBudget == null
            ? t('semanticLibrary', 'pdfImagesAllPages')
            : t('semanticLibrary', 'pdfImagesPageBudget', {
                pages: root.pdfImagePageBudget,
              }),
        )
      : undefined,
    oversizedCount > 0
      ? m(
          'p.fm-semantic-library-skipped',
          { role: 'status' },
          t('semanticLibrary', 'oversizedNotIndexed', oversizedCount),
        )
      : undefined,
    confirmedCompletedSources === undefined
      ? undefined
      : m('p.fm-semantic-library-indexed-count', { role: 'status' }, [
          t('semanticLibrary', 'indexedSources', confirmedCompletedSources),
          unverifiedLegacySources === undefined || unverifiedLegacySources === 0
            ? undefined
            : ` · ${t('semanticLibrary', 'legacyVerificationPending', { count: unverifiedLegacySources })}`,
          scannedEntries === undefined
            ? undefined
            : ` · ${t('semanticLibrary', 'scannedEntries', { count: scannedEntries })}`,
        ]),
    m('details.fm-semantic-library-details', [
      m('summary', t('semanticLibrary', 'folderDetails')),
      lastWorkerDevice == null
        ? undefined
        : m(
            'p.fm-semantic-library-device',
            t('semanticLibrary', 'lastWorkerDevice', {
              device: t('semanticLibrary', lastWorkerDevice),
            }),
          ),
      root.attachedVocabularyIds.length === 0
        ? undefined
        : m('p', [
            `${t('semanticLibrary', 'attachedVocabularies')}: `,
            root.attachedVocabularyIds.join(', '),
          ]),
      root.eligibilityReasonCounts.length === 0
        ? undefined
        : m(
            'ul.fm-semantic-library-reasons',
            root.eligibilityReasonCounts.map((reason) =>
              m('li', { key: reason.reason }, `${reasonLabel(reason.reason)}: ${reason.count}`),
            ),
          ),
      root.ocrRequiredFiles.length === 0
        ? undefined
        : m('section.fm-semantic-library-ocr', [
            m('h6', t('semanticLibrary', 'ocrRequiredFiles')),
            m('p', t('semanticLibrary', 'ocrRequiredGuidance')),
            m(
              'ul',
              root.ocrRequiredFiles.map((location) =>
                m(
                  'li',
                  { key: `${location.providerId}:${location.uri}` },
                  decodedLocation(location.uri),
                ),
              ),
            ),
          ]),
      workspaceId === undefined || !root.workspaceReferences.includes(workspaceId)
        ? undefined
        : m('fieldset.fm-semantic-library-overrides', [
            m('legend', t('semanticLibrary', 'eligibilityOverrides')),
            ...SAFE_OVERRIDE_REASONS.map((reason) => {
              const selected =
                root.eligibilityOverrides.find((override) => override.reason === reason)?.action ??
                'default';
              return m('label.fm-semantic-library-override', [
                m('span', reasonLabel(reason)),
                m(
                  'select.browser-default',
                  {
                    'aria-label': `${reasonLabel(reason)} ${t('semanticLibrary', 'override')}`,
                    value: selected,
                    disabled: busy !== undefined,
                    onchange: (event: Event) => {
                      const value = (event.target as HTMLSelectElement).value;
                      onOverride(
                        root,
                        reason,
                        value === 'default' ? undefined : (value as SemanticEligibilityOverride),
                      );
                    },
                  },
                  [
                    m('option', { value: 'default' }, t('semanticLibrary', 'overrideDefault')),
                    m('option', { value: 'include' }, t('semanticLibrary', 'overrideInclude')),
                    m('option', { value: 'exclude' }, t('semanticLibrary', 'overrideExclude')),
                  ],
                ),
              ]);
            }),
          ]),
    ]),
    root.exclusions.length === 0
      ? undefined
      : m(
          'ul.fm-semantic-library-exclusions',
          root.exclusions.map((exclusion) =>
            m('li', { key: exclusion.id }, [
              m('strong', `${t('semanticLibrary', 'excluded')}: `),
              exclusion.location.uri,
              m(
                'span',
                ` — ${
                  exclusion.cleanup.status === 'complete'
                    ? t('semanticLibrary', 'cleanupComplete')
                    : exclusion.cleanup.status === 'failed'
                      ? t('semanticLibrary', 'cleanupFailed')
                      : exclusion.cleanup.status === 'pending'
                        ? t('semanticLibrary', 'cleanupPending')
                        : t('semanticLibrary', 'cleanupRunning')
                }`,
              ),
              // Any incomplete plan is resumable, not just a failed one: a
              // process killed mid-cleanup leaves a running plan with durable
              // per-batch progress, and the user needs a way to finish it.
              exclusion.cleanup.status !== 'complete' && exclusion.cleanup.planId != null
                ? m(
                    'button.fm-semantic-library-action',
                    {
                      type: 'button',
                      disabled: busy !== undefined,
                      onclick: () => onResumeCleanup(exclusion.cleanup.planId as string),
                    },
                    t('semanticLibrary', 'resumeCleanup'),
                  )
                : undefined,
              m(
                'ul.fm-semantic-library-cleanup',
                exclusion.cleanup.categories.map((category) =>
                  m('li', { key: category.category }, [
                    deletionCategoryLabel(category.category),
                    `: ${category.completedItems}/${category.totalItems}`,
                    category.lastError == null ? undefined : ` — ${category.lastError}`,
                  ]),
                ),
              ),
            ]),
          ),
        ),
  ]);
}

function rootExclusion(
  root: SemanticRootStatus,
): SemanticRootStatus['exclusions'][number] | undefined {
  return root.exclusions.find(
    (exclusion) =>
      exclusion.location.providerId === root.location.providerId &&
      exclusion.location.uri === root.location.uri,
  );
}

/** Settings surface for semantic-library consent and active-folder policy. */
export const SemanticLibraryManagement: FactoryComponent<SemanticLibraryManagementAttrs> = (
  initial,
) => {
  let loadState: LoadState = 'loading';
  let capabilities: SemanticLibraryCapabilities | undefined;
  let status: SemanticLibraryStatus | undefined;
  let indexedRoots: readonly KnowledgeRoot[] = [];
  let gemmaActive = false;
  let showPdfImageOption = false;
  let pdfImageEnabled = false;
  let pdfImagePageBudget: number | null = null;
  let lastWorkerDevice: 'metal' | 'cpu' | null | undefined;
  const scannedRoots = new Map<string, number>();
  let refreshTimer: ReturnType<typeof setInterval> | undefined;
  let refreshInFlight = false;
  let removed = false;
  let unsubscribe: (() => void) | undefined;
  let refreshError: string | undefined;
  let currentAttrs = initial.attrs;
  let folder: SemanticFolderStatus | undefined;
  let preview: SemanticEnrolmentPreview | undefined;
  let exclusionPlan: SemanticExclusionPlan | undefined;
  let busy: BusyAction | undefined;
  let error: string | undefined;
  let selectedMedia: { images: boolean; audio: boolean; video: boolean } | undefined;
  let confirmMediaRemoval = false;
  let contextKey = '';

  function key(attrs: SemanticLibraryManagementAttrs): string {
    return `${attrs.workspaceId ?? ''}:${attrs.location?.providerId ?? ''}:${attrs.location?.uri ?? ''}`;
  }

  async function load(attrs: SemanticLibraryManagementAttrs, preserveError = false): Promise<void> {
    loadState = 'loading';
    if (!preserveError) error = undefined;
    try {
      // Capabilities are resolved for this caller, so a principal the backend
      // does not authorize is told the library is unavailable instead of being
      // shown a failed status request it can never satisfy.
      const nextCapabilities = await attrs.client.getSemanticLibraryCapabilities();
      capabilities = nextCapabilities;
      if (nextCapabilities.operations.length === 0) {
        status = undefined;
        folder = undefined;
        loadState = 'loaded';
        m.redraw();
        return;
      }
      const nextStatus = await attrs.client.getSemanticLibraryStatus();
      status = nextStatus;
      selectedMedia = nextStatus.gemmaMedia ? { ...nextStatus.gemmaMedia } : undefined;
      confirmMediaRemoval = false;
      indexedRoots =
        attrs.workspaceId === undefined || !nextStatus.available
          ? []
          : await attrs.client.listKnowledgeRoots({ workspaceId: attrs.workspaceId });
      const component = await attrs.client.getSemanticComponentStatus();
      gemmaActive = component.activeModel?.profile === 'embeddingGemma2';
      const gemmaSetup = gemmaActive ? await attrs.client.getGemmaLibrarySetup() : null;
      lastWorkerDevice = gemmaSetup?.lastWorkerDevice;
      showPdfImageOption = nextStatus.gemmaMedia?.images ?? gemmaSetup?.images === true;
      pdfImagePageBudget = gemmaSetup?.pdfImagePageBudget ?? null;
      pdfImageEnabled = showPdfImageOption && gemmaSetup?.imageDevice === 'metal';
      folder =
        attrs.workspaceId === undefined || attrs.location === undefined || !nextStatus.available
          ? undefined
          : await attrs.client.getSemanticFolderStatus({
              workspaceId: attrs.workspaceId,
              location: attrs.location,
            });
      loadState = 'loaded';
    } catch (cause) {
      error = errorMessage(cause);
      loadState = 'error';
    }
    m.redraw();
  }

  async function refreshIndexing(attrs: SemanticLibraryManagementAttrs): Promise<void> {
    if (
      refreshInFlight ||
      busy !== undefined ||
      loadState !== 'loaded' ||
      attrs.workspaceId === undefined
    )
      return;
    refreshInFlight = true;
    const refreshKey = contextKey;
    try {
      const [nextStatus, nextRoots, nextSetup] = await Promise.all([
        attrs.client.getSemanticLibraryStatus(),
        attrs.client.listKnowledgeRoots({ workspaceId: attrs.workspaceId }),
        gemmaActive ? attrs.client.getGemmaLibrarySetup() : Promise.resolve(null),
      ]);
      if (removed || busy !== undefined || refreshKey !== contextKey) return;
      status = nextStatus;
      indexedRoots = nextRoots;
      if (gemmaActive) {
        lastWorkerDevice = nextSetup?.lastWorkerDevice;
        showPdfImageOption = nextStatus.gemmaMedia?.images ?? nextSetup?.images === true;
      }
      refreshError = undefined;
      m.redraw();
    } catch (cause) {
      if (!removed && refreshKey === contextKey) {
        indexedRoots = [];
        refreshError = errorMessage(cause);
        m.redraw();
      }
    } finally {
      refreshInFlight = false;
    }
  }

  async function refreshFolder(attrs: SemanticLibraryManagementAttrs): Promise<void> {
    if (attrs.workspaceId === undefined || attrs.location === undefined) return;
    folder = await attrs.client.getSemanticFolderStatus({
      workspaceId: attrs.workspaceId,
      location: attrs.location,
    });
  }

  async function action(
    attrs: SemanticLibraryManagementAttrs,
    name: BusyAction,
    work: () => Promise<void>,
  ): Promise<void> {
    busy = name;
    error = undefined;
    try {
      await work();
      await refreshFolder(attrs);
    } catch (cause) {
      const message = errorMessage(cause);
      preview = undefined;
      exclusionPlan = undefined;
      await load(attrs, true);
      error = message;
    } finally {
      busy = undefined;
      m.redraw();
    }
  }

  function has(operation: SemanticLibraryCapabilities['operations'][number]): boolean {
    return capabilities?.operations.includes(operation) ?? false;
  }

  function saveMedia(attrs: SemanticLibraryManagementAttrs): void {
    if (!status?.gemmaMedia || !selectedMedia) return;
    const media = selectedMedia;
    const revision = status.revision;
    const removing =
      (status.gemmaMedia.images && !media.images) ||
      (status.gemmaMedia.audio && !media.audio) ||
      (status.gemmaMedia.video && !media.video);
    if (removing && !confirmMediaRemoval) {
      confirmMediaRemoval = true;
      return;
    }
    void action(attrs, 'media', async () => {
      status = await attrs.client.updateGemmaMedia({
        policyRevision: revision,
        ...media,
        confirmRemoval: removing,
      });
      selectedMedia = status.gemmaMedia ? { ...status.gemmaMedia } : undefined;
      showPdfImageOption = status.gemmaMedia?.images === true;
      if (!showPdfImageOption) pdfImageEnabled = false;
      confirmMediaRemoval = false;
    });
  }

  function previewInclusion(
    attrs: SemanticLibraryManagementAttrs,
    selectedPdfImages = pdfImageEnabled,
  ): void {
    if (attrs.workspaceId === undefined || attrs.location === undefined) return;
    void action(attrs, 'preview', async () => {
      pdfImageEnabled = showPdfImageOption && selectedPdfImages;
      preview = await attrs.client.previewSemanticEnrolment({
        workspaceId: attrs.workspaceId as WorkspaceId,
        location: attrs.location as Location,
        recursive: true,
        pdfImagesEnabled: pdfImageEnabled,
        pdfImagePageBudget: pdfImageEnabled ? pdfImagePageBudget : null,
      });
    });
  }

  function confirmInclusion(attrs: SemanticLibraryManagementAttrs): void {
    if (attrs.workspaceId === undefined || attrs.location === undefined || preview === undefined)
      return;
    void action(attrs, 'enrol', async () => {
      status = await attrs.client.confirmSemanticEnrolment({
        confirmationId: (preview as SemanticEnrolmentPreview).confirmationId,
        policyRevision: (preview as SemanticEnrolmentPreview).policyRevision,
        workspaceId: attrs.workspaceId as WorkspaceId,
        location: attrs.location as Location,
      });
      preview = undefined;
    });
  }

  function planExclusion(attrs: SemanticLibraryManagementAttrs, location: Location): void {
    if (attrs.workspaceId === undefined || status === undefined) return;
    void action(attrs, 'planExclusion', async () => {
      exclusionPlan = await attrs.client.planSemanticExclusion({
        policyRevision: (status as SemanticLibraryStatus).revision,
        workspaceId: attrs.workspaceId as WorkspaceId,
        location,
      });
    });
  }

  function confirmExclusion(attrs: SemanticLibraryManagementAttrs): void {
    if (attrs.workspaceId === undefined || exclusionPlan === undefined) return;
    void action(attrs, 'confirmExclusion', async () => {
      status = await attrs.client.confirmSemanticExclusion({
        confirmationId: (exclusionPlan as SemanticExclusionPlan).confirmationId,
        policyRevision: (exclusionPlan as SemanticExclusionPlan).policyRevision,
        workspaceId: attrs.workspaceId as WorkspaceId,
        location: (exclusionPlan as SemanticExclusionPlan).location,
      });
      exclusionPlan = undefined;
    });
  }

  async function updateOverride(
    attrs: SemanticLibraryManagementAttrs,
    root: SemanticRootStatus,
    reason: SemanticEligibilityReason,
    override: SemanticEligibilityOverride | undefined,
  ): Promise<void> {
    if (attrs.workspaceId === undefined || status === undefined) return;
    const others = root.eligibilityOverrides.filter((candidate) => candidate.reason !== reason);
    await action(attrs, 'override', async () => {
      status = await attrs.client.updateSemanticEligibilityOverrides({
        rootId: root.id,
        workspaceId: attrs.workspaceId as WorkspaceId,
        policyRevision: (status as SemanticLibraryStatus).revision,
        overrides: override === undefined ? others : [...others, { reason, action: override }],
      });
    });
  }

  void load(initial.attrs);
  contextKey = key(initial.attrs);

  return {
    oncreate: () => {
      refreshTimer = setInterval(() => void refreshIndexing(currentAttrs), 3_000);
      if (currentAttrs.client.connection.get() !== 'closed')
        void currentAttrs.client
          .subscribe((event) => {
            if (event.payload.type !== 'semantic.ingestionProgress') return;
            const rootId = event.payload.jobId.startsWith('root:')
              ? event.payload.jobId.slice(5)
              : undefined;
            if (rootId === undefined || removed) return;
            if (['complete', 'cancelled', 'failed', 'skipped'].includes(event.payload.stage)) {
              scannedRoots.delete(rootId);
            } else {
              scannedRoots.set(rootId, event.payload.completed);
            }
            void refreshIndexing(currentAttrs);
            m.redraw();
          })
          .then((stop) => {
            if (removed) stop();
            else unsubscribe = stop;
          })
          .catch((cause: unknown) => {
            if (!removed) {
              refreshError = errorMessage(cause);
              m.redraw();
            }
          });
    },
    onremove: () => {
      removed = true;
      if (refreshTimer !== undefined) clearInterval(refreshTimer);
      unsubscribe?.();
    },
    onbeforeupdate: ({ attrs }) => {
      currentAttrs = attrs;
      const nextKey = key(attrs);
      if (nextKey !== contextKey) {
        contextKey = nextKey;
        preview = undefined;
        exclusionPlan = undefined;
        void load(attrs);
      }
      return true;
    },
    view: ({ attrs }) => {
      if (loadState === 'loading') {
        return m('section.fm-semantic-library-management', [
          m('p.fm-semantic-library-loading', t('semanticLibrary', 'loading')),
          m('.fm-semantic-loading-line', { 'aria-hidden': 'true' }),
        ]);
      }
      if (
        loadState === 'loaded' &&
        capabilities !== undefined &&
        capabilities.operations.length === 0
      ) {
        return m(
          'section.fm-semantic-library-management',
          { 'aria-label': t('semanticLibrary', 'title') },
          m('p', t('semanticLibrary', 'unavailable')),
        );
      }
      if (loadState === 'error' || capabilities === undefined || status === undefined) {
        return m('section.fm-semantic-library-management', [
          m('p', { role: 'alert' }, error ?? t('semanticLibrary', 'unknownError')),
          m(
            'button.fm-semantic-library-action',
            { type: 'button', onclick: () => void load(attrs) },
            t('semanticLibrary', 'retry'),
          ),
        ]);
      }
      if (capabilities.authority === 'unavailable' || !status.available) {
        return m(
          'section.fm-semantic-library-management',
          { 'aria-label': t('semanticLibrary', 'title') },
          m('p', t('semanticLibrary', 'unavailable')),
        );
      }
      const includedRoots = status.roots.filter((root) => rootExclusion(root) === undefined);
      const removedRoots = status.roots.filter((root) => rootExclusion(root) !== undefined);
      const rootView = (candidate: SemanticRootStatus): Vnode =>
        rootStatusView(
          candidate,
          refreshError === undefined
            ? indexedRoots.find((root) => root.rootId === candidate.id)?.confirmedCompletedSources
            : undefined,
          refreshError === undefined
            ? indexedRoots.find((root) => root.rootId === candidate.id)?.unverifiedLegacySources
            : undefined,
          lastWorkerDevice,
          scannedRoots.get(candidate.id),
          attrs.workspaceId,
          busy,
          has('planExclusion'),
          (location) => planExclusion(attrs, location),
          (planId) =>
            void action(attrs, 'cleanup', async () => {
              status = await attrs.client.resumeSemanticCleanup({
                planId,
                policyRevision: (status as SemanticLibraryStatus).revision,
              });
            }),
          (selectedRoot, reason, selectedOverride) =>
            void updateOverride(attrs, selectedRoot, reason, selectedOverride),
        );
      return m(
        'section.fm-semantic-library-management',
        { 'aria-label': t('semanticLibrary', 'title') },
        [
          refreshError === undefined
            ? undefined
            : m('p.fm-semantic-library-error', { role: 'alert' }, refreshError),
          capabilities.authority === 'administratorProvisioned'
            ? m('p.fm-semantic-library-authority', t('semanticLibrary', 'administratorManaged'))
            : undefined,
          m(
            '.fm-semantic-library-ingestion',
            { 'aria-live': 'polite' },
            status.paused
              ? t('semanticLibrary', 'ingestionPaused')
              : t('semanticLibrary', 'ingestionActive'),
          ),
          gemmaActive && status.gemmaMedia && selectedMedia && has('updateGemmaMedia')
            ? m('section.fm-semantic-library-media', [
                m('h6', t('semanticLibrary', 'gemmaMediaTitle')),
                m('p', t('semanticLibrary', 'gemmaMediaAddingHelp')),
                (['images', 'audio', 'video'] as const).map((medium) =>
                  m('label.fm-semantic-library-pdf-images', { key: medium }, [
                    m('input', {
                      type: 'checkbox',
                      checked: selectedMedia?.[medium] ?? false,
                      disabled: busy !== undefined,
                      onchange: (event: Event) => {
                        if (!selectedMedia) return;
                        selectedMedia = {
                          ...selectedMedia,
                          [medium]: (event.currentTarget as HTMLInputElement).checked,
                        };
                        confirmMediaRemoval = false;
                      },
                    }),
                    t('semanticComponents', GEMMA_MEDIA_LABELS[medium]),
                  ]),
                ),
                confirmMediaRemoval
                  ? m('p', { role: 'alert' }, t('semanticLibrary', 'gemmaMediaRemovingWarning'))
                  : undefined,
                m(
                  'button.fm-semantic-library-action',
                  {
                    type: 'button',
                    disabled:
                      busy !== undefined ||
                      (selectedMedia.images === status.gemmaMedia.images &&
                        selectedMedia.audio === status.gemmaMedia.audio &&
                        selectedMedia.video === status.gemmaMedia.video),
                    onclick: () => saveMedia(attrs),
                  },
                  confirmMediaRemoval
                    ? t('semanticLibrary', 'gemmaMediaConfirmRemoval')
                    : t('semanticLibrary', 'gemmaMediaSave'),
                ),
              ])
            : undefined,
          status.paused ? m('p', t('semanticLibrary', 'pauseExplanation')) : undefined,
          has(status.paused ? 'resume' : 'pause')
            ? m(
                'button.fm-semantic-library-action',
                {
                  type: 'button',
                  disabled: busy !== undefined,
                  onclick: () =>
                    void action(attrs, status?.paused ? 'resume' : 'pause', async () => {
                      status = status?.paused
                        ? await attrs.client.resumeSemanticLibrary({
                            policyRevision: (status as SemanticLibraryStatus).revision,
                          })
                        : await attrs.client.pauseSemanticLibrary({
                            policyRevision: (status as SemanticLibraryStatus).revision,
                          });
                    }),
                },
                status.paused
                  ? t('semanticLibrary', 'resumeIngestion')
                  : t('semanticLibrary', 'pauseIngestion'),
              )
            : undefined,
          status.library == null
            ? undefined
            : m('details.fm-semantic-library-details', [
                m('summary', t('semanticLibrary', 'libraryDetails')),
                m('dl.fm-semantic-library-values', [
                  m('dt', t('semanticLibrary', 'libraryIdentity')),
                  m('dd', status.library.libraryId),
                  m('dt', t('semanticLibrary', 'model')),
                  m('dd', `${status.library.model.modelId} · ${status.library.model.revision}`),
                  m('dt', t('semanticLibrary', 'resourceProfile')),
                  m(
                    'dd',
                    status.resourceProfile == null
                      ? t('semanticLibrary', 'notAvailable')
                      : t(
                          'semanticLibrary',
                          status.resourceProfile.kind === 'compact'
                            ? 'profileCompact'
                            : status.resourceProfile.kind === 'balanced'
                              ? 'profileBalanced'
                              : 'profileQuality',
                        ),
                  ),
                  m('dt', t('semanticLibrary', 'reconciliationInterval')),
                  m(
                    'dd',
                    status.reconciliationIntervalSeconds === 1_800
                      ? t('semanticLibrary', 'everyThirtyMinutes')
                      : t('semanticLibrary', 'everySeconds', {
                          seconds: status.reconciliationIntervalSeconds ?? 0,
                        }),
                  ),
                ]),
                m('p', t('semanticLibrary', 'dataPreservedDisclosure')),
              ]),
          attrs.workspaceId === undefined || attrs.location === undefined
            ? m('p', t('semanticLibrary', 'openFolder'))
            : folder === undefined
              ? m('p', t('semanticLibrary', 'folderStatusUnavailable'))
              : folder.consent === 'includedHere' &&
                  includedRoots.some(
                    (root) =>
                      root.location.providerId === attrs.location?.providerId &&
                      root.location.uri === attrs.location?.uri,
                  )
                ? undefined
                : m('section.fm-semantic-library-folder', [
                    m('h6', t('semanticLibrary', 'currentFolder')),
                    m('p.fm-semantic-library-location', attrs.location.uri),
                    folder.consent === 'notIncluded'
                      ? m('p.fm-semantic-library-folder-help', t('semanticLibrary', 'changeFolder'))
                      : undefined,
                    m(
                      '.fm-semantic-library-consent-state',
                      { 'data-consent': folder.consent, 'aria-live': 'polite' },
                      consentLabel(folder.consent),
                    ),
                    folder.sourceAvailable
                      ? undefined
                      : m('.fm-semantic-library-source-unavailable', { role: 'status' }, [
                          m('strong', t('semanticLibrary', 'sourceUnavailable')),
                          folder.unavailableReason == null
                            ? undefined
                            : ` — ${folder.unavailableReason}`,
                        ]),
                    (folder.consent === 'notIncluded' || !folder.workspaceReferenced) &&
                    folder.consent !== 'excluded' &&
                    has('previewEnrolment')
                      ? m(
                          'button.fm-semantic-library-action',
                          {
                            type: 'button',
                            disabled: busy !== undefined,
                            onclick: () => previewInclusion(attrs),
                          },
                          busy === 'preview'
                            ? t('semanticLibrary', 'working')
                            : folder.consent === 'notIncluded'
                              ? t('semanticLibrary', 'previewInclusion')
                              : t('semanticLibrary', 'attachToWorkspace'),
                        )
                      : undefined,
                    (folder.consent === 'includedHere' ||
                      folder.consent === 'inheritedFromParent') &&
                    folder.workspaceReferenced &&
                    has('planExclusion')
                      ? m(
                          'button.fm-semantic-library-action',
                          {
                            type: 'button',
                            disabled: busy !== undefined,
                            onclick: () => planExclusion(attrs, attrs.location as Location),
                          },
                          busy === 'planExclusion'
                            ? t('semanticLibrary', 'working')
                            : t('semanticLibrary', 'reviewExclusion'),
                        )
                      : undefined,
                  ]),
          preview === undefined
            ? undefined
            : estimateView(
                preview,
                busy,
                () => confirmInclusion(attrs),
                () => {
                  preview = undefined;
                },
                t('semanticLibrary', 'includeFolder'),
                showPdfImageOption
                  ? {
                      enabled: preview.pdfImagesEnabled ?? false,
                      pageBudget: preview.pdfImagePageBudget ?? null,
                      onChange: (enabled) => previewInclusion(attrs, enabled),
                    }
                  : undefined,
              ),
          exclusionPlan === undefined
            ? undefined
            : exclusionPlanView(
                exclusionPlan,
                busy,
                () => confirmExclusion(attrs),
                () => {
                  exclusionPlan = undefined;
                },
              ),
          error === undefined
            ? undefined
            : m('p.fm-semantic-library-error', { role: 'alert' }, error),
          m('section.fm-semantic-library-roots', [
            m('h6', t('semanticLibrary', 'enrolledRoots')),
            includedRoots.length === 0
              ? m('p', t('semanticLibrary', 'noEnrolledRoots'))
              : m('ul', includedRoots.map(rootView)),
          ]),
          removedRoots.filter((root) => rootExclusion(root)?.cleanup.status !== 'complete').length >
          0
            ? m('section.fm-semantic-library-roots', [
                m('h6', t('semanticLibrary', 'cleanupInProgress')),
                m(
                  'ul',
                  removedRoots
                    .filter((root) => rootExclusion(root)?.cleanup.status !== 'complete')
                    .map(rootView),
                ),
              ])
            : undefined,
          removedRoots.some((root) => rootExclusion(root)?.cleanup.status === 'complete')
            ? m('details.fm-semantic-library-roots.fm-semantic-library-details', [
                m('summary', t('semanticLibrary', 'removedFolders')),
                m(
                  'ul',
                  removedRoots
                    .filter((root) => rootExclusion(root)?.cleanup.status === 'complete')
                    .map(rootView),
                ),
              ])
            : undefined,
          m('p.fm-semantic-library-guidance', t('semanticLibrary', 'missingRootGuidance')),
        ],
      );
    },
  };
};
