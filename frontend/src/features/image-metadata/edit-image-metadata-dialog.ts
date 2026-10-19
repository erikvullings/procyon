import m, { type FactoryComponent } from 'mithril';
import { ModalPanel } from 'mithril-materialized';
import { t } from '../../i18n';
import type { ImageMetadataEdit, Location } from '../../models';
import {
  buildImageMetadataEdit,
  emptyForm,
  formProblems,
  type ImageMetadataForm,
} from './image-metadata-edit';

/** The images being edited and which fields their formats can store. */
export interface EditImageMetadataRequest {
  readonly files: readonly Location[];
  /** Title, keywords and rating need JPEG (XMP/IPTC); other formats keep EXIF fields only. */
  readonly richFields: boolean;
}

export interface EditImageMetadataDialogAttrs {
  readonly request: EditImageMetadataRequest | undefined;
  readonly onApply: (edit: ImageMetadataEdit) => void;
  readonly onCancel: () => void;
}

type TextField = 'title' | 'description' | 'copyright' | 'author' | 'keywords';

function blurActive(): void {
  const active = document.activeElement;
  if (active instanceof HTMLElement) active.blur();
}

/** "Edit image metadata…" modal (task 0274): blank fields stay unchanged. */
export const EditImageMetadataDialog: FactoryComponent<EditImageMetadataDialogAttrs> = () => {
  let form: ImageMetadataForm = emptyForm();
  let cleared = new Set<string>();
  let wasOpen = false;

  const update = (patch: Partial<ImageMetadataForm>): void => {
    form = { ...form, ...patch, cleared };
  };

  function toggleCleared(id: string): void {
    cleared = new Set(cleared);
    if (cleared.has(id)) cleared.delete(id);
    else cleared.add(id);
    update({});
  }

  function row(
    id: string,
    label: string,
    input: m.Children,
    clearable: boolean,
    disabled: boolean,
  ): m.Children {
    return m('.fm-image-edit-row', { class: disabled ? 'is-disabled' : undefined }, [
      m('label.fm-create-directory-field', [m('span', label), input]),
      clearable
        ? m('label.fm-image-edit-clear', [
            m('input', {
              type: 'checkbox',
              checked: cleared.has(id),
              disabled,
              onchange: () => toggleCleared(id),
            }),
            m('span', t('imageEdit', 'clear')),
          ])
        : undefined,
    ]);
  }

  function textInput(field: TextField, disabled: boolean): m.Children {
    return m('input', {
      type: 'text',
      value: form[field],
      disabled,
      oninput: (event: InputEvent) =>
        update({ [field]: (event.currentTarget as HTMLInputElement).value }),
    });
  }

  function numberSelect(
    value: string,
    options: readonly string[],
    onChange: (value: string) => void,
    disabled: boolean,
  ): m.Children {
    return m(
      'select.browser-default',
      {
        value,
        disabled,
        onchange: (event: Event) => onChange((event.currentTarget as HTMLSelectElement).value),
      },
      [
        m('option', { value: '', selected: value === '' }, t('imageEdit', 'unchanged')),
        ...options.map((option) =>
          m('option', { value: option, selected: option === value }, option),
        ),
      ],
    );
  }

  function checkbox(label: string, checked: boolean, onChange: (checked: boolean) => void) {
    return m('label.fm-image-edit-check', [
      m('input', {
        type: 'checkbox',
        checked,
        onchange: (event: Event) => onChange((event.currentTarget as HTMLInputElement).checked),
      }),
      m('span', label),
    ]);
  }

  function apply(attrs: EditImageMetadataDialogAttrs): void {
    if (formProblems(form).length > 0) return;
    const edit = buildImageMetadataEdit({ ...form, cleared });
    if (edit === undefined) return;
    blurActive();
    attrs.onApply(edit);
  }

  function cancel(attrs: EditImageMetadataDialogAttrs): void {
    blurActive();
    attrs.onCancel();
  }

  return {
    oncreate: ({ attrs }) => {
      wasOpen = attrs.request !== undefined;
    },
    onupdate: ({ attrs }) => {
      const open = attrs.request !== undefined;
      if (open && !wasOpen) {
        cleared = new Set();
        form = emptyForm();
      }
      wasOpen = open;
    },
    view: ({ attrs }) => {
      const problems = formProblems(form);
      const everything = form.removeAll;
      const plain = everything;
      const rich = attrs.request?.richFields === true && !everything;
      const edit = buildImageMetadataEdit({ ...form, cleared });
      return m(ModalPanel, {
        title: t('imageEdit', 'title'),
        className: 'fm-dense-modal',
        description: m('.fm-image-edit', [
          attrs.request === undefined
            ? undefined
            : m('p', t('imageEdit', 'files', { n: attrs.request.files.length })),
          row(
            'dateTaken',
            t('imageEdit', 'dateTaken'),
            m('input', {
              type: 'datetime-local',
              step: 1,
              value: form.dateTaken,
              disabled: plain,
              oninput: (event: InputEvent) =>
                update({ dateTaken: (event.currentTarget as HTMLInputElement).value }),
            }),
            true,
            plain,
          ),
          row(
            'shift',
            t('imageEdit', 'shiftMinutes'),
            m('input', {
              type: 'number',
              step: 1,
              value: form.shiftMinutes,
              disabled: plain,
              oninput: (event: InputEvent) =>
                update({ shiftMinutes: (event.currentTarget as HTMLInputElement).value }),
            }),
            false,
            plain,
          ),
          row(
            'gps',
            `${t('imageEdit', 'latitude')} / ${t('imageEdit', 'longitude')}`,
            m('.fm-image-edit-gps', [
              m('input', {
                type: 'text',
                inputmode: 'decimal',
                'aria-label': t('imageEdit', 'latitude'),
                value: form.latitude,
                disabled: plain,
                oninput: (event: InputEvent) =>
                  update({ latitude: (event.currentTarget as HTMLInputElement).value }),
              }),
              m('input', {
                type: 'text',
                inputmode: 'decimal',
                'aria-label': t('imageEdit', 'longitude'),
                value: form.longitude,
                disabled: plain,
                oninput: (event: InputEvent) =>
                  update({ longitude: (event.currentTarget as HTMLInputElement).value }),
              }),
            ]),
            true,
            plain,
          ),
          row(
            'description',
            t('imageEdit', 'description'),
            textInput('description', plain),
            true,
            plain,
          ),
          row('copyright', t('imageEdit', 'copyright'), textInput('copyright', plain), true, plain),
          row('author', t('imageEdit', 'author'), textInput('author', plain), true, plain),
          row(
            'orientation',
            t('imageEdit', 'orientation'),
            numberSelect(
              form.orientation,
              ['1', '2', '3', '4', '5', '6', '7', '8'],
              (orientation) => update({ orientation }),
              plain,
            ),
            true,
            plain,
          ),
          row('title', t('imageEdit', 'titleField'), textInput('title', !rich), true, !rich),
          row('keywords', t('imageEdit', 'keywords'), textInput('keywords', !rich), true, !rich),
          row(
            'rating',
            t('imageEdit', 'rating'),
            numberSelect(
              form.rating,
              ['0', '1', '2', '3', '4', '5'],
              (rating) => update({ rating }),
              !rich,
            ),
            true,
            !rich,
          ),
          attrs.request?.richFields === true
            ? undefined
            : m('p.fm-image-edit-note', t('imageEdit', 'richOnlyJpeg')),
          checkbox(t('imageEdit', 'removeLocation'), form.removeLocation, (removeLocation) =>
            update({ removeLocation }),
          ),
          checkbox(t('imageEdit', 'removeAll'), form.removeAll, (removeAll) =>
            update({ removeAll }),
          ),
          checkbox(t('imageEdit', 'backup'), form.backupOriginal, (backupOriginal) =>
            update({ backupOriginal }),
          ),
          problems.map((problem) => m('.fm-field-error', t('imageEdit', problem))),
        ]),
        isOpen: attrs.request !== undefined,
        closeOnEsc: true,
        onToggle: (open: boolean) => {
          if (!open) cancel(attrs);
        },
        buttons: [
          { label: t('button', 'cancel'), onclick: () => cancel(attrs) },
          {
            label: t('imageEdit', 'apply'),
            disabled: problems.length > 0 || edit === undefined,
            onclick: () => apply(attrs),
          },
        ],
      });
    },
  };
};
