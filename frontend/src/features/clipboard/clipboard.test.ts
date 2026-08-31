import { describe, expect, it } from 'vitest';

import type { Location } from '../../models';
import {
  clearClipboard,
  copyToClipboard,
  cutToClipboard,
  emptyClipboard,
  isCutLocation,
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
});
