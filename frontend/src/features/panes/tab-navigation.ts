import type { TabId } from '../../models';

/** Wraps a cycling index by `direction` (+1/-1) around `length`, per spec §37 tab cycling. */
export function cycledTabIndex(currentIndex: number, length: number, direction: 1 | -1): number {
  if (length <= 0) return currentIndex;
  return (currentIndex + direction + length) % length;
}

/** Resolves the tab id for a 1-based "jump to tab N" shortcut; `undefined` if N is out of range. */
export function tabIdForJump(order: readonly TabId[], oneBasedIndex: number): TabId | undefined {
  return order[oneBasedIndex - 1];
}
