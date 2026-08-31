import m, { type FactoryComponent } from 'mithril';
import { ModalPanel } from 'mithril-materialized';
import { t } from '../../i18n';
import { type EntryFormatSettings, formatEntrySize } from '../entry-formatting/entry-formatting';

export interface PermanentDeleteDialogAttrs {
  readonly open: boolean;
  readonly operationId?: string;
  readonly itemCount: number;
  readonly totalBytes: number;
  readonly formatSettings: EntryFormatSettings;
  readonly onConfirm: () => void | Promise<void>;
  readonly onCancel: () => void;
}

/** Irreversible-delete confirmation shown only after backend planning completes. */
export const PermanentDeleteDialog: FactoryComponent<PermanentDeleteDialogAttrs> = () => {
  let keydownHandler: ((event: KeyboardEvent) => void) | undefined;
  let hiddenOperationId: string | undefined;

  const removeFocusTrap = () => {
    if (keydownHandler !== undefined) document.removeEventListener('keydown', keydownHandler);
    keydownHandler = undefined;
  };

  const updateFocusTrap = (dom: Element, open: boolean) => {
    removeFocusTrap();
    if (!open) return;
    const dialog = dom.closest('[role="dialog"]');
    const cancel = dialog?.querySelector<HTMLButtonElement>('.fm-permanent-delete-cancel');
    cancel?.focus();
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
      const formattedSize = formatEntrySize(
        { kind: 'file', size: attrs.totalBytes },
        attrs.formatSettings,
      );
      const hidden = attrs.operationId !== undefined && attrs.operationId === hiddenOperationId;
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
              t('operation', 'permanentDeleteSummary', {
                count: attrs.itemCount,
                size: formattedSize,
              }),
            ),
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
              m.redraw();
              void Promise.resolve(attrs.onConfirm()).catch(() => {
                if (hiddenOperationId === operationId) hiddenOperationId = undefined;
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
