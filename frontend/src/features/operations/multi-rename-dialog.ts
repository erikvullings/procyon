import m, { type FactoryComponent } from 'mithril';
import { InputCheckbox, ModalPanel, NumberInput, Select, TextInput } from 'mithril-materialized';

import { t } from '../../i18n';
import type { MultiRenamePreset } from '../../models';
import {
  type CaseTransform,
  canApplyRenamePlan,
  EMPTY_MULTI_RENAME_RULES,
  type MultiRenameRules,
  proposeRenames,
  type RenameProposal,
  type RenameTarget,
  type SequenceRule,
  validateSearchPattern,
} from './multi-rename-rules';

export interface MultiRenameDialogAttrs {
  readonly open: boolean;
  /** The current selection, in the order rules such as the sequence counter are applied. */
  readonly entries: readonly RenameTarget[];
  /** Every other name in the same directory, excluding the entries being renamed. */
  readonly existingSiblingNames: ReadonlySet<string>;
  readonly presets: readonly MultiRenamePreset[];
  readonly onPresetsChange: (presets: readonly MultiRenamePreset[]) => Promise<void>;
  readonly onApply: (renamed: readonly { id: string; newName: string }[]) => void;
  readonly onCancel: () => void;
}

/**
 * Moves focus away from the input before the modal closes, so the browser
 * never has to apply aria-hidden to an ancestor of the focused element.
 */
function blurActive(): void {
  const active = document.activeElement;
  if (active instanceof HTMLElement) active.blur();
}

function collisionLabel(proposal: RenameProposal): string | undefined {
  if (proposal.invalidNameReason !== undefined) return proposal.invalidNameReason;
  if (proposal.collision === 'plan') return t('multiRename', 'collidesPlan');
  if (proposal.collision === 'existing') return t('multiRename', 'collidesExisting');
  return undefined;
}

