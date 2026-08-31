import m, { type FactoryComponent } from 'mithril';
import { ModalPanel } from 'mithril-materialized';
import { t } from '../../i18n';

export interface SpotlightCommentDialogAttrs {
  readonly open: boolean;
  readonly entryName: string;
  readonly initialComment: string;
  readonly onConfirm: (comment: string) => void;
  readonly onCancel: () => void;
}

/** Moves focus away from the textarea before the modal closes, so the browser never has to apply
 * aria-hidden to an ancestor of the focused element. */
function blurActive(): void {
  const active = document.activeElement;
  if (active instanceof HTMLElement) active.blur();
}

/** Minimal modal for editing an entry's Spotlight comment (Get Info's "Comments:" field, task
 * 0136) - a standalone surface until 0140's properties dialog exists to host it instead. */
export const SpotlightCommentDialog: FactoryComponent<SpotlightCommentDialogAttrs> = () => {
  let comment = '';
  let wasOpen = false;

  function confirm(attrs: SpotlightCommentDialogAttrs): void {
    blurActive();
    attrs.onConfirm(comment);
  }

  function cancel(attrs: SpotlightCommentDialogAttrs): void {
    blurActive();
    attrs.onCancel();
  }

  return {
    onupdate: ({ attrs }) => {
      // ModalPanel keeps this component permanently mounted and only toggles CSS visibility
      // (see CreateDirectoryDialog's identical note), so seed/focus on the false->true edge.
      if (attrs.open && !wasOpen) {
        comment = attrs.initialComment;
        document.getElementById('spotlight-comment-text')?.focus();
      }
      wasOpen = attrs.open;
    },
    view: ({ attrs }) =>
      m(ModalPanel, {
        title: t('entryMetadata', 'commentTitle', { name: attrs.entryName }),
        className: 'fm-dense-modal',
        description: m('label.fm-spotlight-comment-field', [
          m('span', t('entryMetadata', 'comment')),
          m('textarea#spotlight-comment-text', {
            rows: 4,
            value: comment,
            oninput: (event: InputEvent) => {
              comment = (event.currentTarget as HTMLTextAreaElement).value;
            },
            onkeydown: (event: KeyboardEvent) => {
              if (event.key === 'Escape') {
                event.stopPropagation();
                cancel(attrs);
              } else if (event.key === 'Enter' && (event.metaKey || event.ctrlKey)) {
                event.preventDefault();
                event.stopPropagation();
                confirm(attrs);
              }
            },
          }),
        ]),
        isOpen: attrs.open,
        closeOnEsc: true,
        onToggle: (open: boolean) => {
          if (!open) cancel(attrs);
        },
        buttons: [
          { label: t('button', 'cancel'), onclick: () => cancel(attrs) },
          { label: t('button', 'save'), onclick: () => confirm(attrs) },
        ],
      }),
  };
};
