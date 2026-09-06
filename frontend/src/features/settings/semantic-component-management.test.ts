import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { MockClientError, MockFileManagerClient } from '../../api/client/mock-file-manager-client';
import { setLocale } from '../../i18n';
import { SemanticComponentManagement } from './semantic-component-management';

let root: HTMLElement;

beforeEach(() => {
  setLocale('en');
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  setLocale('en');
  m.mount(root, null);
  root.remove();
});

function mountComponent(client: MockFileManagerClient): void {
  m.mount(root, { view: () => m(SemanticComponentManagement, { client }) });
  m.redraw.sync();
}

async function waitForLoaded(): Promise<void> {
  await vi.waitFor(() => expect(root.querySelector('.fm-semantic-loading')).toBeNull());
  m.redraw.sync();
}

function button(label: string): HTMLButtonElement {
  const match = [...root.querySelectorAll<HTMLButtonElement>('button')].find(
    (candidate) => candidate.textContent?.trim() === label,
  );
  if (match === undefined) throw new Error(`No button labelled "${label}"`);
  return match;
}

function setInput(selector: string, value: string): void {
  const input = root.querySelector<HTMLInputElement>(selector);
  if (input === null) throw new Error(`No input matching "${selector}"`);
  input.value = value;
  input.dispatchEvent(new Event('input', { bubbles: true }));
  input.dispatchEvent(new Event('change', { bubbles: true }));
  m.redraw.sync();
}

