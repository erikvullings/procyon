import m, { type FactoryComponent } from 'mithril';
import { ModalPanel } from 'mithril-materialized';
import { t } from '../../i18n';
import type { ApplicationUninstallCandidate, Location } from '../../models';
import { pathFromUri } from '../workspace/workspace-layout';

export interface ApplicationUninstallDialogAttrs {
  readonly open: boolean;
  readonly productName: string;
  readonly relatedFiles: readonly ApplicationUninstallCandidate[];
  /** Whether the current host/provider can move items to the system Trash (mirrors
   * `canUseSystemTrash` in the global keydown handler) - the confirm button only appears when
   * this is `true`, since uninstalling always goes through the Trash-first delete path. */
  readonly canTrash: boolean;
  /** Called with the locations the user left checked (the bundle plus any checked removable
   * related files) when they confirm. */
  readonly onConfirm: (checkedRelatedFiles: readonly Location[]) => void;
  readonly onCancel: () => void;
}

/** Formats a byte count the same simple three-tier way as `operation-centre.ts`'s (unexported)
 * `formatBytes` - duplicated locally rather than importing a private helper from another
 * feature's module. */
function formatBytes(value: number): string {
  if (value < 1_024) return `${value} B`;
  if (value < 1_048_576) return `${(value / 1_024).toFixed(value % 1_024 === 0 ? 0 : 1)} KiB`;
  return `${(value / 1_048_576).toFixed(1)} MiB`;
}

function blurActive(): void {
  const active = document.activeElement;
  if (active instanceof HTMLElement) active.blur();
}

/** Every removable candidate's location URI - the default "everything removable is checked"
 * selection shown when the dialog (re)opens. */
function removableUris(attrs: ApplicationUninstallDialogAttrs): ReadonlySet<string> {
  return new Set(
    attrs.relatedFiles
      .filter((candidate) => candidate.removable)
      .map((candidate) => candidate.location.uri),
  );
}

/** Review checklist shown before an application uninstall (task 0148): lists every related file
 * discovered under the well-known macOS locations, each removable candidate pre-checked, before
 * anything reaches the Trash-first delete path. Non-removable candidates (under `/Library`) are
 * shown for transparency but can never be checked - discovery itself never touches anything, and
 * this dialog never deletes anything outside what the user leaves checked. */
export const ApplicationUninstallDialog: FactoryComponent<ApplicationUninstallDialogAttrs> = (
  initialVnode,
) => {
  let checked: ReadonlySet<string> = initialVnode.attrs.open
    ? removableUris(initialVnode.attrs)
    : new Set();
  let wasOpen = initialVnode.attrs.open;

  function confirm(attrs: ApplicationUninstallDialogAttrs): void {
    blurActive();
    const locations = attrs.relatedFiles
      .filter((candidate) => candidate.removable && checked.has(candidate.location.uri))
      .map((candidate) => candidate.location);
    attrs.onConfirm(locations);
  }

  function cancel(attrs: ApplicationUninstallDialogAttrs): void {
    blurActive();
    attrs.onCancel();
  }

  function toggle(uri: string): void {
    const next = new Set(checked);
    if (next.has(uri)) {
      next.delete(uri);
    } else {
      next.add(uri);
    }
    checked = next;
  }

  return {
    onupdate: ({ attrs }) => {
      if (attrs.open && !wasOpen) checked = removableUris(attrs);
      wasOpen = attrs.open;
    },
    view: ({ attrs }) =>
      m(ModalPanel, {
        title: t('applicationUninstall', 'dialogTitle', { name: attrs.productName }),
        className: 'fm-dense-modal',
        description: m('.fm-application-uninstall', [
          m('p', t('applicationUninstall', 'subtitle', { name: attrs.productName })),
          attrs.relatedFiles.length === 0
            ? m('p.fm-application-uninstall-empty', t('applicationUninstall', 'noRelatedFiles'))
            : m(
                'ul.fm-application-uninstall-list',
                attrs.relatedFiles.map((candidate) =>
                  m(
                    'li.fm-application-uninstall-row',
                    { key: candidate.location.uri },
                    candidate.removable
                      ? m('label.fm-application-uninstall-item', [
                          m('input', {
                            type: 'checkbox',
                            checked: checked.has(candidate.location.uri),
                            onchange: () => toggle(candidate.location.uri),
                          }),
                          m(
                            'span.fm-application-uninstall-path',
                            pathFromUri(candidate.location.uri),
                          ),
                          m('span.fm-application-uninstall-size', formatBytes(candidate.sizeBytes)),
                        ])
                      : m('.fm-application-uninstall-item.fm-application-uninstall-item--locked', [
                          m(
                            'span.fm-application-uninstall-path',
                            pathFromUri(candidate.location.uri),
                          ),
                          m('span.fm-application-uninstall-size', formatBytes(candidate.sizeBytes)),
                          m(
                            'span.fm-application-uninstall-locked-note',
                            t('applicationUninstall', 'requiresAdministratorAccess'),
                          ),
                        ]),
                  ),
                ),
              ),
        ]),
        isOpen: attrs.open,
        closeOnEsc: true,
        onToggle: (open: boolean) => {
          if (!open) cancel(attrs);
        },
        buttons: [
          { label: t('button', 'cancel'), onclick: () => cancel(attrs) },
          ...(attrs.canTrash
            ? [
                {
                  label: t('applicationUninstall', 'confirmButton'),
                  onclick: () => confirm(attrs),
                },
              ]
            : []),
        ],
      }),
  };
};
