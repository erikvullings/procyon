import m, { type FactoryComponent } from 'mithril';

import { t } from '../../i18n';
import type { ActionDescriptor, ActionInvocationContext, KeyChord } from '../../models';
import { availableActions, type CommandAvailabilityContext } from '../commands/availability';

export interface PaletteAction {
  readonly action: ActionDescriptor;
  readonly available: boolean;
  readonly unavailableReason?: string;
}

export interface CommandPaletteAttrs {
  readonly open: boolean;
  readonly actions: readonly ActionDescriptor[];
  readonly recency: ReadonlyMap<string, number>;
  readonly context: ActionInvocationContext;
  readonly availabilityContext: CommandAvailabilityContext;
  readonly onClose: () => void;
  readonly onInvoke: (action: ActionDescriptor, parameters?: unknown) => void;
}

interface ParameterProperty {
  readonly type?: 'string' | 'number' | 'integer' | 'boolean';
  readonly title?: string;
  readonly default?: string | number | boolean;
}

interface ParameterSchema {
  readonly type?: string;
  readonly properties?: Readonly<Record<string, ParameterProperty>>;
  readonly required?: readonly string[];
}

function fuzzyScore(value: string, query: string): number | undefined {
  let position = 0;
  let score = 0;
  for (const character of query) {
    const found = value.indexOf(character, position);
    if (found < 0) return undefined;
    score += found === position ? 3 : 1;
    position = found + 1;
  }
  return score;
}

function actionScore(action: ActionDescriptor, query: string): number | undefined {
  if (query.length === 0) return 0;
  return [action.title, action.id, action.category]
    .map((value) => fuzzyScore(value.toLowerCase(), query))
    .filter((score): score is number => score !== undefined)
    .reduce<number | undefined>(
      (best, score) => (best === undefined ? score : Math.max(best, score)),
      undefined,
    );
}

/** Filters registry actions by fuzzy title/id/category match and orders by match quality then use. */
export function filterPaletteActions(
  actions: readonly ActionDescriptor[],
  query: string,
  recency: ReadonlyMap<string, number>,
  context: CommandAvailabilityContext,
): readonly PaletteAction[] {
  const normalizedQuery = query.replaceAll(/\s+/gu, '').toLowerCase();
  return availableActions(actions, context)
    .flatMap((action) => {
      const score = actionScore(action.action, normalizedQuery);
      if (score === undefined) return [];
      return [
        {
          action: action.action,
          score,
          recency: recency.get(action.action.id) ?? 0,
          available: action.available,
          ...(action.reason === undefined ? {} : { unavailableReason: action.reason }),
        },
      ];
    })
    .sort(
      (left, right) =>
        Number(right.available) - Number(left.available) ||
        right.score - left.score ||
        right.recency - left.recency ||
        left.action.title.localeCompare(right.action.title),
    )
    .map(({ action, available, unavailableReason: reason }) =>
      reason === undefined
        ? { action, available }
        : { action, available, unavailableReason: reason },
    );
}

function formatShortcut(chord: KeyChord): string {
  return [
    chord.ctrl || chord.meta ? 'Ctrl/Cmd' : undefined,
    chord.alt ? 'Alt' : undefined,
    chord.shift ? 'Shift' : undefined,
    chord.key,
  ]
    .filter((part): part is string => part !== undefined)
    .join('+');
}

function schemaProperties(schema: unknown): readonly [string, ParameterProperty][] {
  if (typeof schema !== 'object' || schema === null) return [];
  const candidate = schema as ParameterSchema;
  return candidate.type === 'object' && candidate.properties !== undefined
    ? Object.entries(candidate.properties)
    : [];
}