describe('SemanticComponentManagement', () => {
  it('shows meaningful loading copy, then loads capabilities, status, and profiles', async () => {
    const client = new MockFileManagerClient();
    const capabilities = vi.spyOn(client, 'getSemanticComponentCapabilities');
    const status = vi.spyOn(client, 'getSemanticComponentStatus');
    const profiles = vi.spyOn(client, 'listSemanticComponentProfiles');

    mountComponent(client);

    expect(root.querySelector('.fm-semantic-loading')?.textContent).toContain(
      'Loading semantic component status',
    );
    await waitForLoaded();
    expect(capabilities).toHaveBeenCalledOnce();
    expect(status).toHaveBeenCalledOnce();
    expect(profiles).toHaveBeenCalledOnce();
    expect(root.textContent).toContain('Not installed');
  });

  it('labels an installed developer catalog as non-production', async () => {
    const client = new MockFileManagerClient();
    const profiles = await client.listSemanticComponentProfiles();
    vi.spyOn(client, 'getSemanticComponentCapabilities').mockResolvedValue({
      authority: 'desktopManaged',
      runtimeExecutableDownload: 'directDistribution',
      operations: ['viewStatus', 'viewCatalog', 'createInstallationOffer', 'installOrEnable'],
    });
    vi.spyOn(client, 'listSemanticComponentProfiles').mockResolvedValue(
      profiles.map((profile) => ({
        ...profile,
        resolvedModel: {
          ...profile.resolvedModel,
          modelId: 'procyon.dev.hashing-embedding',
        },
      })),
    );

    mountComponent(client);
    await waitForLoaded();

    expect(root.textContent).toContain('Development-only bundle');
    expect(root.textContent).toContain('must not be used in production');
  });

  it('shows administrator-provisioned status read-only without forbidden controls', async () => {
    const client = new MockFileManagerClient({ semanticLifecycle: 'installedEnabled' });
    vi.spyOn(client, 'getSemanticComponentCapabilities').mockResolvedValue({
      authority: 'administratorProvisioned',
      runtimeExecutableDownload: 'administratorProvisioned',
      operations: ['viewStatus', 'viewCatalog'],
    });

    mountComponent(client);
    await waitForLoaded();

    expect(root.textContent).toContain('managed by your administrator');
    expect(root.querySelectorAll('.fm-semantic-action')).toHaveLength(0);
    expect(root.textContent).not.toContain('Pause indexing');
    expect(root.textContent).not.toContain('Uninstall components');
  });

  it('does not request a catalog or expose actions when semantic support is unavailable', async () => {
    const client = new MockFileManagerClient({ semanticLifecycle: 'unavailable' });
    const profiles = vi.spyOn(client, 'listSemanticComponentProfiles');
    mountComponent(client);
    await waitForLoaded();

    expect(root.textContent).toContain('This build cannot install or use');
    expect(profiles).not.toHaveBeenCalled();
    expect(root.querySelectorAll('.fm-semantic-action')).toHaveLength(0);
  });

  it('shows a load error with a working retry and an empty-profile state', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'getSemanticComponentCapabilities').mockRejectedValueOnce(
      new Error('offline'),
    );
    mountComponent(client);

    await vi.waitFor(() =>
      expect(root.querySelector('[role="alert"]')?.textContent).toContain('offline'),
    );
    button('Retry').click();
    await vi.waitFor(() => expect(root.textContent).toContain('Not installed'));

    m.mount(root, null);
    const emptyClient = new MockFileManagerClient();
    vi.spyOn(emptyClient, 'listSemanticComponentProfiles').mockResolvedValue([]);
    mountComponent(emptyClient);
    await waitForLoaded();
    expect(root.textContent).toContain('No model profiles are available');
    expect(button('Install / enable').disabled).toBe(true);
  });

  it('requires a reviewed complete disclosure before explicit installation consent', async () => {
    const client = new MockFileManagerClient();
    const createOffer = vi.spyOn(client, 'createSemanticComponentInstallationOffer');
    const acceptOffer = vi.spyOn(client, 'acceptSemanticComponentInstallationOffer');

    mountComponent(client);
    await waitForLoaded();

    const profileOptions = root.querySelectorAll<HTMLInputElement>(
      '.fm-semantic-install .fm-semantic-profile-option input[type="radio"]',
    );
    expect(profileOptions).toHaveLength(3);
    expect(profileOptions[0]?.checked).toBe(true);
    expect(root.textContent).toContain('Recommended');
    expect(createOffer).not.toHaveBeenCalled();
    expect(acceptOffer).not.toHaveBeenCalled();

    button('Install / enable').click();
    await vi.waitFor(() =>
      expect(root.querySelectorAll('.fm-semantic-offer-component')).toHaveLength(3),
    );
    expect(root.textContent).toContain('mock-signed-catalog-revision');
    expect(root.textContent).toContain('mock-compact-multilingual-revision');
    expect(root.textContent).toContain(
      'Embedding inference and semantic index data stay on this device.',
    );
    expect(root.textContent).toContain('mock/semantic');
    expect(root.textContent).toContain('Minimum free-space reserve');
    expect(root.textContent).toContain('Total download');
    expect(root.textContent).toContain('Total installed');
    expect(root.textContent).toContain('Peak RAM');
    expect(root.textContent).toContain('License');
    expect(acceptOffer).not.toHaveBeenCalled();

    button('Accept and install').click();
    await vi.waitFor(() => expect(acceptOffer).toHaveBeenCalledWith({ offerId: 'mock-offer-1' }));
    await vi.waitFor(() => expect(root.textContent).toContain('Installed and enabled'));
  });

  it('renders distinct pause, remove, move, and uninstall controls for installed components', async () => {
    const client = new MockFileManagerClient({ semanticLifecycle: 'installedEnabled' });
    const workerPatch = vi.spyOn(client, 'installSemanticComponentWorkerPatch');
    mountComponent(client);
    await waitForLoaded();

    expect(button('Pause indexing').disabled).toBe(false);
    expect(button('Review removal')).toBeInstanceOf(HTMLButtonElement);
    expect(button('Move data')).toBeInstanceOf(HTMLButtonElement);
    expect(button('Confirm uninstall').disabled).toBe(true);
    expect(root.textContent).toContain('Exact active model');
    expect(root.textContent).toContain('mock-compact-multilingual-revision');
    expect(root.querySelectorAll('.fm-semantic-installed-component')).toHaveLength(3);
    expect(root.textContent).toContain('In use');
    expect(root.querySelectorAll('.fm-semantic-disk-category')).toHaveLength(6);
    expect(root.textContent?.toLowerCase()).not.toContain('worker patch');
    expect(workerPatch).not.toHaveBeenCalled();

    m.mount(root, null);
    mountComponent(new MockFileManagerClient({ semanticLifecycle: 'paused' }));
    await waitForLoaded();
    expect(button('Resume indexing').disabled).toBe(false);
    expect(root.textContent).not.toContain('Pause indexing');
  });

  it('disables actions and reports progress while an action is busy', async () => {
    const client = new MockFileManagerClient({ semanticLifecycle: 'installedEnabled' });
    let release: (() => void) | undefined;
    vi.spyOn(client, 'pauseSemanticComponentIndexing').mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          release = resolve;
        }),
    );
    mountComponent(client);
    await waitForLoaded();

    button('Pause indexing').click();
    m.redraw.sync();

    expect(button('Working…').disabled).toBe(true);
    release?.();
    await vi.waitFor(() => expect(root.textContent).toContain('Pause indexing'));
  });

  it('renders low-disk state and action failures as accessible alerts', async () => {
    mountComponent(new MockFileManagerClient({ semanticLifecycle: 'lowDisk' }));
    await waitForLoaded();
    expect(root.querySelector('[role="alert"]')?.textContent).toContain('Not enough free space');

    m.mount(root, null);
    const failure = new MockClientError('indexing', 'Indexer refused to pause');
    mountComponent(
      new MockFileManagerClient({
        semanticLifecycle: 'installedEnabled',
        failures: { pauseSemanticComponentIndexing: failure },
      }),
    );
    await waitForLoaded();
    button('Pause indexing').click();
    await vi.waitFor(() =>
      expect(root.querySelector('.fm-semantic-action-error')?.textContent).toContain(
        'Indexer refused to pause',
      ),
    );
    const actionError = root.querySelector('.fm-semantic-action-error');
    const actions = root.querySelector('.fm-semantic-actions');
    expect(
      actionError !== null &&
        actions !== null &&
        (actionError.compareDocumentPosition(actions) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0,
    ).toBe(true);
  });

  it('shows typed Tauri errors and refreshes consumed offers after a failed install', async () => {
    const client = new MockFileManagerClient();
    const lowDiskStatus = await new MockFileManagerClient({
      semanticLifecycle: 'lowDisk',
    }).getSemanticComponentStatus();
    const getStatus = vi
      .spyOn(client, 'getSemanticComponentStatus')
      .mockResolvedValue(lowDiskStatus);
    vi.spyOn(client, 'acceptSemanticComponentInstallationOffer').mockRejectedValue({
      code: 'insufficientSpace',
      message: 'Free another 2 GiB before installing.',
    });

    mountComponent(client);
    await waitForLoaded();
    button('Install / enable').click();
    await vi.waitFor(() => expect(root.textContent).toContain('Installation disclosure'));
    button('Accept and install').click();

    await vi.waitFor(() =>
      expect(root.querySelector('.fm-semantic-action-error')?.textContent).toContain(
        'Free another 2 GiB before installing.',
      ),
    );
    expect(getStatus.mock.calls.length).toBeGreaterThanOrEqual(3);
    expect(root.textContent).toContain('Not enough free space');
    expect(root.querySelector('.fm-semantic-offer')).toBeNull();
  });

  it.each([
    ['unavailable', 'Semantic components unavailable'],
    ['offered', 'Installation offer awaiting review'],
    ['downloadingResumable', 'Download can resume'],
    ['paused', 'Indexing paused'],
    ['updateFailedRolledBack', 'Update failed; restored version 1.0.0'],
    ['uninstalledRetain', 'Components uninstalled; index retained'],
    ['uninstalledDelete', 'Components and index deleted'],
  ] as const)('renders the %s lifecycle state', async (semanticLifecycle, copy) => {
    mountComponent(new MockFileManagerClient({ semanticLifecycle }));
    await waitForLoaded();

    expect(root.textContent).toContain(copy);
  });

  it('requires an explicit retain/delete choice before uninstalling', async () => {
    const client = new MockFileManagerClient({ semanticLifecycle: 'installedEnabled' });
    const uninstall = vi.spyOn(client, 'uninstallSemanticComponents');
    mountComponent(client);
    await waitForLoaded();

    expect(button('Confirm uninstall').disabled).toBe(true);
    expect(root.textContent).toContain('Retain index');
    expect(root.textContent).toContain('Delete index');
    root.querySelector<HTMLInputElement>('#fm-semantic-uninstall-delete')?.click();
    m.redraw.sync();
    button('Confirm uninstall').click();

    await vi.waitFor(() => expect(uninstall).toHaveBeenCalledWith({ indexDecision: 'delete' }));
    await vi.waitFor(() => expect(root.textContent).toContain('Components and index deleted'));
  });

  it('reviews authoritative removal counts before confirming only the opaque plan ID', async () => {
    const client = new MockFileManagerClient({ semanticLifecycle: 'installedEnabled' });
    const createPlan = vi.spyOn(client, 'createSemanticComponentIndexRemovalPlan');
    const confirmRemoval = vi.spyOn(client, 'confirmSemanticComponentIndexRemoval');
    mountComponent(client);
    await waitForLoaded();

    const removalDetails = root.querySelector<HTMLDetailsElement>('.fm-semantic-remove-details');
    removalDetails?.setAttribute('open', '');
    setInput('#fm-semantic-enrolment-id', 'library-1');
    expect(removalDetails?.querySelector('input[type="number"]')).toBeNull();
    button('Review removal').click();

    await vi.waitFor(() =>
      expect(createPlan).toHaveBeenCalledWith({
        enrolmentId: 'library-1',
      }),
    );
    await vi.waitFor(() => expect(root.textContent).toContain('Authoritative removal plan'));
    expect(root.textContent).toContain('Conversation evidence records');
    expect(root.querySelector('.fm-semantic-removal-conversation-evidence')?.textContent).toBe('3');
    expect(button('Confirm removal').disabled).toBe(true);

    root.querySelector<HTMLInputElement>('#fm-semantic-remove-confirm')?.click();
    m.redraw.sync();
    button('Confirm removal').click();

    await vi.waitFor(() =>
      expect(confirmRemoval).toHaveBeenCalledWith({
        planId: 'mock-index-removal-1',
      }),
    );
  });

  it('exposes resumable migration controls for a managed desktop runtime', async () => {
    const client = new MockFileManagerClient({ semanticLifecycle: 'migrating' });
    vi.spyOn(client, 'getSemanticComponentCapabilities').mockResolvedValue({
      authority: 'desktopManaged',
      runtimeExecutableDownload: 'directDistribution',
      operations: [
        'viewStatus',
        'viewCatalog',
        'createInstallationOffer',
        'installOrEnable',
        'installWorkerPatch',
        'pauseIndexing',
        'resumeIndexing',
        'removeIndex',
        'moveData',
        'uninstallComponents',
        'importLocalModel',
        'planModelMigration',
        'confirmModelMigration',
        'checkpointModelMigration',
        'completeModelMigration',
      ],
    });

    mountComponent(client);
    await waitForLoaded();

    expect(root.querySelector('#fm-semantic-checkpoint-documents')).not.toBeNull();
    expect(root.querySelector('#fm-semantic-checkpoint-cursor')).not.toBeNull();
    expect(root.textContent).toContain('Save checkpoint');
    expect(root.textContent).toContain('Complete migration');
  });

  it('offers the current signed model when an older model used the same profile name', async () => {
    const client = new MockFileManagerClient({ semanticLifecycle: 'installedEnabled' });
    const status = await client.getSemanticComponentStatus();
    const profiles = await client.listSemanticComponentProfiles();
    const compact = profiles.find((profile) => profile.profile === 'compactMultilingual');
    const quality = profiles.find((profile) => profile.profile === 'multilingualQuality');
    if (status.activeModel == null || compact === undefined || quality === undefined) {
      throw new Error('mock semantic profiles are incomplete');
    }
    vi.spyOn(client, 'getSemanticComponentStatus').mockResolvedValue({
      ...status,
      activeModel: {
        profile: 'multilingualQuality',
        identity: compact.resolvedModel,
      },
    });
    const plan = vi.spyOn(client, 'planSemanticComponentModelMigration');

    mountComponent(client);
    await waitForLoaded();

    expect(root.textContent).toContain('This profile still uses an older model package');
    expect(root.textContent).toContain('Update available');
    root
      .querySelector<HTMLDetailsElement>('.fm-semantic-migration-details')
      ?.setAttribute('open', '');
    expect(
      root.querySelector<HTMLInputElement>(
        'input[name="fm-semantic-migration-profile"][value="multilingualQuality"]',
      )?.checked,
    ).toBe(true);
    expect(button('Review model change').disabled).toBe(false);
    button('Review model change').click();

    await vi.waitFor(() =>
      expect(plan).toHaveBeenCalledWith({
        profile: 'multilingualQuality',
        estimate: { documents: 0, sourceBytes: 0 },
      }),
    );
  });

  it('renders resumable migration progress and exposes profile plan and confirmation', async () => {
    mountComponent(new MockFileManagerClient({ semanticLifecycle: 'migrating' }));
    await waitForLoaded();

    expect(root.querySelector<HTMLProgressElement>('.fm-semantic-migration-progress')?.value).toBe(
      4,
    );
    expect(root.textContent).toContain('4 of 10 documents');
    expect(root.textContent).toContain('mock-resume-cursor');

    m.mount(root, null);
    const client = new MockFileManagerClient({ semanticLifecycle: 'installedEnabled' });
    const plan = vi.spyOn(client, 'planSemanticComponentModelMigration');
    const confirm = vi.spyOn(client, 'confirmSemanticComponentModelMigration');
    const checkpoint = vi.spyOn(client, 'checkpointSemanticComponentModelMigration');
    const complete = vi.spyOn(client, 'completeSemanticComponentModelMigration');
    mountComponent(client);
    await waitForLoaded();
    root
      .querySelector<HTMLDetailsElement>('.fm-semantic-migration-details')
      ?.setAttribute('open', '');
    expect(root.querySelector('#fm-semantic-migration-documents')).toBeNull();
    expect(root.querySelector('#fm-semantic-migration-source-bytes')).toBeNull();
    const quality = root.querySelector<HTMLInputElement>(
      'input[name="fm-semantic-migration-profile"][value="multilingualQuality"]',
    );
    quality?.click();
    m.redraw.sync();
    expect(button('Review model change').disabled).toBe(false);
    button('Review model change').click();
    await vi.waitFor(() =>
      expect(plan).toHaveBeenCalledWith({
        profile: 'multilingualQuality',
        estimate: { documents: 0, sourceBytes: 0 },
      }),
    );
    await vi.waitFor(() => expect(root.textContent).toContain('Full reindex required'));
    button('Confirm migration').click();

    await vi.waitFor(() => expect(confirm).toHaveBeenCalledOnce());
    await vi.waitFor(() =>
      expect(checkpoint).toHaveBeenCalledWith({
        migrationId: expect.any(String),
        completedDocuments: 0,
        resumeCursor: null,
      }),
    );
    await vi.waitFor(() => expect(complete).toHaveBeenCalledOnce());
    await vi.waitFor(() => expect(root.textContent).toContain('Multilingual quality'));
  });

  it('shows a failed migration confirmation directly below its confirmation button', async () => {
    const client = new MockFileManagerClient({ semanticLifecycle: 'installedEnabled' });
    vi.spyOn(client, 'confirmSemanticComponentModelMigration').mockRejectedValue(
      new MockClientError(
        'installedArtifactInvalid',
        'artifact `procyon.dev.worker` failed integrity validation',
      ),
    );
    mountComponent(client);
    await waitForLoaded();
    root
      .querySelector<HTMLDetailsElement>('.fm-semantic-migration-details')
      ?.setAttribute('open', '');
    root
      .querySelector<HTMLInputElement>(
        'input[name="fm-semantic-migration-profile"][value="multilingualQuality"]',
      )
      ?.click();
    m.redraw.sync();
    button('Review model change').click();
    await vi.waitFor(() => expect(root.textContent).toContain('Full reindex required'));

    const confirmation = button('Confirm migration');
    confirmation.click();

    await vi.waitFor(() =>
      expect(root.querySelector('.fm-semantic-action-error')?.textContent).toContain(
        'failed integrity validation',
      ),
    );
    expect(root.querySelectorAll('.fm-semantic-action-error')).toHaveLength(1);
    expect(confirmation.nextElementSibling).toBe(root.querySelector('.fm-semantic-action-error'));
  });

  it('collects every required local-model metadata field without offering a URL field', async () => {
    const client = new MockFileManagerClient({ semanticLifecycle: 'installedEnabled' });
    const importModel = vi.spyOn(client, 'importSemanticComponentLocalModel');
    mountComponent(client);
    await waitForLoaded();

    root
      .querySelector<HTMLDetailsElement>('.fm-semantic-local-model-details')
      ?.setAttribute('open', '');
    const localModelDetails = root.querySelector<HTMLDetailsElement>(
      '.fm-semantic-local-model-details',
    );
    expect(localModelDetails?.querySelector('summary')?.textContent).toContain('Developer tool');
    expect(localModelDetails?.textContent).toContain(
      'This does not switch between the signed models above.',
    );
    const dimensions = localModelDetails?.querySelector<HTMLInputElement>(
      '#fm-semantic-local-dimensions',
    );
    expect(dimensions?.classList.contains('fm-semantic-number-input')).toBe(true);
    const normalization = localModelDetails?.querySelector<HTMLSelectElement>(
      '#fm-semantic-local-normalization',
    );
    expect(normalization?.value).toBe('unitLength');
    expect(normalization?.selectedOptions[0]?.textContent).toBe('Unit length');
    const values: ReadonlyArray<readonly [string, string]> = [
      ['#fm-semantic-local-source-path', 'https://models.example.test/model'],
      ['#fm-semantic-local-model-id', 'local-model'],
      ['#fm-semantic-local-revision', 'revision-1'],
      ['#fm-semantic-local-license-spdx', 'Apache-2.0'],
      ['#fm-semantic-local-license-notice', 'Local model'],
      ['#fm-semantic-local-tokenizer', 'tokenizer-1'],
      ['#fm-semantic-local-dimensions', '384'],
      ['#fm-semantic-local-runtime-id', 'mock-runtime'],
      ['#fm-semantic-local-runtime-version', '^1'],
      ['#fm-semantic-local-languages', 'en, nl'],
      ['#fm-semantic-local-disk-bytes', '100'],
      ['#fm-semantic-local-ram-bytes', '200'],
      ['#fm-semantic-local-documents', '10'],
      ['#fm-semantic-local-source-bytes', '1000'],
    ];
    for (const [selector, value] of values) setInput(selector, value);
    expect(root.querySelector('input[type="url"]')).toBeNull();
    expect(button('Review local model migration').disabled).toBe(true);
    setInput('#fm-semantic-local-source-path', '/models/local');
    root
      .querySelector<HTMLInputElement>(
        'input[name="fm-semantic-local-profile"][value="compactEnglish"]',
      )
      ?.click();
    button('Review local model migration').click();

    await vi.waitFor(() => expect(importModel).toHaveBeenCalledOnce());
    expect(importModel).toHaveBeenCalledWith(
      expect.objectContaining({
        sourcePath: '/models/local',
        modelId: 'local-model',
        upstreamRevision: 'revision-1',
        licenseSpdx: 'Apache-2.0',
        licenseNotice: 'Local model',
        tokenizer: 'tokenizer-1',
        dimensions: 384,
        normalization: 'unitLength',
        runtimeComponentId: 'mock-runtime',
        runtimeVersionRequirement: '^1',
        languageCoverage: ['en', 'nl'],
        estimatedDiskBytes: 100,
        estimatedRamBytes: 200,
        profile: 'compactEnglish',
        estimate: { documents: 10, sourceBytes: 1000 },
      }),
    );
  });

  it('does not repeat disclosure titles as inner fieldset legends', async () => {
    const client = new MockFileManagerClient({ semanticLifecycle: 'installedEnabled' });
    mountComponent(client);
    await waitForLoaded();

    for (const selector of [
      '.fm-semantic-remove-details',
      '.fm-semantic-move-details',
      '.fm-semantic-local-model-details',
    ]) {
      const details = root.querySelector<HTMLDetailsElement>(selector);
      expect(details).not.toBeNull();
      const summary = details?.querySelector('summary')?.textContent?.trim();
      const legends = [...(details?.querySelectorAll('legend') ?? [])].map((legend) =>
        legend.textContent?.trim(),
      );
      expect(legends).not.toContain(summary);
    }
  });

  it('uses the Dutch catalogue for all static semantic management copy', async () => {
    setLocale('nl');
    mountComponent(new MockFileManagerClient());
    await waitForLoaded();

    expect(root.querySelector('.fm-semantic-management')?.getAttribute('aria-label')).toBe(
      'Semantische onderdelen',
    );
    expect(root.textContent).toContain('Niet geïnstalleerd');
    expect(root.textContent).toContain('Installeren / inschakelen');
  });
});
