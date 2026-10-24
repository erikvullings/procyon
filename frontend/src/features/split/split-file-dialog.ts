import m, { type FactoryComponent } from 'mithril';
import { ModalPanel } from 'mithril-materialized';
import { t } from '../../i18n';
import type { Location, SplitFile } from '../../models';
import {
  customPartBytes,
  MAX_SPLIT_PARTS,
  partCount,
  SPLIT_PRESETS,
  type SplitUnit,
} from './split-actions';

/** The files to split, with the names and sizes shown to the user. */
export interface SplitFileRequest {
  readonly files: readonly {
    readonly location: Location;
    readonly name: string;
    readonly size: number;
  }[];
  /** Folder shown in the other pane, offered as target; absent when there is none. */
  readonly otherPaneFolder?: Location;
}

export interface SplitFileDialogAttrs {
  readonly request: SplitFileRequest | undefined;
  readonly onSplit: (split: SplitFile, destination: Location | undefined, count: number) => void;
  readonly onCancel: () => void;
}

const UNITS: readonly SplitUnit[] = ['KiB', 'MiB', 'GiB'];
const CUSTOM = 'custom';

/** The part size the form describes, or `undefined` when it is not valid yet. */
export function chosenPartBytes(
  preset: string,
  customValue: string,
  unit: SplitUnit,
): number | undefined {
  if (preset === CUSTOM) return customPartBytes(customValue, unit);
  return SPLIT_PRESETS.find((option) => option.id === preset)?.bytes;
}

/**
 * "Split file…" (task 0257): choose the part size, where the parts go and whether to write a
 * BLAKE3 manifest. The job itself runs in the backend operation engine.
 */
export const SplitFileDialog: FactoryComponent<SplitFileDialogAttrs> = () => {
  let preset = SPLIT_PRESETS[1]?.id ?? CUSTOM;
  let customValue = '50';
  let unit: SplitUnit = 'MiB';
  let manifest = true;
  let target: 'same' | 'other' = 'same';

  return {
    view: ({ attrs }) => {
      const request = attrs.request;
      const bytes = chosenPartBytes(preset, customValue, unit);
      const tooMany =
        bytes !== undefined &&
        request?.files.some((file) => partCount(file.size, bytes) > MAX_SPLIT_PARTS) === true;
      const valid = bytes !== undefined && !tooMany;
      const destination =
        target === 'other' && request?.otherPaneFolder !== undefined
          ? request.otherPaneFolder
          : undefined;
      return m(ModalPanel, {
        title: t('splitCombine', 'splitTitle'),
        className: 'fm-dense-modal',
        description:
          request === undefined
            ? undefined
            : m('.fm-split-file', [
                m('p', t('splitCombine', 'splitFiles', request.files.length)),
                m('label.fm-create-directory-field', [
                  m('span', t('splitCombine', 'partSize')),
                  m(
                    'select.browser-default',
                    {
                      value: preset,
                      onchange: (event: Event) => {
                        preset = (event.currentTarget as HTMLSelectElement).value;
                      },
                    },
                    [
                      ...SPLIT_PRESETS.map((option) =>
                        m(
                          'option',
                          { value: option.id, selected: option.id === preset },
                          t('splitCombine', `preset_${option.id}` as 'preset_mb10'),
                        ),
                      ),
                      m(
                        'option',
                        { value: CUSTOM, selected: preset === CUSTOM },
                        t('splitCombine', 'presetCustom'),
                      ),
                    ],
                  ),
                ]),
                preset === CUSTOM
                  ? m('.fm-split-custom', [
                      m('label.fm-create-directory-field', [
                        m('span', t('splitCombine', 'customSize')),
                        m('input', {
                          type: 'text',
                          inputmode: 'decimal',
                          value: customValue,
                          oninput: (event: InputEvent) => {
                            customValue = (event.currentTarget as HTMLInputElement).value;
                          },
                        }),
                      ]),
                      m(
                        'select.browser-default',
                        {
                          'aria-label': t('splitCombine', 'unit'),
                          value: unit,
                          onchange: (event: Event) => {
                            unit = (event.currentTarget as HTMLSelectElement).value as SplitUnit;
                          },
                        },
                        UNITS.map((option) =>
                          m('option', { value: option, selected: option === unit }, option),
                        ),
                      ),
                    ])
                  : undefined,
                bytes === undefined
                  ? m('p.fm-field-error', { role: 'alert' }, t('splitCombine', 'invalidSize'))
                  : m(
                      'ul.fm-split-summary',
                      request.files.map((file) =>
                        m(
                          'li',
                          { key: file.location.uri },
                          t('splitCombine', 'partsOf', {
                            name: file.name,
                            count: partCount(file.size, bytes),
                          }),
                        ),
                      ),
                    ),
                tooMany
                  ? m(
                      'p.fm-field-error',
                      { role: 'alert' },
                      t('splitCombine', 'tooManyParts', { max: MAX_SPLIT_PARTS }),
                    )
                  : undefined,
                request.otherPaneFolder === undefined
                  ? undefined
                  : m('fieldset.fm-split-target', [
                      m('legend', t('splitCombine', 'target')),
                      ...(['same', 'other'] as const).map((choice) =>
                        m('label.fm-image-edit-check', [
                          m('input', {
                            type: 'radio',
                            name: 'fm-split-target',
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
                m('label.fm-image-edit-check', [
                  m('input', {
                    type: 'checkbox',
                    checked: manifest,
                    onchange: (event: Event) => {
                      manifest = (event.currentTarget as HTMLInputElement).checked;
                    },
                  }),
                  m('span', t('splitCombine', 'writeManifest')),
                ]),
                m('p.fm-geo-hint', t('splitCombine', 'splitHint')),
              ]),
        isOpen: request !== undefined,
        closeOnEsc: true,
        onToggle: (open: boolean) => {
          if (!open) attrs.onCancel();
        },
        buttons: [
          { label: t('button', 'cancel'), onclick: attrs.onCancel },
          {
            label: t('splitCombine', 'split'),
            disabled: !valid,
            onclick: () => {
              if (bytes === undefined || request === undefined) return;
              attrs.onSplit(
                { partSizeBytes: bytes, writeManifest: manifest },
                destination,
                request.files.length,
              );
            },
          },
        ],
      });
    },
  };
};
