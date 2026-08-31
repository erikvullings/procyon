import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { MultiRenameDialog } from './multi-rename-dialog';
import { EMPTY_MULTI_RENAME_RULES } from './multi-rename-rules';

let root: HTMLElement;

beforeEach(() => {
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
});

const entries = [
  { id: '1', name: 'alpha.txt' },
  { id: '2', name: 'beta.txt' },
];
const numberedPreset = {
  name: 'Numbered',
  rules: {
    ...EMPTY_MULTI_RENAME_RULES,
    search: 'alpha',
    replace: 'gamma',
    nameMask: 'file-[C]-[N]',
    sequence: { start: 7, step: 2, padding: 2 },
  },
};

function findButton(text: string): HTMLButtonElement {
  const button = [...document.querySelectorAll('button')].find(
    (candidate) => candidate.textContent?.trim() === text,
  );
  if (button === undefined) throw new Error(`button "${text}" missing`);
  return button;
}

function findInput(id: string): HTMLInputElement {
  const input = document.getElementById(id);
  if (!(input instanceof HTMLInputElement)) throw new Error(`input "#${id}" missing`);
  return input;
}

function previewNames(): string[] {
  return [...document.querySelectorAll('.fm-multi-rename-preview tbody tr td:nth-child(2)')].map(
    (cell) => cell.textContent ?? '',
  );
}

function findPresetDeleteButton(): HTMLButtonElement {
  const button = document.querySelector('.fm-multi-rename-presets button');
  if (!(button instanceof HTMLButtonElement)) throw new Error('preset delete button missing');
  return button;
}

