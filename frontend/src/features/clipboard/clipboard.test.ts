import { describe, expect, it } from 'vitest';

import type { Location } from '../../models';
import {
  clearClipboard,
  copyToClipboard,
  cutToClipboard,
  emptyClipboard,
  isCutLocation,
  isSameFolderPaste,
  validatePasteTarget,
} from './clipboard';

const source: Location = { providerId: 'file', uri: 'file:///home/erik/Projects' };
const document: Location = { providerId: 'file', uri: 'file:///home/erik/Documents' };

describe('in-app clipboard', () => {
  it('records copied locations without marking them cut', () => {
    const clipboard = copyToClipboard(emptyClipboard, [source]);

    expect(clipboard).toEqual({ mode: 'copy', locations: [source] });
    expect(isCutLocation(clipboard, source)).toBe(false);
  });

  it('dims only cut locations and clears the cut after a successful paste', () => {
    const clipboard = cutToClipboard(emptyClipboard, [source]);

    expect(isCutLocation(clipboard, source)).toBe(true);
    expect(clearClipboard(clipboard)).toEqual(emptyClipboard);
  });

  it('rejects unavailable, read-only, and nested paste targets before an operation starts', () => {
    const clipboard = copyToClipboard(emptyClipboard, [source]);

    expect(validatePasteTarget(clipboard, undefined)).toMatchObject({ ok: false });
    expect(
      validatePasteTarget(clipboard, { location: document, writable: false, loaded: true }),
    ).toEqual({ ok: false, message: 'The destination directory is read-only.' });
    expect(
      validatePasteTarget(clipboard, {
        location: { providerId: 'file', uri: 'file:///home/erik/Projects/src' },
        writable: true,
        loaded: true,
      }),
    ).toEqual({ ok: false, message: 'Cannot paste a location into itself or its subtree.' });
  });

  it('accepts a loaded writable sibling directory', () => {
    const clipboard = copyToClipboard(emptyClipboard, [source]);

    expect(
      validatePasteTarget(clipboard, { location: document, writable: true, loaded: true }),
    ).toEqual({ ok: true });
  });

  it('detects only copy-pastes back into every source location parent', () => {
    const projectFile: Location = {
      providerId: 'file',
      uri: 'file:///home/erik/Projects/report.txt',
    };
    const otherProjectFile: Location = {
      providerId: 'file',
      uri: 'file:///home/erik/Projects/notes.txt',
    };

    expect(
      isSameFolderPaste(copyToClipboard(emptyClipboard, [projectFile, otherProjectFile]), source),
    ).toBe(true);
    expect(isSameFolderPaste(copyToClipboard(emptyClipboard, [projectFile]), document)).toBe(false);
    expect(isSameFolderPaste(cutToClipboard(emptyClipboard, [projectFile]), source)).toBe(false);
  });

  it('does not confuse matching paths on different remote authorities', () => {
    expect(
      isSameFolderPaste(
        copyToClipboard(emptyClipboard, [
          {
            providerId: 'sftp',
            uri: 'sftp://connection-a/home/erik/report.txt',
          },
        ]),
        { providerId: 'sftp', uri: 'sftp://connection-b/home/erik' },
      ),
    ).toBe(false);
    expect(
      isSameFolderPaste(
        copyToClipboard(emptyClipboard, [
          {
            providerId: 'local',
            uri: 'file://server-a/share/report.txt',
          },
        ]),
        { providerId: 'local', uri: 'file://server-b/share' },
      ),
    ).toBe(false);
  });
});
