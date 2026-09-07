import { axe } from 'jest-axe';
import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { MockFileManagerClient } from '../../api/client/mock-file-manager-client';
import { setLocale } from '../../i18n';
import type { SemanticOcrStatus } from '../../models';
import { SemanticOcrManagement } from './semantic-ocr-management';

let root: HTMLElement;

const reportedFiles = [
  {
    rootId: 'root-a',
    location: { providerId: 'local', uri: 'file:///Documents/Invoice%201.pdf' },
  },
  {
    rootId: 'root-a',
    location: { providerId: 'local', uri: 'file:///Documents/Invoice%202.pdf' },
  },
  {
    rootId: 'root-b',
    location: { providerId: 'local', uri: 'file:///Archive/Scan.pdf' },
  },
] as const;

function status(overrides: Partial<SemanticOcrStatus> = {}): SemanticOcrStatus {
  return {
    enabled: false,
    availability: {
      state: 'available',
      executable: '/opt/homebrew/bin/ocrmypdf',
      version: '16.10.4',
    },
    reportedFiles,
    jobs: [],
    ...overrides,
  };
}

function button(label: string): HTMLButtonElement {
  const match = [...root.querySelectorAll<HTMLButtonElement>('button')].find(
    (candidate) => candidate.textContent?.trim() === label,
  );
  if (match === undefined) throw new Error(`No button labelled "${label}"`);
  return match;
}

async function mount(client: MockFileManagerClient): Promise<void> {
  m.mount(root, { view: () => m(SemanticOcrManagement, { client }) });
  await vi.waitFor(() => expect(root.querySelector('.fm-semantic-ocr-loading')).toBeNull());
  m.redraw.sync();
}

beforeEach(() => {
  setLocale('en');
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
  vi.useRealTimers();
});

describe('SemanticOcrManagement', () => {
  it('exposes keyboard-native controls for one, selected, root, and all reported scopes', async () => {
    const client = new MockFileManagerClient({
      semanticLifecycle: 'installedEnabled',
      semanticOcrStatus: status(),
    });
    const start = vi.spyOn(client, 'startSemanticOcrRemediation');
    const setConsent = vi.spyOn(client, 'setSemanticOcrConsent');
    await mount(client);

    expect(root.textContent).toContain('OCRmyPDF 16.10.4');
    expect(root.textContent).toContain('/opt/homebrew/bin/ocrmypdf');
    const consent = root.querySelector<HTMLInputElement>('#fm-semantic-ocr-consent');
    expect(consent?.type).toBe('checkbox');
    consent?.click();
    await vi.waitFor(() => expect(setConsent).toHaveBeenCalledWith(true));
    await vi.waitFor(() => expect(button('Run OCR for this file').disabled).toBe(false));

    button('Run OCR for this file').click();
    await vi.waitFor(() =>
      expect(start).toHaveBeenCalledWith({ scope: 'oneFile', file: reportedFiles[0] }),
    );
    await vi.waitFor(() => expect(button('Run OCR for selected files').disabled).toBe(true));

    const selections = root.querySelectorAll<HTMLInputElement>(
      '.fm-semantic-ocr-file input[type="checkbox"]',
    );
    selections[0]?.click();
    selections[1]?.click();
    await vi.waitFor(() => expect(button('Run OCR for selected files').disabled).toBe(false));
    button('Run OCR for selected files').click();
    await vi.waitFor(() =>
      expect(start).toHaveBeenCalledWith({
        scope: 'selectedFiles',
        files: reportedFiles.slice(0, 2),
      }),
    );
    await vi.waitFor(() =>
      expect(root.querySelector<HTMLButtonElement>('[data-ocr-root="root-a"]')?.disabled).toBe(
        false,
      ),
    );

    root.querySelector<HTMLButtonElement>('[data-ocr-root="root-a"]')?.click();
    await vi.waitFor(() =>
      expect(start).toHaveBeenCalledWith({ scope: 'enrolledRoot', rootId: 'root-a' }),
    );
    await vi.waitFor(() => expect(button('Run OCR for all reported files').disabled).toBe(false));
    button('Run OCR for all reported files').click();
    await vi.waitFor(() => expect(start).toHaveBeenCalledWith({ scope: 'allReported' }));

    expect(root.querySelector('.fm-semantic-ocr-jobs[aria-live="polite"]')).not.toBeNull();
    expect(root.querySelectorAll('fieldset legend').length).toBeGreaterThan(0);
    expect(button('Run OCR for this file').type).toBe('button');
    expect(button('Run OCR for this file').getAttribute('aria-label')).toContain(
      '/Documents/Invoice 1.pdf',
    );
    expect(consent?.closest('label')?.textContent).toContain('Enable OCR remediation');
    const accessibility = await axe(root, {
      rules: {
        'color-contrast': { enabled: false },
        region: { enabled: false },
      },
    });
    expect(accessibility.violations).toEqual([]);
  });

  it('distinguishes missing and unsupported installations', async () => {
    const missing = new MockFileManagerClient({
      semanticLifecycle: 'installedEnabled',
      semanticOcrStatus: status({
        availability: {
          state: 'unavailable',
          reason: { code: 'missing' },
          guidance: 'Install OCRmyPDF from trusted platform documentation.',
        },
      }),
    });
    await mount(missing);

    expect(root.textContent).toContain('OCRmyPDF is not installed');
    expect(root.textContent).toContain('trusted platform documentation');
    expect(root.querySelector<HTMLInputElement>('#fm-semantic-ocr-consent')?.disabled).toBe(true);

    m.mount(root, null);
    const unsupported = new MockFileManagerClient({
      semanticLifecycle: 'installedEnabled',
      semanticOcrStatus: status({
        availability: {
          state: 'unavailable',
          reason: { code: 'unsupportedVersion', version: '18.0.0' },
          guidance: 'Install a supported release.',
        },
      }),
    });
    await mount(unsupported);
    expect(root.textContent).toContain('OCRmyPDF 18.0.0 is outside the supported version range');
  });

  it('announces success, execution failure, and post-OCR no-text outcomes and can cancel work', async () => {
    const now = Date.now();
    const client = new MockFileManagerClient({
      semanticLifecycle: 'installedEnabled',
      semanticOcrStatus: status({
        enabled: true,
        jobs: [
          {
            id: '00000000-0000-4000-8000-000000000001',
            state: 'running',
            createdAtMs: now,
            updatedAtMs: now,
            totalFiles: 3,
            processedFiles: 3,
            availabilityFailure: null,
            files: [
              { ...reportedFiles[0], outcome: { outcome: 'succeeded' } },
              {
                ...reportedFiles[1],
                outcome: { outcome: 'executionFailure', detail: 'OCR process exited with code 2' },
              },
              {
                ...reportedFiles[2],
                outcome: { outcome: 'postOcrNoText', detail: 'No searchable text remained' },
              },
            ],
          },
        ],
      }),
    });
    const cancel = vi.spyOn(client, 'cancelSemanticOcrRemediation');
    await mount(client);

    expect(root.textContent).toContain('Successfully ingested');
    expect(root.textContent).toContain('OCR execution failed');
    expect(root.textContent).toContain('OCR completed but no searchable text was found');
    button('Cancel OCR job').click();
    await vi.waitFor(() =>
      expect(cancel).toHaveBeenCalledWith('00000000-0000-4000-8000-000000000001'),
    );
  });
});
