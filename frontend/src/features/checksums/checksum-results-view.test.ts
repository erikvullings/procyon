import m from 'mithril';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { ChecksumEntry } from '../../models';
import { ChecksumResultsView, type ChecksumResultsViewAttrs } from './checksum-results-view';

const mounted: HTMLElement[] = [];

afterEach(() => {
  for (const element of mounted) {
    m.mount(element, null);
    element.remove();
  }
  mounted.length = 0;
});

const ENTRY: ChecksumEntry = {
  location: { providerId: 'local', uri: 'file:///root/a.txt' },
  relativePath: 'a.txt',
  size: 3,
  checksums: { sha256: 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad' },
};

function render(attrs: Partial<ChecksumResultsViewAttrs> = {}): HTMLElement {
  const root = document.createElement('div');
  document.body.appendChild(root);
  mounted.push(root);
  const full: ChecksumResultsViewAttrs = {
    algorithms: ['sha256'],
    entries: [ENTRY],
    totalEntries: 1,
    isComplete: true,
    isCancelled: false,
    onCopy: vi.fn(),
    onSave: vi.fn(),
    suggestedFileName: (algorithm) => `checksums.${algorithm}`,
    onVerify: vi.fn(),
    onCancel: vi.fn(),
    onClose: vi.fn(),
    ...attrs,
  };
  // Mounted rather than rendered: the view keeps internal state (the open save
  // form and its filename), so it needs a real redraw root to re-render after
  // a click.
  m.mount(root, { view: () => m(ChecksumResultsView, full) });
  return root;
}

describe('ChecksumResultsView', () => {
  it('lists an entry with its abbreviated digest', () => {
    const root = render();
    expect(root.textContent).toContain('a.txt');
    expect(root.querySelector('code')?.getAttribute('title')).toBe(ENTRY.checksums.sha256);
  });

  it('keeps copy and save as separate actions', () => {
    const root = render();
    const labels = [...root.querySelectorAll('.checksum-results__actions button')].map(
      (button) => button.textContent,
    );
    expect(labels).toContain('Copy');
    expect(labels?.some((label) => label?.startsWith('Save checksum file'))).toBe(true);
  });

  it('copies without opening the save form', () => {
    const onCopy = vi.fn();
    const root = render({ onCopy });
    const copy = [
      ...root.querySelectorAll<HTMLButtonElement>('.checksum-results__actions button'),
    ].find((button) => button.textContent === 'Copy');
    copy?.click();
    expect(onCopy).toHaveBeenCalledWith('sha256');
    expect(root.querySelector('.checksum-results__save')).toBeNull();
  });

  it('opens a save form prefilled with the suggested filename', () => {
    const root = render();
    root.querySelector<HTMLButtonElement>('.checksum-results__save-open')?.click();
    m.redraw.sync();
    const input = root.querySelector<HTMLInputElement>('.checksum-results__save-name');
    expect(input).not.toBeNull();
    expect(input?.value).toBe('checksums.sha256');
  });

  it('saves the file rather than copying it to the clipboard', () => {
    const onSave = vi.fn();
    const onCopy = vi.fn();
    const root = render({ onSave, onCopy });

    root.querySelector<HTMLButtonElement>('.checksum-results__save-open')?.click();
    m.redraw.sync();
    const form = root.querySelector<HTMLFormElement>('.checksum-results__save');
    expect(form).not.toBeNull();
    form?.dispatchEvent(new Event('submit', { cancelable: true, bubbles: true }));

    expect(onSave).toHaveBeenCalledWith('sha256', 'checksums.sha256');
    expect(onCopy).not.toHaveBeenCalled();
  });

  it('saves under an edited filename', () => {
    const onSave = vi.fn();
    const root = render({ onSave });
    root.querySelector<HTMLButtonElement>('.checksum-results__save-open')?.click();
    m.redraw.sync();

    const input = root.querySelector<HTMLInputElement>('.checksum-results__save-name');
    if (input !== null) {
      input.value = 'release-sums.sha256';
      input.dispatchEvent(new Event('input', { bubbles: true }));
    }
    root
      .querySelector<HTMLFormElement>('.checksum-results__save')
      ?.dispatchEvent(new Event('submit', { cancelable: true, bubbles: true }));

    expect(onSave).toHaveBeenCalledWith('sha256', 'release-sums.sha256');
  });

  it('confirms where the file was written', () => {
    const root = render({
      savedTo: { providerId: 'local', uri: 'file:///root/my%20sums.sha256' },
    });
    const saved = root.querySelector('.checksum-results__saved');
    expect(saved?.textContent).toContain('file:///root/my sums.sha256');
  });

  it('reports a save failure', () => {
    const root = render({ error: 'destination exists' });
    expect(root.querySelector('.checksum-results__error')?.textContent).toBe('destination exists');
  });

  it('offers verification against pasted checksum-file text', () => {
    const onVerify = vi.fn();
    const root = render({ onVerify });
    const textarea = root.querySelector<HTMLTextAreaElement>('.checksum-results__verify-input');
    if (textarea !== null) {
      textarea.value = 'aa  a.txt';
      textarea.dispatchEvent(new Event('input', { bubbles: true }));
    }
    m.redraw.sync();
    const verify = [...root.querySelectorAll<HTMLButtonElement>('button')].find(
      (button) => button.textContent === 'Verify',
    );
    verify?.click();
    expect(onVerify).toHaveBeenCalledWith('aa  a.txt');
  });

  it('summarises a verification report', () => {
    const root = render({
      verification: {
        jobId: 'job-1',
        results: [
          { path: 'a.txt', status: 'match' },
          { path: 'b.txt', status: 'missing' },
        ],
        matched: 1,
        mismatched: 0,
        missing: 1,
      },
    });
    expect(root.querySelector('.checksum-results__verification-summary')?.textContent).toBe(
      '1 matched, 0 mismatched, 1 missing',
    );
  });
});
