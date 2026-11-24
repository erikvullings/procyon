import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { WorkspaceClient } from '../../api/client/workspace-client.ts';
import type { EntrySummary, ImageSummary, Location } from '../../models';
import { ImageSummaryLoader } from './image-summary-loader';

type SummaryClient = Pick<WorkspaceClient, 'readImageSummaries'>;

function entry(name: string, overrides: Partial<EntrySummary> = {}): EntrySummary {
  const extension = name.split('.').pop() ?? '';
  return {
    id: name,
    location: { providerId: 'file', uri: `file:///tmp/${name}` },
    name,
    kind: 'file',
    size: 100,
    modifiedAt: '2026-08-04T00:00:00.000Z',
    hidden: false,
    readOnly: false,
    extension,
    metadataRevision: 1,
    ...overrides,
  };
}

function answer(locations: readonly Location[]): ImageSummary[] {
  return locations.map((location) => ({ location, width: 10, height: 5 }));
}

const BATCH_DELAY_MS = 50;

describe('ImageSummaryLoader', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  function setup(client?: SummaryClient) {
    const readImageSummaries =
      client?.readImageSummaries ??
      vi.fn<SummaryClient['readImageSummaries']>(async (locations) => answer(locations));
    const redraw = vi.fn();
    const loader = new ImageSummaryLoader({ readImageSummaries }, redraw);
    return { loader, readImageSummaries: vi.mocked(readImageSummaries), redraw };
  }

  it('fetches everything requested in one pass as a single batch, then serves it from cache', async () => {
    const { loader, readImageSummaries, redraw } = setup();
    const a = entry('a.jpg');
    const b = entry('b.png');

    expect(loader.summaryFor(a)).toBeUndefined();
    expect(loader.summaryFor(b)).toBeUndefined();
    await vi.advanceTimersByTimeAsync(BATCH_DELAY_MS);

    expect(readImageSummaries).toHaveBeenCalledTimes(1);
    expect(readImageSummaries.mock.calls[0]?.[0]).toEqual([a.location, b.location]);
    expect(redraw).toHaveBeenCalled();
    expect(loader.summaryFor(a)).toMatchObject({ width: 10, height: 5 });
    expect(loader.summaryFor(b)).toMatchObject({ width: 10, height: 5 });
    await vi.advanceTimersByTimeAsync(BATCH_DELAY_MS);
    expect(readImageSummaries).toHaveBeenCalledTimes(1);
  });

  it('never asks for entries that are not images', async () => {
    const { loader, readImageSummaries } = setup();

    expect(loader.summaryFor(entry('notes.txt'))).toBeUndefined();
    expect(loader.summaryFor(entry('folder', { kind: 'directory' }))).toBeUndefined();
    expect(loader.summaryFor(entry('logo.svg'))).toBeUndefined();
    await vi.advanceTimersByTimeAsync(BATCH_DELAY_MS);

    expect(readImageSummaries).not.toHaveBeenCalled();
  });

  it('reads a changed file again but not an unchanged one', async () => {
    const { loader, readImageSummaries } = setup();
    loader.summaryFor(entry('a.jpg'));
    await vi.advanceTimersByTimeAsync(BATCH_DELAY_MS);

    loader.summaryFor(entry('a.jpg'));
    await vi.advanceTimersByTimeAsync(BATCH_DELAY_MS);
    expect(readImageSummaries).toHaveBeenCalledTimes(1);

    loader.summaryFor(entry('a.jpg', { modifiedAt: '2026-09-01T00:00:00.000Z' }));
    await vi.advanceTimersByTimeAsync(BATCH_DELAY_MS);
    expect(readImageSummaries).toHaveBeenCalledTimes(2);
  });

  it('splits a large pass into batches the backend accepts', async () => {
    const { loader, readImageSummaries } = setup();
    for (let index = 0; index < 150; index += 1) loader.summaryFor(entry(`p${index}.jpg`));
    await vi.advanceTimersByTimeAsync(BATCH_DELAY_MS);
    await vi.advanceTimersByTimeAsync(BATCH_DELAY_MS);
    await vi.advanceTimersByTimeAsync(BATCH_DELAY_MS);

    expect(readImageSummaries.mock.calls.map(([locations]) => locations.length)).toEqual([
      64, 64, 22,
    ]);
  });

  it('drops queued requests whose viewer scrolled away before they were sent', async () => {
    const { loader, readImageSummaries } = setup();
    const viewport = loader.createViewport();

    viewport.beginFrame();
    viewport.summaryFor(entry('gone.jpg'));
    viewport.summaryFor(entry('stays.jpg'));
    viewport.endFrame();
    viewport.beginFrame();
    viewport.summaryFor(entry('stays.jpg'));
    viewport.endFrame();
    await vi.advanceTimersByTimeAsync(BATCH_DELAY_MS);

    expect(readImageSummaries).toHaveBeenCalledTimes(1);
    expect(readImageSummaries.mock.calls[0]?.[0]).toEqual([entry('stays.jpg').location]);
  });

  it('aborts a batch in flight when nobody is waiting for it any more', async () => {
    let signal: AbortSignal | undefined;
    const readImageSummaries = vi.fn<SummaryClient['readImageSummaries']>(
      (_locations, abortSignal) => {
        signal = abortSignal;
        return new Promise(() => undefined);
      },
    );
    const { loader } = setup({ readImageSummaries });
    const viewport = loader.createViewport();

    viewport.beginFrame();
    viewport.summaryFor(entry('a.jpg'));
    viewport.endFrame();
    await vi.advanceTimersByTimeAsync(BATCH_DELAY_MS);
    expect(signal?.aborted).toBe(false);

    viewport.beginFrame();
    viewport.endFrame();

    expect(signal?.aborted).toBe(true);
  });

  it('marks images unavailable when the batch fails, without retrying on every render', async () => {
    const readImageSummaries = vi
      .fn<SummaryClient['readImageSummaries']>()
      .mockRejectedValue(new Error('offline'));
    const { loader } = setup({ readImageSummaries });
    const photo = entry('a.jpg');
    loader.summaryFor(photo);
    await vi.advanceTimersByTimeAsync(BATCH_DELAY_MS);

    expect(loader.summaryFor(photo)).toMatchObject({ unreadable: true });
    await vi.advanceTimersByTimeAsync(BATCH_DELAY_MS);
    expect(readImageSummaries).toHaveBeenCalledTimes(1);
  });
});
