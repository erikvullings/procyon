import m, { type FactoryComponent, type VnodeDOM } from 'mithril';
import { technicalTextInputAttrs } from '../../a11y/technical-text-input';
import { closeIcon, filterIcon } from '../../components/tabler-icons';
import { t } from '../../i18n';
import { FILE_CLIPBOARD_PASSTHROUGH_ATTRIBUTE } from './quick-filter';
import './quick-filter.css';

/** Inputs for the inline quick-filter text box. */
export interface QuickFilterInputAttrs {
  readonly query: string;
  readonly onQueryChange: (query: string) => void;
  readonly onCommit: () => void;
  /** Removes the filter and closes the box (Esc and the clear button). */
  readonly onClose: () => void;
}

function hasTextSelection(input: HTMLInputElement): boolean {
  return input.selectionStart !== null && input.selectionStart !== input.selectionEnd;
}

/** Cmd/Ctrl+C or Cmd/Ctrl+X with nothing selected in the input: a file-clipboard command. */
function isFileClipboardShortcut(event: KeyboardEvent): boolean {
  const key = event.key.toLowerCase();
  return (
    (event.metaKey || event.ctrlKey) &&
    !event.altKey &&
    !event.shiftKey &&
    (key === 'c' || key === 'x') &&
    event.currentTarget instanceof HTMLInputElement &&
    !hasTextSelection(event.currentTarget)
  );
}

/** Presentation-only inline filter box; focuses itself once when mounted. */
export const QuickFilterInput: FactoryComponent<QuickFilterInputAttrs> = () => {
  // Closing moves focus to the file list; the resulting blur must not commit the removed filter.
  let closing = false;
  const close = (attrs: QuickFilterInputAttrs) => {
    closing = true;
    attrs.onClose();
  };
  return {
    view: ({ attrs }) =>
      m('.fm-quick-filter', [
        filterIcon({ className: 'fm-quick-filter-icon', size: 14 }),
        m('input.fm-quick-filter-input', {
          ...technicalTextInputAttrs,
          type: 'text',
          value: attrs.query,
          placeholder: t('quickFilter', 'placeholder'),
          'aria-label': t('action', 'quickFilter'),
          [FILE_CLIPBOARD_PASSTHROUGH_ATTRIBUTE]: '',
          oncreate: (vnode: VnodeDOM) => (vnode.dom as HTMLInputElement).focus(),
          oninput: (event: InputEvent) =>
            attrs.onQueryChange((event.currentTarget as HTMLInputElement).value),
          onblur: (event: FocusEvent) => {
            // Moving to the clear button is about to remove the filter; committing first would
            // race the clear against a stale workspace revision.
            if (closing) return;
            const next = event.relatedTarget;
            if (next instanceof HTMLElement && next.classList.contains('fm-quick-filter-clear'))
              return;
            attrs.onCommit();
          },
          onkeydown: (event: KeyboardEvent) => {
            // Let the global handler copy/cut the selected files; everything else is text editing.
            if (isFileClipboardShortcut(event)) return;
            event.stopPropagation();
            if (event.key === 'Escape') {
              event.preventDefault();
              close(attrs);
            } else if (event.key === 'Enter') {
              event.preventDefault();
              attrs.onCommit();
            }
          },
        }),
        m(
          'button.fm-quick-filter-clear',
          {
            type: 'button',
            'aria-label': t('quickFilter', 'clear'),
            title: t('quickFilter', 'clear'),
            // Keep focus in the input on pointer press so its blur doesn't commit before clearing.
            onmousedown: (event: MouseEvent) => event.preventDefault(),
            onclick: () => close(attrs),
            onkeydown: (event: KeyboardEvent) => {
              event.stopPropagation();
              if (event.key === 'Escape') {
                event.preventDefault();
                close(attrs);
              }
            },
          },
          closeIcon({ size: 14 }),
        ),
      ]),
  };
};
