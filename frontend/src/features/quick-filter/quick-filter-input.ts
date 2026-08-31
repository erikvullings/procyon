import m, { type FactoryComponent, type VnodeDOM } from 'mithril';
import { filterIcon } from '../../components/tabler-icons';
import { t } from '../../i18n';
import './quick-filter.css';

/** Inputs for the inline quick-filter text box. */
export interface QuickFilterInputAttrs {
  readonly query: string;
  readonly onQueryChange: (query: string) => void;
  readonly onCommit: () => void;
  readonly onClose: () => void;
}

/** Presentation-only inline filter box; focuses itself once when mounted. */
export const QuickFilterInput: FactoryComponent<QuickFilterInputAttrs> = () => {
  return {
    view: ({ attrs }) =>
      m('.fm-quick-filter', [
        filterIcon({ className: 'fm-quick-filter-icon', size: 14 }),
        m('input.fm-quick-filter-input', {
          type: 'text',
          value: attrs.query,
          placeholder: t('quickFilter', 'placeholder'),
          'aria-label': t('action', 'quickFilter'),
          oncreate: (vnode: VnodeDOM) => (vnode.dom as HTMLInputElement).focus(),
          oninput: (event: InputEvent) =>
            attrs.onQueryChange((event.currentTarget as HTMLInputElement).value),
          onblur: () => attrs.onCommit(),
          onkeydown: (event: KeyboardEvent) => {
            event.stopPropagation();
            if (event.key === 'Escape') {
              event.preventDefault();
              attrs.onClose();
            } else if (event.key === 'Enter') {
              event.preventDefault();
              attrs.onCommit();
            }
          },
        }),
      ]),
  };
};
