import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { FilterPreset } from '../../models/settings';
import { type AdvancedFilterCriteria, EMPTY_ADVANCED_FILTER } from './advanced-filter';
import { AdvancedFilterBar, type AdvancedFilterBarAttrs } from './advanced-filter-bar';
import { AdvancedFilterStore } from './advanced-filter-store';

let root: HTMLElement;

beforeEach(() => {
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
});

const videos: FilterPreset = {
  name: 'Videos',
  criteria: { ...EMPTY_ADVANCED_FILTER, name: '*.mkv', kind: 'file', minSize: 1_048_576 },
};

function mount(overrides: Partial<AdvancedFilterBarAttrs> = {}): AdvancedFilterBarAttrs {
  const attrs: AdvancedFilterBarAttrs = {
    criteria: EMPTY_ADVANCED_FILTER,
    presets: [],
    panelOpen: true,
    notices: [],
    onChange: vi.fn(),
    onClear: vi.fn(),
    onPresetsChange: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  };
  m.mount(root, { view: () => m(AdvancedFilterBar, attrs) });
  return attrs;
}

function button(label: string): HTMLButtonElement {
  const found = [...root.querySelectorAll('button')].find((b) => b.textContent?.trim() === label);
  if (found === undefined) throw new Error(`button ${label} missing`);
  return found;
}

function input(id: string): HTMLInputElement | HTMLSelectElement {
  const el = root.querySelector(`#${id}`);
  if (!(el instanceof HTMLInputElement) && !(el instanceof HTMLSelectElement)) {
    throw new Error(`${id} missing`);
  }
  return el;
}

function type(id: string, value: string): void {
  const el = input(id);
  el.value = value;
  el.dispatchEvent(new Event('input', { bubbles: true }));
}

