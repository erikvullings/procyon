import m, { type FactoryComponent } from 'mithril';
import { FlatButton, Select, TextInput } from 'mithril-materialized';
import { t } from '../../i18n';
import type { GeneratedImage, Location } from '../../models';
import { IMAGE_COUNTS, IMAGE_SIZES, isValidImageFileName } from '../../models';
import {
  canGenerate,
  failureNeedsSettings,
  hasPreview,
  type ImagePaneEntry,
  imageRequestProblem,
  isSaveConflict,
  suggestedFileName,
} from './image-pane-state';
import {
  composeNegativePrompt,
  composePrompt,
  IMAGE_APERTURES,
  IMAGE_LENSES,
  IMAGE_PRESETS,
  presetById,
} from './image-presets';
import './image-pane.css';

export interface ImagePaneAttrs {
  readonly entry: ImagePaneEntry;
  /** Folder a save defaults to - the other pane's directory - and how to name it in the field. */
  readonly saveFolder?: Location | undefined;
  readonly saveFolderName?: string | undefined;
  /** The prompt actually sent, so the preview can show what produced these pictures. */
  readonly promptPreview?: string | undefined;
  readonly onPatch: (patch: Partial<ImagePaneEntry>) => void;
  readonly onGenerate: () => void;
  readonly onStop: () => void;
  readonly onSelect: (imageId: string) => void;
  readonly onSave: (input: {
    imageId: string;
    fileName: string;
    folder: Location;
    overwrite: boolean;
  }) => void;
  readonly onDiscard: () => void;
  readonly onRegenerate: () => void;
  /** Opens the AI settings, where the image role and consent are set. */
  readonly onOpenSettings?: (() => void) | undefined;
}

/**
 * The image generation pane (task 0325): a prompt on one side, the pictures on the other. Nothing is
 * written to disk here - the preview side only ever shows bytes the backend is holding, and Save is
 * the single path to a file. The two sides sit beside each other and stack when the pane is narrow.
 */
