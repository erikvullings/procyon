import { performance } from 'node:perf_hooks';

import m from 'mithril';
import { afterEach, describe, expect, it } from 'vitest';

import { createGeneratedDirectory } from '../../api/client/mock-directory-generator';
import {
  type DirectoryEntrySource,
  DirectoryTable,
  SAMPLE_FILE_AGE_COLUMN,
} from './directory-table';

interface RenderingMeasurements {
  readonly averageScrollRedrawMs: number;
  readonly averageCursorRedrawMs: number;
  readonly mountedRows: number;
}

const generated = createGeneratedDirectory(1_000_000, 24);
const source: DirectoryEntrySource = {
  length: generated.totalEntries,
  entryAt: (index) => generated.page(index, 1)[0],
};

let root: HTMLElement | undefined;

afterEach(() => {
  if (root !== undefined) {
    m.mount(root, null);
    root.remove();
    root = undefined;
  }
});

function measureRendering(
  iterations: number,
  pluginColumns: readonly (typeof SAMPLE_FILE_AGE_COLUMN)[] = [],
): RenderingMeasurements {
  root = document.createElement('div');
  document.body.appendChild(root);
  let cursorIndex = 0;
  m.mount(root, {
    view: () =>
      m(DirectoryTable, {
        state: { type: 'loaded' },
        source,
        cursorIndex,
        viewportHeight: 600,
        pluginColumns,
      }),
  });
  const grid = root.querySelector<HTMLElement>('[role="grid"]');
  if (grid === null) {
    throw new Error('directory grid was not rendered');
  }

  const scrollStart = performance.now();
  for (let iteration = 1; iteration <= iterations; iteration += 1) {
    grid.scrollTop = iteration * 9_000;
    grid.dispatchEvent(new Event('scroll'));
    m.redraw.sync();
  }
  const scrollDuration = performance.now() - scrollStart;

  const cursorStart = performance.now();
  for (let iteration = 1; iteration <= iterations; iteration += 1) {
    cursorIndex = iteration * 10_000;
    m.redraw.sync();
  }
  const cursorDuration = performance.now() - cursorStart;

  return {
    averageScrollRedrawMs: scrollDuration / iterations,
    averageCursorRedrawMs: cursorDuration / iterations,
    mountedRows: root.querySelectorAll('.fm-directory-row').length,
  };
}

describe('DirectoryTable rendering benchmark', () => {
  it('measures responsive scrolling and cursor redraws with one million lazy entries', () => {
    const measurements = measureRendering(20);

    expect(measurements.mountedRows).toBeLessThanOrEqual(32);
    expect(measurements.averageScrollRedrawMs).toBeLessThan(100);
    expect(measurements.averageCursorRedrawMs).toBeLessThan(100);
  });

  it('keeps the file-age column within the same scrolling budget', () => {
    const measurements = measureRendering(20, [SAMPLE_FILE_AGE_COLUMN]);

    expect(measurements.mountedRows).toBeLessThanOrEqual(32);
    expect(measurements.averageScrollRedrawMs).toBeLessThan(100);
    expect(measurements.averageCursorRedrawMs).toBeLessThan(100);
  });
});
