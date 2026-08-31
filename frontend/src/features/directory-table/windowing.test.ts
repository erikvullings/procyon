import { describe, expect, it } from 'vitest';

import { calculateVisibleWindow, scrollOffsetForIndex } from './windowing';

describe('calculateVisibleWindow', () => {
  it('renders the viewport and overscan without exceeding the entry count', () => {
    expect(
      calculateVisibleWindow({
        entryCount: 100,
        rowHeight: 30,
        scrollTop: 300,
        viewportHeight: 120,
        overscan: 2,
      }),
    ).toEqual({ start: 8, end: 16, offsetTop: 240, totalHeight: 3_000 });
  });

  it('clamps a window at the end of a large directory', () => {
    expect(
      calculateVisibleWindow({
        entryCount: 1_000_000,
        rowHeight: 30,
        scrollTop: 29_999_940,
        viewportHeight: 120,
        overscan: 3,
      }),
    ).toEqual({
      start: 999_995,
      end: 1_000_000,
      offsetTop: 29_999_850,
      totalHeight: 30_000_000,
    });
  });
});

describe('scrollOffsetForIndex', () => {
  it('keeps an already visible row at the current offset', () => {
    expect(
      scrollOffsetForIndex({
        index: 12,
        entryCount: 100,
        rowHeight: 30,
        scrollTop: 300,
        viewportHeight: 120,
      }),
    ).toBe(300);
  });

  it('scrolls the minimum distance needed to reveal rows above or below the viewport', () => {
    expect(
      scrollOffsetForIndex({
        index: 2,
        entryCount: 100,
        rowHeight: 30,
        scrollTop: 300,
        viewportHeight: 120,
      }),
    ).toBe(60);
    expect(
      scrollOffsetForIndex({
        index: 20,
        entryCount: 100,
        rowHeight: 30,
        scrollTop: 300,
        viewportHeight: 120,
      }),
    ).toBe(510);
  });
});
