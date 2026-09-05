import m, { type FactoryComponent, type Vnode } from 'mithril';

import type { FileManagerClient } from '../../api/client/file-manager-client';
import { t } from '../../i18n';
import type {
  ImportSemanticLocalModelRequest,
  SemanticComponentCapabilities,
  SemanticComponentKind,
  SemanticComponentOperation,
  SemanticComponentStatus,
  SemanticDataCategory,
  SemanticIndexRemovalPlan,
  SemanticInstallationOffer,
  SemanticModelMigrationPlan,
  SemanticModelMigrationProgress,
  SemanticModelProfile,
  SemanticProfile,
} from '../../models';

export interface SemanticComponentManagementAttrs {
  readonly client: FileManagerClient;
}

type LoadState = 'loading' | 'loaded' | 'error';
type ActionName =
  | 'offer'
  | 'install'
  | 'pause'
  | 'resume'
  | 'planRemoval'
  | 'confirmRemoval'
  | 'move'
  | 'uninstall'
  | 'planMigration'
  | 'confirmMigration'
  | 'checkpointMigration'
  | 'completeMigration'
  | 'importModel';

interface LoadedState {
  capabilities: SemanticComponentCapabilities;
  status: SemanticComponentStatus;
  profiles: SemanticModelProfile[];
}

interface LocalModelFields {
  sourcePath: string;
  modelId: string;
  upstreamRevision: string;
  licenseSpdx: string;
  licenseNotice: string;
  tokenizer: string;
  dimensions: string;
  normalization: 'unitLength' | 'none';
  runtimeComponentId: string;
  runtimeVersionRequirement: string;
  languageCoverage: string;
  estimatedDiskBytes: string;
  estimatedRamBytes: string;
  documents: string;
  sourceBytes: string;
}

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
  return t('semanticComponents', 'unknownError');
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

function profileName(profile: SemanticProfile): string {
  switch (profile) {
    case 'compactMultilingual':
      return t('semanticComponents', 'compactMultilingualName');
    case 'compactEnglish':
      return t('semanticComponents', 'compactEnglishName');
    case 'multilingualQuality':
      return t('semanticComponents', 'multilingualQualityName');
  }
}

function profileDescription(profile: SemanticProfile): string {
  switch (profile) {
    case 'compactMultilingual':
      return t('semanticComponents', 'compactMultilingualDescription');
    case 'compactEnglish':
      return t('semanticComponents', 'compactEnglishDescription');
    case 'multilingualQuality':
      return t('semanticComponents', 'multilingualQualityDescription');
  }
}

function componentKindLabel(kind: SemanticComponentKind): string {
  switch (kind) {
    case 'worker':
      return t('semanticComponents', 'kindWorker');
    case 'runtime':
      return t('semanticComponents', 'kindRuntime');
    case 'model':
      return t('semanticComponents', 'kindModel');
  }
}

function categoryLabel(category: SemanticDataCategory): string {
  switch (category) {
    case 'catalog':
      return t('semanticComponents', 'categoryCatalog');
    case 'extracted':
      return t('semanticComponents', 'categoryExtracted');
    case 'zvec':
      return t('semanticComponents', 'categoryZvec');
    case 'embeddingCache':
      return t('semanticComponents', 'categoryEmbeddingCache');
    case 'models':
      return t('semanticComponents', 'categoryModels');
    case 'workers':
      return t('semanticComponents', 'categoryWorkers');
  }
}

function profileChoices(
  name: string,
  profiles: readonly SemanticModelProfile[],
  selected: SemanticProfile,
  onchange: (profile: SemanticProfile) => void,
  legend = t('semanticComponents', 'profileChoiceLegend'),
): Vnode {
  return m('fieldset.fm-semantic-profile-choices', [
    m('legend', legend),
    profiles.length === 0
      ? m('p.fm-semantic-empty', t('semanticComponents', 'noProfiles'))
      : profiles.map((profile) =>
          m('label.fm-semantic-profile-option', [
            m('input', {
              type: 'radio',
              name,
              value: profile.profile,
              checked: selected === profile.profile,
              onchange: () => onchange(profile.profile),
            }),
            m('span.fm-semantic-profile-copy', [
              m('strong', profileName(profile.profile)),
              profile.recommended
                ? m('span.fm-semantic-recommended', t('semanticComponents', 'recommended'))
                : undefined,
              m('span', profileDescription(profile.profile)),
            ]),
          ]),
        ),
  ]);
}

function lifecycleView(status: SemanticComponentStatus): Vnode {
  const lifecycle = status.lifecycle;
  let copy: m.Children;
  let alert = false;
  switch (lifecycle.state) {
    case 'unavailable':
      copy = t('semanticComponents', 'statusUnavailable');
      break;
    case 'absent':
      copy = t('semanticComponents', 'statusAbsent');
      break;
    case 'offered':
      copy = t('semanticComponents', 'statusOffered');
      break;
    case 'downloading':
      copy = [
        t(
          'semanticComponents',
          lifecycle.resumable ? 'statusDownloadingResumable' : 'statusDownloading',
        ),
        m(
          'span.fm-semantic-status-detail',
          t('semanticComponents', 'downloadProgress', {
            downloaded: formatBytes(lifecycle.downloadedBytes),
            total: formatBytes(lifecycle.totalBytes),
          }),
        ),
      ];
      break;
    case 'installedEnabled':
      copy = t('semanticComponents', 'statusInstalledEnabled');
      break;
    case 'paused':
      copy = t('semanticComponents', 'statusPaused');
      break;
    case 'migrating':
      copy = t('semanticComponents', 'statusMigrating');
      break;
    case 'updateFailedRolledBack':
      alert = true;
      copy = [
        t('semanticComponents', 'statusUpdateFailedRolledBack', {
          version: lifecycle.activeVersion,
        }),
        m(
          'span.fm-semantic-status-detail',
          `${t('semanticComponents', 'failedVersion')}: ${lifecycle.failedVersion}`,
        ),
      ];
      break;
    case 'lowDisk':
      alert = true;
      copy = [
        t('semanticComponents', 'statusLowDisk'),
        m(
          'span.fm-semantic-status-detail',
          `${t('semanticComponents', 'availableSpace')}: ${formatBytes(lifecycle.availableBytes)} · ${t('semanticComponents', 'requiredSpace')}: ${formatBytes(lifecycle.requiredBytes)}`,
        ),
      ];
      break;
    case 'uninstalled':
      copy =
        lifecycle.indexDecision === 'retain'
          ? t('semanticComponents', 'statusUninstalledRetain')
          : t('semanticComponents', 'statusUninstalledDelete');
      break;
  }
  return m(
    '.fm-semantic-lifecycle',
    {
      'data-state': lifecycle.state,
      ...(alert ? { role: 'alert' } : { 'aria-live': 'polite' }),
    },
    copy,
  );
}

