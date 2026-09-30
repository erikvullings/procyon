import { t } from '../../i18n';
import type { EntrySummary, Location } from '../../models';

export type BasketStatus = 'ready' | 'missing' | 'moved' | 'stale';

export interface BasketItem {
  readonly key: string;
  readonly id: string;
  readonly location: Location;
  readonly name: string;
  readonly kind: EntrySummary['kind'];
  readonly size?: number;
  readonly modifiedAt?: string;
  readonly metadataRevision: number;
  readonly status: BasketStatus;
}

export interface BasketState {
  readonly items: readonly BasketItem[];
  readonly selectedKeys: readonly string[];
  readonly persist: boolean;
}

export const emptyBasket: BasketState = { items: [], selectedKeys: [], persist: false };

export function basketKey(id: string, location: Location): string {
  return `${location.providerId}\0${id}`;
}

function basketPath(item: BasketItem): { prefix: string; pathname: string } {
  const url = new URL(item.location.uri);
  return {
    prefix: `${item.location.providerId}\0${url.protocol}\0${url.host}\0`,
    pathname: url.pathname.replace(/\/+$/, '') || '/',
  };
}

function basketAncestor(
  item: BasketItem,
  paths: ReadonlyMap<string, BasketItem>,
  onlyFolders?: ReadonlyMap<string, number>,
): BasketItem | undefined {
  const { prefix, pathname } = basketPath(item);
  if (pathname === '/') return undefined;
  let slash = pathname.lastIndexOf('/');
  while (slash >= 0) {
    const parent = paths.get(prefix + (pathname.slice(0, slash) || '/'));
    if (
      parent?.kind === 'directory' &&
      (onlyFolders === undefined || onlyFolders.has(parent.key))
    ) {
      return parent;
    }
    if (slash === 0) break;
    slash = pathname.lastIndexOf('/', slash - 1);
  }
  return undefined;
}

export function findBasketOverlap(
  items: readonly BasketItem[],
): readonly [BasketItem, BasketItem] | undefined {
  const paths = new Map<string, BasketItem>();
  for (const item of items) {
    const { prefix, pathname } = basketPath(item);
    const path = prefix + pathname;
    const duplicate = paths.get(path);
    if (duplicate !== undefined) return [duplicate, item];
    paths.set(path, item);
  }
  for (const item of items) {
    const parent = basketAncestor(item, paths);
    if (parent !== undefined) return [parent, item];
  }
  return undefined;
}

function safeLocation(location: Location): boolean {
  try {
    const url = new URL(location.uri);
    return !url.username && !url.password && !url.search && !url.hash;
  } catch {
    return false;
  }
}

export function addToBasket(state: BasketState, entries: readonly EntrySummary[]): BasketState {
  const items = new Map(state.items.map((item) => [item.key, item]));
  const newFolders = new Map<string, number>();
  const incomingOrder = new Map<string, number>();
  for (const [index, entry] of entries.entries()) {
    if (!safeLocation(entry.location)) {
      throw new Error(t('basket', 'credentials'));
    }
    const key = basketKey(entry.id, entry.location);
    if (entry.kind === 'directory' && !items.has(key)) newFolders.set(key, index);
    incomingOrder.set(key, index);
    items.set(key, {
      key,
      id: entry.id,
      location: entry.location,
      name: entry.name,
      kind: entry.kind,
      ...(entry.size === undefined ? {} : { size: entry.size }),
      ...(entry.modifiedAt === undefined ? {} : { modifiedAt: entry.modifiedAt }),
      metadataRevision: entry.metadataRevision,
      status: 'ready',
    });
  }
  const collected = [...items.values()];
  const paths = new Map<string, BasketItem>();
  for (const item of collected) {
    const { prefix, pathname } = basketPath(item);
    const path = prefix + pathname;
    if (paths.has(path)) throw new Error(t('basket', 'overlap'));
    paths.set(path, item);
  }
  const retained = collected.filter((item) => {
    const newParent = basketAncestor(item, paths, newFolders);
    if (newParent !== undefined) {
      const parentOrder = newFolders.get(newParent.key);
      const childOrder = incomingOrder.get(item.key);
      if (parentOrder !== undefined && (childOrder === undefined || childOrder < parentOrder)) {
        return false;
      }
    }
    if (basketAncestor(item, paths) === undefined) return true;
    throw new Error(t('basket', 'overlap'));
  });
  const retainedKeys = new Set(retained.map((item) => item.key));
  return {
    ...state,
    items: retained,
    selectedKeys: state.selectedKeys.filter((key) => retainedKeys.has(key)),
  };
}

export function removeFromBasket(state: BasketState, key: string): BasketState {
  return {
    ...state,
    items: state.items.filter((item) => item.key !== key),
    selectedKeys: state.selectedKeys.filter((selected) => selected !== key),
  };
}

export function selectBasketItems(state: BasketState, keys: readonly string[]): BasketState {
  const known = new Set(state.items.map((item) => item.key));
  return { ...state, selectedKeys: [...new Set(keys)].filter((key) => known.has(key)) };
}

export function basketSources(state: BasketState): readonly Location[] {
  const selected = new Set(state.selectedKeys);
  return state.items
    .filter((item) => selected.has(item.key) && item.status === 'ready')
    .map((item) => item.location);
}

