import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { MockFileManagerClient } from '../../api/client/mock-file-manager-client';
import { setLocale } from '../../i18n';
import type { Location, WorkspaceProjection } from '../../models';
import {
  SemanticFolderEnrolmentPrompt,
  SemanticLibraryManagement,
} from './semantic-library-management';

let root: HTMLElement;
const location: Location = { providerId: 'file', uri: 'mock:///' };
let workspace: WorkspaceProjection;
let client: MockFileManagerClient;

beforeEach(async () => {
  setLocale('en');
  root = document.createElement('div');
  document.body.appendChild(root);
  client = new MockFileManagerClient();
  workspace = await client.startWorkspace();
});

afterEach(() => {
  setLocale('en');
  m.mount(root, null);
  root.remove();
});

function mountComponent(client: MockFileManagerClient, activeLocation = location): void {
  m.mount(root, {
    view: () =>
      m(SemanticLibraryManagement, {
        client,
        workspaceId: workspace.id,
        location: activeLocation,
      }),
  });
  m.redraw.sync();
}

async function waitForLoaded(): Promise<void> {
  await vi.waitFor(() => expect(root.querySelector('.fm-semantic-library-loading')).toBeNull());
  m.redraw.sync();
}

function button(label: string): HTMLButtonElement {
  const match = [...root.querySelectorAll<HTMLButtonElement>('button')].find(
    (candidate) => candidate.textContent?.trim() === label,
  );
  if (match === undefined) throw new Error(`No button labelled "${label}"`);
  return match;
}

