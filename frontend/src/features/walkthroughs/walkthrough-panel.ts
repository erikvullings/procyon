import m, { type FactoryComponent } from 'mithril';

import { t } from '../../i18n';
import { safeMarkdownHtml } from '../editor/markdown-preview';
import { type WalkthroughId, walkthroughForHref, walkthroughs } from './walkthroughs';

import './walkthrough-panel.css';

export interface WalkthroughPanelAttrs {
  readonly selected: WalkthroughId;
  readonly onSelect: (id: WalkthroughId) => void;
  readonly onClose: () => void;
}

export const WalkthroughPanel: FactoryComponent<WalkthroughPanelAttrs> = () => ({
  view: ({ attrs }) => {
    const selected = walkthroughs.find((guide) => guide.id === attrs.selected);
    if (selected === undefined) throw new Error(`Unknown walkthrough: ${attrs.selected}`);
    const selectGuide = (id: WalkthroughId): void => {
      document.querySelector<HTMLElement>('.fm-walkthrough-reading')?.scrollTo(0, 0);
      attrs.onSelect(id);
    };
    return m(
      'aside.fm-walkthrough-panel',
      {
        'aria-label': t('walkthroughs', 'title'),
        tabindex: -1,
        oncreate: ({ dom }) => (dom as HTMLElement).focus(),
        onkeydown: (event: KeyboardEvent) => {
          if (event.key === 'Escape') {
            event.stopPropagation();
            attrs.onClose();
          }
        },
      },
      [
        m('.fm-walkthrough-heading', [
          m('strong', t('walkthroughs', 'title')),
          m(
            'button',
            {
              type: 'button',
              'aria-label': t('walkthroughs', 'close'),
              onclick: attrs.onClose,
            },
            '×',
          ),
        ]),
        m('p.fm-walkthrough-language', t('walkthroughs', 'englishContent')),
        m('nav.fm-walkthrough-nav', { 'aria-label': t('walkthroughs', 'chooseGuide') }, [
          ...walkthroughs.map((guide) =>
            m(
              'button',
              {
                type: 'button',
                key: guide.id,
                'aria-current': guide.id === attrs.selected ? 'page' : undefined,
                onclick: () => selectGuide(guide.id),
              },
              guide.title,
            ),
          ),
        ]),
        m('.fm-walkthrough-reading', [
          m(
            '.fm-walkthrough-article',
            {
              // Linked guide Markdown is bundled at build time and sanitized before insertion.
              onclick: (event: MouseEvent) => {
                const anchor = (event.target as Element).closest('a[href]');
                if (!(anchor instanceof HTMLAnchorElement)) return;
                const next = walkthroughForHref(anchor.getAttribute('href') ?? '');
                if (next === undefined) return;
                event.preventDefault();
                selectGuide(next);
              },
            },
            m.trust(safeMarkdownHtml(selected.markdown)),
          ),
        ]),
      ],
    );
  },
});
