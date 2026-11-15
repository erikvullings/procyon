import m, { type FactoryComponent } from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { setLocale } from '../../i18n';
import type { GeneratedImage, ImageBatch, Location } from '../../models';
import { ImagePane, type ImagePaneAttrs } from './image-pane';
import { emptyImagePane, type ImagePaneEntry } from './image-pane-state';

let root: HTMLElement;

const batch: ImageBatch = {
  generationId: 'g1',
  model: 'flux-schnell',
  prompt: 'a lighthouse at dusk',
  negativePrompt: null,
  seed: 42,
  width: 1_024,
  height: 1_024,
  images: [
    {
      id: 'i1',
      mediaType: 'image/png',
      width: 1_024,
      height: 1_024,
      bytes: 136,
      downscaled: false,
      extension: 'png',
    },
  ],
};

const folder: Location = { providerId: 'local', uri: 'file:///shots' };

/** What an endpoint that answers in JPEG sends back. */
const jpegImage: GeneratedImage = {
  id: 'i1',
  mediaType: 'image/jpeg',
  width: 1_024,
  height: 1_024,
  bytes: 90,
  downscaled: false,
  extension: 'jpg',
};

/** Mounts the pane with an entry and spies for everything it can ask the shell to do. */
function mountLoaded(
  overrides: Partial<ImagePaneEntry> = {},
  attrOverrides: Partial<ImagePaneAttrs> = {},
): { entry: ImagePaneEntry; attrs: ImagePaneAttrs } {
  const entry: ImagePaneEntry = { ...emptyImagePane('left:t1', 'left', 't1', 'w1'), ...overrides };
  const attrs: ImagePaneAttrs = {
    entry,
    saveFolder: folder,
    saveFolderName: 'shots',
    onPatch: vi.fn(),
    onGenerate: vi.fn(),
    onStop: vi.fn(),
    onSelect: vi.fn(),
    onSave: vi.fn(),
    onDiscard: vi.fn(),
    onRegenerate: vi.fn(),
    onUseSource: vi.fn(),
    onClearSource: vi.fn(),
    onSelectGeneration: vi.fn(),
    onNewPrompt: vi.fn(),
    onApplyPrompt: vi.fn(),
    onDeletePrompt: vi.fn(),
    onNewTab: vi.fn(),
    onOpenSettings: vi.fn(),
    ...attrOverrides,
  };
  const view: FactoryComponent<ImagePaneAttrs> = () => ({ view: () => m(ImagePane, attrs) });
  m.mount(root, { view: () => m(view, attrs) });
  m.redraw.sync();
  return { entry, attrs };
}

function buttons(root: ParentNode, selector = 'button'): HTMLButtonElement[] {
  return [...root.querySelectorAll<HTMLButtonElement>(selector)];
}

function labelled(root: ParentNode, label: string): HTMLButtonElement | undefined {
  return buttons(root).find((button) => button.getAttribute('aria-label') === label);
}

beforeEach(() => {
  setLocale('en');
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
});

describe('the image pane header', () => {
  it('is a crumb bar of the generations this pane asked for', () => {
    const views = [{ imageId: 'i1', loading: false, previewUrl: 'blob:1' }];
    const entry = mountLoaded({
      batch,
      views,
      activeGenerationId: 'g1',
      history: [
        {
          generationId: 'g0',
          batch: { ...batch, generationId: 'g0', prompt: 'a fox in a boat' },
          views: [],
        },
        { generationId: 'g1', batch, views },
      ],
    }).entry;
    expect(entry.history).toHaveLength(2);

    const bar = root.querySelector('.fm-image-crumbs');
    expect(bar, 'a crumb bar').not.toBeNull();
    const crumbs = [...(bar?.querySelectorAll<HTMLButtonElement>('button') ?? [])];
    expect(crumbs.map((crumb) => crumb.textContent?.trim())).toEqual([
      'a fox in a boat',
      'a lighthouse at dusk',
    ]);
    expect(crumbs[1]?.getAttribute('aria-current')).toBe('true');
  });

  it('asks the shell to show the generation a crumb stands for', () => {
    const { attrs } = mountLoaded({
      history: [{ generationId: 'g0', batch: { ...batch, generationId: 'g0' }, views: [] }],
    });
    const crumb = root.querySelector<HTMLButtonElement>('.fm-image-crumbs button');
    crumb?.click();
    m.redraw.sync();
    expect(attrs.onSelectGeneration).toHaveBeenCalledWith('g0');
  });

  it('reads Image > Prompts, as the chat pane reads Chat > Conversations', () => {
    mountLoaded();
    expect(root.querySelector('.fm-image-title')?.textContent).toBe('Image');
    expect(root.querySelector('.fm-image-toggle')?.textContent).toBe('Prompts');
  });

  it('carries the header actions after the crumbs: new prompt, settings, new tab', () => {
    mountLoaded();
    const header = root.querySelector('.fm-image-header');
    const actions = [
      ...(header?.querySelectorAll<HTMLButtonElement>('button:not(.fm-image-toggle)') ?? []),
    ];
    // The fit toggle leads; the favourites menu does not apply to this pane.
    expect(actions.map((button) => button.getAttribute('aria-label'))).toEqual([
      'Show at full width',
      'New image prompt',
      'Open AI settings',
      'New tab',
    ]);
    // Nothing but an icon: the label travels with the control, not as visible text.
    for (const action of actions) expect(action.textContent?.trim()).toBe('');
  });

  it('opens the AI settings and a fresh prompt from those icons', () => {
    const { attrs } = mountLoaded();
    labelled(root, 'Open AI settings')?.click();
    labelled(root, 'New image prompt')?.click();
    m.redraw.sync();
    expect(attrs.onOpenSettings).toHaveBeenCalledTimes(1);
    expect(attrs.onNewPrompt).toHaveBeenCalledTimes(1);
    // The pane asks the shell to open the settings; it does not reach into them itself.
    expect(attrs.onPatch).not.toHaveBeenCalled();
  });
});

