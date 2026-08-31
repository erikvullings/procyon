import { beforeEach, describe, expect, it, vi } from 'vitest';

import { MockFileManagerClient } from '../../api/client/mock-file-manager-client';
import type { Location } from '../../models';
import { createOperationsController, type OperationsController } from './operations-controller';

const src: Location = { providerId: 'local', uri: 'file:///src/a.txt' };
const src2: Location = { providerId: 'local', uri: 'file:///src/b.txt' };
const dest: Location = { providerId: 'local', uri: 'file:///dst/' };

describe('OperationsController', () => {
  let client: MockFileManagerClient;
  let controller: OperationsController;

  beforeEach(() => {
    client = new MockFileManagerClient();
    vi.spyOn(client, 'startOperation');
    controller = createOperationsController(client);
  });

  it('copy calls startOperation with type copy and conflictPolicy ask', async () => {
    await controller.copy([src], dest);
    expect(client.startOperation).toHaveBeenCalledWith(
      { type: 'copy', sources: [src], destination: dest, conflictPolicy: 'ask' },
      undefined,
    );
  });

  it('move calls startOperation with type move', async () => {
    await controller.move([src, src2], dest);
    expect(client.startOperation).toHaveBeenCalledWith(
      { type: 'move', sources: [src, src2], destination: dest, conflictPolicy: 'ask' },
      undefined,
    );
  });

  it('trash calls startOperation with type trash and no destination', async () => {
    await controller.trash([src]);
    expect(client.startOperation).toHaveBeenCalledWith(
      { type: 'trash', sources: [src], conflictPolicy: 'ask' },
      undefined,
    );
  });

  it('delete passes permanentDeleteConfirmed and overrideReadOnly', async () => {
    await controller.delete([src], true, false);
    expect(client.startOperation).toHaveBeenCalledWith(
      {
        type: 'delete',
        sources: [src],
        conflictPolicy: 'ask',
        permanentDeleteConfirmed: true,
        overrideReadOnly: false,
      },
      undefined,
    );
  });

  it('extract uses type copy with a single-element sources array', async () => {
    await controller.extract(src, dest);
    expect(client.startOperation).toHaveBeenCalledWith(
      { type: 'copy', sources: [src], destination: dest, conflictPolicy: 'ask' },
      undefined,
    );
  });

  it('pack uses type createArchive when moveSources is false', async () => {
    await controller.pack([src], dest, false, 'zip', 5);
    expect(client.startOperation).toHaveBeenCalledWith(
      {
        type: 'createArchive',
        sources: [src],
        destination: dest,
        conflictPolicy: 'ask',
        archiveFormat: 'zip',
        archiveCompressionLevel: 5,
      },
      undefined,
    );
  });

  it('pack uses type moveToArchive when moveSources is true', async () => {
    await controller.pack([src], dest, true, 'sevenZip');
    expect(client.startOperation).toHaveBeenCalledWith(
      {
        type: 'moveToArchive',
        sources: [src],
        destination: dest,
        conflictPolicy: 'ask',
        archiveFormat: 'sevenZip',
        archiveCompressionLevel: undefined,
      },
      undefined,
    );
  });

  it('createDirectory passes name and createIntermediateDirectories false', async () => {
    await controller.createDirectory(dest, 'new-folder');
    expect(client.startOperation).toHaveBeenCalledWith(
      {
        type: 'createDirectory',
        sources: [],
        destination: dest,
        conflictPolicy: 'ask',
        name: 'new-folder',
        createIntermediateDirectories: false,
      },
      undefined,
    );
  });

  it('createFile passes name and no sources', async () => {
    await controller.createFile(dest, 'new-file.txt');
    expect(client.startOperation).toHaveBeenCalledWith(
      {
        type: 'createFile',
        sources: [],
        destination: dest,
        conflictPolicy: 'ask',
        name: 'new-file.txt',
      },
      undefined,
    );
  });

  it('duplicate calls startOperation with type duplicate and no destination', async () => {
    await controller.duplicate([src]);
    expect(client.startOperation).toHaveBeenCalledWith(
      { type: 'duplicate', sources: [src], conflictPolicy: 'ask' },
      undefined,
    );
  });

  it('rename uses sources array and single destination', async () => {
    const renamedDest: Location = { ...src, uri: 'file:///src/renamed.txt' };
    await controller.rename(src, renamedDest);
    expect(client.startOperation).toHaveBeenCalledWith(
      { type: 'rename', sources: [src], destination: renamedDest, conflictPolicy: 'ask' },
      undefined,
    );
  });

  it('multiRename passes parallel sources and destinations arrays', async () => {
    const dest1: Location = { ...src, uri: 'file:///src/x.txt' };
    const dest2: Location = { ...src2, uri: 'file:///src/y.txt' };
    await controller.multiRename([src, src2], [dest1, dest2]);
    expect(client.startOperation).toHaveBeenCalledWith(
      {
        type: 'rename',
        sources: [src, src2],
        destinations: [dest1, dest2],
        conflictPolicy: 'ask',
      },
      undefined,
    );
  });

  it('forwards an AbortSignal to the client', async () => {
    const signal = new AbortController().signal;
    await controller.copy([src], dest, signal);
    expect(client.startOperation).toHaveBeenCalledWith(expect.any(Object), signal);
  });
});
