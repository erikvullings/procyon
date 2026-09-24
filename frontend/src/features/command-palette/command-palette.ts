import m, { type FactoryComponent } from 'mithril';
import {
  type Command,
  Dialog,
  CommandPalette as MaterializedCommandPaletteFactory,
} from 'mithril-materialized';

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

const MaterializedCommandPalette = MaterializedCommandPaletteFactory<string>();

/** Adapts Procyon's action registry to mithril-materialized's accessible command palette. */
export const CommandPalette: FactoryComponent<CommandPaletteAttrs> = () => {
  let parameterAction: ActionDescriptor | undefined;
  let parameterValues: Record<string, string | number | boolean> = {};
  let previousFocus: HTMLElement | undefined;

  const beginParameterEntry = (action: ActionDescriptor): void => {
    parameterAction = action;
    parameterValues = Object.fromEntries(
      schemaProperties(action.parameterSchema).flatMap(([name, property]) =>
        property.default === undefined ? [] : [[name, property.default] as const],
      ),
    );
  };

  const close = (attrs: CommandPaletteAttrs): void => {
    const focusTarget = previousFocus;
    parameterAction = undefined;
    parameterValues = {};
    previousFocus = undefined;
    attrs.onClose();
    focusTarget?.focus();
  };

  const submitParameters = (attrs: CommandPaletteAttrs): void => {
    const action = parameterAction;
    if (action === undefined) return;
    const parameters = Object.fromEntries(
      schemaProperties(action.parameterSchema).map(([name, property]) => [
        name,
        property.type === 'boolean'
          ? parameterValues[name] === true
          : (parameterValues[name] ?? ''),
      ]),
    );
    attrs.onInvoke(action, parameters);
    close(attrs);
  };

  return {
    view: ({ attrs }) => {
      if (attrs.open && previousFocus === undefined) {
        previousFocus = document.activeElement as HTMLElement;
      }
      const paletteActions = filterPaletteActions(
        attrs.actions,
        '',
        attrs.recency,
        attrs.availabilityContext,
      );
      const commands: readonly Command<string>[] = paletteActions.map((item) => ({
        id: item.action.id,
        label: item.action.title,
        description:
          item.unavailableReason === undefined
            ? item.action.id
            : `${item.action.id} · ${item.unavailableReason}`,
        group: item.action.category,
        shortcut: item.action.defaultShortcuts.map(formatShortcut).join(', '),
        disabled: !item.available,
        execute: () => {
          if (schemaProperties(item.action.parameterSchema).length > 0) {
            beginParameterEntry(item.action);
          } else {
            attrs.onInvoke(item.action);
          }
        },
      }));
      const commandsById = new Map(commands.map((command) => [command.id, command]));
      const parameterFields =
        parameterAction === undefined ? [] : schemaProperties(parameterAction.parameterSchema);

      return [
        attrs.open && parameterAction === undefined
          ? m(MaterializedCommandPalette, {
              className: 'fm-command-palette',
              title: t('shell', 'commandPalette'),
              placeholder: t('commandPalette', 'placeholder'),
              emptyText: t('commandPalette', 'commandsCount', 0),
              noResultsText: t('commandPalette', 'commandsCount', 0),
              isOpen: true,
              commands,
              filterCommands: (_commands, query) =>
                filterPaletteActions(
                  attrs.actions,
                  query,
                  attrs.recency,
                  attrs.availabilityContext,
                ).flatMap(({ action }) => {
                  const command = commandsById.get(action.id);
                  return command === undefined ? [] : [command];
                }),
              onClose: (reason) => {
                if (reason !== 'execution' || parameterAction === undefined) close(attrs);
              },
            })
          : undefined,
        parameterAction === undefined
          ? undefined
          : m(Dialog, {
              className: 'fm-command-palette-parameters',
              title: parameterAction.title,
              isOpen: attrs.open,
              showCloseButton: false,
              closeOnButtonClick: false,
              initialFocus: '.fm-command-palette-parameter-form input',
              onToggle: (open: boolean) => {
                if (!open) close(attrs);
              },
              content: m(
                'form.fm-command-palette-parameter-form',
                {
                  onsubmit: (event: SubmitEvent) => {
                    event.preventDefault();
                    submitParameters(attrs);
                  },
                },
                parameterFields.map(([name, property]) =>
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
                        property.type === 'boolean' ? parameterValues[name] === true : undefined,
                      value:
                        property.type === 'boolean' ? undefined : (parameterValues[name] ?? ''),
                      oninput: (event: InputEvent) => {
                        const input = event.currentTarget as HTMLInputElement;
                        parameterValues[name] =
                          property.type === 'boolean' ? input.checked : input.value;
                      },
                    }),
                  ]),
                ),
              ),
              secondaryAction: {
                label: t('button', 'cancel'),
                onclick: () => close(attrs),
              },
              primaryAction: {
                label: t('commandPalette', 'run'),
                onclick: () => submitParameters(attrs),
              },
            }),
      ];
    },
  };
};
