import m, { type FactoryComponent } from 'mithril';
import { NumberInput, Select, Switch, TextInput, ThemeSwitcher } from 'mithril-materialized';
import type { Locale } from '../../i18n';
import { LOCALES, t } from '../../i18n';
import type { en } from '../../i18n/en';
import {
  detectBindingConflicts,
  getLiveBindings,
  type KeybindingRuntime,
} from '../../keybindings/dispatcher';
import type {
  ActionDescriptor,
  PluginDescriptor,
  PluginId,
  PluginLogEntry,
  Settings,
} from '../../models';
import { PluginManagement } from '../plugin-management/plugin-management';
import type { SelectionPlatform } from '../selection/keybindings';
import {
  cloneSettings,
  formatListInput,
  parseListInput,
  setKeybindingOverride,
  validateSettingsDraft,
} from './settings-model';

/** Every column the directory table can show, in the fixed order the table renders them. */
const AVAILABLE_DEFAULT_COLUMNS = [
  { id: 'core.name', labelKey: 'name' },
  { id: 'core.extension', labelKey: 'ext' },
  { id: 'core.size', labelKey: 'size' },
  { id: 'core.gitStatus', labelKey: 'gitStatus' },
  { id: 'core.modified', labelKey: 'modified' },
] as const satisfies readonly { readonly id: string; readonly labelKey: keyof typeof en.table }[];

export interface SettingsEditorAttrs {
  readonly settings: Settings;
  readonly actions: readonly ActionDescriptor[];
  readonly platform: SelectionPlatform;
  readonly runtime: KeybindingRuntime;
  readonly plugins: readonly PluginDescriptor[];
  /** Called on every draft change so appearance fields can preview live without persisting. */
  readonly onPreview: (draft: Settings) => void;
  readonly onSave: (draft: Settings) => Promise<void>;
  readonly onCancel: () => void;
  readonly onTogglePlugin: (pluginId: PluginId, enabled: boolean) => Promise<void>;
  readonly onRequestPluginLogs: (pluginId: PluginId) => Promise<readonly PluginLogEntry[]>;
}

function errorMessage(error: unknown, fallback: string): string {
  return error instanceof Error ? error.message : fallback;
}

/**
 * Complete settings editor (task 0083): appearance, file behavior, operations,
 * new-workspace defaults, terminal command, keybindings, and plugins. Draft
 * edits preview live (appearance) but only persist through one `onSave` call
 * on the whole document; `onCancel` discards them.
 */
