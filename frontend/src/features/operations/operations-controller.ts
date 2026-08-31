import type { FileManagerClient } from '../../api/client/file-manager-client';
import type { Location, Operation } from '../../models';

export interface OperationsController {
  copy(
    sources: readonly Location[],
    destination: Location,
    signal?: AbortSignal,
  ): Promise<Operation>;
  move(
    sources: readonly Location[],
    destination: Location,
    signal?: AbortSignal,
  ): Promise<Operation>;
  trash(sources: readonly Location[], signal?: AbortSignal): Promise<Operation>;
  delete(
    sources: readonly Location[],
    permanentDeleteConfirmed: boolean,
    overrideReadOnly: boolean,
    signal?: AbortSignal,
  ): Promise<Operation>;
  /** Extracts a single archive entry by copying it to the destination. */
  extract(source: Location, destination: Location, signal?: AbortSignal): Promise<Operation>;
  pack(
    sources: readonly Location[],
    destination: Location,
    moveSources: boolean,
    format: 'zip' | 'sevenZip',
    compressionLevel?: number,
    signal?: AbortSignal,
  ): Promise<Operation>;
  createDirectory(location: Location, name: string, signal?: AbortSignal): Promise<Operation>;
  /** Creates an empty file at `location` (Shift+F4). */
  createFile(location: Location, name: string, signal?: AbortSignal): Promise<Operation>;
  rename(source: Location, destination: Location, signal?: AbortSignal): Promise<Operation>;
  multiRename(
    sources: readonly Location[],
    destinations: readonly Location[],
    signal?: AbortSignal,
  ): Promise<Operation>;
  /** Copy-with-rename in the same directory ("Duplicate", Shift+F5, TASKS/0042). */
  duplicate(sources: readonly Location[], signal?: AbortSignal): Promise<Operation>;
}

export function createOperationsController(client: FileManagerClient): OperationsController {
  return {
    copy(sources, destination, signal) {
      return client.startOperation(
        { type: 'copy', sources, destination, conflictPolicy: 'ask' },
        signal,
      );
    },

    move(sources, destination, signal) {
      return client.startOperation(
        { type: 'move', sources, destination, conflictPolicy: 'ask' },
        signal,
      );
    },

    trash(sources, signal) {
      return client.startOperation({ type: 'trash', sources, conflictPolicy: 'ask' }, signal);
    },

    delete(sources, permanentDeleteConfirmed, overrideReadOnly, signal) {
      return client.startOperation(
        {
          type: 'delete',
          sources,
          conflictPolicy: 'ask',
          permanentDeleteConfirmed,
          overrideReadOnly,
        },
        signal,
      );
    },

    extract(source, destination, signal) {
      return client.startOperation(
        { type: 'copy', sources: [source], destination, conflictPolicy: 'ask' },
        signal,
      );
    },

    pack(sources, destination, moveSources, format, compressionLevel, signal) {
      return client.startOperation(
        {
          type: moveSources ? 'moveToArchive' : 'createArchive',
          sources,
          destination,
          conflictPolicy: 'ask',
          archiveFormat: format,
          archiveCompressionLevel: compressionLevel,
        },
        signal,
      );
    },

    createDirectory(location, name, signal) {
      return client.startOperation(
        {
          type: 'createDirectory',
          sources: [],
          destination: location,
          conflictPolicy: 'ask',
          name,
          createIntermediateDirectories: false,
        },
        signal,
      );
    },

    createFile(location, name, signal) {
      return client.startOperation(
        {
          type: 'createFile',
          sources: [],
          destination: location,
          conflictPolicy: 'ask',
          name,
        },
        signal,
      );
    },

    rename(source, destination, signal) {
      return client.startOperation(
        { type: 'rename', sources: [source], destination, conflictPolicy: 'ask' },
        signal,
      );
    },

    multiRename(sources, destinations, signal) {
      return client.startOperation(
        { type: 'rename', sources, destinations, conflictPolicy: 'ask' },
        signal,
      );
    },

    duplicate(sources, signal) {
      return client.startOperation({ type: 'duplicate', sources, conflictPolicy: 'ask' }, signal);
    },
  };
}
