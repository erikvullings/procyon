import m, { type FactoryComponent } from 'mithril';
import { technicalTextInputAttrs } from '../../a11y/technical-text-input';
import { t } from '../../i18n';
import type { FilterKind, FilterPreset, FilterToggle } from '../../models/settings';
import {
  type ActiveCriterionId,
  type AdvancedFilterCriteria,
  type AdvancedFilterNotice,
  activeCriteriaIds,
  formatFilterSize,
  parseFilterSize,
} from './advanced-filter';
import {
  deleteFilterPreset,
  findFilterPreset,
  type PresetEditResult,
  renameFilterPreset,
  saveFilterPreset,
} from './filter-presets';
import './advanced-filter-bar.css';

/** Advanced-filter state and callbacks a pane receives (task 0169). */
export interface AdvancedFilterAttrs {
  readonly criteria: AdvancedFilterCriteria;
  readonly presets: readonly FilterPreset[];
  readonly panelOpen: boolean;
  readonly notices: readonly AdvancedFilterNotice[];
  readonly onChange: (criteria: AdvancedFilterCriteria) => void;
  /** Resets every criterion, the quick-filter text and the panel in one action. */
  readonly onClear: () => void;
  readonly onTogglePanel: () => void;
  readonly onPresetsChange: (presets: readonly FilterPreset[]) => Promise<void>;
}

export interface AdvancedFilterBarAttrs extends Omit<AdvancedFilterAttrs, 'onTogglePanel'> {
  /** Overridable for tests; default to the window dialogs. */
  readonly prompt?: (message: string, initial?: string) => string | null;
  readonly confirm?: (message: string) => boolean;
}

const KINDS: readonly FilterKind[] = ['any', 'file', 'directory', 'symlink'];
const TOGGLES: readonly FilterToggle[] = ['any', 'only', 'exclude'];

function kindLabel(kind: FilterKind): string {
  switch (kind) {
    case 'any':
      return t('advancedFilter', 'kindAny');
    case 'file':
      return t('advancedFilter', 'kindFile');
    case 'directory':
      return t('advancedFilter', 'kindDirectory');
    case 'symlink':
      return t('advancedFilter', 'kindSymlink');
  }
}

function toggleLabel(toggle: FilterToggle): string {
  switch (toggle) {
    case 'any':
      return t('advancedFilter', 'toggleAny');
    case 'only':
      return t('advancedFilter', 'toggleOnly');
    case 'exclude':
      return t('advancedFilter', 'toggleExclude');
  }
}

function noticeText(notice: AdvancedFilterNotice): string {
  switch (notice) {
    case 'sizeUnavailable':
      return t('advancedFilter', 'noticeSizeUnavailable');
    case 'modifiedUnavailable':
      return t('advancedFilter', 'noticeModifiedUnavailable');
    case 'noHiddenLoaded':
      return t('advancedFilter', 'noticeNoHiddenLoaded');
    case 'executableByExtension':
      return t('advancedFilter', 'noticeExecutableByExtension');
    case 'loadedEntriesOnly':
      return t('advancedFilter', 'noticeLoadedEntriesOnly');
  }
}

/** Human-readable text of one active criterion, shown as a chip. */
export function criterionChipText(id: ActiveCriterionId, criteria: AdvancedFilterCriteria): string {
  switch (id) {
    case 'name':
      return t('advancedFilter', 'chipName', { value: criteria.name.trim() });
    case 'kind':
      return t('advancedFilter', 'chipKind', { value: kindLabel(criteria.kind) });
    case 'minSize':
      return t('advancedFilter', 'chipMinSize', { value: formatFilterSize(criteria.minSize ?? 0) });
    case 'maxSize':
      return t('advancedFilter', 'chipMaxSize', { value: formatFilterSize(criteria.maxSize ?? 0) });
    case 'modifiedWithinDays':
      return t('advancedFilter', 'chipWithin', { value: String(criteria.modifiedWithinDays ?? 0) });
    case 'modifiedOlderThanDays':
      return t('advancedFilter', 'chipOlder', {
        value: String(criteria.modifiedOlderThanDays ?? 0),
      });
    case 'hidden':
      return t('advancedFilter', 'chipHidden', { value: toggleLabel(criteria.hidden) });
    case 'executable':
      return t('advancedFilter', 'chipExecutable', { value: toggleLabel(criteria.executable) });
  }
}

