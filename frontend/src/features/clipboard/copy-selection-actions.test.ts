import { afterEach, describe, expect, it, vi } from 'vitest';

import type { EntryId, EntrySummary, Location } from '../../models';
import {
  type CopySelectionActionId,
  copySelectionToClipboard,
  selectionClipboardText,
  writeSystemClipboardText,
} from './copy-selection-actions';

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  Reflect.deleteProperty(document, 'execCommand');
});

function location(uri: string): Location {
  return { providerId: 'local', uri };
}

function entry(name: string, uri: string): EntrySummary {
  return {
    id: name as EntryId,
    location: location(uri),
    name,
    kind: 'file',
    hidden: false,
    readOnly: false,
    metadataRevision: 1,
  };
}

const activeDirectory = location('file:///workspace/project');
const selectedEntries = [
  entry('report one.md', 'file:///workspace/project/docs/report%20one.md'),
  entry('todo.txt', 'file:///workspace/project/todo.txt'),
];

describe('copy selection actions', () => {
  it.each<[CopySelectionActionId, string]>([
    ['core.copyName', 'report one.md\ntodo.txt'],
    ['core.copyPath', '/workspace/project/docs/report one.md\n/workspace/project/todo.txt'],
    ['core.copyRelativePath', 'docs/report one.md\ntodo.txt'],
  ])('formats %s for every selected entry', (actionId, expected) => {
    expect(selectionClipboardText(actionId, selectedEntries, activeDirectory)).toBe(expected);
  });

  it('writes the formatted multi-selection text through the supplied clipboard writer', async () => {
    const writeText = vi.fn<(text: string) => Promise<void>>().mockResolvedValue(undefined);

    await copySelectionToClipboard(
      'core.copyRelativePath',
      selectedEntries,
      activeDirectory,
      writeText,
    );

    expect(writeText).toHaveBeenCalledOnce();
    expect(writeText).toHaveBeenCalledWith('docs/report one.md\ntodo.txt');
  });

  it('returns no text and does not write when nothing is selected', async () => {
    const writeText = vi.fn<(text: string) => Promise<void>>().mockResolvedValue(undefined);

    expect(selectionClipboardText('core.copyPath', [], activeDirectory)).toBeUndefined();
    await expect(
      copySelectionToClipboard('core.copyPath', [], activeDirectory, writeText),
    ).resolves.toBe(false);
    expect(writeText).not.toHaveBeenCalled();
  });

  it('falls back when a WebView exposes but denies the modern Clipboard API', async () => {
    const writeText = vi.fn().mockRejectedValue(new DOMException('Denied', 'NotAllowedError'));
    vi.stubGlobal('navigator', { clipboard: { writeText } });
    const execCommand = vi.fn().mockReturnValue(true);
    Object.defineProperty(document, 'execCommand', {
      configurable: true,
      value: execCommand,
    });

    await expect(writeSystemClipboardText('grounded answer')).resolves.toBeUndefined();

    expect(writeText).toHaveBeenCalledWith('grounded answer');
    expect(execCommand).toHaveBeenCalledWith('copy');
    expect(document.querySelector('textarea')).toBeNull();
  });
});
