import m, { type FactoryComponent } from 'mithril';
import { AlertDialog } from 'mithril-materialized';
import { arrowRightIcon } from '../../components/tabler-icons';
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
  const size = entry.size === undefined ? t('operation', 'sizeUnavailable') : `${entry.size} B`;
  const date =
    entry.modifiedAt === undefined
      ? t('operation', 'modifiedTimeUnavailable')
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
      return m(AlertDialog, {
        id: 'conflict-dialog',
        title: t('operation', 'resolveConflict'),
        className: 'fm-operation-confirmation-modal fm-conflict-dialog',
        isOpen: true,
        closeOnEsc: false,
        initialFocus: '.mm-dialog-primary-action',
        description: m('.fm-operation-confirmation-description', [
          m(
            'p.fm-operation-confirmation-summary',
            t('operation', 'destinationExists', { name: conflict.destination.name }),
          ),
          m('.fm-operation-confirmation-route', [
            m('.fm-operation-confirmation-endpoint', [
              m('span.fm-operation-confirmation-label', t('operation', 'source')),
              m('code', formatConflictMetadata(conflict.source)),
            ]),
            m(
              '.fm-operation-confirmation-arrow',
              { 'aria-hidden': 'true' },
              arrowRightIcon({ size: 18 }),
            ),
            m('.fm-operation-confirmation-endpoint', [
              m('span.fm-operation-confirmation-label', t('operation', 'destination')),
              m('code', formatConflictMetadata(conflict.destination)),
            ]),
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
        actions: [
          { label: t('button', 'skip'), onclick: () => resolve('skip') },
          { label: t('button', 'renameNew'), onclick: () => resolve('renameNew') },
        ],
        secondaryAction: {
          label: t('button', 'cancel'),
          onclick: () => resolve('cancelOperation'),
        },
        primaryAction: {
          label: t('button', 'overwrite'),
          destructive: true,
          onclick: () => resolve('overwrite'),
        },
      });
    },
  };
};
