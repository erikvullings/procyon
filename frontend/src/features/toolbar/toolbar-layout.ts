/**
 * Serializable, versioned description of the window toolbar: an ordered list of registered action
 * ids (and separators), each with a label mode. Pure data, so the same layout drives the browser
 * and Tauri hosts and round-trips through the settings document unchanged.
 */

export const TOOLBAR_LAYOUT_VERSION = 1;

export type ToolbarLabelMode = 'icon' | 'icon-text';

export type ToolbarLayoutItem =
  | { readonly kind: 'action'; readonly actionId: string; readonly labels: ToolbarLabelMode }
  | { readonly kind: 'separator' };

export interface ToolbarLayout {
  readonly version: typeof TOOLBAR_LAYOUT_VERSION;
  readonly items: readonly ToolbarLayoutItem[];
}

export interface ToolbarLayoutRead {
  readonly layout: ToolbarLayout;
  /** Written by a newer build: the caller must keep the stored document rather than overwrite it. */
  readonly newerVersion: boolean;
}

const action = (actionId: string, labels: ToolbarLabelMode = 'icon'): ToolbarLayoutItem => ({
  kind: 'action',
  actionId,
  labels,
});
const separator: ToolbarLayoutItem = { kind: 'separator' };

/** The full toolbar as shipped, in its current order. Changing it changes every default install. */
export const DEFAULT_TOOLBAR_LAYOUT: ToolbarLayout = {
  version: TOOLBAR_LAYOUT_VERSION,
  items: [
    action('core.parent'),
    separator,
    action('core.findFiles'),
    action('client.searchKnowledge'),
    action('client.exploreThemes'),
    separator,
    action('client.basketAdd'),
    action('client.basketToggle'),
    separator,
    action('core.compareDirectories'),
    action('client.syncBrowsing'),
    action('core.palette'),
  ],
};

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

const parseItem = (value: unknown): ToolbarLayoutItem | undefined => {
  if (!isRecord(value)) return undefined;
  if (value['kind'] === 'separator') return separator;
  if (value['kind'] !== 'action') return undefined;
  const actionId = value['actionId'];
  const labels = value['labels'];
  if (typeof actionId !== 'string' || actionId.length === 0) return undefined;
  if (labels !== 'icon' && labels !== 'icon-text') return undefined;
  return action(actionId, labels);
};

/**
 * Reads a stored layout. Malformed documents fall back to the default; malformed items are dropped
 * and a repeated action keeps only its first placement, so a hand-edited file cannot break the bar.
 */
export const readToolbarLayout = (value: unknown): ToolbarLayoutRead => {
  if (!isRecord(value) || !Array.isArray(value['items'])) {
    return { layout: DEFAULT_TOOLBAR_LAYOUT, newerVersion: false };
  }
  const version = value['version'];
  if (typeof version === 'number' && version > TOOLBAR_LAYOUT_VERSION) {
    return { layout: DEFAULT_TOOLBAR_LAYOUT, newerVersion: true };
  }
  const seen = new Set<string>();
  const items: ToolbarLayoutItem[] = [];
  for (const raw of value['items']) {
    const item = parseItem(raw);
    if (item === undefined) continue;
    if (item.kind === 'action') {
      if (seen.has(item.actionId)) continue;
      seen.add(item.actionId);
    }
    items.push(item);
  }
  return { layout: { version: TOOLBAR_LAYOUT_VERSION, items }, newerVersion: false };
};

/** Appends an action, or inserts it at `index`. An action already placed is left where it is. */
export const addToolbarAction = (
  layout: ToolbarLayout,
  actionId: string,
  index?: number,
): ToolbarLayout => {
  if (layout.items.some((item) => item.kind === 'action' && item.actionId === actionId)) {
    return layout;
  }
  const items = [...layout.items];
  const at = Math.min(Math.max(index ?? items.length, 0), items.length);
  items.splice(at, 0, action(actionId));
  return { ...layout, items };
};

export const removeToolbarItem = (layout: ToolbarLayout, index: number): ToolbarLayout => ({
  ...layout,
  items: layout.items.filter((_, position) => position !== index),
});

export const moveToolbarItem = (layout: ToolbarLayout, from: number, to: number): ToolbarLayout => {
  if (from === to) return layout;
  const items = [...layout.items];
  const [moved] = items.splice(from, 1);
  if (moved === undefined) return layout;
  items.splice(to, 0, moved);
  return { ...layout, items };
};

/** Changes an action's label mode. Separators have no label, so they are returned untouched. */
export const setToolbarLabelMode = (
  layout: ToolbarLayout,
  index: number,
  labels: ToolbarLabelMode,
): ToolbarLayout => {
  const target = layout.items[index];
  if (target === undefined || target.kind !== 'action' || target.labels === labels) return layout;
  const items = layout.items.map((item, position) =>
    position === index ? action(target.actionId, labels) : item,
  );
  return { ...layout, items };
};

export const resetToolbarLayout = (): ToolbarLayout => DEFAULT_TOOLBAR_LAYOUT;

export const toolbarLayoutsEqual = (left: ToolbarLayout, right: ToolbarLayout): boolean =>
  left.items.length === right.items.length &&
  left.items.every((item, index) => {
    const other = right.items[index];
    if (other === undefined || other.kind !== item.kind) return false;
    if (item.kind === 'separator') return true;
    return (
      other.kind === 'action' && other.actionId === item.actionId && other.labels === item.labels
    );
  });
