import m, { type FactoryComponent } from 'mithril';
import { ModalPanel } from 'mithril-materialized';
import { t } from '../../i18n';
import type { Location } from '../../models';
import { type EntryFormatSettings, formatEntrySize } from '../entry-formatting/entry-formatting';
import { affectedItems } from './affected-items';

export interface PermanentDeleteDialogAttrs {
  readonly open: boolean;
  readonly operationId?: string;
  readonly itemCount?: number;
  readonly totalBytes?: number;
  /** Top-level items the user selected; named so the confirmation is about files, not a count. */
  readonly sources?: readonly Location[];
  readonly formatSettings: EntryFormatSettings;
  readonly onConfirm: () => void | Promise<void>;
  readonly onCancel: () => void;
}

/** Irreversible-delete confirmation, before planning or for an existing pending job. */
export const PermanentDeleteDialog: FactoryComponent<PermanentDeleteDialogAttrs> = () => {
  let keydownHandler: ((event: KeyboardEvent) => void) | undefined;
  let hiddenOperationId: string | undefined;
  let hidden = false;

  const removeFocusTrap = () => {
    if (keydownHandler !== undefined) document.removeEventListener('keydown', keydownHandler);
    keydownHandler = undefined;
  };

  const updateFocusTrap = (dom: Element, open: boolean) => {
    removeFocusTrap();
    if (!open) return;
    const dialog = dom.closest('[role="dialog"]');
    const confirm = dialog?.querySelector<HTMLButtonElement>('.fm-permanent-delete-confirm');
    confirm?.focus();
    keydownHandler = (event: KeyboardEvent) => {
      if (event.key !== 'Tab' || dialog === null) return;
      const focusable = [
        ...dialog.querySelectorAll<HTMLButtonElement>(
          '.fm-permanent-delete-cancel:not([disabled]), .fm-permanent-delete-confirm:not([disabled])',
        ),
      ];
      if (focusable.length === 0) return;
      const currentIndex = focusable.indexOf(document.activeElement as HTMLButtonElement);
      const nextIndex = event.shiftKey
        ? (currentIndex <= 0 ? focusable.length : currentIndex) - 1
        : (currentIndex + 1) % focusable.length;
      event.preventDefault();
      focusable[nextIndex]?.focus();
    };
    document.addEventListener('keydown', keydownHandler);
  };

  return {
    view: ({ attrs }) => {
      if (
        !attrs.open ||
        (attrs.operationId !== undefined && attrs.operationId !== hiddenOperationId)
      ) {
        hidden = false;
      }
      const formattedSize =
        attrs.totalBytes === undefined
          ? undefined
          : formatEntrySize({ kind: 'file', size: attrs.totalBytes }, attrs.formatSettings);
      return m(ModalPanel, {
        className: 'fm-permanent-delete-modal',
        title: t('operation', 'confirmDeleteTitle'),
        description: m(
          '.fm-permanent-delete-warning',
          {
            oncreate: ({ dom }) => updateFocusTrap(dom, attrs.open),
            onupdate: ({ dom }) => updateFocusTrap(dom, attrs.open),
            onremove: removeFocusTrap,
          },
          [
            m(
              'p',
              attrs.itemCount === undefined || formattedSize === undefined
                ? t('operation', 'permanentDeleteBeforePlanning')
                : t('operation', 'permanentDeleteSummary', {
                    count: attrs.itemCount,
                    size: formattedSize,
                  }),
            ),
            (attrs.sources ?? []).length === 0
              ? undefined
              : m('dl.fm-operation-confirmation-facts', affectedItems(attrs.sources ?? [])),
            m('strong', t('operation', 'irreversible')),
          ],
        ),
        isOpen: attrs.open && !hidden,
        closeOnEsc: true,
        onToggle: (open: boolean) => {
          if (!open && !hidden) attrs.onCancel();
        },
        buttons: [
          {
            label: t('button', 'cancel'),
            onclick: attrs.onCancel,
            className: 'fm-permanent-delete-cancel',
          },
          {
            label: t('button', 'confirmDelete'),
            onclick: () => {
              const operationId = attrs.operationId;
              hiddenOperationId = operationId;
              hidden = true;
              m.redraw();
              void Promise.resolve(attrs.onConfirm()).catch(() => {
                if (hiddenOperationId === operationId) hidden = false;
                m.redraw();
              });
            },
            className: 'fm-permanent-delete-confirm',
          },
        ],
      });
    },
  };
};