describe('the prompt and the preview', () => {
  it('shows the pictures above the prompt, and the status bar below it', () => {
    mountLoaded({ batch, views: [{ imageId: 'i1', loading: false, previewUrl: 'blob:1' }] });
    const preview = root.querySelector('.fm-image-preview');
    const composer = root.querySelector('.fm-image-composer');
    const status = root.querySelector('.fm-image-status');
    expect(preview, 'the preview').not.toBeNull();
    expect(composer, 'the prompt').not.toBeNull();
    expect(status, 'the status bar').not.toBeNull();
    // `querySelectorAll` hands back elements in document order, which is the layout being checked.
    const inOrder = [
      ...root.querySelectorAll('.fm-image-preview, .fm-image-composer, .fm-image-status'),
    ];
    expect(inOrder.map((node) => node.className)).toEqual([
      expect.stringContaining('preview'),
      expect.stringContaining('composer'),
      expect.stringContaining('status'),
    ]);
  });

  it('shows the picture itself, with no box, title or placeholder text around it', () => {
    mountLoaded({ batch, views: [{ imageId: 'i1', loading: false, previewUrl: 'blob:1' }] });
    expect(root.querySelector('.fm-image-preview img')?.getAttribute('src')).toBe('blob:1');
    expect(root.querySelector('.fm-image-side-title'), 'no side titles').toBeNull();
    expect(root.textContent).not.toContain('Preview');
    expect(root.textContent).not.toContain('No preview for this image.');
  });

  it('says nothing when nothing has been generated', () => {
    mountLoaded();
    expect(root.querySelector('.fm-image-preview img')).toBeNull();
    expect(root.textContent).not.toContain('Preview');
  });

  it('sends the prompt on Enter and keeps Shift+Enter for a new line', () => {
    const { attrs } = mountLoaded({ subject: 'a lighthouse at dusk' });
    const field = root.querySelector<HTMLTextAreaElement>('.fm-image-input');
    expect(field?.value).toBe('a lighthouse at dusk');
    field?.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }),
    );
    m.redraw.sync();
    expect(attrs.onGenerate).toHaveBeenCalledTimes(1);

    field?.dispatchEvent(
      new KeyboardEvent('keydown', {
        key: 'Enter',
        shiftKey: true,
        bubbles: true,
        cancelable: true,
      }),
    );
    m.redraw.sync();
    expect(attrs.onGenerate).toHaveBeenCalledTimes(1);
  });

  it('sends through the same button Enter does, and offers Stop while a generation runs', () => {
    const idle = mountLoaded({ subject: 'a lighthouse at dusk' });
    const send = labelled(root, 'Generate');
    send?.click();
    m.redraw.sync();
    expect(idle.attrs.onGenerate).toHaveBeenCalledTimes(1);

    mountLoaded({ subject: 'a fox', pendingRequestId: 'r1' });
    expect(labelled(root, 'Stop'), 'a stop control while pending').toBeDefined();
    labelled(root, 'Stop')?.click();
    m.redraw.sync();
  });

  it('has no send control to press before anything is typed', () => {
    mountLoaded();
    expect(root.querySelector<HTMLButtonElement>('.fm-image-send')?.disabled).toBe(true);
  });

  it('says nothing under an empty prompt', () => {
    mountLoaded();
    expect(root.querySelector('.fm-image-composed')).toBeNull();
    expect(root.textContent, 'a missing subject is shown by the disabled send key').not.toContain(
      'Describe what you want to see first',
    );
  });

  it('keeps the wording sent to the model on the back of the box, not beside it', () => {
    mountLoaded({ subject: 'a lighthouse at dusk', presetIds: ['clipart'] });
    expect(root.querySelector('.fm-image-composed'), 'no repeated line under the box').toBeNull();
    expect(root.querySelector('.fm-image-revealed')).toBeNull();

    labelled(root, 'Show the full prompt sent to the model')?.click();
    m.redraw.sync();

    expect(root.querySelector('.fm-image-revealed')?.textContent).toContain('a lighthouse at dusk');
    expect(root.querySelector('.fm-image-revealed')?.textContent).toContain('clipart');
    expect(root.querySelector('textarea'), 'the editable prompt is flipped away').toBeNull();
    expect(root.querySelector('.fm-image-avoid-row')?.textContent).toContain('Sent to the model');

    labelled(root, 'Back to editing')?.click();
    m.redraw.sync();
    expect(root.querySelector('textarea')).not.toBeNull();
  });

  it('cannot flip an empty prompt', () => {
    mountLoaded();
    expect(labelled(root, 'Show the full prompt sent to the model')?.disabled).toBe(true);
  });

  it('puts the send key at the end of the leave-out row, below the prompt', () => {
    mountLoaded({ subject: 'a fox' });
    const row = root.querySelector('.fm-image-avoid-row');
    expect(row?.querySelector('.fm-image-send'), 'send ends the leave-out row').not.toBeNull();
    expect(row?.lastElementChild?.classList.contains('fm-image-composer-actions')).toBe(true);
    expect(
      root.querySelector('textarea')?.parentElement?.querySelector('.fm-image-send'),
      'nothing sits beside the prompt',
    ).toBe(row?.querySelector('.fm-image-send'));
  });

  it('draws Stop as a filled square while a generation runs', () => {
    mountLoaded({ subject: 'a fox', pendingRequestId: 'r1' });
    const stop = root.querySelector('.fm-image-stop');
    expect(root.querySelector('.fm-image-avoid-row')?.contains(stop ?? null)).toBe(true);
    expect(stop?.querySelector('.fm-icon-filled'), 'a filled glyph').not.toBeNull();
  });

  it('names the leave-out field, so its example is not read as a feature', () => {
    mountLoaded();
    const row = root.querySelector('.fm-image-avoid-row');
    expect(row?.textContent).toContain('Leave out');
    expect(row?.querySelector('input')?.getAttribute('placeholder')).toContain('e.g.');
  });

  it('sends with the Enter glyph, as the chat composer does', () => {
    mountLoaded({ subject: 'a lighthouse at dusk' });
    const send = root.querySelector('.fm-image-send');
    expect(send?.querySelector('.fm-icon-corner-down-left'), 'the Enter key glyph').not.toBeNull();
    expect(send?.querySelector('svg path[d="M5 12l14 0"]'), 'not a plus').toBeNull();
  });

  it('keeps the leave-out list with the prompt it belongs to', () => {
    const { attrs } = mountLoaded({ subject: 'a fox', negativePrompt: 'text' });
    const avoid = root.querySelector<HTMLInputElement>('.fm-image-avoid');
    expect(
      root.querySelector('.fm-image-composer .fm-image-avoid'),
      'inside the composer',
    ).not.toBeNull();
    avoid?.dispatchEvent(new Event('input', { bubbles: true }));
    m.redraw.sync();
    expect(attrs.onPatch).toHaveBeenCalledWith({ negativePrompt: expect.any(String) });
  });
});

