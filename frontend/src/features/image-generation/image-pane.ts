import m, { type FactoryComponent } from 'mithril';
import {
  checkIcon,
  folderOpenIcon,
  heartIcon,
  photoIcon,
  playerStopIcon,
  plusIcon,
  refreshIcon,
  settingsIcon,
  trashIcon,
} from '../../components/tabler-icons';
import { tooltip } from '../../components/tooltip';
import { t } from '../../i18n';
import type { GeneratedImage, Location } from '../../models';
import { IMAGE_SIZES, isValidImageFileName } from '../../models';
import {
  canGenerate,
  crumbLabel,
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
  selectedPresets,
  selectionIsPhotographic,
  togglePreset,
} from './image-presets';
import './image-pane.css';

export interface ImagePaneAttrs {
  readonly entry: ImagePaneEntry;
  /** Folder a save defaults to - the other pane's directory - and how to name it in the field. */
  readonly saveFolder?: Location | undefined;
  readonly saveFolderName?: string | undefined;
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
  /** Shows one of the generations the crumb bar holds. */
  readonly onSelectGeneration: (generationId: string) => void;
  /** Clears the prompt for a fresh idea, leaving the generations already made alone. */
  readonly onNewPrompt: () => void;
  /** Opens a new tab in the pane; the header offers it after the new-prompt control. */
  readonly onNewTab?: () => void;
  /** Opens the AI settings, where the image role and consent are set. */
  readonly onOpenSettings?: (() => void) | undefined;
}

/**
 * The image generation pane (task 0325), laid out the way the chat pane is: a crumb bar of the
 * generations this pane asked for, the pictures above, one prompt field at the bottom, and the
 * settings that shape the request in the status bar.
 *
 * Nothing here writes to disk - the preview only ever shows bytes the backend is holding, and Save
 * is the single path to a file.
 */