function modelAndComponentStatus(status: SemanticComponentStatus): Vnode[] {
  return [
    status.dataRoot == null
      ? undefined
      : m('dl.fm-semantic-definition-list.fm-semantic-data-root', [
          m('dt', t('semanticComponents', 'dataRoot')),
          m('dd', status.dataRoot),
        ]),
    status.activeModel == null
      ? undefined
      : m('section.fm-semantic-status-section', [
          m('h6', t('semanticComponents', 'activeModelHeading')),
          m('dl.fm-semantic-definition-list', [
            m('dt', t('semanticComponents', 'profile')),
            m('dd', profileName(status.activeModel.profile)),
            m('dt', t('semanticComponents', 'modelId')),
            m('dd', status.activeModel.identity.modelId),
            m('dt', t('semanticComponents', 'revision')),
            m('dd', status.activeModel.identity.revision),
          ]),
        ]),
    m('section.fm-semantic-status-section', [
      m('h6', t('semanticComponents', 'installedComponentsHeading')),
      status.components.length === 0
        ? m('p.fm-semantic-empty', t('semanticComponents', 'noInstalledComponents'))
        : m(
            'ul.fm-semantic-installed-components',
            status.components.map((component) =>
              m('li.fm-semantic-installed-component', { key: component.artifactId }, [
                m('strong', componentKindLabel(component.kind)),
                m('span', `${component.componentId} · ${component.version}`),
                m(
                  'span',
                  `${
                    component.state === 'active'
                      ? t('semanticComponents', 'componentStateActive')
                      : t('semanticComponents', 'componentStateRollback')
                  } · ${formatBytes(component.installedBytes)}`,
                ),
              ]),
            ),
          ),
    ]),
    m('section.fm-semantic-status-section', [
      m('h6', t('semanticComponents', 'diskUseHeading')),
      m(
        'dl.fm-semantic-disk-use',
        status.diskUse.categories.flatMap((category) => [
          m('dt.fm-semantic-disk-category', { key: `${category.category}-label` }, [
            categoryLabel(category.category),
          ]),
          m('dd', { key: `${category.category}-value` }, formatBytes(category.bytes)),
        ]),
      ),
      m('p.fm-semantic-disk-total', [
        m('strong', `${t('semanticComponents', 'totalDiskUse')}: `),
        formatBytes(status.diskUse.totalBytes),
      ]),
    ]),
  ].filter((node): node is Vnode => node !== undefined);
}

function offerView(
  offer: SemanticInstallationOffer,
  busy: ActionName | undefined,
  onAccept: () => void,
): Vnode {
  const totals = offer.components.reduce(
    (result, component) => ({
      download: result.download + component.downloadBytes,
      installed: result.installed + component.estimatedInstalledBytes,
      ram: result.ram + component.estimatedRamBytes,
    }),
    { download: 0, installed: 0, ram: 0 },
  );
  return m('section.fm-semantic-offer', { 'aria-labelledby': 'fm-semantic-offer-heading' }, [
    m('h6#fm-semantic-offer-heading', t('semanticComponents', 'offerHeading')),
    m('dl.fm-semantic-definition-list', [
      m('dt', t('semanticComponents', 'profile')),
      m('dd', profileName(offer.profile)),
      m('dt', t('semanticComponents', 'catalogRevision')),
      m('dd', offer.catalogRevision),
      m('dt', t('semanticComponents', 'modelId')),
      m('dd', offer.resolvedModel.modelId),
      m('dt', t('semanticComponents', 'exactModelRevision')),
      m('dd', offer.resolvedModel.revision),
      m('dt', t('semanticComponents', 'localOnly')),
      m('dd', [
        `${offer.embeddingsStayLocal ? t('semanticComponents', 'yes') : t('semanticComponents', 'no')} · `,
        offer.localOnlyDisclosure,
      ]),
      m('dt', t('semanticComponents', 'dataRoot')),
      m('dd', offer.dataRoot),
      m('dt', t('semanticComponents', 'minimumReserve')),
      m('dd', formatBytes(offer.minimumFreeSpaceReserveBytes)),
    ]),
    m('h6', t('semanticComponents', 'offerComponentsHeading')),
    m(
      'div.fm-semantic-table-wrap',
      m('table.fm-semantic-component-table', [
        m('thead', [
          m('tr', [
            m('th', t('semanticComponents', 'kind')),
            m('th', t('semanticComponents', 'component')),
            m('th', t('semanticComponents', 'signedArtifact')),
            m('th', t('semanticComponents', 'version')),
            m('th', t('semanticComponents', 'license')),
            m('th', t('semanticComponents', 'downloadSize')),
            m('th', t('semanticComponents', 'estimatedInstalledSize')),
            m('th', t('semanticComponents', 'estimatedRam')),
          ]),
        ]),
        m(
          'tbody',
          offer.components.map((component) =>
            m('tr.fm-semantic-offer-component', { key: component.artifactId }, [
              m('td', componentKindLabel(component.kind)),
              m('td', [
                component.componentId,
                component.model == null
                  ? undefined
                  : m('small', `${component.model.modelId} · ${component.model.revision}`),
              ]),
              m('td', component.artifactId),
              m('td', component.version),
              m('td', [m('span', component.license.spdx), m('small', component.license.notice)]),
              m('td', formatBytes(component.downloadBytes)),
              m('td', formatBytes(component.estimatedInstalledBytes)),
              m('td', formatBytes(component.estimatedRamBytes)),
            ]),
          ),
        ),
      ]),
    ),
    m('h6', t('semanticComponents', 'aggregateSizes')),
    m('dl.fm-semantic-definition-list', [
      m('dt', t('semanticComponents', 'totalDownload')),
      m('dd', formatBytes(totals.download)),
      m('dt', t('semanticComponents', 'totalInstalled')),
      m('dd', formatBytes(totals.installed)),
      m('dt', t('semanticComponents', 'peakRam')),
      m('dd', formatBytes(totals.ram)),
    ]),
    m('p.fm-semantic-consent-copy', t('semanticComponents', 'consentInstruction')),
    m(
      'button.fm-semantic-action.fm-semantic-accept',
      {
        type: 'button',
        disabled: busy !== undefined,
        onclick: onAccept,
      },
      busy === 'install'
        ? t('semanticComponents', 'working')
        : t('semanticComponents', 'acceptAndInstall'),
    ),
  ]);
}

