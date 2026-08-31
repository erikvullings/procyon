import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { FileManagerClient } from '../../api/client/file-manager-client';
import type { EntrySummary, Location, WorkspaceProjection } from '../../models';
import {
  type ChecksumController,
  type ChecksumControllerContext,
  createChecksumController,
} from './checksum-controller';
import {
  type ChecksumState,
  type DuplicateState,
  initialChecksumState,
  initialDuplicateState,
} from './checksum-state';

function location(uri: string): Location {
  return { providerId: 'local', uri };
}

function fileEntry(name: string): EntrySummary {
  return {
    id: `entry-${name}`,
    location: location(`file:///root/${name}`),
    name,
    kind: 'file',
    hidden: false,
    readOnly: false,
  } as EntrySummary;
}

function directoryEntry(name: string): EntrySummary {
  return { ...fileEntry(name), kind: 'directory' } as EntrySummary;
}

interface Harness {
  controller: ChecksumController;
  client: {
    startChecksums: ReturnType<typeof vi.fn>;
    cancelChecksums: ReturnType<typeof vi.fn>;
    renderChecksumFile: ReturnType<typeof vi.fn>;
    saveChecksumFile: ReturnType<typeof vi.fn>;
    verifyChecksumFile: ReturnType<typeof vi.fn>;
    startDuplicateScan: ReturnType<typeof vi.fn>;
    cancelDuplicateScan: ReturnType<typeof vi.fn>;
  };
  requestDelete: ReturnType<typeof vi.fn>;
  checksumState(): ChecksumState;
  duplicateState(): DuplicateState;
  setSelection(entries: readonly EntrySummary[]): void;
}

function harness(): Harness {
  let checksums = initialChecksumState();
  let duplicates = initialDuplicateState();
  let selection: readonly EntrySummary[] = [fileEntry('a.txt')];

  const client = {
    startChecksums: vi.fn().mockResolvedValue({ jobId: 'job-1' }),
    cancelChecksums: vi.fn().mockResolvedValue(undefined),
    renderChecksumFile: vi
      .fn()
      .mockResolvedValue({ suggestedName: 'checksums.sha256', content: 'aa  a.txt\n' }),
    saveChecksumFile: vi.fn().mockResolvedValue({
      location: { providerId: 'local', uri: 'file:///root/checksums.sha256' },
      bytesWritten: 42,
    }),
    verifyChecksumFile: vi.fn().mockResolvedValue({
      jobId: 'job-1',
      results: [{ path: 'a.txt', status: 'match' }],
      matched: 1,
      mismatched: 0,
      missing: 0,
    }),
    startDuplicateScan: vi.fn().mockResolvedValue({ scanId: 'scan-1' }),
    cancelDuplicateScan: vi.fn().mockResolvedValue(undefined),
  };
  const requestDelete = vi.fn();

  const context: ChecksumControllerContext = {
    getChecksumState: () => checksums,
    setChecksumState: (next) => {
      checksums = next;
    },
    getDuplicateState: () => duplicates,
    setDuplicateState: (next) => {
      duplicates = next;
    },
    getWorkspace: () => ({ id: 'workspace-1' }) as WorkspaceProjection,
    getClient: () => client as unknown as FileManagerClient,
    getSelectedEntries: () => selection,
    getActiveLocation: () => location('file:///root'),
    requestDelete,
    redraw: () => undefined,
  };

  return {
    controller: createChecksumController(context),
    client,
    requestDelete,
    checksumState: () => checksums,
    duplicateState: () => duplicates,
    setSelection: (entries) => {
      selection = entries;
    },
  };
}

