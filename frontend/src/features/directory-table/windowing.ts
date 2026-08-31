/** Inputs shared by fixed-row virtual scrolling calculations. */
export interface WindowingInputs {
  readonly entryCount: number;
  readonly rowHeight: number;
  readonly scrollTop: number;
  readonly viewportHeight: number;
}

/** The half-open range of rows which should be mounted. */
export interface VisibleWindow {
  readonly start: number;
  readonly end: number;
  readonly offsetTop: number;
  readonly totalHeight: number;
}

/** Calculates the visible rows plus a bounded number of rows on either side. */
export function calculateVisibleWindow(
  inputs: WindowingInputs & { readonly overscan: number },
): VisibleWindow {
  const visibleStart = Math.floor(inputs.scrollTop / inputs.rowHeight);
  const visibleEnd = Math.ceil((inputs.scrollTop + inputs.viewportHeight) / inputs.rowHeight);
  const start = Math.max(0, visibleStart - inputs.overscan);
  const end = Math.min(inputs.entryCount, visibleEnd + inputs.overscan);
  return {
    start,
    end,
    offsetTop: start * inputs.rowHeight,
    totalHeight: inputs.entryCount * inputs.rowHeight,
  };
}

/**
 * Returns the nearest scroll offset that fully reveals an index, preserving
 * the current offset when the row is already visible.
 */
export function scrollOffsetForIndex(inputs: WindowingInputs & { readonly index: number }): number {
  if (inputs.entryCount === 0) {
    return 0;
  }
  const index = Math.max(0, Math.min(inputs.entryCount - 1, inputs.index));
  const rowTop = index * inputs.rowHeight;
  const rowBottom = rowTop + inputs.rowHeight;
  if (rowTop < inputs.scrollTop) {
    return rowTop;
  }
  if (rowBottom > inputs.scrollTop + inputs.viewportHeight) {
    return rowBottom - inputs.viewportHeight;
  }
  return inputs.scrollTop;
}