function parseDays(text: string): number | null {
  const trimmed = text.trim();
  if (!/^\d+$/u.test(trimmed)) return null;
  const days = Number(trimmed);
  return Number.isSafeInteger(days) ? days : null;
}

/**
 * Active-criteria chips with a one-action clear, plus (when open) the structured filter form and
 * the saved-preset controls. Presentation only: state lives with the owning pane.
 */
export const AdvancedFilterBar: FactoryComponent<AdvancedFilterBarAttrs> = () => {
  let selectedPreset = '';
  let presetError: string | undefined;
  let presetPending = false;
  let sizeDrafts: { min?: string; max?: string } = {};
  let lastSeen: AdvancedFilterCriteria | undefined;

  const field = (id: string, label: string, control: m.Children) =>
    m('.fm-advanced-filter-field', [m('label', { for: id }, label), control]);

  function runPresetEdit(
    attrs: AdvancedFilterBarAttrs,
    result: PresetEditResult,
    failure: (reason: 'blank' | 'missing' | 'duplicate') => string | undefined,
  ): void {
    if (!result.ok) {
      presetError = failure(result.reason);
      return;
    }
    const name = result.name;
    presetPending = true;
    void attrs
      .onPresetsChange(result.presets)
      .then(() => {
        selectedPreset = name;
        presetError = undefined;
      })
      .catch(() => {
        presetError = t('advancedFilter', 'saveFailed');
      })
      .finally(() => {
        presetPending = false;
        m.redraw();
      });
  }

  function savePreset(attrs: AdvancedFilterBarAttrs): void {
    if (presetPending) return;
    const ask = attrs.prompt ?? ((message, initial) => window.prompt(message, initial));
    const ok = attrs.confirm ?? ((message) => window.confirm(message));
    const raw = ask(t('advancedFilter', 'presetNamePrompt'), selectedPreset);
    if (raw === null) return;
    const result = saveFilterPreset(attrs.presets, raw, attrs.criteria);
    if (!result.ok) {
      presetError = undefined;
      return;
    }
    if (
      findFilterPreset(attrs.presets, result.name) !== undefined &&
      !ok(t('advancedFilter', 'overwritePrompt', { name: result.name }))
    ) {
      return;
    }
    runPresetEdit(attrs, result, () => undefined);
  }

  function renamePreset(attrs: AdvancedFilterBarAttrs): void {
    if (presetPending || selectedPreset === '') return;
    const ask = attrs.prompt ?? ((message, initial) => window.prompt(message, initial));
    const raw = ask(t('advancedFilter', 'renamePrompt', { name: selectedPreset }), selectedPreset);
    if (raw === null) return;
    runPresetEdit(attrs, renameFilterPreset(attrs.presets, selectedPreset, raw), (reason) =>
      reason === 'duplicate'
        ? t('advancedFilter', 'duplicateName', { name: raw.trim() })
        : undefined,
    );
  }

  function deletePreset(attrs: AdvancedFilterBarAttrs): void {
    if (presetPending || selectedPreset === '') return;
    const presets = deleteFilterPreset(attrs.presets, selectedPreset);
    presetPending = true;
    void attrs
      .onPresetsChange(presets)
      .then(() => {
        selectedPreset = '';
        presetError = undefined;
      })
      .catch(() => {
        presetError = t('advancedFilter', 'saveFailed');
      })
      .finally(() => {
        presetPending = false;
        m.redraw();
      });
  }

  return {
    view: ({ attrs }) => {
      if (lastSeen !== attrs.criteria) {
        sizeDrafts = {};
        lastSeen = attrs.criteria;
      }
      const emit = (patch: Partial<AdvancedFilterCriteria>): void => {
        const next = { ...attrs.criteria, ...patch };
        lastSeen = next;
        attrs.onChange(next);
      };
      const active = activeCriteriaIds(attrs.criteria);
      if (active.length === 0 && !attrs.panelOpen) return null;

      const sizeField = (which: 'min' | 'max') => {
        const stored = which === 'min' ? attrs.criteria.minSize : attrs.criteria.maxSize;
        const draft = sizeDrafts[which];
        const value = draft ?? (stored === null ? '' : formatFilterSize(stored));
        const invalid =
          draft !== undefined && draft.trim() !== '' && parseFilterSize(draft) === undefined;
        const id = `fm-advanced-filter-${which}-size`;
        return field(
          id,
          t('advancedFilter', which === 'min' ? 'minSize' : 'maxSize'),
          m('input', {
            ...technicalTextInputAttrs,
            id,
            type: 'text',
            value,
            placeholder: t('advancedFilter', 'sizePlaceholder'),
            'aria-invalid': invalid ? 'true' : undefined,
            title: invalid ? t('advancedFilter', 'sizeInvalid') : undefined,
            oninput: (event: InputEvent) => {
              const text = (event.currentTarget as HTMLInputElement).value;
              sizeDrafts = { ...sizeDrafts, [which]: text };
              if (text.trim() === '') {
                emit(which === 'min' ? { minSize: null } : { maxSize: null });
                return;
              }
              const bytes = parseFilterSize(text);
              if (bytes !== undefined)
                emit(which === 'min' ? { minSize: bytes } : { maxSize: bytes });
            },
          }),
        );
      };

      const dayField = (
        id: string,
        label: string,
        value: number | null,
        key: 'modifiedWithinDays' | 'modifiedOlderThanDays',
      ) =>
        field(
          id,
          label,
          m('input', {
            id,
            type: 'number',
            min: 0,
            step: 1,
            value: value === null ? '' : String(value),
            oninput: (event: InputEvent) =>
              emit({ [key]: parseDays((event.currentTarget as HTMLInputElement).value) }),
          }),
        );

      const toggleField = (
        id: string,
        label: string,
        value: FilterToggle,
        key: 'hidden' | 'executable',
      ) =>
        field(
          id,
          label,
          m(
            'select.browser-default',
            {
              id,
              value,
              onchange: (event: Event) =>
                emit({ [key]: (event.currentTarget as HTMLSelectElement).value as FilterToggle }),
            },
            TOGGLES.map((toggle) =>
              m('option', { value: toggle, selected: toggle === value }, toggleLabel(toggle)),
            ),
          ),
        );

      return m(
        '.fm-advanced-filter',
        {
          role: 'group',
          'aria-label': t('advancedFilter', 'panelLabel'),
          onkeydown: (event: KeyboardEvent) => {
            if (event.key === 'Escape') {
              event.stopPropagation();
              event.preventDefault();
              attrs.onClear();
            }
          },
        },
        [
          active.length === 0
            ? undefined
            : m('.fm-advanced-filter-chips', [
                m(
                  'ul',
                  { 'aria-label': t('advancedFilter', 'activeCriteria') },
                  active.map((id) =>
                    m(
                      'li.fm-advanced-filter-chip',
                      { key: id },
                      criterionChipText(id, attrs.criteria),
                    ),
                  ),
                ),
                m(
                  'button.fm-advanced-filter-clear',
                  { type: 'button', onclick: () => attrs.onClear() },
                  t('advancedFilter', 'clearAll'),
                ),
              ]),
          attrs.panelOpen
            ? m('.fm-advanced-filter-form', [
                field(
                  'fm-advanced-filter-name',
                  t('advancedFilter', 'name'),
                  m('input', {
                    ...technicalTextInputAttrs,
                    id: 'fm-advanced-filter-name',
                    type: 'text',
                    value: attrs.criteria.name,
                    placeholder: t('advancedFilter', 'namePlaceholder'),
                    oninput: (event: InputEvent) =>
                      emit({ name: (event.currentTarget as HTMLInputElement).value }),
                  }),
                ),
                field(
                  'fm-advanced-filter-kind',
                  t('advancedFilter', 'kind'),
                  m(
                    'select.browser-default',
                    {
                      id: 'fm-advanced-filter-kind',
                      value: attrs.criteria.kind,
                      onchange: (event: Event) =>
                        emit({
                          kind: (event.currentTarget as HTMLSelectElement).value as FilterKind,
                        }),
                    },
                    KINDS.map((kind) =>
                      m(
                        'option',
                        { value: kind, selected: kind === attrs.criteria.kind },
                        kindLabel(kind),
                      ),
                    ),
                  ),
                ),
                sizeField('min'),
                sizeField('max'),
                dayField(
                  'fm-advanced-filter-within',
                  t('advancedFilter', 'modifiedWithin'),
                  attrs.criteria.modifiedWithinDays,
                  'modifiedWithinDays',
                ),
                dayField(
                  'fm-advanced-filter-older',
                  t('advancedFilter', 'modifiedOlder'),
                  attrs.criteria.modifiedOlderThanDays,
                  'modifiedOlderThanDays',
                ),
                toggleField(
                  'fm-advanced-filter-hidden',
                  t('advancedFilter', 'hidden'),
                  attrs.criteria.hidden,
                  'hidden',
                ),
                toggleField(
                  'fm-advanced-filter-executable',
                  t('advancedFilter', 'executable'),
                  attrs.criteria.executable,
                  'executable',
                ),
                m('.fm-advanced-filter-presets', [
                  field(
                    'fm-advanced-filter-preset',
                    t('advancedFilter', 'presetsLabel'),
                    m(
                      'select.browser-default',
                      {
                        id: 'fm-advanced-filter-preset',
                        value: selectedPreset,
                        onchange: (event: Event) => {
                          selectedPreset = (event.currentTarget as HTMLSelectElement).value;
                          const preset = findFilterPreset(attrs.presets, selectedPreset);
                          if (preset !== undefined) {
                            lastSeen = undefined;
                            attrs.onChange({ ...preset.criteria });
                          }
                        },
                      },
                      [
                        m('option', { value: '' }, t('advancedFilter', 'presetNone')),
                        ...attrs.presets.map((preset) =>
                          m(
                            'option',
                            { value: preset.name, selected: preset.name === selectedPreset },
                            preset.name,
                          ),
                        ),
                      ],
                    ),
                  ),
                  m(
                    'button',
                    {
                      type: 'button',
                      disabled: presetPending || active.length === 0,
                      onclick: () => savePreset(attrs),
                    },
                    t('advancedFilter', 'savePreset'),
                  ),
                  m(
                    'button',
                    {
                      type: 'button',
                      disabled: presetPending || selectedPreset === '',
                      onclick: () => renamePreset(attrs),
                    },
                    t('advancedFilter', 'renamePreset'),
                  ),
                  m(
                    'button',
                    {
                      type: 'button',
                      disabled: presetPending || selectedPreset === '',
                      onclick: () => deletePreset(attrs),
                    },
                    t('advancedFilter', 'deletePreset'),
                  ),
                ]),
              ])
            : undefined,
          presetError === undefined
            ? undefined
            : m('.fm-advanced-filter-error', { role: 'alert' }, presetError),
          attrs.notices.length === 0
            ? undefined
            : m(
                'ul.fm-advanced-filter-notices',
                { role: 'status' },
                attrs.notices.map((notice) => m('li', { key: notice }, noticeText(notice))),
              ),
        ],
      );
    },
  };
};
