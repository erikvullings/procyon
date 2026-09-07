import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { MockFileManagerClient } from '../../api/client/mock-file-manager-client';
import { setLocale } from '../../i18n';
import type { Location, WorkspaceProjection } from '../../models';
import { SemanticLibraryManagement } from './semantic-library-management';

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
  it('shows active-folder consent and the safe policy projection', async () => {
    mountComponent(client);
    await waitForLoaded();

    expect(root.textContent).toContain('Not included');
    expect(root.textContent).toContain('mock-semantic-model');
    expect(root.textContent).toContain('Balanced');
    expect(root.textContent).toContain('Every 30 minutes');
    expect(root.textContent).toContain('normalized excerpts');
    expect(root.textContent).toContain(location.uri);
    expect(root.textContent).toContain('folder open in the active pane');
    expect(root.textContent).not.toContain('mock-volume');
  });

  it('requires disclosure confirmation and submits no estimate counts', async () => {
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
    expect(root.textContent).toContain('Normalized excerpts will be retained locally');
    expect(button('Include and index folder').disabled).toBe(true);
    expect(preview).toHaveBeenCalledWith({
      workspaceId: workspace.id,
      location,
      recursive: true,
    });

    root.querySelector<HTMLInputElement>('#fm-semantic-library-consent')?.click();
    m.redraw.sync();
    button('Include and index folder').click();
    await vi.waitFor(() => expect(root.textContent).toContain('Included here'));
    expect(confirm).toHaveBeenCalledWith({
      confirmationId: 'mock-enrol-confirmation-1',
      policyRevision: 1,
      workspaceId: workspace.id,
      location,
    });
    expect(confirm.mock.calls[0]?.[0]).not.toHaveProperty('estimatedFiles');
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

    button('Review exclusion').click();
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
    expect(button('Exclude and delete data').disabled).toBe(true);
    root.querySelector<HTMLInputElement>('#fm-semantic-library-exclusion-confirm')?.click();
    m.redraw.sync();
    button('Exclude and delete data').click();
    await vi.waitFor(() => expect(root.textContent).toContain('Excluded'));
    expect(root.textContent).toContain('Cleanup complete');
    expect(confirm.mock.calls[0]?.[0]).not.toHaveProperty('categories');
  });

  it('explains pause retention and queries while toggling pause separately', async () => {
    mountComponent(client);
    await waitForLoaded();

    expect(root.textContent).toContain('Existing indexed data and consent are preserved');
    expect(root.textContent).toContain('Queries can use the last complete generations');
    button('Pause ingestion').click();
    await vi.waitFor(() => expect(root.textContent).toContain('Ingestion paused'));
    button('Resume ingestion').click();
    await vi.waitFor(() => expect(root.textContent).toContain('Ingestion active'));
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
          eligibilityReasonCounts: [{ reason: 'hidden', count: 3 }],
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
    expect(root.textContent).toContain('removable volume is offline');
    expect(root.textContent).toContain('vocabulary-1');
    expect(root.textContent).toContain('Files requiring OCR');
    expect(root.textContent).toContain('/docs/Scanned reference.pdf');
    expect(root.textContent).toContain('pnpm dev:tauri:semantic:ocr');
    expect(root.querySelectorAll('.fm-semantic-library-override')).toHaveLength(7);
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
