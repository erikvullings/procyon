import m from 'mithril';
import { t } from '../../i18n';
import type { Location } from '../../models';
import { lastPathSegment } from '../navigation/navigation';

const VISIBLE_NAMES = 4;

/** Names the top-level items a confirmation acts on, so users confirm files rather than a count.
 *  Renders a `dt`/`dd` pair for use inside a `dl.fm-operation-confirmation-facts` grid. */
export function affectedItems(locations: readonly Location[]): m.Children {
  if (locations.length === 0) return undefined;
  const names = locations.map((location) => lastPathSegment(location) ?? location.uri);
  const visible = names.slice(0, VISIBLE_NAMES);
  const hidden = names.length - visible.length;
  return [
    m('dt.fm-operation-confirmation-label', t('operation', 'affectedItems')),
    m(
      'dd.fm-affected-items',
      m('ul.fm-affected-items-list', { title: names.join('\n') }, [
        ...visible.map((name) => m('li', name)),
        hidden > 0
          ? m('li.fm-affected-items-more', t('operation', 'affectedMore', { n: hidden }))
          : undefined,
      ]),
    ),
  ];
}
