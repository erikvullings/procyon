import m, { type FactoryComponent } from 'mithril';
import { ModalPanel } from 'mithril-materialized';
import { t } from '../../i18n';
import type { ConflictResolution, OperationConflict } from '../../models';

export interface ConflictDialogAttrs {
  readonly conflict: OperationConflict | undefined;
  readonly onResolve: (resolution: ConflictResolution, applyToAllSimilar: boolean) => void;
}

/** Formats a `Date` as a compact `YYYY-MM-DD HH:MM:SS` string in the local time zone. */
function formatLocalCompact(date: Date): string {
  const pad = (value: number) => String(value).padStart(2, '0');
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}

/** Compact, stable metadata for comparing two conflicting entries. */
export function formatConflictMetadata(entry: OperationConflict['source']): string {
  const size = entry.size === undefined ? 'size unavailable' : `${entry.size}b`;
  const date =
    entry.modifiedAt === undefined
      ? 'modified time unavailable'
      : formatLocalCompact(new Date(entry.modifiedAt));
  return `${entry.name} · ${size} · ${date}`;
}

/**
 * Explicit request/response dialog for a pending filesystem conflict.
 *
 * Uses mm's `ModalPanel` (like every other dialog) rather than a bare
 * `role="dialog"` div, so it gets the app's shared modal chrome (backdrop,
 * centering, button styling) for free instead of rendering inline/unstyled.
 * Unlike the other dialogs, this component is only mounted at all while a
 * conflict is pending (`isOpen` is always true when rendered) -- mm's
 * ModalPanel keeps its title/description text in the DOM even while closed
 * (only toggled via CSS `display`), which would otherwise leak the dialog's
 * text into `textContent` between conflicts.
 */
export const ConflictDialog: FactoryComponent<ConflictDialogAttrs> = () => {
  let applyToAllSimilar = false;
  return {
    view: ({ attrs }) => {
      const conflict = attrs.conflict;
      if (conflict === undefined) return undefined;
      const resolve = (resolution: ConflictResolution) =>
        attrs.onResolve(resolution, applyToAllSimilar);
      return m(ModalPanel, {
        id: 'conflict-dialog',
        title: t('operation', 'resolveConflict'),
        className: 'fm-conflict-dialog',
        isOpen: true,
        showCloseButton: false,
        closeOnBackdropClick: false,
        closeOnEsc: false,
        description: m('div', [
          m('p', conflict.message),
          m('dl.fm-conflict-dialog-entries', [
            m('dt', t('operation', 'source')),
            m('dd', formatConflictMetadata(conflict.source)),
            m('dt', t('operation', 'destination')),
            m('dd', formatConflictMetadata(conflict.destination)),
          ]),
          m('label.fm-conflict-dialog-checkbox', [
            m('input', {
              type: 'checkbox',
              onchange: (event: Event) => {
                applyToAllSimilar = (event.currentTarget as HTMLInputElement).checked;
              },
            }),
            m('span', t('operation', 'applyToAllSimilar')),
          ]),
        ]),
        buttons: [
          { label: t('operation', 'cancelOperation'), onclick: () => resolve('cancelOperation') },
          { label: t('button', 'skip'), onclick: () => resolve('skip') },
          { label: t('button', 'renameNew'), onclick: () => resolve('renameNew') },
          { label: t('button', 'overwrite'), onclick: () => resolve('overwrite') },
        ],
      });
    },
  };
};