function indexRemovalPlanView(
  plan: SemanticIndexRemovalPlan,
  busy: ActionName | undefined,
  confirmed: boolean,
  onConfirmed: (confirmed: boolean) => void,
  onConfirm: () => void,
): Vnode {
  return m(
    'section.fm-semantic-removal-plan',
    { 'aria-labelledby': 'fm-semantic-removal-plan-heading' },
    [
      m('h6#fm-semantic-removal-plan-heading', t('semanticComponents', 'removalPlanHeading')),
      m('p', t('semanticComponents', 'removalPlanExplanation')),
      m('dl.fm-semantic-definition-list', [
        m('dt', t('semanticComponents', 'enrolmentId')),
        m('dd', plan.enrolmentId),
        m('dt', t('semanticComponents', 'indexRecords')),
        m('dd', String(plan.expected.indexRecords)),
        m('dt', t('semanticComponents', 'extractedFiles')),
        m('dd', String(plan.expected.extractedFiles)),
        m('dt', t('semanticComponents', 'zvecVectors')),
        m('dd', String(plan.expected.zvecVectors)),
        m('dt', t('semanticComponents', 'cacheEntries')),
        m('dd', String(plan.expected.cacheEntries)),
        m('dt', t('semanticComponents', 'conversationEvidence')),
        m(
          'dd.fm-semantic-removal-conversation-evidence',
          String(plan.expected.conversationEvidence),
        ),
      ]),
      m('label.fm-semantic-confirmation', [
        m('input#fm-semantic-remove-confirm', {
          type: 'checkbox',
          checked: confirmed,
          onchange: (event: Event) => onConfirmed((event.target as HTMLInputElement).checked),
        }),
        m('span', t('semanticComponents', 'confirmIndexRemoval')),
      ]),
      m(
        'button.fm-semantic-action',
        {
          type: 'button',
          disabled: busy !== undefined || !confirmed,
          onclick: onConfirm,
        },
        busy === 'confirmRemoval'
          ? t('semanticComponents', 'working')
          : t('semanticComponents', 'confirmIndexRemovalAction'),
      ),
    ],
  );
}

function migrationProgressView(
  progress: SemanticModelMigrationProgress,
  canCheckpoint: boolean,
  canComplete: boolean,
  busy: ActionName | undefined,
  completedDocuments: string,
  resumeCursor: string,
  onCompletedDocuments: (value: string) => void,
  onResumeCursor: (value: string) => void,
  onCheckpoint: () => void,
  onComplete: () => void,
): Vnode {
  return m('section.fm-semantic-migration-progress-section', [
    m('h6', t('semanticComponents', 'migrationProgressHeading')),
    m('progress.fm-semantic-migration-progress', {
      max: Math.max(1, progress.estimate.documents),
      value: progress.completedDocuments,
    }),
    m(
      'p',
      t('semanticComponents', 'migrationDocuments', {
        completed: progress.completedDocuments,
        total: progress.estimate.documents,
      }),
    ),
    m('dl.fm-semantic-definition-list', [
      m('dt', t('semanticComponents', 'profile')),
      m('dd', profileName(progress.target.profile)),
      m('dt', t('semanticComponents', 'exactModelRevision')),
      m('dd', progress.target.identity.revision),
      m('dt', t('semanticComponents', 'resumeCursor')),
      m(
        'dd',
        progress.resumeCursor == null
          ? t('semanticComponents', 'noResumeCursor')
          : progress.resumeCursor,
      ),
    ]),
    canCheckpoint
      ? m('fieldset.fm-semantic-inline-form', [
          m('legend', t('semanticComponents', 'saveCheckpoint')),
          m('label', { for: 'fm-semantic-checkpoint-documents' }, [
            t('semanticComponents', 'completedDocuments'),
            m('input#fm-semantic-checkpoint-documents', {
              type: 'number',
              min: 0,
              max: progress.estimate.documents,
              value: completedDocuments,
              oninput: (event: InputEvent) =>
                onCompletedDocuments((event.target as HTMLInputElement).value),
            }),
          ]),
          m('label', { for: 'fm-semantic-checkpoint-cursor' }, [
            t('semanticComponents', 'checkpointCursor'),
            m('input#fm-semantic-checkpoint-cursor', {
              type: 'text',
              value: resumeCursor,
              oninput: (event: InputEvent) =>
                onResumeCursor((event.target as HTMLInputElement).value),
            }),
          ]),
          m(
            'button.fm-semantic-action',
            {
              type: 'button',
              disabled:
                busy !== undefined ||
                completedDocuments.trim().length === 0 ||
                Number(completedDocuments) < progress.completedDocuments ||
                Number(completedDocuments) > progress.estimate.documents,
              onclick: onCheckpoint,
            },
            busy === 'checkpointMigration'
              ? t('semanticComponents', 'working')
              : t('semanticComponents', 'saveCheckpoint'),
          ),
        ])
      : undefined,
    canComplete
      ? m(
          'button.fm-semantic-action',
          {
            type: 'button',
            disabled:
              busy !== undefined || progress.completedDocuments < progress.estimate.documents,
            onclick: onComplete,
          },
          busy === 'completeMigration'
            ? t('semanticComponents', 'working')
            : t('semanticComponents', 'completeMigration'),
        )
      : undefined,
  ]);
}

