import { describe, expect, it } from 'vitest';

import { thumbnailMimeType } from './image-mime';

describe('thumbnailMimeType', () => {
  it('recognises a PNG by its signature', () => {
    expect(thumbnailMimeType(Uint8Array.of(0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a))).toBe('image/png');
  });

  it('treats everything else, including empty input, as JPEG', () => {
    expect(thumbnailMimeType(Uint8Array.of(0xff, 0xd8, 0xff, 0xe0))).toBe('image/jpeg');
    expect(thumbnailMimeType(new Uint8Array())).toBe('image/jpeg');
  });
});
