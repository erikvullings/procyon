import { describe, expect, it } from 'vitest';
import type { EntrySummary } from '../../models';
import { galleryPosition, isGalleryEntry } from './gallery';

function entry(name: string, kind: EntrySummary['kind'] = 'file'): EntrySummary {
  const uri = `file:///pics/${name}`;
  const dot = name.lastIndexOf('.');
  return {
    id: uri as EntrySummary['id'],
    location: { providerId: 'local', uri },
    name,
    kind,
    hidden: false,
    readOnly: false,
    ...(dot > 0 ? { extension: name.slice(dot + 1) } : {}),
    metadataRevision: 1,
  };
}

const listing = [
  entry('notes.txt'),
  entry('b.jpg'),
  entry('sub', 'directory'),
  entry('a.png'),
  entry('clip.mp4'),
  entry('report.pdf'),
  entry('c.webp'),
];

describe('gallery', () => {
  it('only flips through images, video and audio', () => {
    expect(listing.filter(isGalleryEntry).map((e) => e.name)).toEqual([
      'b.jpg',
      'a.png',
      'clip.mp4',
      'c.webp',
    ]);
  });

  it('follows the displayed order, not the name order', () => {
    const position = galleryPosition(listing, listing[3]?.location ?? { providerId: '', uri: '' });
    expect(position).toMatchObject({ index: 1, count: 4 });
    expect(position?.previous?.name).toBe('b.jpg');
    expect(position?.next?.name).toBe('clip.mp4');
  });

  it('has no neighbour past either end and no position for foreign items', () => {
    const first = galleryPosition(listing, listing[1]?.location ?? { providerId: '', uri: '' });
    expect(first?.previous).toBeUndefined();
    const last = galleryPosition(listing, listing[6]?.location ?? { providerId: '', uri: '' });
    expect(last?.next).toBeUndefined();
    expect(
      galleryPosition(listing, { providerId: 'local', uri: 'file:///other.jpg' }),
    ).toBeUndefined();
  });
});