describe('SemanticLibraryManagement', () => {
  it('distinguishes observed sources from searchable documents while indexing', async () => {
    const preview = await client.previewSemanticEnrolment({
      workspaceId: workspace.id,
      location,
      recursive: true,
    });
    await client.confirmSemanticEnrolment({
      confirmationId: preview.confirmationId,
      policyRevision: preview.policyRevision,
      workspaceId: workspace.id,
      location,
    });
    const status = await client.getSemanticLibraryStatus();
    const component = await client.getSemanticComponentStatus();
    if (status.roots[0] === undefined) {
      throw new Error('Missing included root in mock');
    }
    vi.spyOn(client, 'getSemanticComponentStatus').mockResolvedValue({
      ...component,
      activeModel: {
        profile: 'embeddingGemma2',
        identity: { modelId: 'gemma', revision: 'test' },
      },
    });
    vi.spyOn(client, 'getGemmaLibrarySetup').mockResolvedValue({
      dimensions: 512,
      images: true,
      audio: true,
      video: true,
      imageDevice: 'metal',
      pdfImagePageBudget: null,
      lastWorkerDevice: 'metal',
    });
    let indexedSources = 0;
    let unverifiedLegacySources = 0;
    const rootId = status.roots[0].id;
    vi.spyOn(client, 'listKnowledgeRoots').mockImplementation(async () => [
      {
        rootId,
        label: 'mock',
        location,
        recursive: true,
        available: true,
        indexedGeneration: 0,
        indexedSources,
        confirmedCompletedSources: indexedSources,
        unverifiedLegacySources,
      },
    ]);
    await client.subscribe(() => undefined);
    mountComponent(client);
    await waitForLoaded();
    expect(root.textContent).toContain('0 confirmed indexed documents');
    const details = root.querySelector<HTMLDetailsElement>('.fm-semantic-library-root details');
    expect(details?.textContent).not.toContain('Reconciliation generation');
    expect(details?.textContent).not.toContain('Indexing compute policy');
    expect(details?.textContent).toContain('Last worker launch: Metal');
    indexedSources = 1;
    client.emit({
      eventId: 1,
      timestamp: new Date().toISOString(),
      payload: {
        type: 'semantic.ingestionProgress',
        jobId: `root:${rootId}`,
        stage: 'reconciling',
        completed: 4,
        total: 0,
        errors: 0,
      },
    });
    await vi.waitFor(() => expect(root.textContent).toContain('1 confirmed indexed document'));
    expect(root.textContent).toContain('4 entries scanned (not all are indexed)');
    unverifiedLegacySources = 2;
    client.emit({
      eventId: 2,
      timestamp: new Date().toISOString(),
      payload: {
        type: 'semantic.ingestionProgress',
        jobId: `root:${rootId}`,
        stage: 'reconciling',
        completed: 5,
        total: 0,
        errors: 0,
      },
    });
    await vi.waitFor(() =>
      expect(root.textContent).toContain('2 earlier sources not yet verified'),
    );
  });

  it('shows active-folder consent and the safe policy projection', async () => {
    mountComponent(client);
    await waitForLoaded();

    expect(root.textContent).toContain('Not included');
    expect(root.textContent).toContain('mock-semantic-model');
    expect(root.textContent).toContain('Balanced');
    expect(root.textContent).toContain('Every 30 minutes');
    expect(root.textContent).toContain('normalized excerpts');
    expect(root.textContent).toContain(location.uri);
    expect(root.textContent).toContain('open it in a pane');
    expect(
      root.querySelector<HTMLDetailsElement>('details.fm-semantic-library-details')?.open,
    ).toBe(false);
    expect(root.textContent).not.toContain('mock-volume');
  });

  it('includes a folder with one confirmation and submits no estimate counts', async () => {
    const preview = vi.spyOn(client, 'previewSemanticEnrolment');
    const confirm = vi.spyOn(client, 'confirmSemanticEnrolment');
    mountComponent(client);
    await waitForLoaded();

    button('Review indexing').click();
    await vi.waitFor(() => expect(root.textContent).toContain('Partial estimate'));
    expect(root.textContent).toContain('Estimated files');
    expect(root.textContent).toContain('42');
    expect(root.textContent).toContain('Missing model download');
    expect(root.textContent).toContain('Unsupported MIME type');
    expect(root.textContent).toContain('stores extracted text and embeddings locally');
    expect(root.querySelector('#fm-semantic-library-consent')).toBeNull();
    expect(button('Include and index folder').disabled).toBe(false);
    expect(preview).toHaveBeenCalledWith({
      workspaceId: workspace.id,
      location,
      recursive: true,
      pdfImagesEnabled: false,
      pdfImagePageBudget: null,
    });

    button('Cancel').click();
    await vi.waitFor(() => expect(root.querySelector('.fm-semantic-library-plan')).toBeNull());
    expect(confirm).not.toHaveBeenCalled();
    button('Review indexing').click();
    await vi.waitFor(() => expect(root.querySelector('.fm-semantic-library-plan')).not.toBeNull());
    button('Include and index folder').click();
    await vi.waitFor(() =>
      expect(root.querySelector('.fm-semantic-library-root-heading')?.textContent).toContain(
        location.uri,
      ),
    );
    expect(confirm).toHaveBeenCalledWith(
      expect.objectContaining({
        policyRevision: 1,
        workspaceId: workspace.id,
        location,
      }),
    );
    expect(confirm.mock.calls[0]?.[0]).not.toHaveProperty('estimatedFiles');
  });

  it('requires explicit CPU PDF-image consent and explains the four-page budget', async () => {
    const component = await client.getSemanticComponentStatus();
    vi.spyOn(client, 'getSemanticComponentStatus').mockResolvedValue({
      ...component,
      activeModel: {
        profile: 'embeddingGemma2',
        identity: { modelId: 'gemma', revision: 'test' },
      },
    });
    vi.spyOn(client, 'getGemmaLibrarySetup').mockResolvedValue({
      dimensions: 512,
      images: true,
      audio: false,
      video: false,
      imageDevice: 'cpu',
      pdfImagePageBudget: 4,
    });
    const preview = vi.spyOn(client, 'previewSemanticEnrolment');
    mountComponent(client);
    await waitForLoaded();

    button('Review indexing').click();
    await vi.waitFor(() =>
      expect(
        root.querySelector<HTMLInputElement>('.fm-semantic-library-pdf-images input'),
      ).not.toBeNull(),
    );
    const checkbox = root.querySelector<HTMLInputElement>('.fm-semantic-library-pdf-images input');
    expect(checkbox?.checked).toBe(false);
    expect(root.textContent).not.toContain('Slow on CPU');

    checkbox?.click();
    await vi.waitFor(() =>
      expect(preview).toHaveBeenLastCalledWith({
        workspaceId: workspace.id,
        location,
        recursive: true,
        pdfImagesEnabled: true,
        pdfImagePageBudget: 4,
      }),
    );
    await vi.waitFor(() => expect(root.textContent).toContain('first 4 page images'));
    expect(root.textContent).toContain('about 8 minutes for an 8-page scan');
    expect(root.textContent).toContain('PDF text is unaffected');
    button('Include and index folder').click();
    await vi.waitFor(() => expect(root.textContent).toContain('first 4 pages per PDF'));
  });

  it('defaults PDF-image consent on for a Metal target without a CPU warning', async () => {
    const component = await client.getSemanticComponentStatus();
    vi.spyOn(client, 'getSemanticComponentStatus').mockResolvedValue({
      ...component,
      activeModel: {
        profile: 'embeddingGemma2',
        identity: { modelId: 'gemma', revision: 'test' },
      },
    });
    vi.spyOn(client, 'getGemmaLibrarySetup').mockResolvedValue({
      dimensions: 512,
      images: true,
      audio: false,
      video: false,
      imageDevice: 'metal',
      pdfImagePageBudget: null,
    });
    const preview = vi.spyOn(client, 'previewSemanticEnrolment');
    mountComponent(client);
    await waitForLoaded();

    button('Review indexing').click();
    await vi.waitFor(() => expect(root.querySelector('.fm-semantic-library-plan')).not.toBeNull());
    expect(
      root.querySelector<HTMLInputElement>('.fm-semantic-library-pdf-images input')?.checked,
    ).toBe(true);
    expect(root.textContent).not.toContain('Slow on CPU');
    expect(preview).toHaveBeenLastCalledWith({
      workspaceId: workspace.id,
      location,
      recursive: true,
      pdfImagesEnabled: true,
      pdfImagePageBudget: null,
    });
  });

  it('lists every destructive category before exclusion confirmation', async () => {
    const preview = await client.previewSemanticEnrolment({
      workspaceId: workspace.id,
      location,
      recursive: true,
    });
    await client.confirmSemanticEnrolment({
      confirmationId: preview.confirmationId,
      policyRevision: preview.policyRevision,
      workspaceId: workspace.id,
      location,
    });
    const confirm = vi.spyOn(client, 'confirmSemanticExclusion');
    mountComponent(client);
    await waitForLoaded();

    button('Remove').click();
    await vi.waitFor(() => expect(root.textContent).toContain('Saved conversation evidence pins'));
    for (const label of [
      'Occurrences',
      'Normalized excerpts',
      'Summaries',
      'Labels',
      'Unreferenced vectors',
      'Saved conversation evidence pins',
    ]) {
      expect(root.textContent).toContain(label);
    }
    expect(root.querySelector('#fm-semantic-library-exclusion-confirm')).toBeNull();
    expect(button('Exclude and delete data').disabled).toBe(false);
    button('Cancel').click();
    expect(confirm).not.toHaveBeenCalled();
    button('Remove').click();
    await vi.waitFor(() => expect(root.textContent).toContain('Saved conversation evidence pins'));
    button('Exclude and delete data').click();
    await vi.waitFor(() => expect(root.textContent).toContain('Excluded'));
    expect(root.textContent).toContain('Cleanup complete');
    expect(confirm.mock.calls[0]?.[0]).not.toHaveProperty('categories');
  });

  it('hides PDF-image consent and skips Gemma setup for an E5 library', async () => {
    const component = await client.getSemanticComponentStatus();
    vi.spyOn(client, 'getSemanticComponentStatus').mockResolvedValue({
      ...component,
      activeModel: {
        profile: 'compactMultilingual',
        identity: { modelId: 'e5', revision: 'test' },
      },
    });
    const setup = vi.spyOn(client, 'getGemmaLibrarySetup');
    mountComponent(client);
    await waitForLoaded();

    button('Review indexing').click();
    await vi.waitFor(() => expect(root.querySelector('.fm-semantic-library-plan')).not.toBeNull());
    expect(root.querySelector('.fm-semantic-library-pdf-images')).toBeNull();
    expect(setup).not.toHaveBeenCalled();
  });

  it('removes a listed folder without navigating to it, while retaining the source files', async () => {
    const preview = await client.previewSemanticEnrolment({
      workspaceId: workspace.id,
      location,
      recursive: true,
    });
    await client.confirmSemanticEnrolment({
      confirmationId: preview.confirmationId,
      policyRevision: preview.policyRevision,
      workspaceId: workspace.id,
      location,
    });
    const plan = vi.spyOn(client, 'planSemanticExclusion');
    const confirm = vi.spyOn(client, 'confirmSemanticExclusion');
    m.mount(root, {
      view: () => m(SemanticLibraryManagement, { client, workspaceId: workspace.id }),
    });
    m.redraw.sync();
    await waitForLoaded();

    button('Remove').click();
    await vi.waitFor(() => expect(plan).toHaveBeenCalled());
    expect(plan.mock.calls[0]?.[0]).toMatchObject({ workspaceId: workspace.id, location });
    await vi.waitFor(() =>
      expect(root.textContent).toContain('Your original files are not deleted'),
    );
    button('Exclude and delete data').click();
    await vi.waitFor(() => expect(confirm).toHaveBeenCalled());
    expect(confirm.mock.calls[0]?.[0]).toMatchObject({ workspaceId: workspace.id, location });
    expect(root.textContent).toContain('Moving or deleting an included folder');
  });

  it('explains pause retention and queries while toggling pause separately', async () => {
    mountComponent(client);
    await waitForLoaded();

    button('Pause indexing').click();
    await vi.waitFor(() => expect(root.textContent).toContain('Indexing paused'));
    expect(root.textContent).toContain('Existing indexed data and consent are preserved');
    expect(root.textContent).toContain('Queries can use the last complete generations');
    button('Resume indexing').click();
    await vi.waitFor(() => expect(root.textContent).toContain('Indexing on'));
  });

  it('shows unavailable sources and only fixed safe eligibility overrides', async () => {
    vi.spyOn(client, 'getSemanticFolderStatus').mockResolvedValue({
      consent: 'includedHere',
      rootId: 'root-1',
      exclusionId: null,
      workspaceReferenced: true,
      sourceAvailable: false,
      unavailableReason: 'removable volume is offline',
    });
    vi.spyOn(client, 'getSemanticLibraryStatus').mockResolvedValue({
      ...(await client.getSemanticLibraryStatus()),
      roots: [
        {
          id: 'root-1',
          location,
          recursive: true,
          stableIdentityVerified: true,
          workspaceReferences: [workspace.id],
          eligibilityOverrides: [],
          attachedVocabularyIds: ['vocabulary-1'],
          eligibilityReasonCounts: [
            { reason: 'hidden', count: 3 },
            { reason: 'oversized', count: 1 },
          ],
          ocrRequiredFiles: [
            {
              providerId: 'file',
              uri: 'file:///docs/Scanned%20reference.pdf',
            },
          ],
          availability: {
            state: 'temporarilyUnavailable',
            reason: 'removable volume is offline',
          },
          reconciliationGeneration: 2,
          indexedGeneration: 2,
          exclusions: [],
        },
      ],
    });
    mountComponent(client);
    await waitForLoaded();

    expect(root.textContent).toContain('Source unavailable');
    expect(root.querySelector('.fm-semantic-library-skipped')?.textContent).toContain(
      '1 file exceeds the 64 MiB indexing limit and is not searchable',
    );
    setLocale('nl');
    m.redraw.sync();
    expect(root.querySelector('.fm-semantic-library-skipped')?.textContent).toContain(
      '1 bestand overschrijdt de indexeringslimiet van 64 MiB',
    );
    setLocale('en');
    m.redraw.sync();
    expect(root.textContent).toContain('removable volume is offline');
    const details = root.querySelector<HTMLDetailsElement>('.fm-semantic-library-root details');
    expect(details?.open).toBe(false);
    details?.setAttribute('open', '');
    expect(root.textContent).toContain('vocabulary-1');
    expect(root.textContent).toContain('Scanned PDFs without searchable text');
    expect(root.textContent).toContain('/docs/Scanned reference.pdf');
    expect(root.textContent).toContain('which scanned PDFs to make searchable');
    expect(root.querySelectorAll('.fm-semantic-library-override')).toHaveLength(7);
    expect(
      [...root.querySelectorAll<HTMLSelectElement>('.fm-semantic-library-override select')].every(
        (select) => select.classList.contains('browser-default'),
      ),
    ).toBe(true);
    expect(root.textContent).not.toContain('Symlink outside root');
    expect(root.textContent).not.toContain('Over budget');
  });

  it('shows failed cleanup and exposes a resumable recovery action', async () => {
    const failedRoot = {
      id: 'root-1',
      location,
      recursive: true,
      stableIdentityVerified: true,
      workspaceReferences: [workspace.id],
      eligibilityOverrides: [],
      attachedVocabularyIds: [],
      eligibilityReasonCounts: [],
      ocrRequiredFiles: [],
      availability: { state: 'available' as const },
      reconciliationGeneration: 1,
      indexedGeneration: 1,
      exclusions: [
        {
          id: 'exclusion-1',
          location,
          cleanup: {
            planId: 'plan-1',
            status: 'failed' as const,
            categories: [
              {
                category: 'summaries' as const,
                totalItems: 3,
                completedItems: 1,
                complete: false,
                lastError: 'worker interrupted',
              },
            ],
          },
        },
      ],
    };
    const current = await client.getSemanticLibraryStatus();
    vi.spyOn(client, 'getSemanticFolderStatus').mockResolvedValue({
      consent: 'excluded',
      rootId: failedRoot.id,
      exclusionId: 'exclusion-1',
      workspaceReferenced: true,
      sourceAvailable: true,
      unavailableReason: null,
    });
    vi.spyOn(client, 'getSemanticLibraryStatus').mockResolvedValue({
      ...current,
      roots: [failedRoot],
    });
    const resume = vi.spyOn(client, 'resumeSemanticCleanup').mockResolvedValue({
      ...current,
      revision: current.revision + 1,
      roots: [
        {
          ...failedRoot,
          exclusions: [
            {
              id: 'exclusion-1',
              location,
              cleanup: {
                planId: 'plan-1',
                status: 'complete',
                categories: [],
              },
            },
          ],
        },
      ],
    });
    mountComponent(client);
    await waitForLoaded();

    expect(root.textContent).toContain('Cleanup failed');
    expect(root.textContent).toContain('worker interrupted');
    button('Resume cleanup').click();
    await vi.waitFor(() =>
      expect(resume).toHaveBeenCalledWith({ planId: 'plan-1', policyRevision: 1 }),
    );
    await vi.waitFor(() => expect(root.textContent).toContain('Cleanup complete'));
  });

  it('offers to resume a cleanup that is still running after an interruption', async () => {
    const runningRoot = {
      id: 'root-1',
      location,
      recursive: true,
      stableIdentityVerified: true,
      workspaceReferences: [workspace.id],
      eligibilityOverrides: [],
      attachedVocabularyIds: [],
      eligibilityReasonCounts: [],
      ocrRequiredFiles: [],
      availability: { state: 'available' as const },
      reconciliationGeneration: 1,
      indexedGeneration: 1,
      exclusions: [
        {
          id: 'exclusion-1',
          location,
          cleanup: {
            planId: 'plan-1',
            // A process killed mid-cleanup leaves a running plan with durable
            // per-batch progress and no error at all.
            status: 'running' as const,
            categories: [
              {
                category: 'occurrences' as const,
                totalItems: 70,
                completedItems: 64,
                complete: false,
                lastError: null,
              },
            ],
          },
        },
      ],
    };
    const current = await client.getSemanticLibraryStatus();
    vi.spyOn(client, 'getSemanticFolderStatus').mockResolvedValue({
      consent: 'excluded',
      rootId: runningRoot.id,
      exclusionId: 'exclusion-1',
      workspaceReferenced: true,
      sourceAvailable: true,
      unavailableReason: null,
    });
    vi.spyOn(client, 'getSemanticLibraryStatus').mockResolvedValue({
      ...current,
      roots: [runningRoot],
    });
    const resume = vi.spyOn(client, 'resumeSemanticCleanup').mockResolvedValue({
      ...current,
      revision: current.revision + 1,
      roots: [
        {
          ...runningRoot,
          exclusions: [
            {
              id: 'exclusion-1',
              location,
              cleanup: { planId: 'plan-1', status: 'complete', categories: [] },
            },
          ],
        },
      ],
    });
    mountComponent(client);
    await waitForLoaded();

    expect(root.textContent).toContain('Cleanup running');
    expect(root.textContent).toContain('64/70');
    button('Resume cleanup').click();
    await vi.waitFor(() =>
      expect(resume).toHaveBeenCalledWith({ planId: 'plan-1', policyRevision: 1 }),
    );
    await vi.waitFor(() => expect(root.textContent).toContain('Cleanup complete'));
  });

  it('reports an unavailable library to a principal with no permitted operations', async () => {
    const capabilities = vi
      .spyOn(client, 'getSemanticLibraryCapabilities')
      .mockResolvedValue({ authority: 'administratorProvisioned', operations: [] });
    const status = vi.spyOn(client, 'getSemanticLibraryStatus');
    mountComponent(client);
    await waitForLoaded();

    expect(capabilities).toHaveBeenCalled();
    expect(status).not.toHaveBeenCalled();
    expect(root.textContent).toContain('unavailable');
    expect(root.querySelectorAll('button')).toHaveLength(0);
  });

  it('renders Dutch consent labels and stale errors accessibly', async () => {
    setLocale('nl');
    vi.spyOn(client, 'previewSemanticEnrolment').mockRejectedValue(
      new Error('Het beleid is gewijzigd'),
    );
    mountComponent(client);
    await waitForLoaded();

    expect(root.textContent).toContain('Niet opgenomen');
    button('Indexering controleren').click();
    await vi.waitFor(() =>
      expect(root.querySelector('[role="alert"]')?.textContent).toContain(
        'Het beleid is gewijzigd',
      ),
    );
  });
});

