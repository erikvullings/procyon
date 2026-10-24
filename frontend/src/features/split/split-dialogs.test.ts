import m from 'mithril';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { InspectSplitSetResult, Location } from '../../models';
import { CombineFilesDialog, type CombineFilesRequest } from './combine-files-dialog';
import { chosenPartBytes, SplitFileDialog, type SplitFileRequest } from './split-file-dialog';

const FILE: Location = { providerId: 'local', uri: 'file:///tmp/movie.mkv' };
const OTHER: Location = { providerId: 'local', uri: 'file:///other' };
const PART: Location = { providerId: 'local', uri: 'file:///tmp/movie.mkv.002' };

afterEach(() => {
  document.body.replaceChildren();
});

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
const button = (root: HTMLElement, label: string) =>
  [...root.querySelectorAll('button')].find((candidate) => candidate.textContent?.includes(label));

describe('SplitFileDialog', () => {
  const request: SplitFileRequest = {
    files: [{ location: FILE, name: 'movie.mkv', size: 250 * 1024 * 1024 }],
    otherPaneFolder: OTHER,
  };

  function open(onSplit = vi.fn()) {
    const root = document.createElement('div');
    document.body.append(root);
    const render = () =>
      m.render(root, m(SplitFileDialog, { request, onSplit, onCancel: vi.fn() }));
    render();
    return { root, render, onSplit };
  }

  it('shows the part count for the default 100 MiB size and splits with a manifest', () => {
    const { root, onSplit } = open();
    expect(root.textContent).toContain('movie.mkv: 3 parts');
    button(root, 'Split')?.click();
    expect(onSplit).toHaveBeenCalledWith(
      { partSizeBytes: 100 * 1024 * 1024, writeManifest: true },
      undefined,
      1,
    );
  });

  it('sends the other pane folder when chosen and no manifest when unticked', () => {
    const { root, render, onSplit } = open();
    const radios = root.querySelectorAll<HTMLInputElement>('input[type="radio"]');
    radios[1]?.click();
    const manifest = root.querySelector<HTMLInputElement>('input[type="checkbox"]');
    manifest?.click();
    render();
    button(root, 'Split')?.click();
    expect(onSplit).toHaveBeenCalledWith(
      { partSizeBytes: 100 * 1024 * 1024, writeManifest: false },
      OTHER,
      1,
    );
  });

  it('rejects an invalid custom size and a size that would need too many parts', () => {
    expect(chosenPartBytes('custom', 'x', 'MiB')).toBeUndefined();
    expect(chosenPartBytes('custom', '2', 'KiB')).toBe(2048);
    const { root, render } = open();
    const select = root.querySelector<HTMLSelectElement>('select');
    if (select === null) throw new Error('select missing');
    select.value = 'custom';
    select.dispatchEvent(new Event('change'));
    render();
    const size = root.querySelector<HTMLInputElement>('input[type="text"]');
    if (size === null) throw new Error('size input missing');
    size.value = '0';
    size.dispatchEvent(new Event('input'));
    render();
    expect(root.textContent).toContain('Enter a positive part size.');
    expect(button(root, 'Split')?.disabled).toBe(true);
  });
});

describe('CombineFilesDialog', () => {
  const request: CombineFilesRequest = { parts: [PART], otherPaneFolder: OTHER };
  const ready: InspectSplitSetResult = {
    outputName: 'movie.mkv',
    parts: [
      { name: 'movie.mkv.001', size: 5 },
      { name: 'movie.mkv.002', size: 5 },
    ],
    totalSize: 10,
    hasManifest: true,
  };

  async function open(result: InspectSplitSetResult, onCombine = vi.fn()) {
    const client = { inspectSplitSet: vi.fn().mockResolvedValue(result) };
    const root = document.createElement('div');
    document.body.append(root);
    const render = (req: CombineFilesRequest | undefined) =>
      m.render(root, m(CombineFilesDialog, { request: req, client, onCombine, onCancel: vi.fn() }));
    render(undefined);
    render(request);
    render(request);
    await settle();
    render(request);
    return { root, render: () => render(request), client, onCombine };
  }

  it('detects the set, mentions the manifest and joins with no rename', async () => {
    const { root, client, onCombine } = await open(ready);
    expect(client.inspectSplitSet).toHaveBeenCalledWith([PART]);
    expect(root.textContent).toContain('2 parts found');
    expect(root.textContent).toContain('checksum manifest was found');
    button(root, 'Combine')?.click();
    expect(onCombine).toHaveBeenCalledWith(undefined, undefined);
  });

  it('passes a changed output name and the other pane folder', async () => {
    const { root, render, onCombine } = await open(ready);
    const name = root.querySelector<HTMLInputElement>('input[type="text"]');
    if (name === null) throw new Error('name input missing');
    name.value = 'joined.mkv';
    name.dispatchEvent(new Event('input'));
    root.querySelectorAll<HTMLInputElement>('input[type="radio"]')[1]?.click();
    render();
    button(root, 'Combine')?.click();
    expect(onCombine).toHaveBeenCalledWith(OTHER, 'joined.mkv');
  });

  it('shows why a set with a gap cannot be joined and disables Combine', async () => {
    const { root } = await open({
      outputName: 'movie.mkv',
      parts: [],
      totalSize: 0,
      hasManifest: false,
      error: 'missing parts: movie.mkv.001',
    });
    expect(root.textContent).toContain('cannot be joined: missing parts: movie.mkv.001');
    expect(button(root, 'Combine')?.disabled).toBe(true);
  });
});