describe('the status bar', () => {
  /** The drawn choices, found by the group they are named for. */
  function triggerFor(label: string): HTMLButtonElement | null {
    return root.querySelector<HTMLButtonElement>(`.fm-image-choice-trigger[aria-label="${label}"]`);
  }

  /** Opens one drawn choice and returns its option rows. */
  function openChoice(label: string): HTMLElement[] {
    triggerFor(label)?.click();
    m.redraw.sync();
    return [...root.querySelectorAll<HTMLElement>('.fm-image-choice-option')];
  }

  function clickOption(label: string, text: string): void {
    openChoice(label)
      .find((option) => option.textContent?.includes(text))
      ?.click();
    m.redraw.sync();
  }

  /** The less-used settings sit behind More; this unfolds them. */
  function openMore(): void {
    root.querySelector<HTMLButtonElement>('.fm-image-more-toggle')?.click();
    m.redraw.sync();
  }

  it('holds the size, the seed and the way to start from a picture', () => {
    mountLoaded();
    expect(root.querySelector('[aria-label="Width"]'), 'a width field').not.toBeNull();
    expect(root.querySelector('[aria-label="Height"]'), 'a height field').not.toBeNull();
    openMore();
    expect(
      root.querySelector('.fm-image-status [aria-label="Seed"]'),
      'a seed field',
    ).not.toBeNull();
    expect(
      root.querySelector('.fm-image-source'),
      'a button to take the other pane’s picture',
    ).not.toBeNull();
    expect(root.textContent, 'the count is fixed at one image').not.toContain('Images');
  });

  it('asks about style and finish separately, because they do not exclude each other', () => {
    mountLoaded();
    expect(
      [...root.querySelectorAll<HTMLButtonElement>('.fm-image-choice-trigger')].map((trigger) =>
        trigger.getAttribute('aria-label'),
      ),
    ).toEqual(['Style', 'Finish', 'Size']);
    openMore();
    expect(
      [...root.querySelectorAll<HTMLButtonElement>('.fm-image-more .fm-image-choice-trigger')].map(
        (trigger) => trigger.getAttribute('aria-label'),
      ),
    ).toEqual(['Quality', 'Steps']);
    expect(triggerFor('Style')?.textContent).toContain('No style');
    expect(triggerFor('Finish')?.textContent).toContain('No finish');
  });

  it('names the style that is on rather than describing it', () => {
    mountLoaded({ presetIds: ['clipart', 'blackAndWhite'] });
    expect(triggerFor('Style')?.textContent).toContain('Clipart');
    expect(triggerFor('Finish')?.textContent).toContain('Black and white');
    expect(
      root.querySelector('.fm-image-preset-trigger'),
      'no list of tick boxes to open',
    ).toBeNull();
  });

  it('lists one style per row, with its label only', () => {
    mountLoaded({ presetIds: ['clipart'] });
    const options = openChoice('Style');
    const labels = options.map((option) => option.textContent?.trim());
    expect(labels).toContain('No style');
    expect(labels).toContain('Clipart');
    expect(labels).toContain('Isometric');
    // A treatment is asked about in its own choice, so it is not offered here.
    expect(labels).not.toContain('Black and white');
    // A Material icon name is only ever drawn as an icon, never spelled out beside the label.
    expect(labels.join(' ')).not.toMatch(/vector|camera|brush|movie/u);
    expect(
      options
        .find((option) => option.textContent?.includes('Clipart'))
        ?.getAttribute('aria-selected'),
    ).toBe('true');
  });

  it('replaces the medium that was on, and brings the shape that suits the new one', () => {
    const { attrs } = mountLoaded({ presetIds: ['clipart', 'blackAndWhite'] });
    clickOption('Style', 'Cinematic');
    expect(attrs.onPatch).toHaveBeenCalledWith(
      expect.objectContaining({
        // Kept in the order the presets are declared, not the order they were picked in.
        presetIds: ['blackAndWhite', 'cinematic'],
        width: 1_344,
        height: 768,
      }),
    );
  });

  it('keeps a finish beside the medium it was asked for', () => {
    const { attrs } = mountLoaded({ presetIds: ['portrait'] });
    clickOption('Finish', 'Black and white');
    expect(attrs.onPatch).toHaveBeenCalledWith(
      expect.objectContaining({ presetIds: ['blackAndWhite', 'portrait'] }),
    );
  });

  it('can be asked for neither style nor finish', () => {
    const { attrs } = mountLoaded({ presetIds: ['clipart'] });
    clickOption('Style', 'No style');
    expect(attrs.onPatch).toHaveBeenCalledWith(expect.objectContaining({ presetIds: [] }));
  });

  it('asks how hard the model should work, in words a person has', () => {
    const { attrs } = mountLoaded();
    openMore();
    expect(triggerFor('Quality')?.textContent).toContain('Any quality');
    clickOption('Quality', 'Super quality');
    expect(attrs.onPatch).toHaveBeenCalledWith({ qualityId: 'superQuality' });
  });

  it('can be told how many steps to take, and asked to leave that to the model', () => {
    const { attrs } = mountLoaded();
    openMore();
    expect(triggerFor('Steps')?.textContent).toContain('Any steps');
    clickOption('Steps', '30');
    expect(attrs.onPatch).toHaveBeenCalledWith({ steps: 30 });
  });

  it('shows the step count that is set', () => {
    mountLoaded({ steps: 50 });
    openMore();
    expect(triggerFor('Steps')?.textContent).toContain('50');
  });

  it('starts from the picture the other pane has selected, and can be talked out of it', () => {
    const without = mountLoaded();
    openMore();
    root.querySelector<HTMLButtonElement>('.fm-image-source')?.click();
    m.redraw.sync();
    expect(
      without.attrs.onUseSource,
      'the shell was asked for the other pane’s picture',
    ).toHaveBeenCalled();

    // Once one is picked, the button says which picture the prompt starts from, and can be undone.
    const withSource = mountLoaded({
      source: { location: { providerId: 'local', uri: 'file:///shots/cat.png' } },
    });
    openMore();
    expect(root.textContent).toContain('cat.png');
    root.querySelector<HTMLButtonElement>('[aria-label="Stop starting from a picture"]')?.click();
    m.redraw.sync();
    expect(withSource.attrs.onClearSource, 'the start was dropped').toHaveBeenCalled();
  });

  it('draws its own choices, because Materialize hides a native select', () => {
    mountLoaded({ width: 1_216, height: 832 });
    expect(root.querySelector('select'), 'no native select to be hidden').toBeNull();

    // The shapes are a chevron beside the two fields, which show the numbers.
    const trigger = triggerFor('Size');
    expect(root.querySelector<HTMLInputElement>('[aria-label="Width"]')?.value).toBe('1216');
    expect(root.querySelector<HTMLInputElement>('[aria-label="Height"]')?.value).toBe('832');
    expect(trigger?.querySelector('.fm-icon-chevron-down'), 'a caret to open it').not.toBeNull();

    trigger?.click();
    m.redraw.sync();
    const list = root.querySelector('.fm-image-choice-list');
    expect(list, 'the list of sizes').not.toBeNull();
    expect(list?.getAttribute('role')).toBe('listbox');
    expect(root.querySelectorAll('[role="option"]')).toHaveLength(5);
  });

  it('patches the shape the size choice picks, and closes the list', () => {
    const { attrs } = mountLoaded();
    triggerFor('Size')?.click();
    m.redraw.sync();
    const option = [...root.querySelectorAll<HTMLElement>('.fm-image-choice-option')].find((row) =>
      row.textContent?.includes('832 × 1216'),
    );
    option?.click();
    m.redraw.sync();
    expect(attrs.onPatch).toHaveBeenCalledWith({ width: 832, height: 1_216 });
    expect(root.querySelector('.fm-image-choice-list'), 'the list folds away').toBeNull();
  });

  it('opens only one choice at a time', () => {
    mountLoaded();
    openMore();
    triggerFor('Style')?.click();
    m.redraw.sync();
    expect(root.querySelectorAll('.fm-image-choice-list')).toHaveLength(1);
    triggerFor('Quality')?.click();
    m.redraw.sync();
    const open = [...root.querySelectorAll<HTMLElement>('.fm-image-choice-list')];
    expect(open).toHaveLength(1);
    expect(open[0]?.getAttribute('aria-label'), 'the quality list, not the style one').toBe(
      'Quality',
    );
  });

  it('offers lens wording only when a photographic preset is on', () => {
    mountLoaded({ presetIds: ['clipart'] });
    openMore();
    expect(root.querySelector('.fm-image-status [aria-label="Lens"]')).toBeNull();
    mountLoaded({ presetIds: ['portrait'] });
    openMore();
    expect(
      root.querySelector('.fm-image-status [aria-label="Lens"]'),
      'a lens control',
    ).not.toBeNull();
  });
});

