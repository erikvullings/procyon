import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { MockFileManagerClient } from '../../api/client/mock-file-manager-client';
import type { ActionDescriptor, PluginDescriptor, Settings } from '../../models';
import { SettingsEditor } from './settings-editor';

let root: HTMLElement;

beforeEach(() => {
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
});

function fixtureSettings(overrides: Partial<Settings> = {}): Settings {
  return {
    schemaVersion: 2,
    theme: 'auto',
    language: 'en',
    fontSize: 13,
    rowHeight: 22,
    dateFormat: 'medium',
    sizeFormat: 'binary',
    showHiddenFiles: false,
    confirmPermanentDelete: true,
    confirmFileOperations: true,
    defaultConflictPolicy: 'ask',
    operationConcurrency: 2,
    defaultPaneLayout: 'dual',
    defaultColumns: ['core.name', 'core.size'],
    columnWidths: {},
    keybindings: {},
    enabledPlugins: [],
    pluginSettings: {},
    terminalCommand: null,
    editorCommand: null,
    defaultStartLocations: [],
    favouriteLocations: [],
    recentLocationsByWorkspace: {},
    multiRenamePresets: [],
    savedSearches: [],
    iconTheme: 'generic',
    ...overrides,
  };
}

const actions: readonly ActionDescriptor[] = [
  {
    id: 'core.rename',
    title: 'Rename',
    category: 'fileOperations',
    defaultShortcuts: [{ key: 'F2' }],
    contextRequirements: {},
    source: { kind: 'core' },
  },
  {
    id: 'core.copy',
    title: 'Copy',
    category: 'fileOperations',
    defaultShortcuts: [{ key: 'F5' }],
    contextRequirements: {},
    source: { kind: 'core' },
  },
];

function mountEditor(overrides: Partial<Parameters<typeof SettingsEditor>[0]['attrs']> = {}) {
  const onPreview = vi.fn();
  const onSave = vi.fn().mockResolvedValue(undefined);
  const onCancel = vi.fn();
  const onTogglePlugin = vi.fn();
  const onRequestPluginLogs = vi.fn();
  const client = new MockFileManagerClient();
  m.mount(root, {
    view: () =>
      m(SettingsEditor, {
        settings: fixtureSettings(),
        actions,
        platform: 'windows',
        runtime: 'desktop',
        client,
        plugins: [],
        onPreview,
        onSave,
        onCancel,
        onTogglePlugin,
        onRequestPluginLogs,
        ...overrides,
      }),
  });
  m.redraw.sync();
  return { client, onPreview, onSave, onCancel, onTogglePlugin, onRequestPluginLogs };
}

function numberInput(label: string): HTMLInputElement {
  const input = [...root.querySelectorAll('input')].find(
    (candidate) => candidate.closest('.input-field')?.querySelector('label')?.textContent === label,
  );
  if (!(input instanceof HTMLInputElement)) throw new Error(`no number input labeled ${label}`);
  return input;
}

function fireChange(input: HTMLInputElement, value: string): void {
  input.value = value;
  input.dispatchEvent(new Event('input', { bubbles: true }));
  m.redraw.sync();
}

function openSection(label: string): void {
  const button = [...root.querySelectorAll<HTMLButtonElement>('.fm-settings-section-button')].find(
    (candidate) => candidate.textContent?.trim() === label,
  );
  if (button === undefined) throw new Error(`no settings section labelled ${label}`);
  button.click();
  m.redraw.sync();
}

