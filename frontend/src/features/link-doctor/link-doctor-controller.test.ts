import { describe, expect, it, vi } from 'vitest';
import type { LinkDoctorPage, LinkProblem, Operation } from '../../models';
import {
  createLinkDoctorController,
  type LinkDoctorControllerContext,
} from './link-doctor-controller';
import { INITIAL_LINK_DOCTOR_STATE, type LinkDoctorState } from './link-doctor-state';

const symlinkProblem: LinkProblem = {
  kind: 'brokenSymlink',
  source: { providerId: 'local', uri: 'file:///links/current' },
  target: '../old/target.txt',
};

function page(overrides: Partial<LinkDoctorPage> = {}): LinkDoctorPage {
  return {
    scanId: 's1',
    roots: [],
    entriesScanned: 4,
    linksChecked: 2,
    offset: 0,
    limit: 500,
    total: 1,
    problems: [symlinkProblem],
    isComplete: true,
    isCancelled: false,
    hasMore: false,
    ...overrides,
  };
}

function setup() {
  let state: LinkDoctorState = INITIAL_LINK_DOCTOR_STATE;
  const trash = vi.fn(() => Promise.resolve({ id: 'op1', state: 'completed' } as Operation));
  const createLink = vi.fn(() => Promise.resolve({} as Operation));
  const findLinkReplacements = vi.fn(() =>
    Promise.resolve({
      candidates: [
        {
          location: { providerId: 'local', uri: 'file:///new/target.txt' },
          size: 5,
          sameSize: true,
          exactName: true,
        },
      ],
      truncated: false,
    }),
  );
  const toast = vi.fn();
  const context: LinkDoctorControllerContext = {
    getClient: () => ({
      startLinkDoctorScan: vi.fn(() => Promise.resolve({ scanId: 's1' })),
      getLinkDoctorScan: vi.fn(() => Promise.resolve(page())),
      cancelLinkDoctorScan: vi.fn(() => Promise.resolve()),
      findLinkReplacements,
      listOperations: vi.fn(() => Promise.resolve([])),
    }),
    getOpsController: () => ({ trash, createLink }),
    getState: () => state,
    setState: (next) => {
      state = next;
    },
    toast,
    redraw: vi.fn(),
    openInEditor: vi.fn(),
    refreshPanes: vi.fn(),
  };
  return {
    controller: createLinkDoctorController(context),
    state: () => state,
    trash,
    createLink,
    findLinkReplacements,
    toast,
  };
}

describe('link doctor controller', () => {
  it('scans to completion and exposes the problems', async () => {
    const { controller, state } = setup();
    await controller.start([{ providerId: 'local', uri: 'file:///links' }]);
    expect(state().isComplete).toBe(true);
    expect(state().problems).toEqual([symlinkProblem]);
    expect(state().entriesScanned).toBe(4);
  });

  it('searches the scan roots for the missing target name', async () => {
    const { controller, state, findLinkReplacements } = setup();
    await controller.start([{ providerId: 'local', uri: 'file:///links' }]);
    await controller.findReplacement(symlinkProblem);
    expect(findLinkReplacements).toHaveBeenCalledWith({
      roots: [{ providerId: 'local', uri: 'file:///links' }],
      targetName: 'target.txt',
    });
    expect(Object.values(state().candidates)[0]?.candidates).toHaveLength(1);
  });

  it('repoints a broken symlink by trashing it and creating a relative link', async () => {
    const { controller, state, trash, createLink } = setup();
    await controller.start([{ providerId: 'local', uri: 'file:///links' }]);
    await controller.findReplacement(symlinkProblem);
    const candidate = Object.values(state().candidates)[0]?.candidates[0];
    if (candidate === undefined) throw new Error('expected a candidate');

    await controller.repointSymlink(symlinkProblem, candidate);

    expect(trash).toHaveBeenCalledWith([symlinkProblem.source]);
    expect(createLink).toHaveBeenCalledWith(
      candidate.location,
      { providerId: 'local', uri: 'file:///links' },
      'current',
      { kind: 'symbolicLink', targetStyle: 'relative' },
    );
    expect(state().problems).toEqual([]);
  });

  it('reports a failure instead of creating the link when the trash step fails', async () => {
    const { controller, state, trash, createLink, toast } = setup();
    trash.mockResolvedValueOnce({ id: 'op2', state: 'failed' } as Operation);
    await controller.start([{ providerId: 'local', uri: 'file:///links' }]);
    await controller.repointSymlink(symlinkProblem, {
      location: { providerId: 'local', uri: 'file:///new/target.txt' },
      size: 5,
      sameSize: true,
      exactName: true,
    });
    expect(createLink).not.toHaveBeenCalled();
    expect(toast).toHaveBeenCalled();
    expect(state().problems).toHaveLength(1);
  });
});