export const ImagePane: FactoryComponent<ImagePaneAttrs> = () => {
  /** The save form is local UI state: a file name, the folder text, and a replace confirmation. */
  let saveImageId: string | undefined;
  let fileName = '';
  let folderText = '';
  let confirmingReplace = false;
  /** The list of styles is folded into the status bar; there is no room to show which are on. */
  let stylesOpen = false;

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

  /** A chrome-density flat button (DESIGN.md "button-chrome": 20px, flat). */
  const chromeButton = (
    label: string,
    onclick: () => void,
    options: { readonly type?: 'button' | 'submit'; readonly disabled?: boolean } = {},
  ) =>
    m(
      'button.btn-flat.fm-image-chrome',
      { type: options.type ?? 'button', disabled: options.disabled ?? false, onclick },
      label,
    );

  /** An icon-only chrome button; the tooltip gives its name, as DESIGN.md requires. */
  const iconButton = (label: string, icon: m.Children, onclick: () => void, disabled = false) =>
    tooltip(
      label,
      m(
        'button.btn-flat.fm-image-chrome.fm-image-chrome-icon',
        { type: 'button', 'aria-label': label, disabled, onclick },
        icon,
      ),
    );

  /**
   * A choice in the status bar. A native select keeps the 20px row, needs no script to open and
   * speaks its own name to a screen reader, so nothing has to be drawn beside it.
   */
  const nativeSelect = (
    label: string,
    checkedId: string,
    options: readonly { readonly id: string; readonly label: string }[],
    disabled: boolean,
    onchange: (id: string) => void,
  ) =>
    m(
      // No `browser-default`: the shell makes those full width, and this row has none to spare.
      'select.fm-image-select',
      {
        'aria-label': label,
        disabled,
        value: checkedId,
        onchange: (event: Event) => {
          const target = event.target as HTMLSelectElement;
          onchange(target.value);
        },
      },
      options.map((option) => m('option', { value: option.id }, option.label)),
    );

  const crumbs = (attrs: ImagePaneAttrs) => {
    const { entry } = attrs;
    return m(
      'nav.fm-image-crumbs',
      { role: 'navigation', 'aria-label': t('image', 'generations') },
      entry.history.map((record) => {
        const active = record.generationId === entry.activeGenerationId;
        const label = crumbLabel(record.batch.prompt);
        return m(
          'button.fm-image-crumb',
          {
            type: 'button',
            title: record.batch.prompt,
            'aria-current': active ? 'true' : undefined,
            class: active ? 'fm-image-crumb-active' : undefined,
            onclick: () => attrs.onSelectGeneration(record.generationId),
          },
          label === '' ? t('image', 'untitledGeneration') : label,
        );
      }),
    );
  };

  /** The styles live behind one status bar control that counts them rather than naming them. */
  const styleControl = (attrs: ImagePaneAttrs) => {
    const { entry } = attrs;
    const chosen = selectedPresets(entry.presetIds);
    const pending = entry.pendingRequestId !== undefined;
    return m('.fm-image-presets', [
      m(
        'button.btn-flat.fm-image-chrome.fm-image-preset-trigger',
        {
          type: 'button',
          'aria-expanded': String(stylesOpen),
          'aria-controls': stylesOpen ? 'fm-image-preset-list' : undefined,
          disabled: pending,
          onclick: () => {
            stylesOpen = !stylesOpen;
          },
          onkeydown: (event: KeyboardEvent) => {
            if (event.key !== 'Escape' || !stylesOpen) return;
            stylesOpen = false;
          },
        },
        chosen.length === 0
          ? t('image', 'styles')
          : t('image', 'stylesCount', { count: String(chosen.length) }),
      ),
      stylesOpen
        ? m(
            'div.fm-image-preset-list',
            {
              id: 'fm-image-preset-list',
              role: 'group',
              'aria-label': t('image', 'styles'),
            },
            IMAGE_PRESETS.map((preset) => {
              const label = t('imagePreset', preset.id);
              const on = entry.presetIds.includes(preset.id);
              return m('label.fm-image-preset-option', [
                m('input', {
                  type: 'checkbox',
                  'aria-label': label,
                  checked: on,
                  disabled: pending,
                  onclick: (event: Event) => {
                    event.preventDefault();
                    const next = togglePreset(entry.presetIds, preset.id);
                    const medium = presetById(togglePreset([], preset.id)[0] ?? '');
                    // A medium also brings the shape it suits; the size control beside it stays open
                    // to the person afterwards.
                    attrs.onPatch(
                      medium !== undefined && medium.group === 'medium'
                        ? { presetIds: next, width: medium.width, height: medium.height }
                        : { presetIds: next },
                    );
                  },
                }),
                m('span.fm-image-preset-label', label),
              ]);
            }),
          )
        : undefined,
    ]);
  };

  const status = (attrs: ImagePaneAttrs) => {
    const { entry } = attrs;
    const pending = entry.pendingRequestId !== undefined;
    const photographic = selectionIsPhotographic(entry.presetIds);
    return m('footer.fm-image-status', [
      styleControl(attrs),
      nativeSelect(
        t('image', 'size'),
        `${entry.width}x${entry.height}`,
        IMAGE_SIZES.map((size) => ({
          id: `${size.width}x${size.height}`,
          label: `${size.width} × ${size.height} · ${t('imageSize', size.labelKey)}`,
        })),
        pending,
        (value) => {
          const [width, height] = value.split('x').map(Number);
          if (width === undefined || height === undefined) return;
          attrs.onPatch({ width, height });
        },
      ),
      photographic
        ? nativeSelect(
            t('image', 'lens'),
            entry.lensId,
            IMAGE_LENSES.map((lens) => ({
              id: lens.id,
              label: lens.id === '' ? t('image', 'lensNone') : lens.id,
            })),
            pending,
            (value) => attrs.onPatch({ lensId: value }),
          )
        : undefined,
      photographic
        ? nativeSelect(
            t('image', 'aperture'),
            entry.apertureId,
            IMAGE_APERTURES.map((aperture) => ({
              id: aperture.id,
              label: aperture.id === '' ? t('image', 'apertureNone') : aperture.id,
            })),
            pending,
            (value) => attrs.onPatch({ apertureId: value }),
          )
        : undefined,
      // `browser-default` keeps Materialize from stretching the field to the whole row.
      m('input.fm-image-seed.browser-default', {
        type: 'number',
        'aria-label': t('image', 'seed'),
        min: '0',
        max: '4294967295',
        placeholder: t('image', 'seedPlaceholder'),
        disabled: pending,
        value: entry.seed === undefined ? '' : String(entry.seed),
        oninput: (event: Event) => {
          const text = (event.target as HTMLInputElement).value.trim();
          const parsed = Number.parseInt(text, 10);
          attrs.onPatch({ seed: text !== '' && Number.isFinite(parsed) ? parsed : undefined });
        },
      }),
      m('.fm-image-status-actions', [
        hasPreview(entry)
          ? iconButton(
              t('image', 'discard'),
              trashIcon({ size: 14 }),
              () => {
                closeSaveForm();
                attrs.onDiscard();
              },
              entry.savingImageId !== undefined,
            )
          : undefined,
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
          if (!valid || attrs.saveFolder === undefined) return;
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
        m('input.fm-image-folder', {
          type: 'text',
          'aria-label': t('image', 'folder'),
          value: folderText,
          placeholder: attrs.saveFolderName ?? attrs.saveFolder?.uri ?? '',
          oninput: (event: Event) => {
            folderText = (event.target as HTMLInputElement).value;
          },
        }),
        m('input.fm-image-name', {
          type: 'text',
          'aria-label': t('image', 'fileName'),
          value: fileName,
          oninput: (event: Event) => {
            fileName = (event.target as HTMLInputElement).value;
          },
        }),
        m('p.fm-image-save-hint', t('image', 'saveHint', { extension: image.extension })),
        m('.fm-image-save-actions', [
          chromeButton(t('image', 'cancel'), () => closeSaveForm()),
          confirmingReplace
            ? chromeButton(t('image', 'confirmReplace'), () => undefined, {
                type: 'submit',
                disabled: !valid,
              })
            : chromeButton(t('image', 'save'), () => undefined, {
                type: 'submit',
                disabled: !valid,
              }),
        ]),
      ],
    );
  };

  const imageActions = (attrs: ImagePaneAttrs, image: GeneratedImage, index: number) => {
    const saving = attrs.entry.savingImageId !== undefined;
    if (saveImageId === image.id) return saveForm(attrs, image);
    return m('.fm-image-image-actions', [
      iconButton(t('image', 'save'), folderOpenIcon({ size: 14 }), () =>
        openSaveForm(attrs, image, index),
      ),
      iconButton(
        t('image', 'regenerate'),
        refreshIcon({ size: 14 }),
        () => attrs.onRegenerate(),
        saving || attrs.entry.pendingRequestId !== undefined,
      ),
    ]);
  };

  const preview = (attrs: ImagePaneAttrs) => {
    const { entry } = attrs;
    const pending = entry.pendingRequestId !== undefined;
    const images = entry.batch?.images ?? [];
    // One picture is normally on screen, so no click is needed to make it the chosen one.
    const selectedId = entry.selectedImageId ?? images[0]?.id;
    return m('section.fm-image-preview', { role: 'log', 'aria-live': 'polite' }, [
      pending
        ? m('p.fm-image-pending', t('image', 'waitingForModel'))
        : images.length === 0
          ? undefined
          : images.map((image, index) => {
              const view = entry.views.find((candidate) => candidate.imageId === image.id);
              const selected = image.id === selectedId;
              return m(
                'figure.fm-image-figure',
                {
                  class: selected ? 'fm-image-figure-selected' : undefined,
                  onclick: () => {
                    if (!selected) attrs.onSelect(image.id);
                  },
                  tabindex: '0',
                  role: 'button',
                  'aria-label': t('image', 'showImage', { number: index + 1 }),
                  onkeydown: (event: KeyboardEvent) => {
                    if (event.key !== 'Enter' && event.key !== ' ') return;
                    event.preventDefault();
                    attrs.onSelect(image.id);
                  },
                },
                [
                  view?.loading
                    ? m('p.fm-image-pending', t('image', 'loading'))
                    : view?.previewUrl === undefined
                      ? undefined
                      : m('img', { src: view.previewUrl, alt: t('image', 'previewAlt') }),
                  selected ? imageActions(attrs, image, index) : undefined,
                ],
              );
            }),
      entry.error
        ? m('.fm-image-error', { role: 'alert' }, [
            m('span', entry.error),
            failureNeedsSettings(entry.failure) && attrs.onOpenSettings
              ? chromeButton(t('image', 'openSettings'), () => attrs.onOpenSettings?.())
              : undefined,
            isSaveConflict(entry.failure)
              ? chromeButton(
                  t('image', 'replace'),
                  () => {
                    confirmingReplace = true;
                    const selected =
                      images.find((image) => image.id === entry.selectedImageId) ?? images[0];
                    if (selected !== undefined) {
                      openSaveForm(attrs, selected, images.indexOf(selected));
                    }
                  },
                  { disabled: entry.savingImageId !== undefined },
                )
              : undefined,
            !pending && entry.batch !== undefined && entry.selectedImageId !== undefined
              ? chromeButton(t('image', 'regenerate'), () => attrs.onRegenerate())
              : undefined,
          ])
        : undefined,
      entry.views.find((view) => view.imageId === entry.selectedImageId)?.savedLocation ===
      undefined
        ? undefined
        : m(
            'p.fm-image-saved',
            { role: 'status' },
            t('image', 'savedTo', {
              location:
                entry.views.find((view) => view.imageId === entry.selectedImageId)?.savedLocation ??
                '',
            }),
          ),
    ]);
  };

  const composer = (attrs: ImagePaneAttrs) => {
    const { entry } = attrs;
    const pending = entry.pendingRequestId !== undefined;
    const problem = imageRequestProblem(entry);
    const composed = composePrompt(entry.subject, entry.presetIds, entry.lensId, entry.apertureId);
    const negative = composeNegativePrompt(entry.negativePrompt, entry.presetIds);
    return m(
      'form.fm-image-composer',
      {
        onsubmit: (event: Event) => {
          event.preventDefault();
          if (pending || !canGenerate(entry)) return;
          attrs.onGenerate();
        },
      },
      [
        m('.fm-image-composer-row', [
          m('textarea.fm-image-input', {
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
          m('.fm-image-composer-actions', [
            pending
              ? tooltip(
                  t('image', 'stop'),
                  m(
                    'button.btn-flat.fm-image-chrome.fm-image-chrome-icon.fm-image-stop',
                    {
                      type: 'button',
                      'aria-label': t('image', 'stop'),
                      onclick: () => attrs.onStop(),
                    },
                    playerStopIcon({ size: 14 }),
                  ),
                )
              : tooltip(
                  t('image', 'generate'),
                  m(
                    'button.btn-flat.fm-image-chrome.fm-image-chrome-icon.fm-image-send',
                    {
                      type: 'submit',
                      'aria-label': t('image', 'generate'),
                      disabled: !canGenerate(entry),
                    },
                    checkIcon({ size: 14 }),
                  ),
                ),
          ]),
        ]),
        // What to leave out travels with the prompt it belongs to, in the same box.
        m('input.fm-image-avoid', {
          type: 'text',
          'aria-label': t('image', 'negative'),
          placeholder: t('image', 'negativePlaceholder'),
          disabled: pending,
          value: entry.negativePrompt,
          oninput: (event: Event) => {
            attrs.onPatch({ negativePrompt: (event.target as HTMLInputElement).value });
          },
        }),
        // The wording the presets and lens picks added is what the model will be sent; a person
        // who cannot see it cannot trust it.
        m('p.fm-image-composed', { 'aria-live': 'polite' }, [
          m('span.fm-image-composed-label', t('image', 'sent')),
          m('span.fm-image-composed-text', composed === '' ? t('image', 'nothingYet') : composed),
          negative === ''
            ? undefined
            : [
                m('span.fm-image-composed-label', t('image', 'negative')),
                m('span.fm-image-composed-text', negative),
              ],
          problem === undefined || pending
            ? undefined
            : m('span.fm-image-problem', t('image', problem)),
        ]),
      ],
    );
  };

  return {
    view: ({ attrs }) => {
      const { entry } = attrs;
      return m('section.fm-image', [
        m('header.fm-image-header', [
          crumbs(attrs),
          iconButton(
            t('image', 'newPrompt'),
            photoIcon({ size: 16 }),
            () => attrs.onNewPrompt(),
            entry.pendingRequestId !== undefined,
          ),
          attrs.onNewTab === undefined
            ? undefined
            : iconButton(t('pane', 'newTab'), plusIcon({ size: 16 }), () => attrs.onNewTab?.()),
          // The pane's favourites menu is its own chrome; the viewer's header mirrors the button
          // the tab strip would otherwise show.
          iconButton(t('pane', 'favourites'), heartIcon({ size: 16 }), () => undefined),
          iconButton(t('image', 'openSettings'), settingsIcon({ size: 16 }), () =>
            attrs.onOpenSettings?.(),
          ),
        ]),
        m('.fm-image-body', [preview(attrs), composer(attrs)]),
        status(attrs),
      ]);
    },
  };
};
