import m, { type FactoryComponent } from 'mithril';
import {
  adjustmentsHorizontalIcon,
  arrowsMaximizeIcon,
  arrowsMinimizeIcon,
  checkIcon,
  chevronDownIcon,
  chevronRightIcon,
  cornerDownLeftIcon,
  eyeIcon,
  eyeOffIcon,
  folderOpenIcon,
  photoIcon,
  playerStopIcon,
  plusIcon,
  refreshIcon,
  settingsIcon,
  trashIcon,
} from '../../components/tabler-icons';
import { tooltip } from '../../components/tooltip';
import { t } from '../../i18n';
import type { GeneratedImage, ImagePrompt, Location } from '../../models';
import { IMAGE_QUALITIES, IMAGE_SIZES, isValidImageFileName, qualityIdOf } from '../../models';
import {
  activeGeneration,
  canGenerate,
  distinctTitles,
  failureNeedsSettings,
  folderDisplayPath,
  hasPreview,
  IMAGE_SIZE_LIMITS,
  IMAGE_STEP_OPTIONS,
  type ImagePaneEntry,
  isSaveConflict,
  shortTitle,
  sizeAllowed,
  sourceNameOf,
  suggestedFileName,
} from './image-pane-state';
import {
  chooseMedium,
  chooseTreatment,
  composeNegativePrompt,
  composePrompt,
  IMAGE_APERTURES,
  IMAGE_LENSES,
  IMAGE_PRESETS,
  mediumOf,
  presetById,
  selectionIsPhotographic,
  treatmentOf,
} from './image-presets';
import './image-pane.css';

export interface ImagePaneAttrs {
  readonly entry: ImagePaneEntry;
  /** Folder a save defaults to - the other pane's directory - and how to name it in the field. */
  readonly saveFolder?: Location | undefined;
  readonly saveFolderName?: string | undefined;
  /** Every folder a save may go to, best first; the folder field suggests them. */
  readonly saveFolders?: readonly { readonly location: Location; readonly name: string }[];
  /** Lower-cased names of the files in a folder the app knows, or nothing when it knows none. */
  readonly fileNamesIn?: (folder: Location) => ReadonlySet<string> | undefined;
  /** The home directory, so folders read `~/Pictures` instead of a file URI. */
  readonly homeDirectory?: string | undefined;
  /** Turns a path the person typed into a location beside the one they were saving to. */
  readonly resolveFolder?: (current: Location, path: string) => Location;
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
  /** Takes the picture the other pane has selected as the one this prompt starts from. */
  readonly onUseSource: () => void;
  /** Stops starting from a picture. */
  readonly onClearSource: () => void;
  /** Shows one of the generations the crumb bar holds. */
  readonly onSelectGeneration: (generationId: string) => void;
  /** Clears the prompt for a fresh idea, leaving the generations already made alone. */
  readonly onNewPrompt: () => void;
  /** Puts a remembered prompt, with every setting it was used with, back in the form. */
  readonly onApplyPrompt: (prompt: ImagePrompt) => void;
  /** Forgets a remembered prompt. */
  readonly onDeletePrompt: (promptId: string) => void;
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
  /** Which image of the batch the open save form is for, so a new folder can re-suggest its name. */
  let saveImageIndex = 0;
  /** Which status choice has its list open, so at most one is ever drawn. */
  let openChoice: string | undefined;
  /** Whether the list of remembered prompts is unfolded, and which one is asking to be deleted. */
  let promptsOpen = false;
  let confirmingDelete: string | undefined;

  /** Whether the picture is fitted to the pane (true) or shown at the pane's full width and scrolled. */
  let fitPreview = true;
  /** Whether the prompt box shows the full wording sent to the model instead of the editable fields. */
  let showComposed = false;
  /** Whether the less-used settings row is unfolded above the status bar. */
  let moreOpen = false;

  /** The text of a folder as the person reads it: a plain path, `~` for home. */
  const folderText0 = (attrs: ImagePaneAttrs, folder: Location | undefined): string =>
    folder === undefined ? '' : folderDisplayPath(folder.uri, attrs.homeDirectory);

  /** The names a suggestion must avoid: what the folder already holds. */
  const takenIn = (attrs: ImagePaneAttrs, folder: Location | undefined) =>
    folder === undefined ? undefined : attrs.fileNamesIn?.(folder);

