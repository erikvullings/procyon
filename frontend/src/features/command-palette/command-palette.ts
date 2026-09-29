import m, { type FactoryComponent } from 'mithril';
import {
  type Command,
  Dialog,
  CommandPalette as MaterializedCommandPaletteFactory,
} from 'mithril-materialized';

import { actionCategoryLabel, t } from '../../i18n';
import type { ActionDescriptor, ActionInvocationContext } from '../../models';
import { availableActions, type CommandAvailabilityContext } from '../commands/availability';
import { formatShortcut } from '../commands/shortcut-label';
import type { SelectionPlatform } from '../selection/keybindings';

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
  readonly platform?: SelectionPlatform;
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

export interface PaletteEntry extends PaletteAction {
  /** Section header; only set while browsing so headers never repeat in ranked results. */
  readonly group?: string;
  readonly detail?: string;
}

const RECENT_LIMIT = 5;
const CATEGORY_ORDER = [
  'fileOperations',
  'navigation',
  'selection',
  'clipboard',
  'tools',
  'application',
];

function categoryRank(category: string): number {
  const index = CATEGORY_ORDER.indexOf(category);
  return index < 0 ? CATEGORY_ORDER.length : index;
}

/**
 * Arranges palette rows. Browsing (empty query) shows only runnable commands: recently used
 * first, then one section per category. Typing switches to a flat ranked list that also reveals
 * unavailable matches with their reason, so nothing is hidden from a deliberate search.
 */
export function arrangePaletteEntries(
  actions: readonly ActionDescriptor[],
  query: string,
  recency: ReadonlyMap<string, number>,
  context: CommandAvailabilityContext,
): readonly PaletteEntry[] {
  const matches = filterPaletteActions(actions, query, recency, context);
  if (query.trim().length > 0) {
    return matches.map((item) => {
      const category = actionCategoryLabel(item.action.category);
      return {
        ...item,
        detail:
          item.unavailableReason === undefined
            ? category
            : `${category} · ${item.unavailableReason}`,
      };
    });
  }
  const runnable = matches.filter((item) => item.available);
  const recent = runnable
    .filter((item) => (recency.get(item.action.id) ?? 0) > 0)
    .sort((left, right) => (recency.get(right.action.id) ?? 0) - (recency.get(left.action.id) ?? 0))
    .slice(0, RECENT_LIMIT);
  const recentIds = new Set(recent.map((item) => item.action.id));
  const rest = runnable
    .filter((item) => !recentIds.has(item.action.id))
    .sort(
      (left, right) =>
        categoryRank(left.action.category) - categoryRank(right.action.category) ||
        actionCategoryLabel(left.action.category).localeCompare(
          actionCategoryLabel(right.action.category),
        ) ||
        left.action.title.localeCompare(right.action.title),
    );
  return [
    ...recent.map((item) => ({ ...item, group: t('commandCategory', 'recent') })),
    ...rest.map((item) => ({ ...item, group: actionCategoryLabel(item.action.category) })),
  ];
}

function schemaProperties(schema: unknown): readonly [string, ParameterProperty][] {
  if (typeof schema !== 'object' || schema === null) return [];
  const candidate = schema as ParameterSchema;
  return candidate.type === 'object' && candidate.properties !== undefined
    ? Object.entries(candidate.properties)
    : [];
}

export { formatShortcut };

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
      const toCommand = (item: PaletteEntry): Command<string> => ({
        id: item.action.id,
        label: item.action.title,
        ...(item.detail === undefined ? {} : { description: item.detail }),
        ...(item.group === undefined ? {} : { group: item.group }),
        shortcut: item.action.defaultShortcuts
          .map((chord) => formatShortcut(chord, attrs.platform))
          .join(', '),
        disabled: !item.available,
        execute: () => {
          if (schemaProperties(item.action.parameterSchema).length > 0) {
            beginParameterEntry(item.action);
          } else {
            attrs.onInvoke(item.action);
          }
        },
      });
      const commands = arrangePaletteEntries(
        attrs.actions,
        '',
        attrs.recency,
        attrs.availabilityContext,
      ).map(toCommand);
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
                arrangePaletteEntries(
                  attrs.actions,
                  query,
                  attrs.recency,
                  attrs.availabilityContext,
                ).map(toCommand),
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
