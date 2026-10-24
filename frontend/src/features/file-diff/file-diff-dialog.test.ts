import m from 'mithril';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { DiffRow, FileDiffResult, Location } from '../../models';
import { DIFF_ROW_HEIGHT, FileDiffDialog, type FileDiffRequest } from './file-diff-dialog';

const LEFT: Location = { providerId: 'local', uri: 'file:///tmp/a.txt' };
const RIGHT: Location = { providerId: 'local', uri: 'file:///tmp/b.txt' };
const request: FileDiffRequest = {
  left: { location: LEFT, name: 'a.txt' },
  right: { location: RIGHT, name: 'b.txt' },
};

afterEach(() => {
  document.body.replaceChildren();
});

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

function equalRow(line: number): DiffRow {
  return {
    kind: 'equal',
    leftLine: line,
    rightLine: line,
    leftText: `line ${line}`,
    rightText: `line ${line}`,
    leftSegments: [],
    rightSegments: [],
  };
}

function textResult(overrides: Partial<FileDiffResult> = {}): FileDiffResult {
  const rows: DiffRow[] = Array.from({ length: 5_000 }, (_, index) => equalRow(index + 1));
  rows[100] = {
    kind: 'replace',
    leftLine: 101,
    rightLine: 101,
    leftText: 'value',
    rightText: 'valve',
    leftSegments: [
      { text: 'val', changed: false },
      { text: 'u', changed: true },
      { text: 'e', changed: false },
    ],
    rightSegments: [
      { text: 'val', changed: false },
      { text: 'v', changed: true },
      { text: 'e', changed: false },
    ],
  };
  return {
    outcome: 'text',
    leftSize: 10,
    rightSize: 10,
    identical: false,
    sizeLimitBytes: 2 * 1024 * 1024,
    lineLimit: 100_000,
    rows,
    changeStarts: [100, 3_000],
    ...overrides,
  };
}

async function open(result: FileDiffResult) {
  const diffFiles = vi.fn().mockResolvedValue(result);
  const root = document.createElement('div');
  document.body.append(root);
  const render = (req: FileDiffRequest | undefined) =>
    m.render(root, m(FileDiffDialog, { request: req, client: { diffFiles }, onClose: vi.fn() }));
  render(undefined);
  render(request);
  render(request);
  await settle();
  render(request);
  return { root, render: () => render(request), diffFiles };
}

describe('FileDiffDialog', () => {
  it('requests the diff with the ignore options and renders only a window of the rows', async () => {
    const { root, diffFiles } = await open(textResult());
    expect(diffFiles).toHaveBeenCalledWith(
      LEFT,
      RIGHT,
      { ignoreWhitespace: false, ignoreCase: false },
      expect.any(AbortSignal),
    );
    const rows = root.querySelectorAll('.fm-diff-row');
    expect(rows.length).toBeGreaterThan(0);
    expect(rows.length).toBeLessThan(100);
    const spacer = root.querySelector<HTMLElement>('.fm-diff-spacer');
    expect(spacer?.style.height).toBe(`${5_000 * DIFF_ROW_HEIGHT}px`);
    expect(root.textContent).toContain('2 changes');
  });

  it('highlights the changed segments of a replaced line', async () => {
    const { root } = await open(
      textResult({ changeStarts: [0], rows: [textResult().rows[100] as DiffRow] }),
    );
    const marks = [...root.querySelectorAll('mark')].map((mark) => mark.textContent);
    expect(marks).toEqual(['u', 'v']);
  });

  it('re-requests the diff when an ignore option is toggled', async () => {
    const { root, render, diffFiles } = await open(textResult());
    const [whitespace] = root.querySelectorAll<HTMLInputElement>('.fm-diff-option input');
    if (whitespace === undefined) throw new Error('option missing');
    whitespace.checked = true;
    whitespace.dispatchEvent(new Event('change'));
    render();
    render();
    await settle();
    expect(diffFiles).toHaveBeenLastCalledWith(
      LEFT,
      RIGHT,
      { ignoreWhitespace: true, ignoreCase: false },
      expect.any(AbortSignal),
    );
  });

  it('navigates between change blocks', async () => {
    const { root, render } = await open(textResult());
    const next = [...root.querySelectorAll('button')].find((b) => b.textContent === 'Next change');
    next?.click();
    render();
    expect(root.textContent).toContain('Change 1 of 2');
    next?.click();
    render();
    expect(root.textContent).toContain('Change 2 of 2');
    const previous = [...root.querySelectorAll('button')].find(
      (b) => b.textContent === 'Previous change',
    );
    previous?.click();
    render();
    expect(root.textContent).toContain('Change 1 of 2');
  });

  it('reports binary results without rendering a diff', async () => {
    const { root } = await open(
      textResult({ outcome: 'binary', rows: [], changeStarts: [], firstDifferenceOffset: 42 }),
    );
    expect(root.textContent).toContain('First difference at byte offset 42');
    expect(root.querySelector('.fm-diff-row')).toBeNull();
  });

  it('explains when a file is too large to diff', async () => {
    const { root } = await open(
      textResult({ outcome: 'tooLarge', rows: [], changeStarts: [], leftSize: 5 * 1024 * 1024 }),
    );
    expect(root.textContent).toContain('too large to diff');
    expect(root.textContent).toContain('2.0 MiB');
    expect(root.querySelector('.fm-diff-row')).toBeNull();
  });

  it('shows a failure', async () => {
    const diffFiles = vi.fn().mockRejectedValue(new Error('boom'));
    const root = document.createElement('div');
    document.body.append(root);
    const render = (req: FileDiffRequest | undefined) =>
      m.render(root, m(FileDiffDialog, { request: req, client: { diffFiles }, onClose: vi.fn() }));
    render(undefined);
    render(request);
    render(request);
    await settle();
    render(request);
    expect(root.textContent).toContain('Could not compare the files: boom');
  });
});
