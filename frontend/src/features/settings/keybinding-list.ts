import { displayShortcut } from '../../app/shell-helpers';
import { actionCategoryLabel, actionTitle } from '../../i18n';
import type { LiveBinding } from '../../keybindings/dispatcher';
import type { ActionDescriptor } from '../../models';
import type { SelectionPlatform } from '../selection/keybindings';

/** Same order as the command palette's sections, so both lists read alike. */
const CATEGORY_ORDER = [
  'fileOperations',
  'navigation',
  'selection',
  'clipboard',
  'tools',
  'application',
];

export interface KeybindingRowModel {
  readonly action: ActionDescriptor;
  readonly title: string;
}

export interface KeybindingGroup {
  readonly category: string;
  readonly label: string;
  readonly rows: readonly KeybindingRowModel[];
}

function categoryRank(category: string): number {
  const index = CATEGORY_ORDER.indexOf(category);
  return index < 0 ? CATEGORY_ORDER.length : index;
}

/** Lower-cases and drops spaces so "cmd + ." and "Cmd+." compare equal. */
function squash(text: string): string {
  return text.toLowerCase().replaceAll(/\s+/gu, '');
}

/**
 * Groups the rebindable actions by category (palette order, unknown categories last) and sorts
 * each group by localized title. A non-empty `query` keeps only actions whose title, id,
 * category, current shortcut (as shown on this platform) or override text contains it.
 */
export function keybindingGroups(
  actions: readonly ActionDescriptor[],
  liveBindings: readonly LiveBinding[],
  overrides: Readonly<Record<string, string>>,
  platform: SelectionPlatform,
  query: string,
): readonly KeybindingGroup[] {
  const needle = squash(query);
  const byCategory = new Map<string, KeybindingRowModel[]>();
  for (const action of actions) {
    const title = actionTitle(action.id, action.title);
    if (needle.length > 0) {
      const shortcuts = [
        ...liveBindings
          .filter((binding) => binding.actionId === action.id)
          .map((binding) => displayShortcut(binding.shortcut, platform)),
        overrides[action.id] ?? '',
      ];
      const haystack = [title, action.id, actionCategoryLabel(action.category), ...shortcuts].map(
        squash,
      );
      if (!haystack.some((text) => text.includes(needle))) continue;
    }
    const rows = byCategory.get(action.category) ?? [];
    rows.push({ action, title });
    byCategory.set(action.category, rows);
  }
  return [...byCategory.entries()]
    .sort(
      ([left], [right]) =>
        categoryRank(left) - categoryRank(right) ||
        actionCategoryLabel(left).localeCompare(actionCategoryLabel(right)),
    )
    .map(([category, rows]) => ({
      category,
      label: actionCategoryLabel(category),
      rows: [...rows].sort((left, right) => left.title.localeCompare(right.title)),
    }));
}