/** Custom keyboard-first command palette; intentionally not a Material dialog. */
export const CommandPalette: FactoryComponent<CommandPaletteAttrs> = () => {
  let query = '';
  let activeIndex = 0;
  let previousFocus: HTMLElement | undefined;
  let parameterAction: ActionDescriptor | undefined;
  let parameterValues: Record<string, string | boolean> = {};

  function close(attrs: CommandPaletteAttrs): void {
    query = '';
    activeIndex = 0;
    parameterAction = undefined;
    attrs.onClose();
    previousFocus?.focus();
    previousFocus = undefined;
  }

  function invoke(attrs: CommandPaletteAttrs, item: PaletteAction): void {
    if (!item.available) return;
    if (schemaProperties(item.action.parameterSchema).length > 0) {
      parameterAction = item.action;
      parameterValues = {};
      return;
    }
    attrs.onInvoke(item.action);
    close(attrs);
  }

  return {
    onupdate: ({ attrs }) => {
      if (attrs.open && previousFocus === undefined)
        previousFocus = document.activeElement as HTMLElement;
    },
    view: ({ attrs }) => {
      if (!attrs.open) return undefined;
      const items = filterPaletteActions(
        attrs.actions,
        query,
        attrs.recency,
        attrs.availabilityContext,
      );
      activeIndex = Math.min(activeIndex, Math.max(items.length - 1, 0));
      const active = items[activeIndex];
      const parameterFields =
        parameterAction === undefined ? [] : schemaProperties(parameterAction.parameterSchema);
      return m('.fm-command-palette-backdrop', { onclick: () => close(attrs) }, [
        m(
          '.fm-command-palette',
          {
            role: 'dialog',
            'aria-modal': 'true',
            'aria-label': t('shell', 'commandPalette'),
            onclick: (event: MouseEvent) => event.stopPropagation(),
            onkeydown: (event: KeyboardEvent) => {
              if (event.key === 'Escape') {
                event.preventDefault();
                close(attrs);
                return;
              }
              if (event.key === 'Tab') {
                event.preventDefault();
                const palette = event.currentTarget as HTMLElement;
                const focusable = [
                  ...palette.querySelectorAll<HTMLElement>(
                    'input:not([disabled]), button:not([disabled])',
                  ),
                ];
                const currentIndex = focusable.indexOf(document.activeElement as HTMLElement);
                const nextIndex = event.shiftKey
                  ? (currentIndex - 1 + focusable.length) % focusable.length
                  : (currentIndex + 1) % focusable.length;
                focusable[nextIndex]?.focus();
                return;
              }
              if (parameterAction !== undefined) return;
              if (event.key === 'ArrowDown') {
                event.preventDefault();
                activeIndex = Math.min(activeIndex + 1, Math.max(items.length - 1, 0));
              } else if (event.key === 'ArrowUp') {
                event.preventDefault();
                activeIndex = Math.max(activeIndex - 1, 0);
              } else if (event.key === 'Enter' && active !== undefined) {
                event.preventDefault();
                invoke(attrs, active);
              }
            },
          },
          [
            parameterAction === undefined
              ? [
                  m('input.fm-command-palette-input', {
                    type: 'text',
                    autofocus: true,
                    role: 'combobox',
                    'aria-autocomplete': 'list',
                    'aria-controls': 'command-palette-results',
                    'aria-expanded': 'true',
                    'aria-activedescendant':
                      active === undefined ? undefined : `command-palette-option-${activeIndex}`,
                    placeholder: t('commandPalette', 'placeholder'),
                    value: query,
                    // autofocus alone is unreliable once the trigger button already holds focus.
                    oncreate: ({ dom }) => (dom as HTMLInputElement).focus(),
                    oninput: (event: InputEvent) => {
                      query = (event.currentTarget as HTMLInputElement).value;
                      activeIndex = 0;
                    },
                  }),
                  m(
                    '.fm-command-palette-status',
                    { role: 'status', 'aria-live': 'polite' },
                    t('commandPalette', 'commandsCount', items.length),
                  ),
                  m(
                    'ul#command-palette-results.fm-command-palette-results',
                    { role: 'listbox' },
                    items.map((item, index) =>
                      m(
                        'li',
                        {
                          id: `command-palette-option-${index}`,
                          role: 'option',
                          'aria-selected': index === activeIndex ? 'true' : 'false',
                          'aria-disabled': item.available ? undefined : 'true',
                          class: index === activeIndex ? 'fm-command-palette-active' : undefined,
                          onclick: () => invoke(attrs, item),
                        },
                        [
                          m('span', [
                            m('strong', item.action.title),
                            m('small', `${item.action.category} · ${item.action.id}`),
                          ]),
                          m('span', [
                            m('kbd', item.action.defaultShortcuts.map(formatShortcut).join(', ')),
                            item.unavailableReason === undefined
                              ? undefined
                              : m('small.fm-command-palette-unavailable', item.unavailableReason),
                          ]),
                        ],
                      ),
                    ),
                  ),
                ]
              : m(
                  'form.fm-command-palette-parameters',
                  {
                    onsubmit: (event: SubmitEvent) => {
                      event.preventDefault();
                      const action = parameterAction;
                      if (action === undefined) return;
                      const parameters = Object.fromEntries(
                        parameterFields.map(([name, property]) => [
                          name,
                          property.type === 'boolean'
                            ? parameterValues[name] === true
                            : (parameterValues[name] ?? ''),
                        ]),
                      );
                      attrs.onInvoke(action, parameters);
                      close(attrs);
                    },
                  },
                  [
                    m('h2', parameterAction.title),
                    ...parameterFields.map(([name, property]) =>
                      m('label', [
                        property.title ?? name,
                        m('input', {
                          type:
                            property.type === 'boolean'
                              ? 'checkbox'
                              : property.type === 'number' || property.type === 'integer'
                                ? 'number'
                                : 'text',
                          required: (
                            parameterAction?.parameterSchema as ParameterSchema | undefined
                          )?.required?.includes(name),
                          checked:
                            property.type === 'boolean'
                              ? parameterValues[name] === true
                              : undefined,
                          value:
                            property.type === 'boolean'
                              ? undefined
                              : (parameterValues[name] ?? property.default ?? ''),
                          oninput: (event: InputEvent) => {
                            const input = event.currentTarget as HTMLInputElement;
                            parameterValues[name] =
                              property.type === 'boolean' ? input.checked : input.value;
                          },
                        }),
                      ]),
                    ),
                    m('button', { type: 'submit' }, t('commandPalette', 'run')),
                  ],
                ),
          ],
        ),
      ]);
    },
  };
};