describe('SettingsEditor', () => {
  it('splits the editor into keyboard-accessible sections', () => {
    mountEditor();

    expect(
      root.querySelector('.fm-settings-section-button[aria-current="page"]')?.textContent,
    ).toBe('Appearance');
    expect(numberInput('Font size (px)')).toBeInstanceOf(HTMLInputElement);
    expect(root.querySelector('.fm-settings-editor-body')?.textContent).not.toContain(
      'Keybindings',
    );

    openSection('Keybindings');

    expect(root.querySelectorAll('.fm-settings-keybinding-row')).toHaveLength(2);
    expect(root.querySelector('input[type="number"]')).toBeNull();
  });

  it('shows only semantic activation until components are installed', async () => {
    const { client } = mountEditor();
    const status = vi.spyOn(client, 'getSemanticComponentStatus');
    const library = vi.spyOn(client, 'getSemanticLibraryStatus');

    expect(status).not.toHaveBeenCalled();
    openSection('Semantic');

    await vi.waitFor(() => expect(root.querySelector('.fm-semantic-management')).not.toBeNull());
    expect(root.textContent).toContain('Semantic components');
    expect(status).toHaveBeenCalledOnce();
    expect(root.textContent).not.toContain('Semantic libraries');
    expect(root.textContent).not.toContain('Vocabularies');
    expect(library).not.toHaveBeenCalled();
  });

  it('reveals semantic configuration after components are enabled', async () => {
    const client = new MockFileManagerClient({ semanticLifecycle: 'installedEnabled' });
    mountEditor({ client });
    openSection('Semantic');

    await vi.waitFor(() => expect(root.textContent).toContain('Semantic library'));
    expect(root.textContent).toContain('Concept vocabularies');
    expect(root.textContent).toContain('Generation profiles');
  });

  it('saves a changed generation profile through the main settings action', async () => {
    const client = new MockFileManagerClient({ semanticLifecycle: 'installedEnabled' });
    const createProfile = vi.spyOn(client, 'createLlmProfile');
    const { onSave } = mountEditor({ client });
    openSection('Semantic');

    await vi.waitFor(() => expect(root.textContent).toContain('Provider preset'));
    const nameInput = [...root.querySelectorAll<HTMLInputElement>('input')].find(
      (input) => input.value === 'Ollama' && !input.classList.contains('select-dropdown'),
    );
    if (nameInput === undefined) throw new Error('generation profile name input was not rendered');
    fireChange(nameInput, 'Local assistant');
    openSection('Appearance');

    expect(
      [...root.querySelectorAll<HTMLButtonElement>('.fm-llm-profile-actions button')].some(
        (button) => button.textContent?.trim() === 'Save',
      ),
    ).toBe(false);
    root.querySelector<HTMLButtonElement>('.fm-settings-save')?.click();

    await vi.waitFor(() => expect(createProfile).toHaveBeenCalledOnce());
    expect(createProfile.mock.calls[0]?.[0].name).toBe('Local assistant');
    await vi.waitFor(() => expect(onSave).toHaveBeenCalledOnce());
  });

  it('renders initial appearance values from the loaded settings', () => {
    mountEditor({ settings: fixtureSettings({ fontSize: 17, rowHeight: 30 }) });

    expect(numberInput('Font size (px)').value).toBe('17');
    expect(numberInput('Row height (px)').value).toBe('30');
  });

  it('offers every supported language', () => {
    const { onPreview } = mountEditor();
    root.querySelectorAll<HTMLInputElement>('input.select-dropdown')[0]?.click();
    m.redraw.sync();

    expect(root.textContent).toContain('English');
    expect(root.textContent).toContain('Dutch');
    expect(root.textContent).toContain('German');
    expect(root.textContent).toContain('French');
    expect(root.textContent).toContain('Spanish');
    expect(root.textContent).toContain('Italian');
    expect(root.textContent).toContain('Portuguese');
    expect(root.textContent).toContain('Polish');

    Array.from(root.querySelectorAll('li'))
      .find((item) => item.textContent === 'German')
      ?.click();
    expect(onPreview).toHaveBeenCalledWith(expect.objectContaining({ language: 'de' }));
  });

  it('renders without throwing when a plugin has no icon theme (backend sends null, not undefined)', () => {
    // The JSON DTO serializes an absent Option<T> field as null, though `PluginDescriptor` models it as `undefined`.
    const pluginWithNullIconTheme = {
      id: 'sample.plugin',
      name: 'Sample',
      version: '1.0.0',
      description: '',
      enabled: true,
      iconTheme: null,
    } as unknown as PluginDescriptor;

    expect(() => mountEditor({ plugins: [pluginWithNullIconTheme] })).not.toThrow();
  });

  it('previews an edited field immediately without saving', () => {
    const { onPreview, onSave } = mountEditor();

    fireChange(numberInput('Font size (px)'), '20');

    expect(onPreview).toHaveBeenCalledWith(expect.objectContaining({ fontSize: 20 }));
    expect(onSave).not.toHaveBeenCalled();
  });

  it('reverts the draft and calls onCancel without persisting', () => {
    const { onCancel, onSave } = mountEditor();

    fireChange(numberInput('Font size (px)'), '20');
    root.querySelector<HTMLButtonElement>('.fm-settings-cancel')?.click();
    m.redraw.sync();

    expect(onCancel).toHaveBeenCalledOnce();
    expect(onSave).not.toHaveBeenCalled();
    expect(numberInput('Font size (px)').value).toBe('13');
  });

  it('saves the whole edited document as one call', async () => {
    const { onSave } = mountEditor();

    fireChange(numberInput('Font size (px)'), '20');
    root.querySelector<HTMLButtonElement>('.fm-settings-save')?.click();
    m.redraw.sync();

    expect(onSave).toHaveBeenCalledWith(expect.objectContaining({ fontSize: 20 }));
  });

  it('saves a plugin enabled via Plugin Management instead of reverting it (regression)', async () => {
    // Plugin Management's toggle persists directly to the backend, bypassing the draft entirely
    // (see handleTogglePlugin's doc comment) - so this must not send the dialog's stale opening
    // snapshot of `enabledPlugins` back to the backend and silently re-disable the plugin.
    const onTogglePlugin = vi.fn().mockResolvedValue(undefined);
    const plugin: PluginDescriptor = {
      id: 'catppuccin.icons',
      name: 'Catppuccin Icons',
      version: '1.0.0',
      description: '',
      enabled: false,
    };
    const { onSave } = mountEditor({ plugins: [plugin], onTogglePlugin });

    openSection('Plugins');
    root.querySelector<HTMLInputElement>('.fm-plugin-toggle input')?.click();
    await Promise.resolve();
    await Promise.resolve();
    m.redraw.sync();
    expect(onTogglePlugin).toHaveBeenCalledWith('catppuccin.icons', true);

    root.querySelector<HTMLButtonElement>('.fm-settings-save')?.click();
    m.redraw.sync();

    expect(onSave).toHaveBeenCalledWith(
      expect.objectContaining({ enabledPlugins: ['catppuccin.icons'] }),
    );
  });

  it('keeps the draft visible and shows an error when saving fails', async () => {
    const onSave = vi.fn().mockRejectedValue(new Error('backend refused the request'));
    mountEditor({ onSave });

    fireChange(numberInput('Font size (px)'), '20');
    root.querySelector<HTMLButtonElement>('.fm-settings-save')?.click();
    await Promise.resolve();
    await Promise.resolve();
    m.redraw.sync();

    expect(root.querySelector('.fm-settings-save-error')?.textContent).toBe(
      'backend refused the request',
    );
    expect(numberInput('Font size (px)').value).toBe('20');
  });

  it('disables saving and shows a validation message for an out-of-range field', () => {
    mountEditor();

    fireChange(numberInput('Font size (px)'), '4');

    expect(root.querySelector('.fm-settings-validation-errors')?.textContent).toContain(
      'Font size',
    );
    expect(root.querySelector<HTMLButtonElement>('.fm-settings-save')?.disabled).toBe(true);
  });

  it('lists the effective keybindings and flags a conflict between two actions', () => {
    mountEditor({
      settings: fixtureSettings({ keybindings: { 'core.copy': 'F2' } }),
    });
    openSection('Keybindings');

    const rows = [...root.querySelectorAll('.fm-settings-keybinding-row')];
    expect(rows).toHaveLength(2);
    const conflicted = rows.filter((row) => row.getAttribute('data-conflict') === 'true');
    expect(conflicted.map((row) => row.getAttribute('data-action-id')).sort()).toEqual([
      'core.copy',
      'core.rename',
    ]);
    expect(root.querySelector('.fm-settings-keybinding-conflicts')?.textContent).toContain('F2');
  });

  it('embeds plugin management for enable/disable rather than a second path', () => {
    mountEditor({
      plugins: [
        {
          id: 'example.plugin',
          name: 'Example plugin',
          version: '1.0.0',
          description: 'An example plugin.',
          enabled: true,
        },
      ],
    });
    openSection('Plugins');

    expect(root.querySelector('.fm-plugin-row strong')?.textContent).toBe('Example plugin');
  });
});