export const SettingsEditor: FactoryComponent<SettingsEditorAttrs> = () => {
  // Seeded lazily from the first real `view()` call rather than the factory's
  // own vnode argument, since only the former is guaranteed to carry attrs.
  let draft: Settings | undefined;
  let startLocationsText = '';
  let saving = false;
  let saveError: string | undefined;

  function resetDraft(current: SettingsEditorAttrs): void {
    draft = cloneSettings(current.settings);
    startLocationsText = formatListInput(draft.defaultStartLocations);
    saveError = undefined;
  }

  function toggleColumn(current: SettingsEditorAttrs, columnId: string, enabled: boolean): void {
    if (draft === undefined) return;
    const nextIds = new Set(draft.defaultColumns);
    if (enabled) {
      nextIds.add(columnId);
    } else {
      nextIds.delete(columnId);
    }
    update(current, {
      defaultColumns: AVAILABLE_DEFAULT_COLUMNS.map((column) => column.id).filter((id) =>
        nextIds.has(id),
      ),
    });
  }

  function update(current: SettingsEditorAttrs, patch: Partial<Settings>): void {
    if (draft === undefined) return;
    draft = { ...draft, ...patch };
    current.onPreview(draft);
    m.redraw();
  }

  function handleCancel(current: SettingsEditorAttrs): void {
    resetDraft(current);
    current.onCancel();
  }

  /**
   * Plugin Management's enable/disable toggle persists directly to the backend (unlike every
   * other field here, which only writes on Save), so it doesn't go through `update()`. Without
   * this, `draft.enabledPlugins` stays whatever it was when the dialog opened, and Save would
   * send that stale snapshot back to the backend, silently re-disabling a plugin the user just
   * enabled (and reverting any icon theme install still in flight for it).
   */
  function handleTogglePlugin(
    current: SettingsEditorAttrs,
    pluginId: PluginId,
    enabled: boolean,
  ): Promise<void> {
    return current.onTogglePlugin(pluginId, enabled).then(() => {
      if (draft === undefined) return;
      const nextEnabled = new Set(draft.enabledPlugins);
      if (enabled) {
        nextEnabled.add(pluginId);
      } else {
        nextEnabled.delete(pluginId);
      }
      draft = { ...draft, enabledPlugins: [...nextEnabled] };
    });
  }

  function handleSave(current: SettingsEditorAttrs): void {
    if (draft === undefined) return;
    saving = true;
    saveError = undefined;
    current.onSave(draft).then(
      () => {
        saving = false;
        m.redraw();
      },
      (error: unknown) => {
        saving = false;
        saveError = errorMessage(error, 'Failed to save settings.');
        m.redraw();
      },
    );
  }

  return {
    view: ({ attrs: current }) => {
      if (draft === undefined) {
        resetDraft(current);
      }
      const activeDraft = draft as Settings;
      const errors = validateSettingsDraft(activeDraft);
      const errorsByField = new Map<string, string>(
        errors.map((error): [string, string] => [error.field, error.message]),
      );
      // exactOptionalPropertyTypes forbids assigning `dataError: string | undefined`
      // directly; spread this instead so the key is omitted entirely when unset.
      function errorAttrs(field: string): { dataError?: string } {
        const message = errorsByField.get(field);
        return message === undefined ? {} : { dataError: message };
      }
      const context = {
        scope: 'table' as const,
        platform: current.platform,
        runtime: current.runtime,
      };
      const liveBindings = getLiveBindings(current.actions, activeDraft.keybindings, context);
      const conflicts = detectBindingConflicts(current.actions, activeDraft.keybindings, context);
      const conflictedActionIds = new Set(conflicts.flatMap((conflict) => conflict.actionIds));

      return [
        m('.fm-settings-editor-body', { 'aria-label': t('settings', 'settingsEditor') }, [
          m('.row', m('h4.fm-settings-section-heading.col.s12', t('settings', 'appearance'))),
          m('.row', [
            m(Select<Locale>, {
              className: 'col s12',
              label: t('settings', 'language'),
              options: LOCALES.map((locale) => ({
                id: locale,
                label:
                  locale === 'en'
                    ? t('settings', 'languageEnglish')
                    : t('settings', 'languageDutch'),
              })),
              checkedId: activeDraft.language,
              onchange: ([value]) => value !== undefined && update(current, { language: value }),
            }),
            m(ThemeSwitcher, {
              className: 'col s12',
              theme: activeDraft.theme,
              showLabels: true,
              onThemeChange: (next: Settings['theme']) => update(current, { theme: next }),
            }),
          ]),
          m('.row', [
            m(NumberInput, {
              className: 'col s6',
              label: t('settings', 'fontSize'),
              value: activeDraft.fontSize,
              min: 8,
              max: 32,
              oninput: (value: number) => update(current, { fontSize: value }),
              ...errorAttrs('fontSize'),
            }),
            m(NumberInput, {
              className: 'col s6',
              label: t('settings', 'rowHeight'),
              value: activeDraft.rowHeight,
              min: 16,
              max: 64,
              oninput: (value: number) => update(current, { rowHeight: value }),
              ...errorAttrs('rowHeight'),
            }),
          ]),
          m('.row', [
            m(Select<Settings['dateFormat']>, {
              className: 'col s6',
              label: t('settings', 'dateFormat'),
              options: [
                { id: 'short', label: t('settings', 'dateFormatShort') },
                { id: 'medium', label: t('settings', 'dateFormatMedium') },
                { id: 'iso', label: t('settings', 'dateFormatIso') },
              ],
              checkedId: activeDraft.dateFormat,
              onchange: ([value]) => value !== undefined && update(current, { dateFormat: value }),
            }),
            m(Select<Settings['sizeFormat']>, {
              className: 'col s6',
              label: t('settings', 'sizeFormat'),
              options: [
                { id: 'binary', label: t('settings', 'sizeFormatBinary') },
                { id: 'decimal', label: t('settings', 'sizeFormatDecimal') },
                { id: 'bytes', label: t('settings', 'sizeFormatBytes') },
              ],
              checkedId: activeDraft.sizeFormat,
              onchange: ([value]) => value !== undefined && update(current, { sizeFormat: value }),
            }),
          ]),
          m('.row', [
            m(Select<string>, {
              label: t('settings', 'iconTheme'),
              options: [
                { id: 'generic', label: t('settings', 'iconThemeGeneric') },
                { id: 'native', label: t('settings', 'iconThemeNative') },
                ...current.plugins
                  .filter(
                    (plugin) =>
                      // Backend DTOs serialize absent Option<T> fields as JSON null, not undefined.
                      plugin.iconTheme != null &&
                      Object.keys(plugin.iconTheme.iconDefinitions).length > 0,
                  )
                  .map((plugin) => ({
                    id: plugin.id,
                    label: plugin.enabled ? plugin.name : `${plugin.name} (plugin disabled)`,
                  })),
              ],
              checkedId: activeDraft.iconTheme,
              onchange: ([value]) => value !== undefined && update(current, { iconTheme: value }),
            }),
          ]),

          m('.row', m('h4.fm-settings-section-heading.col.s12', t('settings', 'fileBehavior'))),
          m('.row', [
            m(Switch, {
              className: 'col s12 m6',
              label: t('settings', 'showHiddenFiles'),
              checked: activeDraft.showHiddenFiles,
              left: 'Hidden',
              right: 'Shown',
              onchange: (checked: boolean) => update(current, { showHiddenFiles: checked }),
            }),
            m(Switch, {
              className: 'col s12 m6',
              label: t('settings', 'confirmPermanentDelete'),
              checked: activeDraft.confirmPermanentDelete,
              left: 'Off',
              right: 'On',
              onchange: (checked: boolean) => update(current, { confirmPermanentDelete: checked }),
            }),
          ]),

          m('.row', m('h4.fm-settings-section-heading.col.s12', t('settings', 'operations'))),
          m('.row', [
            m(Select<Settings['defaultConflictPolicy']>, {
              className: 'col s6',
              label: t('settings', 'defaultConflictPolicy'),
              options: [
                { id: 'ask', label: t('settings', 'conflictAsk') },
                { id: 'overwrite', label: t('settings', 'conflictOverwrite') },
                { id: 'keepBoth', label: t('settings', 'conflictKeepBoth') },
                { id: 'skip', label: t('settings', 'conflictSkip') },
              ],
              checkedId: activeDraft.defaultConflictPolicy,
              onchange: ([value]) =>
                value !== undefined && update(current, { defaultConflictPolicy: value }),
            }),
            m(NumberInput, {
              className: 'col s6',
              label: t('settings', 'operationConcurrency'),
              value: activeDraft.operationConcurrency,
              min: 1,
              oninput: (value: number) => update(current, { operationConcurrency: value }),
              ...errorAttrs('operationConcurrency'),
            }),
          ]),

          m(
            '.row',
            m('h4.fm-settings-section-heading.col.s12', t('settings', 'newWorkspaceDefaults')),
          ),
          m('.row', [
            m(Select<Settings['defaultPaneLayout']>, {
              label: t('settings', 'defaultPaneLayout'),
              options: [
                { id: 'dual', label: t('settings', 'paneLayoutDual') },
                { id: 'single', label: t('settings', 'paneLayoutSingle') },
              ],
              checkedId: activeDraft.defaultPaneLayout,
              onchange: ([value]) =>
                value !== undefined && update(current, { defaultPaneLayout: value }),
            }),
          ]),
          m('.row', [
            m('fieldset.fm-settings-column-select.col.s12', [
              m('legend', t('settings', 'defaultColumns')),
              m(
                'ul.fm-settings-column-list',
                AVAILABLE_DEFAULT_COLUMNS.map((column) =>
                  m('li', [
                    m('label', [
                      m('input', {
                        type: 'checkbox',
                        checked: activeDraft.defaultColumns.includes(column.id),
                        onchange: (event: Event) =>
                          toggleColumn(
                            current,
                            column.id,
                            (event.target as HTMLInputElement).checked,
                          ),
                      }),
                      m('span', t('table', column.labelKey)),
                    ]),
                  ]),
                ),
              ),
            ]),
          ]),
          m('.row', [
            m(TextInput, {
              label: t('settings', 'defaultStartLocations'),
              value: startLocationsText,
              oninput: (value: string) => {
                startLocationsText = value;
              },
              onchange: (value: string) => {
                startLocationsText = value;
                update(current, { defaultStartLocations: parseListInput(value) });
              },
            }),
          ]),

          m('.row', m('h4.fm-settings-section-heading.col.s12', t('settings', 'terminal'))),
          m('.row', [
            m(TextInput, {
              label: t('settings', 'terminalCommand'),
              value: activeDraft.terminalCommand ?? '',
              placeholder: t('settings', 'systemDefault'),
              oninput: (value: string) =>
                update(current, { terminalCommand: value.trim().length === 0 ? null : value }),
              onchange: (value: string) =>
                update(current, { terminalCommand: value.trim().length === 0 ? null : value }),
            }),
          ]),

          m('.row', m('h4.fm-settings-section-heading.col.s12', t('settings', 'editor'))),
          m('.row', [
            m(TextInput, {
              label: t('settings', 'editorCommand'),
              value: activeDraft.editorCommand ?? '',
              placeholder: t('settings', 'systemDefault'),
              oninput: (value: string) =>
                update(current, { editorCommand: value.trim().length === 0 ? null : value }),
              onchange: (value: string) =>
                update(current, { editorCommand: value.trim().length === 0 ? null : value }),
            }),
          ]),

          m('.row', m('h4.fm-settings-section-heading.col.s12', t('settings', 'keybindings'))),
          conflicts.length === 0
            ? undefined
            : m(
                'ul.fm-settings-keybinding-conflicts',
                { role: 'alert' },
                conflicts.map((conflict) =>
                  m(
                    'li',
                    t('settings', 'keybindingConflict', {
                      shortcut: conflict.shortcut,
                      actions: conflict.actionIds.join(', '),
                    }),
                  ),
                ),
              ),
          m(
            'ul.fm-settings-keybindings.row',
            current.actions.map((action) => {
              const shortcut = liveBindings.find((binding) => binding.actionId === action.id);
              return m(
                'li.fm-settings-keybinding-row.col.s12.m6',
                {
                  'data-action-id': action.id,
                  'data-conflict': String(conflictedActionIds.has(action.id)),
                },
                [
                  m('span.fm-settings-keybinding-title', action.title),
                  m(TextInput, {
                    className: 'fm-settings-keybinding-input',
                    // label: 'Shortcut',
                    value: activeDraft.keybindings[action.id] ?? '',
                    placeholder: shortcut?.shortcut ?? 'None',
                    oninput: (value: string) =>
                      update(current, {
                        keybindings: setKeybindingOverride(
                          activeDraft.keybindings,
                          action.id,
                          value,
                        ),
                      }),
                    onchange: (value: string) =>
                      update(current, {
                        keybindings: setKeybindingOverride(
                          activeDraft.keybindings,
                          action.id,
                          value,
                        ),
                      }),
                  }),
                  shortcut?.available === false
                    ? m(
                        'span.fm-settings-keybinding-unavailable',
                        t('settings', 'unavailableInBrowser'),
                      )
                    : undefined,
                ],
              );
            }),
          ),

          m('.row', m('h4.fm-settings-section-heading.col.s12', t('settings', 'plugins'))),
          m(PluginManagement, {
            plugins: current.plugins,
            onToggle: (pluginId, enabled) => handleTogglePlugin(current, pluginId, enabled),
            onRequestLogs: current.onRequestPluginLogs,
          }),

          errors.length === 0
            ? undefined
            : m(
                'ul.fm-settings-validation-errors',
                { role: 'alert' },
                errors.map((error) => m('li', error.message)),
              ),
          saveError === undefined
            ? undefined
            : m('.fm-settings-save-error', { role: 'alert' }, saveError),
        ]),
        m('.fm-settings-editor-actions', [
          m(
            'button.fm-settings-cancel',
            { type: 'button', onclick: () => handleCancel(current) },
            t('button', 'cancel'),
          ),
          m(
            'button.fm-settings-save',
            {
              type: 'button',
              disabled: errors.length > 0 || saving,
              onclick: () => handleSave(current),
            },
            saving ? t('button', 'saving') : t('button', 'save'),
          ),
        ]),
      ];
    },
  };
};
