import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { MockFileManagerClient } from '../../api/client/mock-file-manager-client';
import { DiagnosticsViewComponent } from './diagnostics-view';

let root: HTMLElement;

beforeEach(() => {
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
  vi.restoreAllMocks();
});

async function settle(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 0));
  m.redraw.sync();
}

describe('DiagnosticsViewComponent', () => {
  it('copies the recent errors, stack included, to the clipboard', async () => {
    const client = new MockFileManagerClient();
    await client.recordFrontendDiagnostic({
      code: 'FRONTEND_CONSOLE_ERROR',
      message: "TypeError: Cannot read properties of null (reading 'key')",
      context: 'at x (index.js:2:8032)',
      timestamp: '2026-10-29T12:24:56.000Z',
    });
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal('navigator', { clipboard: { writeText } });
    m.mount(root, { view: () => m(DiagnosticsViewComponent, { client }) });
    await settle();

    const button = [...root.querySelectorAll('button')].find(
      (candidate) => candidate.textContent === 'Copy Errors',
    );
    expect(button).toBeDefined();
    button?.click();
    await settle();

    expect(writeText).toHaveBeenCalledTimes(1);
    const text = String(writeText.mock.calls[0]?.[0]);
    expect(text).toContain('FRONTEND_CONSOLE_ERROR: TypeError: Cannot read properties of null');
    expect(text).toContain('at x (index.js:2:8032)');
    vi.unstubAllGlobals();
  });
});