describe('the picture and its actions', () => {
  it('asks for a name before anything is written', () => {
    const { attrs } = mountLoaded({
      batch,
      views: [{ imageId: 'i1', loading: false, previewUrl: 'blob:1' }],
    });
    labelled(root, 'Save')?.click();
    m.redraw.sync();
    expect(root.querySelector('.fm-image-save'), 'a save form').not.toBeNull();
    // Opening the form is not a save: nothing has reached the shell yet.
    expect(attrs.onSave).not.toHaveBeenCalled();
  });

  it('regenerates from the prompt the picture on screen came from', () => {
    const { attrs } = mountLoaded({
      batch,
      views: [{ imageId: 'i1', loading: false, previewUrl: 'blob:1' }],
    });
    labelled(root, 'Regenerate with this prompt')?.click();
    m.redraw.sync();
    expect(attrs.onRegenerate).toHaveBeenCalledTimes(1);
  });

  it('discards from the status bar rather than beside the picture', () => {
    const { attrs } = mountLoaded({
      batch,
      views: [{ imageId: 'i1', loading: false, previewUrl: 'blob:1' }],
    });
    const discard = labelled(root, 'Discard');
    expect(
      root.querySelector('.fm-image-status')?.contains(discard ?? null),
      'it sits in the status bar',
    ).toBe(true);
    discard?.click();
    m.redraw.sync();
    expect(attrs.onDiscard).toHaveBeenCalledTimes(1);
  });

  it('points a role or consent failure at the settings it needs', () => {
    const { attrs } = mountLoaded({
      subject: 'a fox',
      failure: 'roleNotAssigned',
      error: 'No image model is set. Choose one in Settings.',
    });
    expect(root.querySelector('.fm-image-error')?.textContent).toContain('No image model is set');
    const toSettings = buttons(root).find((button) =>
      button.textContent?.includes('Open AI settings'),
    );
    toSettings?.click();
    m.redraw.sync();
    expect(attrs.onOpenSettings).toHaveBeenCalledTimes(1);
  });
});