  /** The file name's stem: the format is fixed, so the person only ever edits what comes before it. */
  const stemOf = (name: string, extension: string): string => {
    const suffix = `.${extension.toLowerCase()}`;
    return name.toLowerCase().endsWith(suffix) ? name.slice(0, name.length - suffix.length) : name;
  };

  /** Suggests a name that is not taken in `folder`, counting the index up past any that are. */
  const suggestName = (
    attrs: ImagePaneAttrs,
    image: GeneratedImage,
    index: number,
    folder: Location | undefined,
  ): void => {
    fileName = stemOf(
      suggestedFileName(
        // What was typed for this generation, not the composed wording the endpoint echoes back.
        activeGeneration(attrs.entry)?.subject ?? attrs.entry.batch?.prompt ?? attrs.entry.subject,
        index,
        image.width,
        image.height,
        image.extension,
        takenIn(attrs, folder),
      ),
      image.extension,
    );
  };

  const openSaveForm = (attrs: ImagePaneAttrs, image: GeneratedImage, index: number): void => {
    saveImageId = image.id;
    saveImageIndex = index;
    confirmingReplace = false;
    folderText = folderText0(attrs, attrs.saveFolder);
    suggestName(attrs, image, index, attrs.saveFolder);
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
   * A choice in the status bar, drawn rather than left to the browser: Materialize hides every
   * `select` it is not told about, and a 20px row cannot carry the field it builds in their place.
   * The trigger names the setting, so the value alone is what the row has to fit.
   */
  const statusChoice = (
    key: string,
    label: string,
    checkedId: string,
    options: readonly { readonly id: string; readonly label: string }[],
    disabled: boolean,
    onchange: (id: string) => void,
    compact = false,
  ) => {
    const current = options.find((option) => option.id === checkedId) ?? options[0];
    const open = openChoice === key;
    const listId = `fm-image-choice-${key}`;
    return m('.fm-image-choice', [
      m(
        'button.btn-flat.fm-image-chrome.fm-image-choice-trigger',
        {
          type: 'button',
          'aria-label': label,
          'aria-haspopup': 'listbox',
          'aria-expanded': String(open),
          'aria-controls': open ? listId : undefined,
          disabled,
          onclick: () => {
            openChoice = open ? undefined : key;
          },
          onkeydown: (event: KeyboardEvent) => {
            if (event.key !== 'Escape') return;
            openChoice = undefined;
          },
        },
        [
          compact ? undefined : m('span.fm-image-choice-value', current?.label ?? ''),
          chevronDownIcon({ size: 12 }),
        ],
      ),
      open
        ? m(
            `ul#${listId}.fm-image-choice-list`,
            { role: 'listbox', 'aria-label': label },
            options.map((option) =>
              m(
                'li.fm-image-choice-option',
                {
                  role: 'option',
                  'aria-selected': String(option.id === checkedId),
                  tabindex: '0',
                  onclick: () => {
                    openChoice = undefined;
                    onchange(option.id);
                  },
                  onkeydown: (event: KeyboardEvent) => {
                    if (event.key !== 'Enter' && event.key !== ' ') return;
                    event.preventDefault();
                    openChoice = undefined;
                    onchange(option.id);
                  },
                },
                option.label,
              ),
            ),
          )
        : undefined,
    ]);
  };

  const crumbs = (attrs: ImagePaneAttrs) => {
    const { entry } = attrs;
    // Each generation is its own name; the same name twice is numbered rather than repeated.
    const titles = distinctTitles(
      entry.history.map((record) => record.subject ?? record.batch.prompt),
    );
    return m(
      'nav.fm-image-crumbs',
      { role: 'navigation', 'aria-label': t('image', 'generations') },
      entry.history.map((record, index) => {
        const active = record.generationId === entry.activeGenerationId;
        const label = titles[index] ?? '';
        return m(
          'button.fm-image-crumb',
          {
            type: 'button',
            title: record.subject ?? record.batch.prompt,
            'aria-current': active ? 'true' : undefined,
            class: active ? 'fm-image-crumb-active' : undefined,
            onclick: () => attrs.onSelectGeneration(record.generationId),
          },
          label === '' ? t('image', 'untitledGeneration') : label,
        );
      }),
    );
  };

  /** One remembered prompt: its title opens it in the form; the trash forgets it after a second click. */
  const promptItem = (attrs: ImagePaneAttrs, prompt: ImagePrompt, title: string) =>
    m('li.fm-image-prompt', { key: prompt.id }, [
      m(
        'button.fm-image-prompt-open',
        {
          type: 'button',
          title: prompt.subject,
          onclick: () => {
            attrs.onApplyPrompt(prompt);
            promptsOpen = false;
            confirmingDelete = undefined;
          },
        },
        title === '' ? shortTitle(prompt.subject) : title,
      ),
      confirmingDelete === prompt.id
        ? iconButton(t('image', 'confirmDeletePrompt'), checkIcon({ size: 14 }), () => {
            confirmingDelete = undefined;
            attrs.onDeletePrompt(prompt.id);
          })
        : iconButton(t('image', 'deletePrompt'), trashIcon({ size: 14 }), () => {
            confirmingDelete = prompt.id;
          }),
    ]);

  const promptList = (attrs: ImagePaneAttrs) => {
    const titles = distinctTitles(attrs.entry.prompts.map((prompt) => prompt.subject));
    return m(
      'section.fm-image-prompts',
      { id: 'fm-image-prompts', role: 'navigation', 'aria-label': t('image', 'prompts') },
      attrs.entry.prompts.length === 0
        ? m('.fm-image-prompts-empty', t('image', 'noPrompts'))
        : m(
            'ul',
            attrs.entry.prompts.map((prompt, index) =>
              promptItem(attrs, prompt, titles[index] ?? ''),
            ),
          ),
    );
  };

  /** Width and height are two fields: any pair the backend accepts, snapped to a multiple of 8 on leaving. */
  const sizeFields = (attrs: ImagePaneAttrs, pending: boolean) => {
    const { entry } = attrs;
    const valid = sizeAllowed(entry.width, entry.height);
    const field = (label: string, name: 'width' | 'height') =>
      m('input.fm-image-dimension.browser-default', {
        type: 'number',
        'aria-label': label,
        'aria-invalid': valid ? undefined : 'true',
        title: t('image', 'sizeHint', {
          min: IMAGE_SIZE_LIMITS.minEdge,
          max: IMAGE_SIZE_LIMITS.maxEdge,
        }),
        min: String(IMAGE_SIZE_LIMITS.minEdge),
        max: String(IMAGE_SIZE_LIMITS.maxEdge),
        step: String(IMAGE_SIZE_LIMITS.step),
        disabled: pending,
        value: String(entry[name]),
        oninput: (event: Event) => {
          const parsed = Number.parseInt((event.target as HTMLInputElement).value, 10);
          if (Number.isFinite(parsed)) attrs.onPatch({ [name]: parsed });
        },
        // Leaving the field rounds to what the backend takes, so a near miss never blocks Generate.
        onchange: (event: Event) => {
          const { minEdge, maxEdge, step } = IMAGE_SIZE_LIMITS;
          const snapped = Math.min(
            maxEdge,
            Math.max(minEdge, Math.round(entry[name] / step) * step),
          );
          if (snapped !== entry[name]) attrs.onPatch({ [name]: snapped });
          (event.target as HTMLInputElement).value = String(snapped);
        },
      });
    return m('.fm-image-size', [
      field(t('image', 'width'), 'width'),
      m('span.fm-image-size-times', { 'aria-hidden': 'true' }, '×'),
      field(t('image', 'height'), 'height'),
      // The named shapes stay one click away; picking one fills both fields.
      statusChoice(
        'size',
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
        true,
      ),
    ]);
  };

  /** How many of the less-used settings are set, so the folded row can say it is not empty. */
  const moreCount = (entry: ImagePaneEntry): number =>
    [
      entry.qualityId !== '',
      entry.steps !== undefined,
      entry.seed !== undefined,
      entry.lensId !== '',
      entry.apertureId !== '',
      entry.source !== undefined,
    ].filter(Boolean).length;

  /**
   * The status bar keeps what is changed every time - style, finish, size. Quality, steps, seed, lens,
   * aperture and the starting picture are set once and left, so they sit on a second row behind
   * "More" instead of crowding the first: nothing scrolls out of reach, and nothing is cut off.
   */
  const status = (attrs: ImagePaneAttrs) => {
    const { entry } = attrs;
    const pending = entry.pendingRequestId !== undefined;
    const photographic = selectionIsPhotographic(entry.presetIds);
    const count = moreCount(entry);
    return m('.fm-image-statusbar', [
      moreOpen
        ? m('.fm-image-status.fm-image-more', [
            statusChoice(
              'quality',
              t('image', 'quality'),
              entry.qualityId,
              [
                { id: '', label: t('image', 'qualityAny') },
                ...IMAGE_QUALITIES.map((quality) => ({
                  id: quality.id,
                  label: t('imageQuality', quality.labelKey),
                })),
              ],
              pending,
              (value) => attrs.onPatch({ qualityId: qualityIdOf(value) }),
            ),
            statusChoice(
              'steps',
              t('image', 'steps'),
              entry.steps === undefined ? '' : String(entry.steps),
              IMAGE_STEP_OPTIONS.map((steps) => ({
                id: String(steps),
                label: steps === '' ? t('image', 'stepsAny') : String(steps),
              })),
              pending,
              (value) => attrs.onPatch({ steps: value === '' ? undefined : Number(value) }),
            ),
            photographic
              ? statusChoice(
                  'lens',
                  t('image', 'lens'),
                  entry.lensId,
                  IMAGE_LENSES.map((lens) => ({
                    id: lens.id,
                    label: lens.id === '' ? t('image', 'lensNone') : lens.label,
                  })),
                  pending,
                  (value) => attrs.onPatch({ lensId: value }),
                )
              : undefined,
            photographic
              ? statusChoice(
                  'aperture',
                  t('image', 'aperture'),
                  entry.apertureId,
                  IMAGE_APERTURES.map((aperture) => ({
                    id: aperture.id,
                    label: aperture.id === '' ? t('image', 'apertureNone') : aperture.label,
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
                attrs.onPatch({
                  seed: text !== '' && Number.isFinite(parsed) ? parsed : undefined,
                });
              },
            }),
            entry.source === undefined
              ? tooltip(
                  t('image', 'sourceHint'),
                  m(
                    'button.btn-flat.fm-image-chrome.fm-image-source',
                    { type: 'button', disabled: pending, onclick: () => attrs.onUseSource() },
                    [photoIcon({ size: 14 }), t('image', 'source')],
                  ),
                )
              : m('.fm-image-source-set', [
                  tooltip(
                    entry.source.location.uri,
                    m(
                      'button.btn-flat.fm-image-chrome.fm-image-source',
                      { type: 'button', disabled: pending, onclick: () => attrs.onUseSource() },
                      sourceNameOf(entry.source) ?? t('image', 'source'),
                    ),
                  ),
                  iconButton(t('image', 'sourceClear'), trashIcon({ size: 14 }), () =>
                    attrs.onClearSource(),
                  ),
                ]),
          ])
        : undefined,
      m('footer.fm-image-status', [
        // Style is one choice: the ways of making a picture exclude each other, so a list of toggles
        // would only offer combinations that cannot be had. What can join any of them - taking the
        // colour out - is a second choice beside it.
        statusChoice(
          'style',
          t('image', 'style'),
          mediumOf(entry.presetIds)?.id ?? '',
          [
            { id: '', label: t('image', 'styleNone') },
            ...IMAGE_PRESETS.filter((preset) => preset.group === 'medium').map((preset) => ({
              id: preset.id,
              label: t('imagePreset', preset.id),
            })),
          ],
          pending,
          (value) => {
            const medium = presetById(value);
            const presetIds = chooseMedium(entry.presetIds, value);
            // A medium also brings the shape it suits, and the two size fields show it.
            attrs.onPatch(
              medium !== undefined && medium.group === 'medium'
                ? { presetIds, width: medium.width, height: medium.height }
                : { presetIds },
            );
          },
        ),
        statusChoice(
          'finish',
          t('image', 'finish'),
          treatmentOf(entry.presetIds)?.id ?? '',
          [
            { id: '', label: t('image', 'finishNone') },
            ...IMAGE_PRESETS.filter((preset) => preset.group === 'treatment').map((preset) => ({
              id: preset.id,
              label: t('imagePreset', preset.id),
            })),
          ],
          pending,
          (value) => attrs.onPatch({ presetIds: chooseTreatment(entry.presetIds, value) }),
        ),
        sizeFields(attrs, pending),
        m(
          'button.btn-flat.fm-image-chrome.fm-image-more-toggle',
          {
            type: 'button',
            'aria-expanded': String(moreOpen),
            onclick: () => {
              moreOpen = !moreOpen;
              openChoice = undefined;
            },
          },
          [
            adjustmentsHorizontalIcon({ size: 14 }),
            count === 0 ? t('image', 'more') : `${t('image', 'more')} (${count})`,
          ],
        ),
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
      ]),
    ]);
  };

  /** The folder the form would write to: a known one by its text, or a typed path beside it. */
  const chosenFolder = (attrs: ImagePaneAttrs): Location | undefined => {
    const text = folderText.trim();
    if (text === '') return undefined;
    const known = attrs.saveFolders?.find(
      (candidate) => folderText0(attrs, candidate.location) === text,
    );
    if (known !== undefined) return known.location;
    const base = attrs.saveFolder ?? attrs.saveFolders?.[0]?.location;
    // Only an absolute path or `~` names a folder; anything else is a typo, not a relative path.
    if (base === undefined || !/^(~|\/|[A-Za-z]:[\\/])/u.test(text)) return undefined;
    try {
      return (
        attrs.resolveFolder?.(base, text) ?? (text === folderText0(attrs, base) ? base : undefined)
      );
    } catch {
      return undefined;
    }
  };

  const saveForm = (attrs: ImagePaneAttrs, image: GeneratedImage) => {
    const folder = chosenFolder(attrs);
    const name = `${fileName}.${image.extension}`;
    const taken = takenIn(attrs, folder)?.has(name.toLowerCase()) === true;
    const valid = isValidImageFileName(name, image.extension) && folder !== undefined;
    const replacing = taken || confirmingReplace;
    return m(
      'form.fm-image-save',
      {
        onsubmit: (event: Event) => {
          event.preventDefault();
          if (!valid || folder === undefined) return;
          attrs.onSave({
            imageId: image.id,
            fileName: name,
            folder,
            overwrite: replacing,
          });
          closeSaveForm();
        },
      },
      [
        m('input.fm-image-folder.browser-default', {
          type: 'text',
          list: 'fm-image-save-folders',
          'aria-label': t('image', 'folder'),
          'aria-invalid': folder === undefined ? 'true' : undefined,
          value: folderText,
          placeholder: attrs.saveFolderName ?? '',
          spellcheck: false,
          oninput: (event: Event) => {
            folderText = (event.target as HTMLInputElement).value;
          },
          // A folder picked from the list gets a name that is free there, not the one that fit the last folder.
          onchange: () => {
            const picked = chosenFolder(attrs);
            if (picked !== undefined) suggestName(attrs, image, saveImageIndex, picked);
          },
        }),
        // The folders the person is already looking at are one click away; the field stays for the
        // folder that is none of them, so a path can still be typed.
        (attrs.saveFolders ?? []).length === 0
          ? undefined
          : m(
              '.fm-image-folder-choices',
              { role: 'group', 'aria-label': t('image', 'folderChoices') },
              (attrs.saveFolders ?? []).map((candidate) => {
                const text = folderText0(attrs, candidate.location);
                return tooltip(
                  t('image', 'saveToFolder', { name: candidate.name }),
                  m(
                    'button.btn-flat.fm-image-chrome.fm-image-folder-chip',
                    {
                      type: 'button',
                      'aria-pressed': String(text === folderText.trim()),
                      onclick: () => {
                        folderText = text;
                        // The free name the other folder offered may be taken here.
                        suggestName(attrs, image, saveImageIndex, candidate.location);
                      },
                    },
                    candidate.name,
                  ),
                );
              }),
            ),
        m(
          'datalist#fm-image-save-folders',
          (attrs.saveFolders ?? []).map((candidate) =>
            m('option', { value: folderText0(attrs, candidate.location) }, candidate.name),
          ),
        ),
        // The format is not a choice: the extension is shown after the field and cannot be edited.
        m('.fm-image-name-field', [
          m('input.fm-image-name.browser-default', {
            type: 'text',
            'aria-label': t('image', 'fileName'),
            value: fileName,
            spellcheck: false,
            oninput: (event: Event) => {
              fileName = stemOf((event.target as HTMLInputElement).value, image.extension);
            },
          }),
          m('span.fm-image-name-extension', `.${image.extension}`),
        ]),
        taken ? m('p.fm-image-save-note', { role: 'status' }, t('image', 'nameTaken')) : undefined,
        m('.fm-image-save-actions', [
          chromeButton(t('image', 'cancel'), () => closeSaveForm()),
          replacing
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
    return m(
      'section.fm-image-preview',
      {
        role: 'log',
        'aria-live': 'polite',
        class: fitPreview ? 'fm-image-preview-fit' : 'fm-image-preview-full',
      },
      [
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
                  entry.views.find((view) => view.imageId === entry.selectedImageId)
                    ?.savedLocation ?? '',
              }),
            ),
      ],
    );
  };

  const composer = (attrs: ImagePaneAttrs) => {
    const { entry } = attrs;
    const pending = entry.pendingRequestId !== undefined;
    const composed = composePrompt(entry.subject, entry.presetIds, entry.lensId, entry.apertureId);
    const negative = composeNegativePrompt(entry.negativePrompt, entry.presetIds);
    // Nothing to reveal until there is a prompt; the box falls back to editing when it empties.
    const revealing = showComposed && composed !== '';
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
        revealing
          ? // The other side of the box: the wording the presets and lens added, as the model gets it.
            m('.fm-image-revealed', { 'aria-live': 'polite' }, [
              m('p.fm-image-revealed-text', composed),
              negative === ''
                ? undefined
                : m('p.fm-image-revealed-text', [
                    m('span.fm-image-composed-label', `${t('image', 'negative')} `),
                    negative,
                  ]),
            ])
          : m('textarea.fm-image-input', {
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
        // What to leave out travels with the prompt it belongs to, in the same box - named, because
        // an unlabelled example list reads like a feature of its own. The send and stop keys end
        // this row, so the prompt above can use the whole width.
        m('.fm-image-avoid-row', [
          revealing
            ? m('span.fm-image-avoid-label.fm-image-reveal-note', t('image', 'sent'))
            : [
                m('span.fm-image-avoid-label', t('image', 'negative')),
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
              ],
          m('.fm-image-composer-actions', [
            iconButton(
              revealing ? t('image', 'editPrompt') : t('image', 'revealPrompt'),
              revealing ? eyeOffIcon({ size: 14 }) : eyeIcon({ size: 14 }),
              () => {
                showComposed = !revealing;
              },
              composed === '',
            ),
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
                    // A filled square: the open one reads as a checkbox.
                    playerStopIcon({ size: 14, className: 'fm-icon-filled' }),
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
                    cornerDownLeftIcon({ size: 14 }),
                  ),
                ),
          ]),
        ]),
      ],
    );
  };

  return {
    view: ({ attrs }) => {
      const { entry } = attrs;
      return m('section.fm-image', [
        m('header.fm-image-header', [
          m('span.fm-image-title', t('image', 'title')),
          m(
            'button.btn-flat.fm-image-chrome.fm-image-toggle',
            {
              type: 'button',
              'aria-expanded': String(promptsOpen),
              'aria-controls': promptsOpen ? 'fm-image-prompts' : undefined,
              onclick: () => {
                promptsOpen = !promptsOpen;
                confirmingDelete = undefined;
              },
            },
            [
              m(
                'span.fm-image-chevron',
                { 'aria-hidden': 'true' },
                promptsOpen ? chevronDownIcon({ size: 12 }) : chevronRightIcon({ size: 12 }),
              ),
              t('image', 'prompts'),
            ],
          ),
          // While the list is open it is the thing being looked at; the generations step aside.
          promptsOpen ? m('.fm-image-crumbs') : crumbs(attrs),
          // Fit or full width: a picture taller than the pane either shrinks to it or scrolls.
          iconButton(
            fitPreview ? t('image', 'showFullWidth') : t('image', 'fitToPane'),
            fitPreview ? arrowsMaximizeIcon({ size: 16 }) : arrowsMinimizeIcon({ size: 16 }),
            () => {
              fitPreview = !fitPreview;
            },
          ),
          iconButton(
            t('image', 'newPrompt'),
            photoIcon({ size: 16 }),
            () => attrs.onNewPrompt(),
            entry.pendingRequestId !== undefined,
          ),
          iconButton(t('image', 'openSettings'), settingsIcon({ size: 16 }), () =>
            attrs.onOpenSettings?.(),
          ),
          attrs.onNewTab === undefined
            ? undefined
            : iconButton(t('pane', 'newTab'), plusIcon({ size: 16 }), () => attrs.onNewTab?.()),
        ]),
        m('.fm-image-body', [
          promptsOpen ? promptList(attrs) : undefined,
          preview(attrs),
          composer(attrs),
        ]),
        status(attrs),
      ]);
    },
  };
};