/**
 * Backend-authoritative semantic component management. Only transient form,
 * offer, and busy state live in this closure; every completed action reloads
 * status from the selected runtime adapter.
 */
export const SemanticComponentManagement: FactoryComponent<SemanticComponentManagementAttrs> = ({
  attrs,
}) => {
  const client = attrs.client;
  const loadController = new AbortController();
  let loadState: LoadState = 'loading';
  let loaded: LoadedState | undefined;
  let loadError: string | undefined;
  let actionError: string | undefined;
  let actionMessage: string | undefined;
  let busy: ActionName | undefined;
  let offer: SemanticInstallationOffer | undefined;
  let pendingIndexRemoval: SemanticIndexRemovalPlan | undefined;
  let pendingMigration: SemanticModelMigrationPlan | undefined;
  let installProfile: SemanticProfile = 'compactMultilingual';
  let migrationProfile: SemanticProfile = 'compactMultilingual';
  let localModelProfile: SemanticProfile = 'compactMultilingual';
  let migrationDocuments = '';
  let migrationSourceBytes = '';
  let checkpointDocuments = '';
  let checkpointCursor = '';
  let moveDestination = '';
  let enrolmentId = '';
  let removeConfirmed = false;
  let uninstallDecision: 'retain' | 'delete' | undefined;
  const localModel: LocalModelFields = {
    sourcePath: '',
    modelId: '',
    upstreamRevision: '',
    licenseSpdx: '',
    licenseNotice: '',
    tokenizer: '',
    dimensions: '',
    normalization: 'unitLength',
    runtimeComponentId: '',
    runtimeVersionRequirement: '',
    languageCoverage: '',
    estimatedDiskBytes: '',
    estimatedRamBytes: '',
    documents: '',
    sourceBytes: '',
  };

  function syncMigrationForm(status: SemanticComponentStatus): void {
    const progress = status.migration;
    if (progress == null) return;
    checkpointDocuments = String(progress.completedDocuments);
    checkpointCursor = progress.resumeCursor ?? '';
  }

  async function load(): Promise<void> {
    loadState = 'loading';
    loadError = undefined;
    try {
      const capabilities = await client.getSemanticComponentCapabilities(loadController.signal);
      const [status, profiles] = await Promise.all([
        client.getSemanticComponentStatus(loadController.signal),
        capabilities.operations.includes('viewCatalog')
          ? client.listSemanticComponentProfiles(loadController.signal)
          : Promise.resolve([]),
      ]);
      const recommended = profiles.find((profile) => profile.recommended) ?? profiles[0];
      if (recommended !== undefined) {
        installProfile = recommended.profile;
        migrationProfile = recommended.profile;
        localModelProfile = recommended.profile;
      }
      syncMigrationForm(status);
      loaded = { capabilities, status, profiles };
      loadState = 'loaded';
    } catch (error: unknown) {
      if (loadController.signal.aborted) return;
      loadError = errorMessage(error);
      loadState = 'error';
    }
    m.redraw();
  }

  async function refreshStatus(): Promise<void> {
    if (loaded === undefined) return;
    const status = await client.getSemanticComponentStatus();
    syncMigrationForm(status);
    loaded = { ...loaded, status };
  }

  async function runAction(
    name: ActionName,
    action: () => Promise<void>,
    success?: () => string,
  ): Promise<void> {
    if (busy !== undefined) return;
    busy = name;
    actionError = undefined;
    actionMessage = undefined;
    try {
      await action();
      actionMessage = success?.();
    } catch (error: unknown) {
      actionError = errorMessage(error);
    } finally {
      try {
        await refreshStatus();
      } catch (error: unknown) {
        actionError ??= errorMessage(error);
      }
      busy = undefined;
      m.redraw();
    }
  }

  function can(operation: SemanticComponentOperation): boolean {
    if (loaded === undefined) return false;
    const authority = loaded.capabilities.authority;
    return (
      (authority === 'desktopManaged' || authority === 'deterministicMock') &&
      loaded.capabilities.operations.includes(operation)
    );
  }

  function inputNumber(value: string): number {
    return Math.max(0, Number.parseInt(value, 10) || 0);
  }

  function validLocalModel(): boolean {
    return (
      localModel.sourcePath.trim().length > 0 &&
      !/^[A-Za-z][A-Za-z0-9+.-]*:\/\//u.test(localModel.sourcePath.trim()) &&
      localModel.modelId.trim().length > 0 &&
      localModel.upstreamRevision.trim().length > 0 &&
      localModel.licenseSpdx.trim().length > 0 &&
      localModel.licenseNotice.trim().length > 0 &&
      localModel.tokenizer.trim().length > 0 &&
      inputNumber(localModel.dimensions) > 0 &&
      localModel.runtimeComponentId.trim().length > 0 &&
      localModel.runtimeVersionRequirement.trim().length > 0 &&
      localModel.languageCoverage.split(',').some((language) => language.trim().length > 0) &&
      inputNumber(localModel.estimatedDiskBytes) > 0 &&
      inputNumber(localModel.estimatedRamBytes) > 0 &&
      inputNumber(localModel.documents) > 0 &&
      inputNumber(localModel.sourceBytes) > 0
    );
  }

  function createOffer(): void {
    void runAction('offer', async () => {
      offer = await client.createSemanticComponentInstallationOffer({
        profile: installProfile,
      });
    });
  }

  function acceptOffer(): void {
    if (offer === undefined) return;
    const acceptedOffer = offer;
    void runAction('install', async () => {
      try {
        await client.acceptSemanticComponentInstallationOffer({ offerId: acceptedOffer.offerId });
      } finally {
        // Offers are single-use even when installation fails after consent.
        offer = undefined;
      }
    });
  }

  function planIndexRemoval(): void {
    const requestedEnrolmentId = enrolmentId.trim();
    if (requestedEnrolmentId.length === 0) return;
    void runAction('planRemoval', async () => {
      pendingIndexRemoval = undefined;
      removeConfirmed = false;
      pendingIndexRemoval = await client.createSemanticComponentIndexRemovalPlan({
        enrolmentId: requestedEnrolmentId,
      });
    });
  }

  function confirmIndexRemoval(): void {
    if (!removeConfirmed || pendingIndexRemoval === undefined) return;
    const removalPlan = pendingIndexRemoval;
    let removedEnrolmentId = removalPlan.enrolmentId;
    void runAction(
      'confirmRemoval',
      async () => {
        try {
          const receipt = await client.confirmSemanticComponentIndexRemoval({
            planId: removalPlan.planId,
          });
          removedEnrolmentId = receipt.enrolmentId;
        } finally {
          pendingIndexRemoval = undefined;
          removeConfirmed = false;
        }
      },
      () => t('semanticComponents', 'indexRemoved', { enrolmentId: removedEnrolmentId }),
    );
  }

  function moveData(): void {
    const destination = moveDestination.trim();
    if (destination.length === 0) return;
    void runAction(
      'move',
      () => client.moveSemanticComponentData({ destination }).then(() => undefined),
      () => t('semanticComponents', 'dataMoved', { destination }),
    );
  }

  function uninstall(): void {
    if (uninstallDecision === undefined) return;
    const indexDecision = uninstallDecision;
    void runAction(
      'uninstall',
      () => client.uninstallSemanticComponents({ indexDecision }).then(() => undefined),
      () =>
        t(
          'semanticComponents',
          indexDecision === 'retain'
            ? 'componentsUninstalledRetain'
            : 'componentsUninstalledDelete',
        ),
    );
  }

  function planMigration(): void {
    if (inputNumber(migrationDocuments) === 0 || inputNumber(migrationSourceBytes) === 0) return;
    void runAction('planMigration', async () => {
      pendingMigration = await client.planSemanticComponentModelMigration({
        profile: migrationProfile,
        estimate: {
          documents: inputNumber(migrationDocuments),
          sourceBytes: inputNumber(migrationSourceBytes),
        },
      });
    });
  }

  function confirmMigration(): void {
    if (pendingMigration === undefined) return;
    const migrationId = pendingMigration.migrationId;
    void runAction('confirmMigration', async () => {
      await client.confirmSemanticComponentModelMigration({ migrationId });
      pendingMigration = undefined;
    });
  }

  function checkpointMigration(progress: SemanticModelMigrationProgress): void {
    void runAction('checkpointMigration', async () => {
      await client.checkpointSemanticComponentModelMigration({
        migrationId: progress.migrationId,
        completedDocuments: inputNumber(checkpointDocuments),
        resumeCursor: checkpointCursor.trim().length === 0 ? null : checkpointCursor.trim(),
      });
    });
  }

  function completeMigration(progress: SemanticModelMigrationProgress): void {
    void runAction('completeMigration', async () => {
      await client.completeSemanticComponentModelMigration({
        migrationId: progress.migrationId,
      });
    });
  }

  function importLocalModel(): void {
    if (!validLocalModel()) return;
    const request: ImportSemanticLocalModelRequest = {
      sourcePath: localModel.sourcePath.trim(),
      modelId: localModel.modelId.trim(),
      upstreamRevision: localModel.upstreamRevision.trim(),
      licenseSpdx: localModel.licenseSpdx.trim(),
      licenseNotice: localModel.licenseNotice.trim(),
      tokenizer: localModel.tokenizer.trim(),
      dimensions: inputNumber(localModel.dimensions),
      normalization: localModel.normalization,
      runtimeComponentId: localModel.runtimeComponentId.trim(),
      runtimeVersionRequirement: localModel.runtimeVersionRequirement.trim(),
      languageCoverage: localModel.languageCoverage
        .split(',')
        .map((language) => language.trim())
        .filter((language) => language.length > 0),
      estimatedDiskBytes: inputNumber(localModel.estimatedDiskBytes),
      estimatedRamBytes: inputNumber(localModel.estimatedRamBytes),
      profile: localModelProfile,
      estimate: {
        documents: inputNumber(localModel.documents),
        sourceBytes: inputNumber(localModel.sourceBytes),
      },
    };
    void runAction('importModel', async () => {
      pendingMigration = await client.importSemanticComponentLocalModel(request);
    });
  }

  function textField(
    id: string,
    label: string,
    value: string,
    onchange: (value: string) => void,
    type: 'text' | 'number' = 'text',
  ): Vnode {
    return m('label.fm-semantic-field', { for: id }, [
      m('span', label),
      m(`input#${id}`, {
        type,
        ...(type === 'number' ? { min: 0 } : {}),
        value,
        oninput: (event: InputEvent) => onchange((event.target as HTMLInputElement).value),
      }),
    ]);
  }

  function renderPendingMigration(plan: SemanticModelMigrationPlan): Vnode {
    return m('section.fm-semantic-migration-plan', [
      m('h6', t('semanticComponents', 'migrationPlanHeading')),
      m('dl.fm-semantic-definition-list', [
        m('dt', t('semanticComponents', 'profile')),
        m('dd', profileName(plan.target.profile)),
        m('dt', t('semanticComponents', 'exactModelRevision')),
        m('dd', plan.target.identity.revision),
        m('dt', t('semanticComponents', 'estimatedDocuments')),
        m('dd', String(plan.estimate.documents)),
        m('dt', t('semanticComponents', 'estimatedSourceBytes')),
        m('dd', formatBytes(plan.estimate.sourceBytes)),
      ]),
      m('ul.fm-semantic-plan-properties', [
        plan.fullReindex ? m('li', t('semanticComponents', 'fullReindexRequired')) : undefined,
        plan.requiresConfirmation
          ? m('li', t('semanticComponents', 'migrationConfirmationRequired'))
          : undefined,
        plan.resumable ? m('li', t('semanticComponents', 'migrationResumable')) : undefined,
      ]),
      can('confirmModelMigration')
        ? m(
            'button.fm-semantic-action',
            {
              type: 'button',
              disabled: busy !== undefined,
              onclick: confirmMigration,
            },
            busy === 'confirmMigration'
              ? t('semanticComponents', 'working')
              : t('semanticComponents', 'confirmMigration'),
          )
        : undefined,
    ]);
  }

  function renderManagement(current: LoadedState): Vnode[] {
    const { capabilities, profiles, status } = current;
    const managed =
      capabilities.authority === 'desktopManaged' || capabilities.authority === 'deterministicMock';
    if (!managed) {
      return [
        capabilities.authority === 'unavailable'
          ? m('section.fm-semantic-authority-note', [
              m('h6', t('semanticComponents', 'unavailableTitle')),
              m('p', t('semanticComponents', 'unavailableExplanation')),
            ])
          : m('section.fm-semantic-authority-note', [
              m('h6', t('semanticComponents', 'administratorTitle')),
              m('p', t('semanticComponents', 'administratorExplanation')),
            ]),
      ];
    }

    const canOffer =
      can('createInstallationOffer') &&
      (capabilities.runtimeExecutableDownload === 'directDistribution' ||
        capabilities.runtimeExecutableDownload === 'simulated');
    const installableLifecycle =
      status.lifecycle.state === 'absent' ||
      status.lifecycle.state === 'offered' ||
      status.lifecycle.state === 'downloading' ||
      status.lifecycle.state === 'lowDisk' ||
      status.lifecycle.state === 'uninstalled';
    const progress =
      status.migration ??
      (status.lifecycle.state === 'migrating' ? status.lifecycle.progress : undefined);
    return [
      canOffer && installableLifecycle
        ? m('fieldset.fm-semantic-install', [
            m('legend', t('semanticComponents', 'installEnableLegend')),
            status.lifecycle.state === 'offered' && offer === undefined
              ? m('p', t('semanticComponents', 'existingOffer'))
              : m('p', t('semanticComponents', 'reviewInstallation')),
            profileChoices('fm-semantic-install-profile', profiles, installProfile, (profile) => {
              installProfile = profile;
              offer = undefined;
            }),
            m(
              'button.fm-semantic-action',
              {
                type: 'button',
                disabled: busy !== undefined || profiles.length === 0,
                onclick: createOffer,
              },
              busy === 'offer'
                ? t('semanticComponents', 'working')
                : t('semanticComponents', 'installEnableAction'),
            ),
          ])
        : undefined,
      !canOffer &&
      installableLifecycle &&
      can('createInstallationOffer') &&
      capabilities.runtimeExecutableDownload === 'prohibitedByMacAppStore'
        ? m('p.fm-semantic-authority-note', t('semanticComponents', 'executableDownloadProhibited'))
        : undefined,
      offer === undefined ? undefined : offerView(offer, busy, acceptOffer),
      m('section.fm-semantic-actions', [
        m('h6', t('semanticComponents', 'actionsHeading')),
        m('.fm-semantic-primary-actions', [
          can('pauseIndexing') && status.lifecycle.state === 'installedEnabled'
            ? m(
                'button.fm-semantic-action',
                {
                  type: 'button',
                  disabled: busy !== undefined,
                  onclick: () =>
                    void runAction('pause', () => client.pauseSemanticComponentIndexing()),
                },
                busy === 'pause'
                  ? t('semanticComponents', 'working')
                  : t('semanticComponents', 'pauseIndexing'),
              )
            : undefined,
          can('resumeIndexing') && status.lifecycle.state === 'paused'
            ? m(
                'button.fm-semantic-action',
                {
                  type: 'button',
                  disabled: busy !== undefined,
                  onclick: () =>
                    void runAction('resume', () => client.resumeSemanticComponentIndexing()),
                },
                busy === 'resume'
                  ? t('semanticComponents', 'working')
                  : t('semanticComponents', 'resumeIndexing'),
              )
            : undefined,
        ]),
        can('removeIndex')
          ? m('details.fm-semantic-action-details.fm-semantic-remove-details', [
              m('summary', t('semanticComponents', 'removeIndexSummary')),
              m('fieldset.fm-semantic-form-grid', [
                m('legend', t('semanticComponents', 'removeIndexSummary')),
                textField(
                  'fm-semantic-enrolment-id',
                  t('semanticComponents', 'enrolmentId'),
                  enrolmentId,
                  (value) => {
                    if (value !== enrolmentId) {
                      pendingIndexRemoval = undefined;
                      removeConfirmed = false;
                    }
                    enrolmentId = value;
                  },
                ),
                m(
                  'button.fm-semantic-action',
                  {
                    type: 'button',
                    disabled: busy !== undefined || enrolmentId.trim().length === 0,
                    onclick: planIndexRemoval,
                  },
                  busy === 'planRemoval'
                    ? t('semanticComponents', 'working')
                    : t('semanticComponents', 'reviewIndexRemoval'),
                ),
                pendingIndexRemoval === undefined
                  ? undefined
                  : indexRemovalPlanView(
                      pendingIndexRemoval,
                      busy,
                      removeConfirmed,
                      (confirmed) => {
                        removeConfirmed = confirmed;
                      },
                      confirmIndexRemoval,
                    ),
              ]),
            ])
          : undefined,
        can('moveData')
          ? m('details.fm-semantic-action-details.fm-semantic-move-details', [
              m('summary', t('semanticComponents', 'moveDataSummary')),
              m('fieldset.fm-semantic-inline-form', [
                m('legend', t('semanticComponents', 'moveDataSummary')),
                textField(
                  'fm-semantic-move-destination',
                  t('semanticComponents', 'moveDestination'),
                  moveDestination,
                  (value) => {
                    moveDestination = value;
                  },
                ),
                m(
                  'button.fm-semantic-action',
                  {
                    type: 'button',
                    disabled: busy !== undefined || moveDestination.trim().length === 0,
                    onclick: moveData,
                  },
                  busy === 'move'
                    ? t('semanticComponents', 'working')
                    : t('semanticComponents', 'moveData'),
                ),
              ]),
            ])
          : undefined,
        can('uninstallComponents') && status.components.length > 0
          ? m('details.fm-semantic-action-details.fm-semantic-uninstall-details', [
              m('summary', t('semanticComponents', 'uninstallSummary')),
              m('fieldset.fm-semantic-uninstall-options', [
                m('legend', t('semanticComponents', 'uninstallDecisionLegend')),
                m('label', [
                  m('input#fm-semantic-uninstall-retain', {
                    type: 'radio',
                    name: 'fm-semantic-uninstall-decision',
                    value: 'retain',
                    checked: uninstallDecision === 'retain',
                    onchange: () => {
                      uninstallDecision = 'retain';
                    },
                  }),
                  m('span', [
                    m('strong', t('semanticComponents', 'retainIndex')),
                    m('small', t('semanticComponents', 'retainIndexDescription')),
                  ]),
                ]),
                m('label', [
                  m('input#fm-semantic-uninstall-delete', {
                    type: 'radio',
                    name: 'fm-semantic-uninstall-decision',
                    value: 'delete',
                    checked: uninstallDecision === 'delete',
                    onchange: () => {
                      uninstallDecision = 'delete';
                    },
                  }),
                  m('span', [
                    m('strong', t('semanticComponents', 'deleteIndex')),
                    m('small', t('semanticComponents', 'deleteIndexDescription')),
                  ]),
                ]),
                m(
                  'button.fm-semantic-action.fm-semantic-uninstall-confirm',
                  {
                    type: 'button',
                    disabled: busy !== undefined || uninstallDecision === undefined,
                    onclick: uninstall,
                  },
                  busy === 'uninstall'
                    ? t('semanticComponents', 'working')
                    : t('semanticComponents', 'confirmUninstall'),
                ),
              ]),
            ])
          : undefined,
      ]),
      progress === undefined
        ? undefined
        : migrationProgressView(
            progress,
            can('checkpointModelMigration'),
            can('completeModelMigration'),
            busy,
            checkpointDocuments,
            checkpointCursor,
            (value) => {
              checkpointDocuments = value;
            },
            (value) => {
              checkpointCursor = value;
            },
            () => checkpointMigration(progress),
            () => completeMigration(progress),
          ),
      can('planModelMigration') && status.activeModel != null
        ? m('details.fm-semantic-action-details.fm-semantic-migration-details', [
            m('summary', t('semanticComponents', 'migrationSummary')),
            m('fieldset.fm-semantic-form-grid', [
              m('legend', t('semanticComponents', 'migrationTargetLegend')),
              profileChoices(
                'fm-semantic-migration-profile',
                profiles,
                migrationProfile,
                (profile) => {
                  migrationProfile = profile;
                  pendingMigration = undefined;
                },
              ),
              textField(
                'fm-semantic-migration-documents',
                t('semanticComponents', 'estimatedDocuments'),
                migrationDocuments,
                (value) => {
                  migrationDocuments = value;
                },
                'number',
              ),
              textField(
                'fm-semantic-migration-source-bytes',
                t('semanticComponents', 'estimatedSourceBytes'),
                migrationSourceBytes,
                (value) => {
                  migrationSourceBytes = value;
                },
                'number',
              ),
              m(
                'button.fm-semantic-action',
                {
                  type: 'button',
                  disabled:
                    busy !== undefined ||
                    inputNumber(migrationDocuments) === 0 ||
                    inputNumber(migrationSourceBytes) === 0,
                  onclick: planMigration,
                },
                busy === 'planMigration'
                  ? t('semanticComponents', 'working')
                  : t('semanticComponents', 'reviewMigrationPlan'),
              ),
            ]),
          ])
        : undefined,
      pendingMigration === undefined ? undefined : renderPendingMigration(pendingMigration),
      can('importLocalModel')
        ? m('details.fm-semantic-action-details.fm-semantic-local-model-details', [
            m('summary', t('semanticComponents', 'localModelSummary')),
            m('p', t('semanticComponents', 'localModelExplanation')),
            m('fieldset.fm-semantic-form-grid', [
              m('legend', t('semanticComponents', 'localModelSummary')),
              profileChoices(
                'fm-semantic-local-profile',
                profiles,
                localModelProfile,
                (profile) => {
                  localModelProfile = profile;
                  pendingMigration = undefined;
                },
                t('semanticComponents', 'localModelProfileLegend'),
              ),
              textField(
                'fm-semantic-local-source-path',
                t('semanticComponents', 'sourcePath'),
                localModel.sourcePath,
                (value) => {
                  localModel.sourcePath = value;
                },
              ),
              textField(
                'fm-semantic-local-model-id',
                t('semanticComponents', 'modelId'),
                localModel.modelId,
                (value) => {
                  localModel.modelId = value;
                },
              ),
              textField(
                'fm-semantic-local-revision',
                t('semanticComponents', 'upstreamRevision'),
                localModel.upstreamRevision,
                (value) => {
                  localModel.upstreamRevision = value;
                },
              ),
              textField(
                'fm-semantic-local-license-spdx',
                t('semanticComponents', 'licenseSpdx'),
                localModel.licenseSpdx,
                (value) => {
                  localModel.licenseSpdx = value;
                },
              ),
              textField(
                'fm-semantic-local-license-notice',
                t('semanticComponents', 'licenseNotice'),
                localModel.licenseNotice,
                (value) => {
                  localModel.licenseNotice = value;
                },
              ),
              textField(
                'fm-semantic-local-tokenizer',
                t('semanticComponents', 'tokenizer'),
                localModel.tokenizer,
                (value) => {
                  localModel.tokenizer = value;
                },
              ),
              textField(
                'fm-semantic-local-dimensions',
                t('semanticComponents', 'dimensions'),
                localModel.dimensions,
                (value) => {
                  localModel.dimensions = value;
                },
                'number',
              ),
              m('label.fm-semantic-field', { for: 'fm-semantic-local-normalization' }, [
                m('span', t('semanticComponents', 'normalization')),
                m(
                  'select#fm-semantic-local-normalization',
                  {
                    value: localModel.normalization,
                    onchange: (event: Event) => {
                      localModel.normalization = (event.target as HTMLSelectElement).value as
                        | 'unitLength'
                        | 'none';
                    },
                  },
                  [
                    m(
                      'option',
                      { value: 'unitLength' },
                      t('semanticComponents', 'normalizationUnitLength'),
                    ),
                    m('option', { value: 'none' }, t('semanticComponents', 'normalizationNone')),
                  ],
                ),
              ]),
              textField(
                'fm-semantic-local-runtime-id',
                t('semanticComponents', 'runtimeComponentId'),
                localModel.runtimeComponentId,
                (value) => {
                  localModel.runtimeComponentId = value;
                },
              ),
              textField(
                'fm-semantic-local-runtime-version',
                t('semanticComponents', 'runtimeVersionRequirement'),
                localModel.runtimeVersionRequirement,
                (value) => {
                  localModel.runtimeVersionRequirement = value;
                },
              ),
              textField(
                'fm-semantic-local-languages',
                t('semanticComponents', 'languageCoverage'),
                localModel.languageCoverage,
                (value) => {
                  localModel.languageCoverage = value;
                },
              ),
              textField(
                'fm-semantic-local-disk-bytes',
                t('semanticComponents', 'estimatedDiskBytes'),
                localModel.estimatedDiskBytes,
                (value) => {
                  localModel.estimatedDiskBytes = value;
                },
                'number',
              ),
              textField(
                'fm-semantic-local-ram-bytes',
                t('semanticComponents', 'estimatedRamBytes'),
                localModel.estimatedRamBytes,
                (value) => {
                  localModel.estimatedRamBytes = value;
                },
                'number',
              ),
              textField(
                'fm-semantic-local-documents',
                t('semanticComponents', 'estimatedDocuments'),
                localModel.documents,
                (value) => {
                  localModel.documents = value;
                },
                'number',
              ),
              textField(
                'fm-semantic-local-source-bytes',
                t('semanticComponents', 'estimatedSourceBytes'),
                localModel.sourceBytes,
                (value) => {
                  localModel.sourceBytes = value;
                },
                'number',
              ),
              m(
                'button.fm-semantic-action',
                {
                  type: 'button',
                  disabled: busy !== undefined || !validLocalModel(),
                  onclick: importLocalModel,
                },
                busy === 'importModel'
                  ? t('semanticComponents', 'working')
                  : t('semanticComponents', 'reviewLocalModelMigration'),
              ),
            ]),
          ])
        : undefined,
    ].filter((node): node is Vnode => node !== undefined);
  }

  return {
    oncreate: () => {
      void load();
    },
    onremove: () => {
      loadController.abort();
    },
    view: () => {
      if (loadState === 'loading') {
        return m(
          '.fm-semantic-management.fm-semantic-loading',
          {
            'aria-live': 'polite',
          },
          [
            m('.fm-semantic-loading-line', { 'aria-hidden': 'true' }),
            m('p', t('semanticComponents', 'loading')),
          ],
        );
      }
      if (loadState === 'error') {
        return m('.fm-semantic-management.fm-semantic-load-error', { role: 'alert' }, [
          m(
            'p',
            t('semanticComponents', 'loadError', {
              error: loadError ?? t('semanticComponents', 'unknownError'),
            }),
          ),
          m(
            'button',
            {
              type: 'button',
              onclick: () => void load(),
            },
            t('semanticComponents', 'retry'),
          ),
        ]);
      }
      if (loaded === undefined) {
        return m('.fm-semantic-management', [
          m('p.fm-semantic-empty', t('semanticComponents', 'statusUnavailable')),
        ]);
      }
      return m('.fm-semantic-management', { 'aria-label': t('semanticComponents', 'title') }, [
        m('section.fm-semantic-status', { 'aria-live': 'polite' }, [
          m('h6', t('semanticComponents', 'statusHeading')),
          lifecycleView(loaded.status),
          ...modelAndComponentStatus(loaded.status),
        ]),
        ...renderManagement(loaded),
        actionMessage === undefined
          ? undefined
          : m('p.fm-semantic-action-message', { role: 'status' }, actionMessage),
        actionError === undefined
          ? undefined
          : m(
              'p.fm-semantic-action-error',
              { role: 'alert' },
              t('semanticComponents', 'actionFailed', { error: actionError }),
            ),
      ]);
    },
  };
};