describe('the remembered prompts', () => {
  const prompt = {
    id: 'p1',
    title: 'A lighthouse at dusk',
    updatedAt: '2026-11-14T10:00:00.000Z',
    subject: 'A lighthouse at dusk, watercolour',
    negativePrompt: 'text',
    presetIds: ['watercolour'],
    lensId: '',
    apertureId: '',
    width: 1_024,
    height: 768,
    count: 2,
    seed: 7,
    quality: 'good' as const,
    steps: 30,
  };

  const toggle = () => root.querySelector<HTMLButtonElement>('.fm-image-toggle');

  it('stays folded until the Prompts toggle is pressed', () => {
    mountLoaded({ prompts: [prompt] });
    expect(root.querySelector('.fm-image-prompts')).toBeNull();
    expect(toggle()?.getAttribute('aria-expanded')).toBe('false');

    toggle()?.click();
    m.redraw.sync();

    expect(toggle()?.getAttribute('aria-expanded')).toBe('true');
    const titles = [...root.querySelectorAll('.fm-image-prompt-open')].map(
      (item) => item.textContent,
    );
    expect(titles).toEqual(['A lighthouse at dusk']);
    // The whole prompt is one hover away, since the title is only its first clause.
    expect(root.querySelector('.fm-image-prompt-open')?.getAttribute('title')).toBe(prompt.subject);
  });

  it('puts the chosen prompt back in the form and folds the list', () => {
    const { attrs } = mountLoaded({ prompts: [prompt] });
    toggle()?.click();
    m.redraw.sync();
    root.querySelector<HTMLButtonElement>('.fm-image-prompt-open')?.click();
    m.redraw.sync();

    expect(attrs.onApplyPrompt).toHaveBeenCalledWith(prompt);
    expect(root.querySelector('.fm-image-prompts')).toBeNull();
  });

  it('forgets a prompt only on the second click', () => {
    const { attrs } = mountLoaded({ prompts: [prompt] });
    toggle()?.click();
    m.redraw.sync();

    labelled(root, 'Forget this prompt')?.click();
    m.redraw.sync();
    expect(attrs.onDeletePrompt).not.toHaveBeenCalled();

    labelled(root, 'Click again to forget this prompt')?.click();
    m.redraw.sync();
    expect(attrs.onDeletePrompt).toHaveBeenCalledWith('p1');
  });

  it('says what the list is for when it is empty', () => {
    mountLoaded();
    toggle()?.click();
    m.redraw.sync();
    expect(root.querySelector('.fm-image-prompts-empty')?.textContent).toContain('No prompts yet');
  });
});

