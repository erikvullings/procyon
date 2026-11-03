import { describe, expect, it, vi } from 'vitest';
import type { FileManagerClient } from '../../api/client/file-manager-client';
import type { Volume } from '../../models';
import {
  createWorkspaceController,
  locationsMatch,
  type WorkspaceControllerContext,
} from './workspace-controller';

describe('native drag location matching', () => {
  it('matches equivalent local paths after a native macOS round trip', () => {
    expect(
      locationsMatch(
        [{ providerId: 'local', uri: 'file:///Users/example/caf%C3%A9.txt' }],
        [{ providerId: 'local', uri: 'file:///Users/example/cafe\u0301.txt' }],
      ),
    ).toBe(true);
  });

  it('does not match different providers or native paths', () => {
    expect(
      locationsMatch(
        [{ providerId: 'local', uri: 'file:///Users/example/report.txt' }],
        [{ providerId: 'archive', uri: 'file:///Users/example/report.txt' }],
      ),
    ).toBe(false);
    expect(
      locationsMatch(
        [{ providerId: 'local', uri: 'file:///Users/example/report.txt' }],
        [{ providerId: 'local', uri: 'file:///Users/example/other.txt' }],
      ),
    ).toBe(false);
  });
});

describe('refreshVolumes', () => {
  const usb: Volume = {
    name: 'USB',
    location: { providerId: 'local', uri: 'file:///Volumes/USB' },
  };

  function harness(getVolumes: () => Promise<readonly Volume[]>) {
    let volumes: readonly Volume[] = [];
    const redraw = vi.fn();
    const context = {
      getVolumes: () => volumes,
      setVolumes: (next: readonly Volume[]) => {
        volumes = next;
      },
      setVolumesError: vi.fn(),
      redraw,
    } as unknown as WorkspaceControllerContext;
    const controller = createWorkspaceController(
      { getVolumes } as unknown as FileManagerClient,
      context,
    );
    return { controller, redraw, volumes: () => volumes };
  }

  it('picks up a drive mounted after startup and redraws once', async () => {
    const h = harness(async () => [usb]);
    await h.controller.refreshVolumes();
    await h.controller.refreshVolumes();
    expect(h.volumes()).toEqual([usb]);
    expect(h.redraw).toHaveBeenCalledTimes(1);
  });

  it('keeps the last known list when discovery fails', async () => {
    let fail = false;
    const h = harness(async () => {
      if (fail) throw new Error('boom');
      return [usb];
    });
    await h.controller.refreshVolumes();
    fail = true;
    await h.controller.refreshVolumes();
    expect(h.volumes()).toEqual([usb]);
  });
});