/** The F2 multi-selection rename dialog (task 0072), modeled on Total Commander's tool. */
export const MultiRenameDialog: FactoryComponent<MultiRenameDialogAttrs> = () => {
  let rules: MultiRenameRules = EMPTY_MULTI_RENAME_RULES;
  let wasOpen = false;
  let presetError: string | undefined;
  let selectedPresetName = '';
  let presetMutationPending = false;

  function reset(): void {
    rules = EMPTY_MULTI_RENAME_RULES;
    presetError = undefined;
    selectedPresetName = '';
  }

  function savePreset(attrs: MultiRenameDialogAttrs): void {
    if (presetMutationPending) return;
    const name = window.prompt(t('multiRename', 'presetNamePrompt'))?.trim();
    if (!name) return;
    const existingIndex = attrs.presets.findIndex((preset) => preset.name === name);
    if (
      existingIndex >= 0 &&
      !window.confirm(t('multiRename', 'overwritePresetPrompt', { name }))
    ) {
      return;
    }
    const preset: MultiRenamePreset = {
      name,
      rules: { ...rules, sequence: { ...rules.sequence } },
    };
    const presets =
      existingIndex < 0
        ? [...attrs.presets, preset]
        : attrs.presets.map((current, index) => (index === existingIndex ? preset : current));
    presetMutationPending = true;
    void attrs
      .onPresetsChange(presets)
      .then(() => {
        selectedPresetName = name;
        presetError = undefined;
      })
      .catch(() => {
        presetError = t('multiRename', 'savePresetFailed');
      })
      .finally(() => {
        presetMutationPending = false;
        m.redraw();
      });
  }

  function deletePreset(attrs: MultiRenameDialogAttrs): void {
    if (selectedPresetName.length === 0 || presetMutationPending) return;
    const presets = attrs.presets.filter((preset) => preset.name !== selectedPresetName);
    presetMutationPending = true;
    void attrs
      .onPresetsChange(presets)
      .then(() => {
        selectedPresetName = '';
        presetError = undefined;
      })
      .catch(() => {
        presetError = t('multiRename', 'savePresetFailed');
      })
      .finally(() => {
        presetMutationPending = false;
        m.redraw();
      });
  }

  function update(patch: Partial<MultiRenameRules>): void {
    rules = { ...rules, ...patch };
  }

  function updateSequence(patch: Partial<SequenceRule>): void {
    update({ sequence: { ...rules.sequence, ...patch } });
  }

  function hasPersistableSequence(): boolean {
    return (
      Number.isSafeInteger(rules.sequence.start) &&
      Number.isSafeInteger(rules.sequence.step) &&
      Number.isSafeInteger(rules.sequence.padding) &&
      rules.sequence.padding >= 0 &&
      rules.sequence.padding <= 4_294_967_295
    );
  }

  function cancel(attrs: MultiRenameDialogAttrs): void {
    blurActive();
    attrs.onCancel();
  }

  function apply(attrs: MultiRenameDialogAttrs, plan: readonly RenameProposal[]): void {
    if (!canApplyRenamePlan(plan)) return;
    blurActive();
    attrs.onApply(
      plan
        .filter((proposal) => proposal.changed)
        .map((proposal) => ({ id: proposal.id, newName: proposal.newName })),
    );
  }

  return {
    view: ({ attrs }) => {
      // ModalPanel keeps this component permanently mounted and only toggles CSS visibility, so
      // reset state on the false->true open transition here (synchronously, before rendering)
      // rather than in onupdate, which would only take effect on a subsequent redraw.
      if (attrs.open && !wasOpen) reset();
      wasOpen = attrs.open;

      const searchError = validateSearchPattern(rules.search, rules.useRegex);
      const plan =
        searchError === undefined
          ? proposeRenames(attrs.entries, rules, attrs.existingSiblingNames)
          : attrs.entries.map((entry) => ({
              id: entry.id,
              oldName: entry.name,
              newName: entry.name,
              changed: false,
            }));
      const canApply = searchError === undefined && canApplyRenamePlan(plan);

      return m(ModalPanel, {
        id: 'multi-rename-dialog',
        title: t('multiRename', 'title', { count: attrs.entries.length }),
        className: 'fm-multi-rename-modal',
        fixedFooter: true,
        description: m('.fm-multi-rename-body', [
          m('.fm-multi-rename-presets', [
            m('label', { for: 'multi-rename-preset' }, t('multiRename', 'preset')),
            m(
              'select#multi-rename-preset.browser-default',
              {
                value: selectedPresetName,
                onchange: (event: Event) => {
                  const name = (event.currentTarget as HTMLSelectElement).value;
                  selectedPresetName = name;
                  const preset = attrs.presets.find((candidate) => candidate.name === name);
                  if (preset !== undefined) {
                    rules = { ...preset.rules, sequence: { ...preset.rules.sequence } };
                  }
                },
              },
              [
                m('option', { key: '', value: '' }, t('multiRename', 'choosePreset')),
                ...attrs.presets.map((preset) =>
                  m('option', { key: preset.name, value: preset.name }, preset.name),
                ),
              ],
            ),
            m(
              'button.btn-flat',
              {
                type: 'button',
                disabled: selectedPresetName.length === 0 || presetMutationPending,
                onclick: () => deletePreset(attrs),
              },
              t('multiRename', 'deletePreset'),
            ),
          ]),
          m('.ignored-fm-multi-rename-rules', [
            m(
              'div',
              {
                style: {
                  border: '1px solid var(--fm-border)',
                  borderRadius: '4px',
                  paddingTop: '8px',
                },
              },
              [
                m(
                  '.row',
                  { style: { marginBottom: '8px' } },
                  m(TextInput, {
                    id: 'multi-rename-name-mask',
                    label: t('multiRename', 'nameMask'),
                    helperText: t('multiRename', 'nameMaskHelp'),
                    className: 'col s8',
                    value: rules.nameMask,
                    oninput: (nameMask) => {
                      update({ nameMask });
                    },
                  }),
                  m(TextInput, {
                    id: 'multi-rename-extension-mask',
                    label: t('multiRename', 'extension'),
                    helperText: t('multiRename', 'extensionHelp'),
                    className: 'col s4',
                    value: rules.extensionMask,
                    oninput: (extensionMask) => {
                      update({ extensionMask });
                    },
                  }),

                  m(NumberInput, {
                    id: 'multi-rename-sequence-start',
                    label: t('multiRename', 'counterStart'),
                    className: 'col s4',
                    value: rules.sequence.start,
                    step: 1,
                    oninput: (start) => {
                      updateSequence({ start });
                    },
                  }),
                  m(NumberInput, {
                    id: 'multi-rename-sequence-step',
                    label: t('multiRename', 'stepBy'),
                    className: 'col s4',
                    value: rules.sequence.step,
                    step: 1,
                    oninput: (step) => {
                      updateSequence({ step });
                    },
                  }),
                  m(NumberInput, {
                    id: 'multi-rename-sequence-padding',
                    label: t('multiRename', 'digits'),
                    className: 'col s4',
                    value: rules.sequence.padding,
                    min: 0,
                    max: 4_294_967_295,
                    step: 1,
                    oninput: (padding) => {
                      updateSequence({ padding });
                    },
                  }),
                ),
              ],
            ),
            m(
              'div',
              {
                style: {
                  border: '1px solid var(--fm-border)',
                  borderRadius: '4px',
                  paddingTop: '8px',
                  marginTop: '12px',
                },
              },
              m(
                '.row',
                { style: { marginBottom: '8px' } },
                m(TextInput, {
                  id: 'multi-rename-search',
                  label: t('multiRename', 'searchFor'),
                  className: 'col s4',
                  value: rules.search,
                  oninput: (search) => {
                    update({ search });
                  },
                }),
                m(TextInput, {
                  id: 'multi-rename-replace',
                  label: t('multiRename', 'replaceWith'),
                  className: 'col s4',
                  value: rules.replace,
                  oninput: (replace) => {
                    update({ replace });
                  },
                }),
                m(InputCheckbox, {
                  inputId: 'multi-rename-use-regex',
                  label: t('multiRename', 'useRegex'),
                  className: 'col s4',
                  style: { marginTop: '16px' },
                  checked: rules.useRegex,
                  onchange: (useRegex) => {
                    update({ useRegex });
                  },
                }),
              ),
              searchError === undefined ? undefined : m('.fm-field-error', searchError),
            ),

            m(
              '.row',
              {
                style: {
                  border: '1px solid var(--fm-border)',
                  borderRadius: '4px',
                  padding: '8px 0',
                  margin: '12px auto 8px auto',
                },
              },
              m(Select, {
                id: 'multi-rename-case',
                label: t('multiRename', 'case'),
                options: [
                  { id: 'unchanged', label: t('multiRename', 'caseUnchanged') },
                  { id: 'upper', label: t('multiRename', 'caseUpper') },
                  { id: 'lower', label: t('multiRename', 'caseLower') },
                  { id: 'title', label: t('multiRename', 'caseTitle') },
                ],
                checkedId: rules.caseTransform,
                onchange: (v) => update({ caseTransform: v[0] as CaseTransform }),
              }),
            ),
          ]),
          m('.fm-multi-rename-preview-container', [
            m('table.fm-multi-rename-preview', [
              m(
                'thead',
                m('tr', [
                  m('th', t('multiRename', 'oldName')),
                  m('th', t('multiRename', 'newName')),
                ]),
              ),
              m(
                'tbody',
                plan.map((proposal) => {
                  const problem = collisionLabel(proposal);
                  return m(
                    'tr',
                    {
                      key: proposal.id,
                      className: problem === undefined ? undefined : 'fm-multi-rename-row--problem',
                    },
                    [
                      m('td', proposal.oldName),
                      m('td', [
                        proposal.newName,
                        problem === undefined ? undefined : m('.fm-field-error', problem),
                      ]),
                    ],
                  );
                }),
              ),
            ]),
          ]),
          presetError === undefined ? undefined : m('.fm-field-error', presetError),
        ]),
        isOpen: attrs.open,
        closeOnEsc: true,
        onToggle: (open: boolean) => {
          if (!open) cancel(attrs);
        },
        buttons: [
          { label: t('button', 'cancel'), onclick: () => cancel(attrs) },
          {
            label: t('multiRename', 'saveAsPreset'),
            disabled: presetMutationPending || !hasPersistableSequence(),
            onclick: () => savePreset(attrs),
          },
          {
            label: t('multiRename', 'rename'),
            disabled: !canApply,
            onclick: () => apply(attrs, plan),
          },
        ],
      });
    },
  };
};
