import { describe, expect, it } from 'vitest';

import { cycledTabIndex, tabIdForJump } from './tab-navigation';

describe('cycledTabIndex', () => {
  it('wraps forward past the last tab', () => {
    expect(cycledTabIndex(2, 3, 1)).toBe(0);
  });

  it('wraps backward before the first tab', () => {
    expect(cycledTabIndex(0, 3, -1)).toBe(2);
  });

  it('steps within bounds without wrapping', () => {
    expect(cycledTabIndex(0, 3, 1)).toBe(1);
  });

  it('returns the current index when there are no tabs', () => {
    expect(cycledTabIndex(0, 0, 1)).toBe(0);
  });
});

describe('tabIdForJump', () => {
  const order = ['a', 'b', 'c'];

  it('selects the Nth tab if it exists', () => {
    expect(tabIdForJump(order, 3)).toBe('c');
  });

  it('no-ops for an index beyond the tab count', () => {
    expect(tabIdForJump(order, 9)).toBeUndefined();
  });

  it('no-ops for index 0', () => {
    expect(tabIdForJump(order, 0)).toBeUndefined();
  });
});