describe('AdvancedFilterBar', () => {
  it('renders nothing when closed with no criteria', () => {
    mount({ panelOpen: false });
    expect(root.querySelector('.fm-advanced-filter')).toBeNull();
  });

  it('emits structured criteria from the form controls', () => {
    const attrs = mount();
    type('fm-advanced-filter-name', '*.png');
    expect(attrs.onChange).toHaveBeenLastCalledWith({ ...EMPTY_ADVANCED_FILTER, name: '*.png' });
    type('fm-advanced-filter-min-size', '2 MB');
    expect(attrs.onChange).toHaveBeenLastCalledWith({
      ...EMPTY_ADVANCED_FILTER,
      minSize: 2_097_152,
    });
    type('fm-advanced-filter-within', '7');
    expect(attrs.onChange).toHaveBeenLastCalledWith({
      ...EMPTY_ADVANCED_FILTER,
      modifiedWithinDays: 7,
    });
    const kind = input('fm-advanced-filter-kind');
    kind.value = 'directory';
    kind.dispatchEvent(new Event('change', { bubbles: true }));
    expect(attrs.onChange).toHaveBeenLastCalledWith({
      ...EMPTY_ADVANCED_FILTER,
      kind: 'directory',
    });
  });

  it('flags an unparsable size without applying it', () => {
    const attrs = mount();
    type('fm-advanced-filter-max-size', 'lots');
    m.redraw.sync();
    expect(attrs.onChange).not.toHaveBeenCalled();
    expect(input('fm-advanced-filter-max-size').getAttribute('aria-invalid')).toBe('true');
  });

  it('shows active criteria chips that clear in one action', () => {
    const criteria: AdvancedFilterCriteria = { ...videos.criteria, hidden: 'exclude' };
    const attrs = mount({ criteria, panelOpen: false });
    const chips = [...root.querySelectorAll('.fm-advanced-filter-chip')].map((c) => c.textContent);
    expect(chips).toEqual(['Name: *.mkv', 'Type: Files', 'Size ≥ 1 MB', 'Hidden: Exclude']);
    button('Clear filter').click();
    expect(attrs.onClear).toHaveBeenCalledTimes(1);
  });

  it('clears everything with Escape from inside the panel', () => {
    const attrs = mount();
    const name = input('fm-advanced-filter-name');
    name.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    expect(attrs.onClear).toHaveBeenCalledTimes(1);
  });

  it('explains unsupported criteria instead of ignoring them', () => {
    mount({
      criteria: { ...EMPTY_ADVANCED_FILTER, executable: 'only' },
      notices: ['executableByExtension', 'loadedEntriesOnly'],
    });
    const notices = root.querySelector('[role=status]')?.textContent ?? '';
    expect(notices).toContain('file extension');
    expect(notices).toContain('further pages are not fetched');
  });

  it('keeps every control reachable by keyboard (native, enabled, labelled controls)', () => {
    mount({ criteria: videos.criteria, presets: [videos] });
    const controls = [...root.querySelectorAll('input, select, button')];
    expect(controls.length).toBeGreaterThanOrEqual(11);
    for (const control of controls) {
      expect((control as HTMLElement).tabIndex).not.toBe(-1);
      if (control instanceof HTMLButtonElement) continue;
      const label = root.querySelector(`label[for="${control.id}"]`);
      expect(label?.textContent, control.id).toBeTruthy();
    }
  });

  describe('presets', () => {
    it('saves under a new name', async () => {
      const attrs = mount({
        criteria: videos.criteria,
        prompt: () => ' Movies ',
        confirm: vi.fn(),
      });
      button('Save as…').click();
      await Promise.resolve();
      expect(attrs.onPresetsChange).toHaveBeenCalledWith([
        { name: 'Movies', criteria: videos.criteria },
      ]);
      expect(attrs.confirm).not.toHaveBeenCalled();
    });

    it('asks before overwriting and keeps the old preset when declined', () => {
      const confirm = vi.fn().mockReturnValue(false);
      const attrs = mount({
        criteria: { ...videos.criteria, minSize: 5 },
        presets: [videos],
        prompt: () => 'videos',
        confirm,
      });
      button('Save as…').click();
      expect(confirm).toHaveBeenCalledWith(expect.stringContaining('"Videos"'));
      expect(attrs.onPresetsChange).not.toHaveBeenCalled();
    });

    it('overwrites after confirmation', () => {
      const attrs = mount({
        criteria: { ...videos.criteria, minSize: 5 },
        presets: [videos],
        prompt: () => 'Videos',
        confirm: () => true,
      });
      button('Save as…').click();
      expect(attrs.onPresetsChange).toHaveBeenCalledWith([
        { name: 'Videos', criteria: { ...videos.criteria, minSize: 5 } },
      ]);
    });

    it('loads a preset into the current pane, then renames and deletes it', async () => {
      const prompt = vi.fn().mockReturnValue('Films');
      const attrs = mount({ presets: [videos], prompt });
      const select = input('fm-advanced-filter-preset');
      select.value = 'Videos';
      select.dispatchEvent(new Event('change', { bubbles: true }));
      expect(attrs.onChange).toHaveBeenCalledWith(videos.criteria);
      m.redraw.sync();

      button('Rename…').click();
      await Promise.resolve();
      expect(attrs.onPresetsChange).toHaveBeenLastCalledWith([{ ...videos, name: 'Films' }]);
    });

    it('deletes the selected preset', () => {
      const attrs = mount({ presets: [videos] });
      const select = input('fm-advanced-filter-preset');
      select.value = 'Videos';
      select.dispatchEvent(new Event('change', { bubbles: true }));
      m.redraw.sync();
      button('Delete').click();
      expect(attrs.onPresetsChange).toHaveBeenLastCalledWith([]);
    });

    it('rejects a rename onto another preset name', () => {
      const other: FilterPreset = { name: 'Docs', criteria: EMPTY_ADVANCED_FILTER };
      const attrs = mount({ presets: [videos, other], prompt: () => 'docs' });
      const select = input('fm-advanced-filter-preset');
      select.value = 'Videos';
      select.dispatchEvent(new Event('change', { bubbles: true }));
      m.redraw.sync();
      button('Rename…').click();
      m.redraw.sync();
      expect(attrs.onPresetsChange).not.toHaveBeenCalled();
      expect(root.querySelector('[role=alert]')?.textContent).toContain('already exists');
    });

    it('reports a failed persistence', async () => {
      mount({
        criteria: videos.criteria,
        prompt: () => 'X',
        onPresetsChange: vi.fn().mockRejectedValue(new Error('nope')),
      });
      button('Save as…').click();
      await new Promise((resolve) => setTimeout(resolve, 0));
      m.redraw.sync();
      expect(root.querySelector('[role=alert]')?.textContent).toContain('could not be updated');
    });
  });
});

describe('AdvancedFilterStore', () => {
  it('tracks criteria and panel per tab, and ignores inactive criteria', () => {
    const store = new AdvancedFilterStore();
    expect(store.criteriaFor('a')).toBe(EMPTY_ADVANCED_FILTER);
    store.setCriteria('a', videos.criteria);
    store.setPanelOpen('a', true);
    expect(store.isActive('a')).toBe(true);
    expect(store.isActive('b')).toBe(false);
    store.setCriteria('b', EMPTY_ADVANCED_FILTER);
    expect(store.isActive('b')).toBe(false);
  });

  it('moves state with a tab and clears it in one call', () => {
    const store = new AdvancedFilterStore();
    store.setCriteria('a', videos.criteria);
    store.setPanelOpen('a', true);
    store.rekey('a', 'b');
    expect(store.isActive('a')).toBe(false);
    expect(store.criteriaFor('b')).toBe(videos.criteria);
    expect(store.panelOpen('b')).toBe(true);
    store.clear('b');
    expect(store.isActive('b')).toBe(false);
    expect(store.panelOpen('b')).toBe(false);
  });
});