export function basketSummary(state: BasketState): {
  fileCount: number;
  folderCount: number;
  knownSize: number;
  incompleteSize: boolean;
  unavailableCount: number;
  selectedCount: number;
  selectedKnownSize: number;
} {
  const selected = new Set(state.selectedKeys);
  let fileCount = 0;
  let folderCount = 0;
  let knownSize = 0;
  let selectedKnownSize = 0;
  let incompleteSize = false;
  let unavailableCount = 0;
  for (const item of state.items) {
    if (item.kind === 'directory') folderCount += 1;
    else fileCount += 1;
    if (item.status !== 'ready') unavailableCount += 1;
    if (item.status !== 'ready' || (item.kind !== 'directory' && item.size === undefined)) {
      incompleteSize = true;
    }
    if (item.status === 'ready' && item.kind !== 'directory' && item.size !== undefined) {
      knownSize += item.size;
      if (selected.has(item.key)) selectedKnownSize += item.size;
    }
  }
  return {
    fileCount,
    folderCount,
    knownSize,
    incompleteSize,
    unavailableCount,
    selectedCount: selected.size,
    selectedKnownSize,
  };
}

export function classifyBasketAbsence(
  item: BasketItem,
  visibleEntries: readonly EntrySummary[],
): 'moved' | 'missing' {
  return visibleEntries.some(
    (entry) =>
      entry.location.providerId === item.location.providerId &&
      entry.id === item.id &&
      entry.location.uri !== item.location.uri,
  )
    ? 'moved'
    : 'missing';
}

export function withBasketStatus(
  state: BasketState,
  key: string,
  status: BasketStatus,
): BasketState {
  return {
    ...state,
    items: state.items.map((item) => (item.key === key ? { ...item, status } : item)),
  };
}

export async function refreshBasket(
  state: BasketState,
  inspect: (item: BasketItem) => Promise<BasketStatus>,
  keys?: ReadonlySet<string>,
): Promise<BasketState> {
  const indices = state.items.flatMap((item, index) =>
    keys === undefined || keys.has(item.key) ? [index] : [],
  );
  const statuses = new Map<number, BasketStatus>();
  let next = 0;
  await Promise.all(
    Array.from({ length: Math.min(8, indices.length) }, async () => {
      while (next < indices.length) {
        const index = indices[next++];
        if (index === undefined) continue;
        const item = state.items[index];
        if (item !== undefined) statuses.set(index, await inspect(item));
      }
    }),
  );
  return {
    ...state,
    items: state.items.map((item, index) => ({
      ...item,
      status: statuses.get(index) ?? item.status,
    })),
  };
}

export interface BasketStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

function storageKey(workspaceId: string): string {
  return `procyon.basket.${workspaceId}`;
}

export function saveBasket(storage: BasketStorage, workspaceId: string, state: BasketState): void {
  if (state.persist) {
    storage.setItem(storageKey(workspaceId), JSON.stringify(state));
  } else {
    storage.removeItem(storageKey(workspaceId));
  }
}

export function loadBasket(storage: BasketStorage, workspaceId: string): BasketState {
  const saved = storage.getItem(storageKey(workspaceId));
  if (saved === null) return emptyBasket;
  const value: unknown = JSON.parse(saved);
  if (
    typeof value !== 'object' ||
    value === null ||
    !('persist' in value && value.persist === true) ||
    !('items' in value && Array.isArray(value.items)) ||
    !('selectedKeys' in value && Array.isArray(value.selectedKeys))
  ) {
    throw new Error(t('basket', 'invalidSaved'));
  }
  const items: BasketItem[] = [];
  for (const item of value.items) {
    if (
      typeof item !== 'object' ||
      item === null ||
      typeof item.id !== 'string' ||
      typeof item.name !== 'string' ||
      !['file', 'directory', 'symlink'].includes(item.kind) ||
      typeof item.metadataRevision !== 'number' ||
      (item.size !== undefined && (typeof item.size !== 'number' || item.size < 0)) ||
      (item.modifiedAt !== undefined && typeof item.modifiedAt !== 'string') ||
      typeof item.location !== 'object' ||
      item.location === null ||
      typeof item.location.providerId !== 'string' ||
      typeof item.location.uri !== 'string' ||
      !safeLocation(item.location)
    ) {
      throw new Error(t('basket', 'invalidEntry'));
    }
    const location: Location = { providerId: item.location.providerId, uri: item.location.uri };
    items.push({
      key: basketKey(item.id, location),
      id: item.id,
      location,
      name: item.name,
      kind: item.kind as EntrySummary['kind'],
      ...(item.size === undefined ? {} : { size: item.size }),
      ...(item.modifiedAt === undefined ? {} : { modifiedAt: item.modifiedAt }),
      metadataRevision: item.metadataRevision,
      status: 'stale',
    });
  }
  const selectedKeys = value.selectedKeys.filter(
    (key: unknown): key is string =>
      typeof key === 'string' && items.some((item) => item.key === key),
  );
  return {
    items: [...new Map(items.map((item) => [item.key, item])).values()],
    selectedKeys,
    persist: true,
  };
}
