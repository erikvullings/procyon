import m, { type FactoryComponent } from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { setLocale } from '../../i18n';
import type { ImageBatch, Location } from '../../models';
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

  it('drops the word Image from the header', () => {
    mountLoaded();
    expect(root.querySelector('.fm-image-title'), 'no title in the header').toBeNull();
  });

  it('carries the four header actions after the crumbs, each named by a tooltip', () => {
    mountLoaded();
    const header = root.querySelector('.fm-image-header');
    const actions = [...(header?.querySelectorAll<HTMLButtonElement>('button') ?? [])];
    expect(actions.map((button) => button.getAttribute('aria-label'))).toEqual([
      'New image prompt',
      'New tab',
      'Favourites',
      'Open AI settings',
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

  it('shows the wording that will be sent once there are words', () => {
    mountLoaded({ subject: 'a lighthouse at dusk', presetIds: ['clipart'] });
    const line = root.querySelector('.fm-image-composed');
    expect(line?.textContent).toContain('a lighthouse at dusk');
    expect(line?.textContent).toContain('Sent to the model');
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

  it('holds the size, the seed and the way to start from a picture', () => {
    mountLoaded();
    expect(triggerFor('Size'), 'a size control').not.toBeNull();
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
    ).toEqual(['Style', 'Finish', 'Size', 'Quality', 'Steps']);
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
    expect(triggerFor('Quality')?.textContent).toContain('Any quality');
    clickOption('Quality', 'Super quality');
    expect(attrs.onPatch).toHaveBeenCalledWith({ qualityId: 'superQuality' });
  });

  it('can be told how many steps to take, and asked to leave that to the model', () => {
    const { attrs } = mountLoaded();
    expect(triggerFor('Steps')?.textContent).toContain('Any steps');
    clickOption('Steps', '30');
    expect(attrs.onPatch).toHaveBeenCalledWith({ steps: 30 });
  });

  it('shows the step count that is set', () => {
    mountLoaded({ steps: 50 });
    expect(triggerFor('Steps')?.textContent).toContain('50');
  });

  it('starts from the picture the other pane has selected, and can be talked out of it', () => {
    const without = mountLoaded();
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
    expect(root.textContent).toContain('cat.png');
    root.querySelector<HTMLButtonElement>('[aria-label="Stop starting from a picture"]')?.click();
    m.redraw.sync();
    expect(withSource.attrs.onClearSource, 'the start was dropped').toHaveBeenCalled();
  });

  it('draws its own choices, because Materialize hides a native select', () => {
    mountLoaded({ width: 1_216, height: 832 });
    expect(root.querySelector('select'), 'no native select to be hidden').toBeNull();

    const trigger = triggerFor('Size');
    expect(trigger?.textContent).toContain('1216 × 832');
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
    expect(root.querySelector('.fm-image-status [aria-label="Lens"]')).toBeNull();
    mountLoaded({ presetIds: ['portrait'] });
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