describe('the header, the preview mode and the size fields', () => {
  it('toggles the picture between fit and full width', () => {
    mountLoaded({ batch, views: [{ imageId: 'i1', loading: false, previewUrl: 'blob:1' }] });
    expect(
      root.querySelector('.fm-image-preview')?.classList.contains('fm-image-preview-fit'),
    ).toBe(true);
    labelled(root, 'Show at full width')?.click();
    m.redraw.sync();
    expect(
      root.querySelector('.fm-image-preview')?.classList.contains('fm-image-preview-full'),
    ).toBe(true);
    labelled(root, 'Fit to the pane')?.click();
    m.redraw.sync();
    expect(
      root.querySelector('.fm-image-preview')?.classList.contains('fm-image-preview-fit'),
    ).toBe(true);
  });

  it('sets the width and the height separately', () => {
    const { attrs } = mountLoaded();
    const width = root.querySelector<HTMLInputElement>('[aria-label="Width"]');
    const height = root.querySelector<HTMLInputElement>('[aria-label="Height"]');
    if (width === null || height === null) throw new Error('size fields missing');
    width.value = '1000';
    width.dispatchEvent(new Event('input', { bubbles: true }));
    height.value = '600';
    height.dispatchEvent(new Event('input', { bubbles: true }));
    expect(attrs.onPatch).toHaveBeenCalledWith({ width: 1_000 });
    expect(attrs.onPatch).toHaveBeenCalledWith({ height: 600 });
  });

  it('snaps a near miss to a multiple of 8 on leaving the field', () => {
    const { attrs } = mountLoaded({ width: 1_001 });
    const width = root.querySelector<HTMLInputElement>('[aria-label="Width"]');
    width?.dispatchEvent(new Event('change', { bubbles: true }));
    expect(attrs.onPatch).toHaveBeenCalledWith({ width: 1_000 });
  });

  it('marks a size the backend would refuse', () => {
    mountLoaded({ width: 2_048, height: 2_048 });
    expect(root.querySelector('[aria-label="Width"]')?.getAttribute('aria-invalid')).toBe('true');
  });

  it('shows the seed field as just "seed", smaller than before', () => {
    mountLoaded();
    root.querySelector<HTMLButtonElement>('.fm-image-more-toggle')?.click();
    m.redraw.sync();
    expect(root.querySelector('.fm-image-seed')?.getAttribute('placeholder')).toBe('seed');
  });

  it('counts what is set behind More', () => {
    mountLoaded({ seed: 3, steps: 20 });
    expect(root.querySelector('.fm-image-more-toggle')?.textContent).toBe('More (2)');
  });

  it('names its generations briefly and numbers a repeated name', () => {
    mountLoaded({
      history: [
        { generationId: 'a', subject: 'A beautiful woman in the park', batch, views: [] },
        { generationId: 'b', subject: 'A beautiful woman in a cafe', batch, views: [] },
      ],
    });
    const labels = [...root.querySelectorAll('.fm-image-crumb')].map((crumb) => crumb.textContent);
    expect(labels).toEqual(['A beautiful woman', 'A beautiful woman (2)']);
  });
});