export const ImagePane: FactoryComponent<ImagePaneAttrs> = () => {
  /** The save form is local UI state: a file name, the folder text, and a replace confirmation. */
  let saveImageId: string | undefined;
  let fileName = '';
  let folderText = '';
  let confirmingReplace = false;

  const openSaveForm = (attrs: ImagePaneAttrs, image: GeneratedImage, index: number): void => {
    saveImageId = image.id;
    confirmingReplace = false;
    fileName = suggestedFileName(
      attrs.entry.batch?.prompt ?? attrs.entry.subject,
      index,
      image.width,
      image.height,
      image.extension,
    );
    folderText = attrs.saveFolder?.uri ?? '';
  };

  const closeSaveForm = (): void => {
    saveImageId = undefined;
    confirmingReplace = false;
  };

  const presetChips = (attrs: ImagePaneAttrs) =>
    m(
      '.fm-image-presets',
      { role: 'group', 'aria-label': t('image', 'presets') },
      IMAGE_PRESETS.map((preset) =>
        m(
          'button.fm-image-chip',
          {
            type: 'button',
            'aria-pressed': String(attrs.entry.presetId === preset.id),
            onclick: () => {
              // Picking a preset also takes the shape it suits; the person can change it afterwards.
              attrs.onPatch({
                presetId: attrs.entry.presetId === preset.id ? '' : preset.id,
                width: preset.width,
                height: preset.height,
              });
            },
          },
          [
            m('i.fm-image-chip-icon', { 'aria-hidden': 'true' }, preset.icon),
            t('imagePreset', preset.id),
          ],
        ),
      ),
    );

  const sizeFields = (attrs: ImagePaneAttrs) => {
    const { entry } = attrs;
    const preset = presetById(entry.presetId);
    return m('.fm-image-controls', [
      m(Select<string>, {
        label: t('image', 'size'),
        disabled: entry.pendingRequestId !== undefined,
        options: IMAGE_SIZES.map((size) => ({
          id: `${size.width}x${size.height}`,
          label: `${size.width} × ${size.height} · ${t('imageSize', size.labelKey)}`,
        })),
        checkedId: `${entry.width}x${entry.height}`,
        onchange: ([value]) => {
          if (value === undefined) return;
          const [width, height] = value.split('x').map(Number);
          if (width === undefined || height === undefined) return;
          attrs.onPatch({ width, height });
        },
      }),
      m(Select<number>, {
        label: t('image', 'count'),
        disabled: entry.pendingRequestId !== undefined,
        options: IMAGE_COUNTS.map((count) => ({ id: count, label: String(count) })),
        checkedId: entry.count,
        onchange: ([value]) => {
          if (value === undefined) return;
          attrs.onPatch({ count: value });
        },
      }),
      m(TextInput, {
        label: t('image', 'seed'),
        type: 'number',
        disabled: entry.pendingRequestId !== undefined,
        value: entry.seed === undefined ? '' : String(entry.seed),
        onchange: (value: string) => {
          const parsed = Number.parseInt(value.trim(), 10);
          attrs.onPatch({
            seed: Number.isFinite(parsed) && value.trim() !== '' ? parsed : undefined,
          });
        },
      }),
      m(TextInput, {
        label: t('image', 'negative'),
        disabled: entry.pendingRequestId !== undefined,
        active: entry.negativePrompt !== '',
        value: entry.negativePrompt,
        placeholder: preset === undefined ? t('image', 'negativePlaceholder') : preset.negative,
        onchange: (value: string) => {
          attrs.onPatch({ negativePrompt: value });
        },
      }),
    ]);
  };

  const lensFields = (attrs: ImagePaneAttrs) => {
    const preset = presetById(attrs.entry.presetId);
    if (preset === undefined || !preset.photographic) return undefined;
    return m('.fm-image-controls.fm-image-lens', [
      m(Select<string>, {
        label: t('image', 'lens'),
        disabled: attrs.entry.pendingRequestId !== undefined,
        options: IMAGE_LENSES.map((lens) => ({
          id: lens.id,
          label: lens.id === '' ? t('image', 'lensNone') : lens.id,
        })),
        checkedId: attrs.entry.lensId,
        onchange: ([value]) => {
          if (value === undefined) return;
          attrs.onPatch({ lensId: value });
        },
      }),
      m(Select<string>, {
        label: t('image', 'aperture'),
        disabled: attrs.entry.pendingRequestId !== undefined,
        options: IMAGE_APERTURES.map((aperture) => ({
          id: aperture.id,
          label: aperture.id === '' ? t('image', 'apertureNone') : aperture.id,
        })),
        checkedId: attrs.entry.apertureId,
        onchange: ([value]) => {
          if (value === undefined) return;
          attrs.onPatch({ apertureId: value });
        },
      }),
    ]);
  };

  const promptSide = (attrs: ImagePaneAttrs) => {
    const { entry } = attrs;
    const pending = entry.pendingRequestId !== undefined;
    const problem = imageRequestProblem(entry);
    return m('section.fm-image-side.fm-image-prompt', [
      m('h5.fm-image-side-title', t('image', 'promptTitle')),
      m('textarea.fm-image-subject', {
        'aria-label': t('image', 'subject'),
        placeholder: t('image', 'subjectPlaceholder'),
        value: entry.subject,
        disabled: pending,
        oninput: (event: Event) => {
          attrs.onPatch({ subject: (event.target as HTMLTextAreaElement).value });
        },
        onkeydown: (event: KeyboardEvent) => {
          if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) {
            event.preventDefault();
            if (canGenerate(entry)) attrs.onGenerate();
          }
        },
      }),
      presetChips(attrs),
      sizeFields(attrs),
      lensFields(attrs),
      m('p.fm-image-composed', [
        m('span.fm-image-composed-label', t('image', 'sent')),
        m(
          'span.fm-image-composed-text',
          composePrompt(entry.subject, entry.presetId, entry.lensId, entry.apertureId) === ''
            ? t('image', 'nothingYet')
            : composePrompt(entry.subject, entry.presetId, entry.lensId, entry.apertureId),
        ),
      ]),
      problem === undefined || pending
        ? undefined
        : m('p.fm-image-problem', { role: 'status' }, t('image', problem)),
      m('.fm-image-actions', [
        pending
          ? m(FlatButton, { label: t('image', 'stop'), onclick: () => attrs.onStop() })
          : m(FlatButton, {
              label: t('image', 'generate'),
              className: 'fm-primary-action',
              disabled: !canGenerate(entry),
              onclick: () => attrs.onGenerate(),
            }),
      ]),
    ]);
  };

  const saveForm = (attrs: ImagePaneAttrs, image: GeneratedImage) => {
    const valid =
      isValidImageFileName(fileName, image.extension) &&
      folderText.trim() !== '' &&
      attrs.saveFolder !== undefined;
    return m(
      'form.fm-image-save',
      {
        onsubmit: (event: Event) => {
          event.preventDefault();
          if (!valid) return;
          if (attrs.saveFolder === undefined) return;
          attrs.onSave({
            imageId: image.id,
            fileName: fileName.trim(),
            folder: { providerId: attrs.saveFolder.providerId, uri: folderText.trim() },
            overwrite: confirmingReplace,
          });
          closeSaveForm();
        },
      },
      [
        m(TextInput, {
          label: t('image', 'folder'),
          value: folderText,
          placeholder: attrs.saveFolderName ?? attrs.saveFolder?.uri ?? '',
          onchange: (value: string) => {
            folderText = value;
          },
        }),
        m(TextInput, {
          label: t('image', 'fileName'),
          value: fileName,
          onchange: (value: string) => {
            fileName = value;
          },
        }),
        m('p.fm-image-save-hint', t('image', 'saveHint', { extension: image.extension })),
        m('.fm-image-actions', [
          m(FlatButton, { label: t('image', 'cancel'), onclick: () => closeSaveForm() }),
          confirmingReplace
            ? m(FlatButton, {
                label: t('image', 'confirmReplace'),
                type: 'submit',
                disabled: !valid,
              })
            : m(FlatButton, {
                label: t('image', 'save'),
                className: 'fm-primary-action',
                type: 'submit',
                disabled: !valid,
              }),
        ]),
      ],
    );
  };

  const thumbnail = (
    attrs: ImagePaneAttrs,
    image: GeneratedImage,
    index: number,
    view: ImagePaneEntry['views'][number] | undefined,
  ) => {
    const selected = attrs.entry.selectedImageId === image.id;
    return m(
      'button.fm-image-thumb',
      {
        type: 'button',
        'aria-pressed': String(selected),
        'aria-label': t('image', 'showImage', { number: index + 1 }),
        onclick: () => attrs.onSelect(image.id),
      },
      view?.previewUrl === undefined
        ? m('span.fm-image-thumb-empty', { 'aria-hidden': 'true' }, m('i', 'image'))
        : m('img.fm-image-thumb-img', { src: view.previewUrl, alt: '' }),
    );
  };

  const previewSide = (attrs: ImagePaneAttrs) => {
    const { entry } = attrs;
    const saving = entry.savingImageId !== undefined;
    if (!hasPreview(entry) && entry.batch === undefined) {
      return m('section.fm-image-side.fm-image-preview', [
        m('h5.fm-image-side-title', t('image', 'previewTitle')),
        m('p.fm-image-empty', t('image', 'previewEmpty')),
      ]);
    }
    const batch = entry.batch;
    const selected =
      batch?.images.find((image) => image.id === entry.selectedImageId) ?? batch?.images[0];
    const selectedIndex = batch?.images.findIndex((image) => image.id === selected?.id) ?? 0;
    const selectedView = entry.views.find((view) => view.imageId === selected?.id);
    return m('section.fm-image-side.fm-image-preview', [
      m('h5.fm-image-side-title', t('image', 'previewTitle')),
      m('.fm-image-thumbs', { role: 'group', 'aria-label': t('image', 'images') }, [
        ...(batch?.images.map((image, index) =>
          thumbnail(
            attrs,
            image,
            index,
            entry.views.find((view) => view.imageId === image.id),
          ),
        ) ?? []),
        m(FlatButton, {
          label: t('image', 'discard'),
          disabled: saving,
          onclick: () => {
            closeSaveForm();
            attrs.onDiscard();
          },
        }),
      ]),
      selectedView?.loading
        ? m('p.fm-image-pending', { role: 'status' }, t('image', 'loading'))
        : undefined,
      selectedView?.previewUrl === undefined
        ? m('.fm-image-large.fm-image-large-empty', m('span', t('image', 'noPreview')))
        : m(
            '.fm-image-large',
            m('img', { src: selectedView.previewUrl, alt: t('image', 'previewAlt') }),
          ),
      entry.error
        ? m('.fm-image-error', { role: 'alert' }, [
            m('span', entry.error),
            failureNeedsSettings(entry.failure) && attrs.onOpenSettings
              ? m(FlatButton, {
                  label: t('image', 'openSettings'),
                  onclick: () => attrs.onOpenSettings?.(),
                })
              : undefined,
            isSaveConflict(entry.failure)
              ? m(FlatButton, {
                  label: t('image', 'replace'),
                  disabled: saving,
                  onclick: () => {
                    confirmingReplace = true;
                    if (selected !== undefined) openSaveForm(attrs, selected, selectedIndex);
                  },
                })
              : undefined,
          ])
        : undefined,
      selected === undefined
        ? undefined
        : m('.fm-image-preview-actions', [
            saveImageId === selected.id
              ? saveForm(attrs, selected)
              : m(FlatButton, {
                  label: t('image', 'save'),
                  disabled: saving,
                  onclick: () => openSaveForm(attrs, selected, selectedIndex),
                }),
            m(FlatButton, {
              label: t('image', 'regenerate'),
              disabled: saving || entry.pendingRequestId !== undefined,
              onclick: () => attrs.onRegenerate(),
            }),
          ]),
      selectedView?.savedLocation === undefined
        ? undefined
        : m(
            'p.fm-image-saved',
            { role: 'status' },
            t('image', 'savedTo', { location: selectedView.savedLocation }),
          ),
      selected === undefined || entry.batch === undefined
        ? undefined
        : m(
            'p.fm-image-meta',
            t('image', 'meta', {
              width: selected.width,
              height: selected.height,
              model: entry.batch.model,
              downscaled: selected.downscaled ? t('image', 'downscaled') : t('image', 'original'),
            }),
          ),
    ]);
  };

  return {
    view: ({ attrs }) => {
      const { entry } = attrs;
      const composed = composePrompt(entry.subject, entry.presetId, entry.lensId, entry.apertureId);
      const negative = composeNegativePrompt(entry.negativePrompt, entry.presetId);
      return m('div.fm-image', [
        m('header.fm-image-header', [
          m('span.fm-image-title', t('image', 'title')),
          m(FlatButton, {
            label: t('image', 'openSettings'),
            className: 'btn-flat fm-image-settings',
            onclick: () => attrs.onOpenSettings?.(),
          }),
        ]),
        m('.fm-image-split', [promptSide(attrs), previewSide(attrs)]),
        m('p.fm-image-footnote', { 'aria-live': 'polite' }, [
          composed === ''
            ? t('image', 'footnoteIdle')
            : t('image', 'footnoteReady', { prompt: composed, negative }),
        ]),
      ]);
    },
  };
};
