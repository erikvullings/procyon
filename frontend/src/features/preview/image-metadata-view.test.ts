import m from 'mithril';
import { describe, expect, it } from 'vitest';
import type { ImageMetadata, ImageTagGroup } from '../../models';
import { filterTagGroups, ImageMetadataView, tagGroupsAsText } from './image-metadata-view';

const groups: readonly ImageTagGroup[] = [
  {
    namespace: 'EXIF',
    tags: [
      { name: 'Make', value: 'ACME', raw: 'ACME\0' },
      { name: 'Model', value: 'Cam 9000' },
    ],
  },
  { namespace: 'IPTC', tags: [{ name: 'Keywords', value: 'sunset' }] },
];

describe('filterTagGroups', () => {
  it('matches name, value and namespace case-insensitively and drops empty groups', () => {
    expect(filterTagGroups(groups, 'cam 9')).toEqual([
      { namespace: 'EXIF', tags: [{ name: 'Model', value: 'Cam 9000' }] },
    ]);
    expect(filterTagGroups(groups, 'iptc')).toHaveLength(1);
    expect(filterTagGroups(groups, 'nothing')).toEqual([]);
    expect(filterTagGroups(groups, '  ')).toBe(groups);
  });
});

describe('tagGroupsAsText', () => {
  it('writes one line per tag', () => {
    expect(tagGroupsAsText(groups)).toBe(
      'EXIF: Make = ACME\nEXIF: Model = Cam 9000\nIPTC: Keywords = sunset',
    );
  });
});

function render(
  metadata: ImageMetadata | 'loading' | 'failed',
  onOpenOnMap?: () => void,
): HTMLElement {
  const root = document.createElement('div');
  m.mount(root, {
    view: () => m(ImageMetadataView, { metadata, ...(onOpenOnMap ? { onOpenOnMap } : {}) }),
  });
  m.redraw.sync();
  return root;
}

describe('ImageMetadataView', () => {
  it('says so when the format is unsupported instead of showing an empty panel', () => {
    const root = render({ supported: false, groups: [] });
    expect(root.textContent).toContain('no readable image metadata');
    expect(root.querySelector('dl')).toBeNull();
  });

  it('lists grouped tags and offers the map action for GPS positions', () => {
    let opened = false;
    const root = render(
      { supported: true, format: 'JPEG', groups, gps: { latitude: 52.5, longitude: 4.9 } },
      () => {
        opened = true;
      },
    );
    expect(root.querySelectorAll('section')).toHaveLength(2);
    expect(root.textContent).toContain('52.500000, 4.900000');
    (root.querySelector('.fm-image-metadata-map') as HTMLButtonElement).click();
    expect(opened).toBe(true);
  });

  it('reveals raw values on demand', () => {
    const root = render({ supported: true, groups });
    expect(root.querySelector('.fm-image-metadata-raw')).toBeNull();
    (root.querySelector('.fm-image-metadata-raw-toggle') as HTMLButtonElement).click();
    m.redraw.sync();
    expect(root.querySelector('.fm-image-metadata-raw')).not.toBeNull();
  });
});
