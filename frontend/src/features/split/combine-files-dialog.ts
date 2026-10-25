import m, { type FactoryComponent } from 'mithril';
import { ModalPanel } from 'mithril-materialized';
import type { FileManagerClient } from '../../api/client/file-manager-client';
import { errorText, t } from '../../i18n';
import type { InspectSplitSetResult, Location } from '../../models';

/** The selected part files to join. */
export interface CombineFilesRequest {
  readonly parts: readonly Location[];
  /** Folder shown in the other pane, offered as target; absent when there is none. */
  readonly otherPaneFolder?: Location;
}

export interface CombineFilesDialogAttrs {
  readonly request: CombineFilesRequest | undefined;
  readonly client: Pick<FileManagerClient, 'inspectSplitSet'>;
  /** `name` is set only when the user changed the proposed output name. */
  readonly onCombine: (destination: Location | undefined, name: string | undefined) => void;
  readonly onCancel: () => void;
}

/** Whether the set was detected without a problem, so joining may start. */
export function canCombine(set: InspectSplitSetResult | undefined, name: string): boolean {
  return set !== undefined && set.error == null && set.parts.length > 0 && name.trim().length > 0;
}

function formatBytes(bytes: number): string {
  const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB'];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${unit === 0 ? value : value.toFixed(1)} ${units[unit]}`;
}

/**
 * "Combine files…" (task 0257): shows the detected set of parts, whether a manifest will verify
 * them, and why joining is impossible (for example a missing part), before anything is written.
 */
export const CombineFilesDialog: FactoryComponent<CombineFilesDialogAttrs> = () => {
  let set: InspectSplitSetResult | undefined;
  let failure: string | undefined;
  let name = '';
  let target: 'same' | 'other' = 'same';
  let token = 0;
  let inspectedKey = '';
  let wasOpen = false;

  function inspect(attrs: CombineFilesDialogAttrs): void {
    const request = attrs.request;
    if (request === undefined) return;
    const current = ++token;
    set = undefined;
    failure = undefined;
    attrs.client
      .inspectSplitSet(request.parts)
      .then((result) => {
        if (current !== token) return;
        set = result;
        name = result.outputName;
        m.redraw();
      })
      .catch((error: unknown) => {
        if (current !== token) return;
        failure = errorText(error);
        m.redraw();
      });
  }

  return {
    onupdate: ({ attrs }) => {
      const open = attrs.request !== undefined;
      const key = attrs.request?.parts.map((part) => part.uri).join(',') ?? '';
      if (open && (!wasOpen || key !== inspectedKey)) {
        inspectedKey = key;
        inspect(attrs);
      }
      if (!open) {
        token += 1;
        set = undefined;
        failure = undefined;
        target = 'same';
      }
      wasOpen = open;
    },
    view: ({ attrs }) => {
      const request = attrs.request;
      const loading = request !== undefined && set === undefined && failure === undefined;
      return m(ModalPanel, {
        title: t('splitCombine', 'combineTitle'),
        className: 'fm-dense-modal',
        description:
          request === undefined
            ? undefined
            : m('.fm-combine-files', [
                loading ? m('p', { role: 'status' }, t('splitCombine', 'detecting')) : undefined,
                failure === undefined
                  ? undefined
                  : m(
                      'p.fm-field-error',
                      { role: 'alert' },
                      t('splitCombine', 'detectFailed', { message: failure }),
                    ),
                set?.error == null
                  ? undefined
                  : m(
                      'p.fm-field-error',
                      { role: 'alert' },
                      t('splitCombine', 'cannotCombine', { message: set.error }),
                    ),
                set === undefined || set.error != null
                  ? undefined
                  : [
                      m(
                        'p',
                        t('splitCombine', 'detected', {
                          count: set.parts.length,
                          size: formatBytes(set.totalSize),
                        }),
                      ),
                      m(
                        'p.fm-geo-hint',
                        set.hasManifest
                          ? t('splitCombine', 'manifestFound')
                          : t('splitCombine', 'noManifest'),
                      ),
                      m('label.fm-create-directory-field', [
                        m('span', t('splitCombine', 'outputName')),
                        m('input', {
                          type: 'text',
                          value: name,
                          oninput: (event: InputEvent) => {
                            name = (event.currentTarget as HTMLInputElement).value;
                          },
                        }),
                      ]),
                      request.otherPaneFolder === undefined
                        ? undefined
                        : m('fieldset.fm-split-target', [
                            m('legend', t('splitCombine', 'target')),
                            ...(['same', 'other'] as const).map((choice) =>
                              m('label.fm-image-edit-check', [
                                m('input', {
                                  type: 'radio',
                                  name: 'fm-combine-target',
                                  checked: target === choice,
                                  onchange: () => {
                                    target = choice;
                                  },
                                }),
                                m(
                                  'span',
                                  choice === 'same'
                                    ? t('splitCombine', 'targetSame')
                                    : t('splitCombine', 'targetOther'),
                                ),
                              ]),
                            ),
                          ]),
                    ],
              ]),
        isOpen: request !== undefined,
        closeOnEsc: true,
        onToggle: (open: boolean) => {
          if (!open) attrs.onCancel();
        },
        buttons: [
          { label: t('button', 'cancel'), onclick: attrs.onCancel },
          {
            label: t('splitCombine', 'combine'),
            disabled: !canCombine(set, name),
            onclick: () => {
              if (request === undefined || set === undefined) return;
              const changed = name.trim() !== set.outputName ? name.trim() : undefined;
              attrs.onCombine(target === 'other' ? request.otherPaneFolder : undefined, changed);
            },
          },
        ],
      });
    },
  };
};