describe('MultiRenameDialog', () => {
  it('renders a preview row per entry, unchanged by default', () => {
    m.mount(root, {
      view: () =>
        m(MultiRenameDialog, {
          open: true,
          entries,
          existingSiblingNames: new Set<string>(),
          presets: [],
          onPresetsChange: vi.fn(),
          onApply: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    const rows = [...document.querySelectorAll('.fm-multi-rename-preview tbody tr')];
    expect(rows).toHaveLength(2);
    expect(rows[0]?.textContent).toContain('alpha.txt');
    expect(findButton('Rename').hasAttribute('disabled')).toBe(true);
  });

  it('updates the preview live as search/replace rules change', () => {
    m.mount(root, {
      view: () =>
        m(MultiRenameDialog, {
          open: true,
          entries,
          existingSiblingNames: new Set<string>(),
          presets: [],
          onPresetsChange: vi.fn(),
          onApply: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    const search = findInput('multi-rename-search');
    const replace = findInput('multi-rename-replace');
    search.value = 'alpha';
    search.dispatchEvent(new InputEvent('input', { bubbles: true }));
    replace.value = 'gamma';
    replace.dispatchEvent(new InputEvent('input', { bubbles: true }));
    m.redraw.sync();

    const rows = [...document.querySelectorAll('.fm-multi-rename-preview tbody tr')];
    expect(rows[0]?.textContent).toContain('gamma.txt');
    expect(findButton('Rename').hasAttribute('disabled')).toBe(false);
  });

  it('saves the current rules as a named preset', () => {
    const onPresetsChange = vi.fn().mockResolvedValue(undefined);
    vi.spyOn(window, 'prompt').mockReturnValue('Numbered');
    m.mount(root, {
      view: () =>
        m(MultiRenameDialog, {
          open: true,
          entries,
          existingSiblingNames: new Set<string>(),
          presets: [],
          onPresetsChange,
          onApply: vi.fn(),
          onCancel: vi.fn(),
        }),
    });

    m.redraw.sync();

    const nameMask = findInput('multi-rename-name-mask');
    nameMask.value = 'file-[C]';
    nameMask.dispatchEvent(new InputEvent('input', { bubbles: true }));
    findButton('Save as preset…').click();

    expect(onPresetsChange).toHaveBeenCalledWith([
      expect.objectContaining({
        name: 'Numbered',
        rules: expect.objectContaining({ nameMask: 'file-[C]' }),
      }),
    ]);
  });

  it('loading a preset produces the same preview as entering its rules manually', () => {
    let open = true;
    let presets = [] as (typeof numberedPreset)[];
    m.mount(root, {
      view: () =>
        m(MultiRenameDialog, {
          open,
          entries,
          existingSiblingNames: new Set<string>(),
          presets,
          onPresetsChange: vi.fn(),
          onApply: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    for (const [id, value] of [
      ['multi-rename-search', numberedPreset.rules.search],
      ['multi-rename-replace', numberedPreset.rules.replace],
      ['multi-rename-name-mask', numberedPreset.rules.nameMask],
    ] as const) {
      const input = findInput(id);
      input.value = value;
      input.dispatchEvent(new InputEvent('input', { bubbles: true }));
    }
    for (const [id, value] of [
      ['multi-rename-sequence-start', numberedPreset.rules.sequence.start],
      ['multi-rename-sequence-step', numberedPreset.rules.sequence.step],
      ['multi-rename-sequence-padding', numberedPreset.rules.sequence.padding],
    ] as const) {
      const input = findInput(id);
      input.value = String(value);
      input.dispatchEvent(new InputEvent('input', { bubbles: true }));
    }
    m.redraw.sync();
    const manualPreview = previewNames();

    open = false;
    m.redraw.sync();
    presets = [numberedPreset];
    open = true;
    m.redraw.sync();
    const picker = document.getElementById('multi-rename-preset');
    if (!(picker instanceof HTMLSelectElement)) throw new Error('preset picker missing');
    picker.value = numberedPreset.name;
    picker.dispatchEvent(new Event('change', { bubbles: true }));
    m.redraw.sync();

    expect(previewNames()).toEqual(manualPreview);
  });

  it('asks before overwriting a preset with the same name', () => {
    const onPresetsChange = vi.fn().mockResolvedValue(undefined);
    vi.spyOn(window, 'prompt').mockReturnValue(numberedPreset.name);
    const confirm = vi.spyOn(window, 'confirm').mockReturnValue(false);
    m.mount(root, {
      view: () =>
        m(MultiRenameDialog, {
          open: true,
          entries,
          existingSiblingNames: new Set<string>(),
          presets: [numberedPreset],
          onPresetsChange,
          onApply: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    findButton('Save as preset…').click();

    expect(confirm).toHaveBeenCalledOnce();
    expect(onPresetsChange).not.toHaveBeenCalled();

    const nameMask = findInput('multi-rename-name-mask');
    nameMask.value = 'replacement-[N]';
    nameMask.dispatchEvent(new InputEvent('input', { bubbles: true }));
    confirm.mockReturnValue(true);
    findButton('Save as preset…').click();

    expect(onPresetsChange).toHaveBeenCalledWith([
      expect.objectContaining({
        name: numberedPreset.name,
        rules: expect.objectContaining({ nameMask: 'replacement-[N]' }),
      }),
    ]);
  });

  it('deletes the deliberately selected preset', () => {
    const onPresetsChange = vi.fn().mockResolvedValue(undefined);
    m.mount(root, {
      view: () =>
        m(MultiRenameDialog, {
          open: true,
          entries,
          existingSiblingNames: new Set<string>(),
          presets: [numberedPreset],
          onPresetsChange,
          onApply: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();
    const picker = document.getElementById('multi-rename-preset');
    if (!(picker instanceof HTMLSelectElement)) throw new Error('preset picker missing');
    picker.value = numberedPreset.name;
    picker.dispatchEvent(new Event('change', { bubbles: true }));
    m.redraw.sync();

    const deleteButton = findPresetDeleteButton();
    expect(deleteButton.disabled).toBe(false);
    deleteButton.click();

    expect(onPresetsChange).toHaveBeenCalledWith([]);
  });

  it('prevents overlapping preset mutations while settings are being saved', async () => {
    let open = true;
    let resolveSave: (() => void) | undefined;
    const onPresetsChange = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          resolveSave = resolve;
        }),
    );
    vi.spyOn(window, 'prompt').mockReturnValue(numberedPreset.name);
    m.mount(root, {
      view: () =>
        m(MultiRenameDialog, {
          open,
          entries,
          existingSiblingNames: new Set<string>(),
          presets: [],
          onPresetsChange,
          onApply: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    findButton('Save as preset…').click();
    m.redraw.sync();

    expect(findButton('Save as preset…').disabled).toBe(true);
    open = false;
    m.redraw.sync();
    open = true;
    m.redraw.sync();
    expect(findButton('Save as preset…').disabled).toBe(true);
    resolveSave?.();
    await vi.waitFor(() => expect(findButton('Save as preset…').disabled).toBe(false));
  });

  it('does not save counter values that the settings transport cannot represent', () => {
    m.mount(root, {
      view: () =>
        m(MultiRenameDialog, {
          open: true,
          entries,
          existingSiblingNames: new Set<string>(),
          presets: [],
          onPresetsChange: vi.fn(),
          onApply: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    const padding = findInput('multi-rename-sequence-padding');
    padding.value = '4294967296';
    padding.dispatchEvent(new InputEvent('input', { bubbles: true }));
    m.redraw.sync();

    expect(findButton('Save as preset…').disabled).toBe(true);
  });

  it('applies only the changed entries with their new names', () => {
    const onApply = vi.fn();
    m.mount(root, {
      view: () =>
        m(MultiRenameDialog, {
          open: true,
          entries,
          existingSiblingNames: new Set<string>(),
          presets: [],
          onPresetsChange: vi.fn(),
          onApply,
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    const nameMask = findInput('multi-rename-name-mask');
    nameMask.value = 'new-[N]';
    nameMask.dispatchEvent(new InputEvent('input', { bubbles: true }));
    m.redraw.sync();

    findButton('Rename').dispatchEvent(new MouseEvent('click', { bubbles: true }));

    expect(onApply).toHaveBeenCalledWith([
      { id: '1', newName: 'new-alpha.txt' },
      { id: '2', newName: 'new-beta.txt' },
    ]);
  });

  it('disables Rename and shows an error for an invalid regex, without throwing', () => {
    m.mount(root, {
      view: () =>
        m(MultiRenameDialog, {
          open: true,
          entries,
          existingSiblingNames: new Set<string>(),
          presets: [],
          onPresetsChange: vi.fn(),
          onApply: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    const regexCheckbox = findInput('multi-rename-use-regex');
    const search = findInput('multi-rename-search');
    regexCheckbox.click();
    search.value = '(unterminated';
    search.dispatchEvent(new InputEvent('input', { bubbles: true }));
    m.redraw.sync();

    expect(document.querySelector('.fm-field-error')?.textContent).toBeTruthy();
    expect(findButton('Rename').hasAttribute('disabled')).toBe(true);
  });

  it('blocks Rename and flags the row when a collision would occur', () => {
    m.mount(root, {
      view: () =>
        m(MultiRenameDialog, {
          open: true,
          entries,
          existingSiblingNames: new Set(['same.txt']),
          presets: [],
          onPresetsChange: vi.fn(),
          onApply: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    const search = findInput('multi-rename-search');
    const replace = findInput('multi-rename-replace');
    search.value = 'alpha';
    search.dispatchEvent(new InputEvent('input', { bubbles: true }));
    replace.value = 'same';
    replace.dispatchEvent(new InputEvent('input', { bubbles: true }));
    m.redraw.sync();

    expect(document.querySelector('.fm-multi-rename-row--problem')).not.toBeNull();
    expect(findButton('Rename').hasAttribute('disabled')).toBe(true);
  });

  it('cancels without applying when Cancel is clicked', () => {
    const onCancel = vi.fn();
    const onApply = vi.fn();
    m.mount(root, {
      view: () =>
        m(MultiRenameDialog, {
          open: true,
          entries,
          existingSiblingNames: new Set<string>(),
          presets: [],
          onPresetsChange: vi.fn(),
          onApply,
          onCancel,
        }),
    });
    m.redraw.sync();

    findButton('Cancel').dispatchEvent(new MouseEvent('click', { bubbles: true }));

    expect(onCancel).toHaveBeenCalledOnce();
    expect(onApply).not.toHaveBeenCalled();
  });

  it('resets rules on each open transition', () => {
    let open = true;
    m.mount(root, {
      view: () =>
        m(MultiRenameDialog, {
          open,
          entries,
          existingSiblingNames: new Set<string>(),
          presets: [],
          onPresetsChange: vi.fn(),
          onApply: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    const nameMask = findInput('multi-rename-name-mask');
    nameMask.value = 'temp-[N]';
    nameMask.dispatchEvent(new InputEvent('input', { bubbles: true }));
    m.redraw.sync();
    expect(nameMask.value).toBe('temp-[N]');

    open = false;
    m.redraw.sync();
    open = true;
    m.redraw.sync();

    const reopenedNameMask = findInput('multi-rename-name-mask');
    expect(reopenedNameMask.value).toBe('[N]');
  });

  it('composes the name mask from tokens, live', () => {
    m.mount(root, {
      view: () =>
        m(MultiRenameDialog, {
          open: true,
          entries,
          existingSiblingNames: new Set<string>(),
          presets: [],
          onPresetsChange: vi.fn(),
          onApply: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    const nameMask = findInput('multi-rename-name-mask');
    nameMask.value = '[N1-3]-[C]';
    nameMask.dispatchEvent(new InputEvent('input', { bubbles: true }));
    m.redraw.sync();

    const rows = [...document.querySelectorAll('.fm-multi-rename-preview tbody tr')];
    expect(rows[0]?.textContent).toContain('alp-1.txt');
    expect(rows[1]?.textContent).toContain('bet-2.txt');
  });
});