describe('ChecksumController', () => {
  let test: Harness;

  beforeEach(() => {
    test = harness();
  });

  it('starts a job for the selected files', async () => {
    test.controller.calculateChecksums(['sha256', 'blake3']);
    await vi.waitFor(() => expect(test.checksumState().jobId).toBe('job-1'));
    expect(test.client.startChecksums).toHaveBeenCalledWith({
      workspaceId: 'workspace-1',
      entries: [location('file:///root/a.txt')],
      algorithms: ['sha256', 'blake3'],
    });
    expect(test.checksumState().totalEntries).toBe(1);
  });

  it('ignores directories in the selection', async () => {
    test.setSelection([fileEntry('a.txt'), directoryEntry('nested')]);
    test.controller.calculateChecksums(['sha256']);
    await vi.waitFor(() => expect(test.client.startChecksums).toHaveBeenCalled());
    expect(test.client.startChecksums.mock.calls[0]?.[0].entries).toEqual([
      location('file:///root/a.txt'),
    ]);
  });

  it('reports an error when nothing hashable is selected', () => {
    test.setSelection([directoryEntry('nested')]);
    test.controller.calculateChecksums(['sha256']);
    expect(test.client.startChecksums).not.toHaveBeenCalled();
    expect(test.checksumState().error).toMatch(/select one or more files/i);
  });

  it('cancels a previous job before starting a new one', async () => {
    test.controller.calculateChecksums(['sha256']);
    await vi.waitFor(() => expect(test.checksumState().jobId).toBe('job-1'));
    test.controller.calculateChecksums(['sha256']);
    await vi.waitFor(() => expect(test.client.cancelChecksums).toHaveBeenCalledWith('job-1'));
  });

  it('surfaces a start failure as an error', async () => {
    test.client.startChecksums.mockRejectedValueOnce(new Error('backend down'));
    test.controller.calculateChecksums(['sha256']);
    await vi.waitFor(() => expect(test.checksumState().error).toBe('backend down'));
  });

  it('applies a streamed batch to the tracked job', async () => {
    test.controller.calculateChecksums(['sha256']);
    await vi.waitFor(() => expect(test.checksumState().jobId).toBe('job-1'));
    test.controller.handleChecksumBatch(
      'job-1',
      [
        {
          location: location('file:///root/a.txt'),
          relativePath: 'a.txt',
          size: 3,
          checksums: { sha256: 'aa' },
        },
      ],
      true,
      false,
    );
    expect(test.checksumState().entries).toHaveLength(1);
    expect(test.checksumState().isComplete).toBe(true);
  });

  it('renders and copies checksum-file text', async () => {
    test.controller.calculateChecksums(['sha256']);
    await vi.waitFor(() => expect(test.checksumState().jobId).toBe('job-1'));
    await expect(test.controller.copyChecksums('sha256')).resolves.toBe('aa  a.txt\n');
    expect(test.client.renderChecksumFile).toHaveBeenCalledWith('job-1', 'sha256');
  });

  it('does not render a checksum file when no job is running', async () => {
    await expect(test.controller.renderChecksumFile('sha256')).resolves.toBeUndefined();
    expect(test.client.renderChecksumFile).not.toHaveBeenCalled();
  });

  it('saves the checksum file into the active directory', async () => {
    test.controller.calculateChecksums(['sha256']);
    await vi.waitFor(() => expect(test.checksumState().jobId).toBe('job-1'));

    await expect(test.controller.saveChecksumFile('sha256', 'checksums.sha256')).resolves.toEqual(
      location('file:///root/checksums.sha256'),
    );

    expect(test.client.saveChecksumFile).toHaveBeenCalledWith('job-1', {
      destination: location('file:///root/checksums.sha256'),
      algorithm: 'sha256',
    });
    expect(test.checksumState().savedTo?.uri).toBe('file:///root/checksums.sha256');
  });

  it('percent-encodes a filename with spaces', async () => {
    test.controller.calculateChecksums(['sha256']);
    await vi.waitFor(() => expect(test.checksumState().jobId).toBe('job-1'));
    await test.controller.saveChecksumFile('sha256', 'my sums.sha256');
    expect(test.client.saveChecksumFile.mock.calls[0]?.[1].destination.uri).toBe(
      'file:///root/my%20sums.sha256',
    );
  });

  it('passes the overwrite opt-in through', async () => {
    test.controller.calculateChecksums(['sha256']);
    await vi.waitFor(() => expect(test.checksumState().jobId).toBe('job-1'));
    await test.controller.saveChecksumFile('sha256', 'checksums.sha256', { overwrite: true });
    expect(test.client.saveChecksumFile.mock.calls[0]?.[1].overwrite).toBe(true);
  });

  it('refuses to save with no job, no directory or a blank name', async () => {
    await expect(test.controller.saveChecksumFile('sha256', 'x')).resolves.toBeUndefined();
    test.controller.calculateChecksums(['sha256']);
    await vi.waitFor(() => expect(test.checksumState().jobId).toBe('job-1'));
    await expect(test.controller.saveChecksumFile('sha256', '   ')).resolves.toBeUndefined();
    expect(test.client.saveChecksumFile).not.toHaveBeenCalled();
  });

  it('surfaces a save failure as an error rather than a silent no-op', async () => {
    test.controller.calculateChecksums(['sha256']);
    await vi.waitFor(() => expect(test.checksumState().jobId).toBe('job-1'));
    test.client.saveChecksumFile.mockRejectedValueOnce(new Error('destination exists'));

    await expect(
      test.controller.saveChecksumFile('sha256', 'checksums.sha256'),
    ).resolves.toBeUndefined();
    expect(test.checksumState().error).toBe('destination exists');
    expect(test.checksumState().savedTo).toBeUndefined();
  });

  it('offers a default filename derived from the algorithm', () => {
    expect(test.controller.suggestedFileName('sha256')).toBe('checksums.sha256');
    expect(test.controller.suggestedFileName('blake3')).toBe('checksums.blake3');
  });

  it('stores a verification report', async () => {
    test.controller.calculateChecksums(['sha256']);
    await vi.waitFor(() => expect(test.checksumState().jobId).toBe('job-1'));
    test.controller.verifyAgainst('aa  a.txt\n');
    await vi.waitFor(() => expect(test.checksumState().verification?.matched).toBe(1));
  });

  it('clears the panel on cancel', async () => {
    test.controller.calculateChecksums(['sha256']);
    await vi.waitFor(() => expect(test.checksumState().jobId).toBe('job-1'));
    test.controller.cancelChecksums();
    expect(test.client.cancelChecksums).toHaveBeenCalledWith('job-1');
    expect(test.checksumState().jobId).toBeUndefined();
  });

  it('starts a duplicate scan rooted at the active directory', async () => {
    test.controller.findDuplicates();
    await vi.waitFor(() => expect(test.duplicateState().scanId).toBe('scan-1'));
    expect(test.client.startDuplicateScan).toHaveBeenCalledWith({
      workspaceId: 'workspace-1',
      roots: [location('file:///root')],
    });
  });

  it('routes duplicate deletion through the shared delete flow', async () => {
    test.controller.findDuplicates();
    await vi.waitFor(() => expect(test.duplicateState().scanId).toBe('scan-1'));
    test.controller.handleDuplicateResults(
      'scan-1',
      [
        {
          fullHash: 'abc',
          size: 10,
          hardlinkClusters: [],
          distinctLocations: [location('file:///root/a'), location('file:///root/b')],
          reclaimableBytes: 10,
        },
      ],
      false,
      0,
    );
    test.controller.toggleDuplicateSelection('file:///root/a');
    test.controller.deleteSelectedDuplicates();

    expect(test.requestDelete).toHaveBeenCalledWith([location('file:///root/a')]);
    // Ticks are cleared so a second click cannot re-delete the same paths.
    expect(test.duplicateState().selectedUris.size).toBe(0);
  });

  it('does nothing when deleting with nothing ticked', () => {
    test.controller.deleteSelectedDuplicates();
    expect(test.requestDelete).not.toHaveBeenCalled();
  });

  it('surfaces a duplicate-scan failure as an error', async () => {
    test.client.startDuplicateScan.mockRejectedValueOnce(new Error('scan refused'));
    test.controller.findDuplicates();
    await vi.waitFor(() => expect(test.duplicateState().error).toBe('scan refused'));
  });
});
