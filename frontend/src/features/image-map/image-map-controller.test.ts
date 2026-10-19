import { describe, expect, it, vi } from 'vitest';
import type { EntrySummary, ImageGpsEntry, Location, Operation } from '../../models';
import { createImageMapController, type ImageMapControllerContext } from './image-map-controller';
import { type ImageMapState, INITIAL_IMAGE_MAP_STATE } from './image-map-state';

function entry(name: string): EntrySummary {
  return {
    id: name,
    name,
    kind: 'file',
    extension: name.split('.').pop(),
    modifiedAt: '2024-01-01T00:00:00Z',
    location: { providerId: 'local', uri: `file:///${name}` },
  } as EntrySummary;
}

function setup(options: { operationState?: Operation['state'] } = {}) {
  let state: ImageMapState = INITIAL_IMAGE_MAP_STATE;
  const readImageGps = vi.fn(
    async (locations: readonly Location[]): Promise<ImageGpsEntry[]> =>
      locations.map((location) =>
        location.uri.includes('geo')
          ? { location, gps: { latitude: 10, longitude: 20 } }
          : { location },
      ),
  );
  const editImageMetadata = vi.fn(async () => ({ id: 'op-1' }) as Operation);
  const listOperations = vi.fn(async () => [
    { id: 'op-1', state: options.operationState ?? 'completed' } as Operation,
  ]);
  const toast = vi.fn();
  const context: ImageMapControllerContext = {
    getClient: () => ({ readImageGps, listOperations }),
    getOpsController: () => ({ editImageMetadata }),
    getState: () => state,
    setState: (next) => {
      state = next;
    },
    toast,
    redraw: vi.fn(),
  };
  return {
    controller: createImageMapController(context),
    get state() {
      return state;
    },
    readImageGps,
    editImageMetadata,
    toast,
  };
}

const source = (entries: EntrySummary[], folder = true) => ({
  folder,
  collect: vi.fn(async (recursive: boolean) => ({
    entries: recursive ? [...entries, entry('geo-deep.jpg')] : entries,
    truncated: false,
  })),
});

describe('image map controller', () => {
  it('scans the source and separates located from unlocated images', async () => {
    const harness = setup();
    await harness.controller.open(source([entry('geo1.jpg'), entry('plain.jpg')]));
    expect(harness.state).toMatchObject({
      open: true,
      scanning: false,
      total: 2,
      foldersOffered: true,
    });
    expect(harness.state.located.map((image) => image.entry.name)).toEqual(['geo1.jpg']);
    expect(harness.state.unlocated.map((e) => e.name)).toEqual(['plain.jpg']);
  });

  it('rescans with subfolders and reuses the cache for known images', async () => {
    const harness = setup();
    const src = source([entry('geo1.jpg')]);
    await harness.controller.open(src);
    await harness.controller.setRecursive(true);
    expect(src.collect).toHaveBeenLastCalledWith(true, expect.anything());
    expect(harness.state.recursive).toBe(true);
    expect(harness.state.located).toHaveLength(2);
    // geo1.jpg came from the cache; only the new image hit the backend.
    expect(harness.readImageGps).toHaveBeenCalledTimes(2);
  });

  it('asks for confirmation before changing a location and then edits through the operation engine', async () => {
    const harness = setup();
    await harness.controller.open(source([entry('plain.jpg')]));
    const image = harness.state.unlocated[0] as EntrySummary;
    harness.controller.beginPlacing(image);
    expect(harness.state.placing?.name).toBe('plain.jpg');
    harness.controller.proposePlacement(image, 1.5, 2.5);
    expect(harness.state.pending).toMatchObject({ latitude: 1.5, longitude: 2.5 });
    expect(harness.editImageMetadata).not.toHaveBeenCalled();

    await harness.controller.confirmPlacement();
    expect(harness.editImageMetadata).toHaveBeenCalledWith([image.location], {
      gps: { latitude: 1.5, longitude: 2.5 },
      backupOriginal: true,
    });
    expect(harness.state.pending).toBeUndefined();
  });

  it('does nothing when the placement is cancelled or the format cannot be written', async () => {
    const harness = setup();
    await harness.controller.open(source([entry('geo1.jpg'), entry('plain.png')]));
    const png = harness.state.unlocated[0] as EntrySummary;
    harness.controller.beginPlacing(png);
    expect(harness.state.placing).toBeUndefined();
    harness.controller.proposePlacement(png, 1, 1);
    expect(harness.state.pending).toBeUndefined();
    expect(harness.toast).toHaveBeenCalled();

    const jpeg = harness.state.located[0]?.entry as EntrySummary;
    harness.controller.proposePlacement(jpeg, 3, 4);
    harness.controller.cancelPlacement();
    expect(harness.state.pending).toBeUndefined();
    expect(harness.editImageMetadata).not.toHaveBeenCalled();
  });

  it('reports a failed edit and closes cleanly', async () => {
    const harness = setup({ operationState: 'failed' });
    await harness.controller.open(source([entry('plain.jpg')]));
    const image = harness.state.unlocated[0] as EntrySummary;
    harness.controller.proposePlacement(image, 1, 1);
    await harness.controller.confirmPlacement();
    expect(harness.toast).toHaveBeenCalledWith(expect.stringContaining('could not be changed'));
    harness.controller.close();
    expect(harness.state.open).toBe(false);
  });

  it('shows the failure when collecting the images fails', async () => {
    const harness = setup();
    await harness.controller.open({
      folder: true,
      collect: vi.fn().mockRejectedValue(new Error('nope')),
    });
    expect(harness.state).toMatchObject({ scanning: false, failure: 'nope' });
  });
});