describe('SemanticFolderEnrolmentPrompt', () => {
  it('shows impact without a second checkbox and keeps failures visible', async () => {
    const preview = await client.previewSemanticEnrolment({
      workspaceId: workspace.id,
      location,
      recursive: true,
    });
    vi.spyOn(client, 'previewSemanticEnrolment').mockResolvedValue({
      ...preview,
      estimate: {
        ...preview.estimate,
        completeness: 'unavailable',
        estimatedFiles: null,
        estimatedSourceBytes: null,
        estimatedExtractedBytes: null,
        estimatedVectorBytes: null,
        estimatedAdditionalLocalBytes: null,
        missingModelDownloadBytes: null,
        skippedReasonCounts: [],
        exceededBudgets: [],
        unavailableReason:
          'A provider-neutral recursive estimate is not available for this source.',
      },
    });
    vi.spyOn(client, 'confirmSemanticEnrolment').mockRejectedValue(
      new Error('Indexing unavailable'),
    );
    const onEnrolled = vi.fn();
    const onCancel = vi.fn();
    m.mount(root, {
      view: () =>
        m(SemanticFolderEnrolmentPrompt, {
          client,
          workspaceId: workspace.id,
          location,
          onEnrolled,
          onCancel,
        }),
    });
    await vi.waitFor(() => expect(root.textContent).toContain('OK, index folder'));
    expect(root.textContent).toContain(location.uri);
    expect(root.textContent).toContain('subfolders');
    expect(root.textContent).toContain('stores extracted text and embeddings locally');
    expect(root.textContent).not.toContain('Estimate unavailable');
    expect(root.textContent).not.toContain('additional disk space');
    expect(root.textContent).not.toContain('No unsupported or skipped content');
    expect(root.textContent).not.toContain('provider-neutral');
    expect(root.querySelector('details')).toBeNull();
    expect(root.querySelectorAll<HTMLInputElement>('input[type="checkbox"]')).toHaveLength(0);

    button('OK, index folder').click();
    await vi.waitFor(() => expect(root.textContent).toContain('Indexing unavailable'));
    expect(onEnrolled).not.toHaveBeenCalled();
    button('Cancel').click();
    expect(onCancel).toHaveBeenCalledOnce();
  });

  it('retains a real budget alert without displaying an unavailable estimate', async () => {
    const preview = await client.previewSemanticEnrolment({
      workspaceId: workspace.id,
      location,
      recursive: true,
    });
    vi.spyOn(client, 'previewSemanticEnrolment').mockResolvedValue({
      ...preview,
      estimate: {
        ...preview.estimate,
        completeness: 'unavailable',
        skippedReasonCounts: [],
        exceededBudgets: ['sourceBytes'],
      },
    });
    m.mount(root, {
      view: () =>
        m(SemanticFolderEnrolmentPrompt, {
          client,
          workspaceId: workspace.id,
          location,
          onEnrolled: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    await vi.waitFor(() => expect(root.textContent).toContain('Estimated source bytes'));
    expect(root.querySelector('.fm-semantic-library-plan [role="alert"]')).not.toBeNull();
    expect(root.textContent).not.toContain('Estimated files');
    expect(root.textContent).not.toContain('Unsupported MIME type');
  });

  it('shows only measured values and positive skip counts in a partial preview', async () => {
    const preview = await client.previewSemanticEnrolment({
      workspaceId: workspace.id,
      location,
      recursive: true,
    });
    vi.spyOn(client, 'previewSemanticEnrolment').mockResolvedValue({
      ...preview,
      estimate: {
        ...preview.estimate,
        completeness: 'partial',
        estimatedFiles: 3,
        estimatedSourceBytes: null,
        estimatedExtractedBytes: null,
        estimatedVectorBytes: null,
        estimatedAdditionalLocalBytes: null,
        missingModelDownloadBytes: null,
        skippedReasonCounts: [
          { reason: 'unsupportedMime', count: 2 },
          { reason: 'hidden', count: 0 },
        ],
      },
    });
    m.mount(root, {
      view: () =>
        m(SemanticFolderEnrolmentPrompt, {
          client,
          workspaceId: workspace.id,
          location,
          onEnrolled: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    await vi.waitFor(() => expect(root.textContent).toContain('Partial estimate'));
    expect(root.textContent).toContain('Estimated files3');
    expect(root.textContent).toContain('Unsupported MIME type: 2');
    expect(root.textContent).not.toContain('Hidden');
    expect(root.textContent).not.toContain('Estimated source bytes');
    expect(root.textContent).not.toContain('Unavailable');
  });
});