describe('saving a picture', () => {
  const shown = {
    batch,
    views: [{ imageId: 'i1', loading: false, previewUrl: 'blob:1' }],
  };
  const files: Location = { providerId: 'local', uri: 'file:///Users/erik/Downloads' };
  const pictures: Location = { providerId: 'local', uri: 'file:///Users/erik/Pictures' };
  const attrsFor = (taken: Record<string, string[]> = {}): Partial<ImagePaneAttrs> => ({
    saveFolder: files,
    homeDirectory: '/Users/erik',
    saveFolders: [
      { location: files, name: 'Downloads' },
      { location: pictures, name: 'Pictures' },
    ],
    fileNamesIn: (folder) => {
      const names = taken[folder.uri];
      return names === undefined ? undefined : new Set(names);
    },
    resolveFolder: (current, path) => ({
      ...current,
      uri: `file://${path.replace(/^~/u, '/Users/erik')}`,
    }),
  });
  const openForm = () => {
    labelled(root, 'Save')?.click();
    m.redraw.sync();
  };
  /** Opens the way to another folder; until then the form only states where the picture goes. */
  const openChoice = () => {
    [...root.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent?.trim() === 'Change folder')
      ?.click();
    m.redraw.sync();
  };

  it('states where the picture will go, and asks nothing else', () => {
    mountLoaded(shown, attrsFor());
    openForm();

    expect(root.querySelector('.fm-image-save-folder-path')?.textContent).toBe('~/Downloads');
    expect(root.querySelector('input.fm-image-folder'), 'no folder to fill in').toBeNull();
    expect(root.querySelector('.fm-image-name-extension')?.textContent).toBe('.png');
    expect(root.querySelector<HTMLInputElement>('.fm-image-name')?.value).not.toContain('.png');
    expect(root.querySelector('.fm-image-save-hint'), 'no format lecture').toBeNull();
    expect(root.querySelector('.fm-image-save')?.textContent).not.toContain('has to end in');
  });

  it('shows a plain folder path and a name whose format cannot be edited', () => {
    mountLoaded(shown, attrsFor());
    openForm();
    openChoice();
    expect(root.querySelector<HTMLInputElement>('.fm-image-folder')?.value).toBe('~/Downloads');
    expect(root.querySelector('.fm-image-name-extension')?.textContent).toBe('.png');
    expect(root.querySelector<HTMLInputElement>('.fm-image-name')?.value).not.toContain('.png');
    expect(root.querySelector('.fm-image-save-hint'), 'no format lecture').toBeNull();
    expect(root.querySelector('.fm-image-save')?.textContent).not.toContain('has to end in');
  });

  it('keeps the format the model returned', () => {
    const drawn = { ...batch, images: [jpegImage] };
    mountLoaded(
      { ...shown, batch: drawn, views: [{ imageId: 'i1', loading: false, previewUrl: 'blob:1' }] },
      attrsFor(),
    );
    openForm();

    // The name is offered with what the model actually sent, not with a format invented for it.
    expect(root.querySelector('.fm-image-name-extension')?.textContent).toBe('.jpg');
    expect(root.querySelector('.fm-image-save')?.textContent).not.toContain('.png');
  });

  it('offers the folders it knows, so none has to be typed', () => {
    mountLoaded(shown, attrsFor());
    openForm();
    openChoice();
    const options = [...root.querySelectorAll('datalist option')].map((option) =>
      option.getAttribute('value'),
    );
    expect(options).toEqual(['~/Downloads', '~/Pictures']);
  });

  it('picks a known folder with one click, and fits the name to the folder chosen', () => {
    const { attrs } = mountLoaded(
      shown,
      attrsFor({ 'file:///Users/erik/Pictures': ['a-lighthouse-at-dusk-1024x1024-1.png'] }),
    );
    openForm();
    openChoice();
    const chips = [...root.querySelectorAll<HTMLButtonElement>('.fm-image-folder-chip')];
    // A dual-pane manager already knows the two folders on screen; typing a path is the way out.
    expect(chips.map((chip) => chip.textContent)).toEqual(['Downloads', 'Pictures']);
    expect(chips[0]?.getAttribute('aria-pressed')).toBe('true');

    chips[1]?.click();
    m.redraw.sync();
    expect(root.querySelector<HTMLInputElement>('.fm-image-folder')?.value).toBe('~/Pictures');
    expect(chips[0]?.getAttribute('aria-pressed')).not.toBe('true');
    // The suggestion is made again for the folder now chosen, so it is free there.
    expect(root.querySelector<HTMLInputElement>('.fm-image-name')?.value).toBe(
      'a-lighthouse-at-dusk-1024x1024-2',
    );

    root
      .querySelector<HTMLFormElement>('.fm-image-save')
      ?.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    expect(attrs.onSave).toHaveBeenCalledWith({
      imageId: 'i1',
      fileName: 'a-lighthouse-at-dusk-1024x1024-2.png',
      folder: pictures,
      overwrite: false,
    });
  });

  it('leaves a typed name alone when the folder already chosen is clicked again', () => {
    mountLoaded(shown, attrsFor());
    openForm();
    openChoice();
    const name = root.querySelector<HTMLInputElement>('.fm-image-name');
    if (name === null) throw new Error('name field missing');
    name.value = 'my own name';
    name.dispatchEvent(new Event('input', { bubbles: true }));
    m.redraw.sync();

    // Downloads is the folder already chosen, so this click is a no-op rather than a fresh guess.
    root.querySelectorAll<HTMLButtonElement>('.fm-image-folder-chip')[0]?.click();
    m.redraw.sync();
    expect(root.querySelector<HTMLInputElement>('.fm-image-name')?.value).toBe('my own name');
  });

  it('suggests a name that is not already in the folder', () => {
    mountLoaded(
      shown,
      attrsFor({
        'file:///Users/erik/Downloads': [
          'a-lighthouse-at-dusk-1024x1024-1.png',
          'a-lighthouse-at-dusk-1024x1024-2.png',
        ],
      }),
    );
    openForm();
    expect(root.querySelector<HTMLInputElement>('.fm-image-name')?.value).toBe(
      'a-lighthouse-at-dusk-1024x1024-3',
    );
    expect(root.querySelector('.fm-image-save-note')).toBeNull();
  });

  it('names the file after what was typed for this generation, not what the presets added', () => {
    mountLoaded(
      {
        ...shown,
        activeGenerationId: 'g2',
        history: [
          { generationId: 'g1', subject: 'a fox', batch, views: [] },
          { generationId: 'g2', subject: 'a fox in a boat at dusk', batch, views: shown.views },
        ],
      },
      attrsFor(),
    );
    openForm();
    // The endpoint echoes the composed prompt, which runs on past 40 characters of preset wording.
    expect(root.querySelector<HTMLInputElement>('.fm-image-name')?.value).toBe(
      'a-fox-in-a-boat-at-dusk-1024x1024-1',
    );
  });

  it('saves with the fixed extension and the folder as a location', () => {
    const { attrs } = mountLoaded(shown, attrsFor());
    openForm();
    root
      .querySelector<HTMLFormElement>('.fm-image-save')
      ?.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    expect(attrs.onSave).toHaveBeenCalledWith({
      imageId: 'i1',
      fileName: 'a-lighthouse-at-dusk-1024x1024-1.png',
      folder: files,
      overwrite: false,
    });
  });

  it('says so, and offers Replace, when an edited name is taken', () => {
    const { attrs } = mountLoaded(
      shown,
      attrsFor({ 'file:///Users/erik/Downloads': ['holiday.png'] }),
    );
    openForm();
    const name = root.querySelector<HTMLInputElement>('.fm-image-name');
    if (name === null) throw new Error('name field missing');
    name.value = 'Holiday';
    name.dispatchEvent(new Event('input', { bubbles: true }));
    m.redraw.sync();

    expect(root.querySelector('.fm-image-save-note')?.textContent).toContain('already there');
    const replace = buttons(root).find((button) => button.textContent === 'Replace and save');
    expect(replace, 'a Replace button').toBeDefined();
    root
      .querySelector<HTMLFormElement>('.fm-image-save')
      ?.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    expect(attrs.onSave).toHaveBeenCalledWith(
      expect.objectContaining({ fileName: 'Holiday.png', overwrite: true }),
    );
  });

  it('suggests a free name again after another folder is chosen', () => {
    mountLoaded(
      shown,
      attrsFor({
        'file:///Users/erik/Pictures': ['a-lighthouse-at-dusk-1024x1024-1.png'],
      }),
    );
    openForm();
    openChoice();
    const folder = root.querySelector<HTMLInputElement>('.fm-image-folder');
    if (folder === null) throw new Error('folder field missing');
    folder.value = '~/Pictures';
    folder.dispatchEvent(new Event('input', { bubbles: true }));
    folder.dispatchEvent(new Event('change', { bubbles: true }));
    m.redraw.sync();
    expect(root.querySelector<HTMLInputElement>('.fm-image-name')?.value).toBe(
      'a-lighthouse-at-dusk-1024x1024-2',
    );
  });
});
