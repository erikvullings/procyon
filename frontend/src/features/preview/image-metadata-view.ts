import m, { type FactoryComponent } from 'mithril';
import { copyIcon } from '../../components/tabler-icons';
import { t } from '../../i18n';
import type { GpsPosition, ImageMetadata, ImageTagGroup } from '../../models';
import { copyText } from './clipboard';
import { copyWithToast } from './copy-with-toast';

export interface ImageMetadataViewAttrs {
  readonly metadata: ImageMetadata | 'loading' | 'failed';
  /** Shown as an "Open on map" button next to the coordinates when provided (task 0276). */
  readonly onOpenOnMap?: (position: GpsPosition) => void;
}

/** Tags whose name, value or group match `query` (case-insensitive); empty groups are dropped. */
export function filterTagGroups(
  groups: readonly ImageTagGroup[],
  query: string,
): readonly ImageTagGroup[] {
  const needle = query.trim().toLowerCase();
  if (needle === '') return groups;
  return groups
    .map((group) => {
      const groupMatches = group.namespace.toLowerCase().includes(needle);
      return {
        namespace: group.namespace,
        tags: group.tags.filter(
          (tag) =>
            groupMatches ||
            tag.name.toLowerCase().includes(needle) ||
            tag.value.toLowerCase().includes(needle),
        ),
      };
    })
    .filter((group) => group.tags.length > 0);
}

/** The metadata as plain text, one `Group: Name = value` line per tag. */
export function tagGroupsAsText(groups: readonly ImageTagGroup[]): string {
  return groups
    .flatMap((group) => group.tags.map((tag) => `${group.namespace}: ${tag.name} = ${tag.value}`))
    .join('\n');
}

/** Searchable, copyable list of every image tag grouped by namespace (task 0273). The filter and
 * raw-value toggle are transient view state, not shared application state. */
export const ImageMetadataView: FactoryComponent<ImageMetadataViewAttrs> = () => {
  let query = '';
  let showRaw = false;
  return {
    view: ({ attrs }) => {
      const { metadata } = attrs;
      if (metadata === 'loading') {
        return m(
          '.fm-image-metadata',
          m('span', { role: 'status' }, t('imageMetadata', 'loading')),
        );
      }
      if (metadata === 'failed') {
        return m('.fm-image-metadata', m('span', { role: 'alert' }, t('imageMetadata', 'failed')));
      }
      if (!metadata.supported) {
        return m('.fm-image-metadata', m('span', t('imageMetadata', 'unsupported')));
      }
      const visible = filterTagGroups(metadata.groups, query);
      return m('.fm-image-metadata', [
        m('h4.fm-image-metadata-heading', [
          t('imageMetadata', 'title'),
          metadata.format ? m('span.fm-image-metadata-format', ` (${metadata.format})`) : undefined,
        ]),
        metadata.gps
          ? m('.fm-image-metadata-gps', [
              m('span', `${t('imageMetadata', 'coordinates')}: `),
              m(
                'span.fm-image-metadata-coordinates',
                `${metadata.gps.latitude.toFixed(6)}, ${metadata.gps.longitude.toFixed(6)}`,
              ),
              attrs.onOpenOnMap === undefined
                ? undefined
                : m(
                    'button.fm-image-metadata-map',
                    {
                      type: 'button',
                      onclick: () => metadata.gps && attrs.onOpenOnMap?.(metadata.gps),
                    },
                    t('imageMetadata', 'openOnMap'),
                  ),
            ])
          : undefined,
        metadata.groups.length === 0
          ? m('span', t('imageMetadata', 'empty'))
          : [
              m('.fm-image-metadata-toolbar', [
                m('input.fm-image-metadata-search', {
                  type: 'search',
                  value: query,
                  placeholder: t('imageMetadata', 'search'),
                  'aria-label': t('imageMetadata', 'search'),
                  oninput: (event: Event) => {
                    query = (event.target as HTMLInputElement).value;
                  },
                }),
                m(
                  'button.fm-image-metadata-raw-toggle',
                  {
                    type: 'button',
                    'aria-pressed': String(showRaw),
                    onclick: () => (showRaw = !showRaw),
                  },
                  showRaw ? t('imageMetadata', 'hideRaw') : t('imageMetadata', 'showRaw'),
                ),
                m(
                  'button.fm-image-metadata-copy-all',
                  {
                    type: 'button',
                    onclick: () =>
                      void copyWithToast(
                        () => copyText(tagGroupsAsText(visible)),
                        t('imageMetadata', 'allCopied'),
                      ),
                  },
                  t('imageMetadata', 'copyAll'),
                ),
              ]),
              visible.length === 0
                ? m('span', t('imageMetadata', 'noMatches'))
                : visible.map((group) =>
                    m('section.fm-image-metadata-group', { key: group.namespace }, [
                      m('h5', group.namespace),
                      m(
                        'dl',
                        group.tags.map((tag, index) =>
                          m('.fm-image-metadata-tag', { key: `${tag.name}:${index}` }, [
                            m('dt', tag.name),
                            m('dd', [
                              m('span.fm-image-metadata-value', tag.value),
                              showRaw && tag.raw
                                ? m('code.fm-image-metadata-raw', tag.raw)
                                : undefined,
                              m(
                                'button.fm-file-viewer-metadata-copy',
                                {
                                  type: 'button',
                                  'aria-label': t('imageMetadata', 'copyValue', { name: tag.name }),
                                  onclick: () =>
                                    void copyWithToast(
                                      () => copyText(tag.value),
                                      t('imageMetadata', 'copied', { name: tag.name }),
                                    ),
                                },
                                copyIcon({ size: 12 }),
                              ),
                            ]),
                          ]),
                        ),
                      ),
                    ]),
                  ),
            ],
      ]);
    },
  };
};
