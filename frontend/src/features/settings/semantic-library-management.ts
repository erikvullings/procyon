import m, { type FactoryComponent, type Vnode } from 'mithril';

import type { FileManagerClient } from '../../api/client/file-manager-client';
import { t } from '../../i18n';
import type {
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

type LoadState = 'loading' | 'loaded' | 'error';
type BusyAction =
  | 'preview'
  | 'enrol'
  | 'planExclusion'
  | 'confirmExclusion'
  | 'pause'
  | 'resume'
  | 'cleanup'
  | 'override';

const SAFE_OVERRIDE_REASONS = [
  'hidden',
  'system',
  'applicationOrPackageBundle',
  'dependencyDirectory',
  'buildDirectory',
  'cacheDirectory',
  'gitIgnored',
] as const satisfies readonly SemanticEligibilityReason[];

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

function formatBytes(bytes: number | null | undefined): string {
  if (bytes == null) return t('semanticLibrary', 'estimateUnavailableValue');
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
  confirmed: boolean,
  busy: BusyAction | undefined,
  onConfirmed: (value: boolean) => void,
  onEnrol: () => void,
): Vnode {
  const estimate = preview.estimate;
  const estimateHeading =
    estimate.completeness === 'estimated'
      ? t('semanticLibrary', 'estimate')
      : estimate.completeness === 'partial'
        ? t('semanticLibrary', 'partialEstimate')
        : t('semanticLibrary', 'estimateUnavailable');
  return m(
    'section.fm-semantic-library-plan',
    { 'aria-labelledby': 'semantic-enrolment-preview' },
    [
      m('h6#semantic-enrolment-preview', estimateHeading),
      estimate.completeness === 'unavailable'
        ? m(
            'p.fm-semantic-library-estimate-warning',
            estimate.unavailableReason ?? t('semanticLibrary', 'estimateUnavailable'),
          )
        : m('dl.fm-semantic-library-values', [
            m('dt', t('semanticLibrary', 'estimatedFiles')),
            m('dd', String(estimate.estimatedFiles ?? 0)),
            m('dt', t('semanticLibrary', 'estimatedSourceBytes')),
            m('dd', formatBytes(estimate.estimatedSourceBytes)),
            m('dt', t('semanticLibrary', 'estimatedExtractedBytes')),
            m('dd', formatBytes(estimate.estimatedExtractedBytes)),
            m('dt', t('semanticLibrary', 'estimatedVectorBytes')),
            m('dd', formatBytes(estimate.estimatedVectorBytes)),
            m('dt', t('semanticLibrary', 'estimatedLocalBytes')),
            m('dd', formatBytes(estimate.estimatedAdditionalLocalBytes)),
            m('dt', t('semanticLibrary', 'missingModelDownload')),
            m('dd', formatBytes(estimate.missingModelDownloadBytes)),
          ]),
      estimate.skippedReasonCounts.length === 0
        ? m('p', t('semanticLibrary', 'noSkippedCounts'))
        : m(
            'ul.fm-semantic-library-reasons',
            estimate.skippedReasonCounts.map((reason) =>
              m('li', { key: reason.reason }, `${reasonLabel(reason.reason)}: ${reason.count}`),
            ),
          ),
      estimate.exceededBudgets.length === 0
        ? undefined
        : m(
            'p',
            { role: 'alert' },
            t('semanticLibrary', 'budgetWarning', {
              budgets: estimate.exceededBudgets.join(', '),
            }),
          ),
      m('p.fm-semantic-library-disclosure', t('semanticLibrary', 'normalizedExcerptsDisclosure')),
      m('label.fm-semantic-library-confirmation', [
        m('input#fm-semantic-library-consent', {
          type: 'checkbox',
          checked: confirmed,
          onchange: (event: Event) => onConfirmed((event.target as HTMLInputElement).checked),
        }),
        m('span', t('semanticLibrary', 'confirmConsent')),
      ]),
      m(
        'button.fm-semantic-library-action',
        {
          type: 'button',
          disabled: !confirmed || busy !== undefined,
          onclick: onEnrol,
        },
        busy === 'enrol' ? t('semanticLibrary', 'working') : t('semanticLibrary', 'includeFolder'),
      ),
    ],
  );
}

function exclusionPlanView(
  plan: SemanticExclusionPlan,
  confirmed: boolean,
  busy: BusyAction | undefined,
  onConfirmed: (value: boolean) => void,
  onExclude: () => void,
): Vnode {
  return m('section.fm-semantic-library-plan', { 'aria-labelledby': 'semantic-exclusion-plan' }, [
    m('h6#semantic-exclusion-plan', t('semanticLibrary', 'exclusionPlan')),
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
    m('label.fm-semantic-library-confirmation', [
      m('input#fm-semantic-library-exclusion-confirm', {
        type: 'checkbox',
        checked: confirmed,
        onchange: (event: Event) => onConfirmed((event.target as HTMLInputElement).checked),
      }),
      m('span', t('semanticLibrary', 'confirmExclusion')),
    ]),
    m(
      'button.fm-semantic-library-action.fm-semantic-library-destructive',
      {
        type: 'button',
        disabled: !confirmed || busy !== undefined,
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
  workspaceId: WorkspaceId | undefined,
  busy: BusyAction | undefined,
  onResumeCleanup: (planId: string) => void,
  onOverride: (
    root: SemanticRootStatus,
    reason: SemanticEligibilityReason,
    action: SemanticEligibilityOverride | undefined,
  ) => void,
): Vnode {
  return m('li.fm-semantic-library-root', { key: root.id }, [
    m('strong', root.location.uri),
    root.availability.state === 'temporarilyUnavailable'
      ? m('.fm-semantic-library-source-unavailable', { role: 'status' }, [
          m('strong', t('semanticLibrary', 'sourceUnavailable')),
          ` — ${root.availability.reason}`,
        ])
      : undefined,
    m('dl.fm-semantic-library-values', [
      m('dt', t('semanticLibrary', 'stableIdentity')),
      m(
        'dd',
        root.stableIdentityVerified
          ? t('semanticLibrary', 'verified')
          : t('semanticLibrary', 'unverified'),
      ),
      m('dt', t('semanticLibrary', 'reconciliationGeneration')),
      m('dd', String(root.reconciliationGeneration)),
      m('dt', t('semanticLibrary', 'indexedGeneration')),
      m('dd', String(root.indexedGeneration)),
    ]),
    root.attachedVocabularyIds.length === 0
      ? m('p', t('semanticLibrary', 'noVocabularies'))
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
                'select',
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

/** Settings surface for semantic-library consent and active-folder policy. */
export const SemanticLibraryManagement: FactoryComponent<SemanticLibraryManagementAttrs> = (
  initial,
) => {
  let loadState: LoadState = 'loading';
  let capabilities: SemanticLibraryCapabilities | undefined;
  let status: SemanticLibraryStatus | undefined;
  let folder: SemanticFolderStatus | undefined;
  let preview: SemanticEnrolmentPreview | undefined;
  let exclusionPlan: SemanticExclusionPlan | undefined;
  let consentConfirmed = false;
  let exclusionConfirmed = false;
  let busy: BusyAction | undefined;
  let error: string | undefined;
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
      consentConfirmed = false;
      exclusionConfirmed = false;
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

  function previewInclusion(attrs: SemanticLibraryManagementAttrs): void {
    if (attrs.workspaceId === undefined || attrs.location === undefined) return;
    void action(attrs, 'preview', async () => {
      preview = await attrs.client.previewSemanticEnrolment({
        workspaceId: attrs.workspaceId as WorkspaceId,
        location: attrs.location as Location,
        recursive: true,
      });
      consentConfirmed = false;
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
      consentConfirmed = false;
    });
  }

  function planExclusion(attrs: SemanticLibraryManagementAttrs): void {
    if (attrs.workspaceId === undefined || attrs.location === undefined || status === undefined)
      return;
    void action(attrs, 'planExclusion', async () => {
      exclusionPlan = await attrs.client.planSemanticExclusion({
        policyRevision: (status as SemanticLibraryStatus).revision,
        workspaceId: attrs.workspaceId as WorkspaceId,
        location: attrs.location as Location,
      });
      exclusionConfirmed = false;
    });
  }

  function confirmExclusion(attrs: SemanticLibraryManagementAttrs): void {
    if (
      attrs.workspaceId === undefined ||
      attrs.location === undefined ||
      exclusionPlan === undefined
    )
      return;
    void action(attrs, 'confirmExclusion', async () => {
      status = await attrs.client.confirmSemanticExclusion({
        confirmationId: (exclusionPlan as SemanticExclusionPlan).confirmationId,
        policyRevision: (exclusionPlan as SemanticExclusionPlan).policyRevision,
        workspaceId: attrs.workspaceId as WorkspaceId,
        location: attrs.location as Location,
      });
      exclusionPlan = undefined;
      exclusionConfirmed = false;
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
    onbeforeupdate: ({ attrs }) => {
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
      return m(
        'section.fm-semantic-library-management',
        { 'aria-label': t('semanticLibrary', 'title') },
        [
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
          m('p', t('semanticLibrary', 'pauseExplanation')),
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
            : m('dl.fm-semantic-library-values', [
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
          m('p.fm-semantic-library-disclosure', t('semanticLibrary', 'dataPreservedDisclosure')),
          attrs.workspaceId === undefined || attrs.location === undefined
            ? m('p', t('semanticLibrary', 'openFolder'))
            : folder === undefined
              ? m('p', t('semanticLibrary', 'folderStatusUnavailable'))
              : m('section.fm-semantic-library-folder', [
                  m('h6', t('semanticLibrary', 'currentFolder')),
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
                  (folder.consent === 'includedHere' || folder.consent === 'inheritedFromParent') &&
                  folder.workspaceReferenced &&
                  has('planExclusion')
                    ? m(
                        'button.fm-semantic-library-action',
                        {
                          type: 'button',
                          disabled: busy !== undefined,
                          onclick: () => planExclusion(attrs),
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
                consentConfirmed,
                busy,
                (value) => {
                  consentConfirmed = value;
                },
                () => confirmInclusion(attrs),
              ),
          exclusionPlan === undefined
            ? undefined
            : exclusionPlanView(
                exclusionPlan,
                exclusionConfirmed,
                busy,
                (value) => {
                  exclusionConfirmed = value;
                },
                () => confirmExclusion(attrs),
              ),
          error === undefined
            ? undefined
            : m('p.fm-semantic-library-error', { role: 'alert' }, error),
          m('section.fm-semantic-library-roots', [
            m('h6', t('semanticLibrary', 'enrolledRoots')),
            status.roots.length === 0
              ? m('p', t('semanticLibrary', 'noEnrolledRoots'))
              : m(
                  'ul',
                  status.roots.map((candidate) =>
                    rootStatusView(
                      candidate,
                      attrs.workspaceId,
                      busy,
                      (planId) =>
                        void action(attrs, 'cleanup', async () => {
                          status = await attrs.client.resumeSemanticCleanup({
                            planId,
                            policyRevision: (status as SemanticLibraryStatus).revision,
                          });
                        }),
                      (selectedRoot, reason, selectedOverride) =>
                        void updateOverride(attrs, selectedRoot, reason, selectedOverride),
                    ),
                  ),
                ),
          ]),
        ],
      );
    },
  };
};
