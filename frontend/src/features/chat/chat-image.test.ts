import { describe, expect, it } from 'vitest';
import { CHAT_IMAGE_MAX_EDGE, fitWithin, pastedImageFiles } from './chat-image';

describe('pasted images', () => {
  it('extracts image file items and leaves text items alone', () => {
    const image = new Blob(['image'], { type: 'image/png' }) as File;
    expect(
      pastedImageFiles([
        { kind: 'string', type: 'text/plain', getAsFile: () => null },
        { kind: 'file', type: 'image/png', getAsFile: () => image },
        { kind: 'file', type: 'image/jpeg', getAsFile: () => null },
      ]),
    ).toEqual([image]);
    expect(pastedImageFiles([])).toEqual([]);
  });

  it('keeps small images and scales the longest side of large ones to the limit', () => {
    expect(fitWithin(800, 600)).toEqual({ width: 800, height: 600 });
    expect(fitWithin(2048, 1024)).toEqual({ width: CHAT_IMAGE_MAX_EDGE, height: 512 });
    expect(fitWithin(600, 3000)).toEqual({ width: 205, height: CHAT_IMAGE_MAX_EDGE });
  });

  it('never collapses a side to zero', () => {
    expect(fitWithin(5000, 1)).toEqual({ width: CHAT_IMAGE_MAX_EDGE, height: 1 });
  });
});
