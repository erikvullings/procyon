import type { FileManagerClient } from '../../api/client/file-manager-client';
import type { LinkRequest, Location, Operation } from '../../models';

export interface OperationsController {
  copy(
    sources: readonly Location[],
    destination: Location,
    signal?: AbortSignal,
  ): Promise<Operation | undefined>;
  move(
    sources: readonly Location[],
    destination: Location,
    signal?: AbortSignal,
  ): Promise<Operation | undefined>;
  trash(sources: readonly Location[], signal?: AbortSignal): Promise<Operation | undefined>;
  delete(
    sources: readonly Location[],
    permanentDeleteConfirmed: boolean,
    overrideReadOnly: boolean,
    signal?: AbortSignal,
  ): Promise<Operation | undefined>;
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
  createDirectory(
    location: Location,
    name: string,
    createIntermediateDirectories?: boolean,
    signal?: AbortSignal,
  ): Promise<Operation>;
  /** Creates an empty file at `location` (Shift+F4). */
  createFile(location: Location, name: string, signal?: AbortSignal): Promise<Operation>;
  /** Creates a link named `name` in `destination` that points at `target` (task 0168). */
  createLink(
    target: Location,
    destination: Location,
    name: string,
    link: LinkRequest,
    signal?: AbortSignal,
  ): Promise<Operation>;
  rename(source: Location, destination: Location, signal?: AbortSignal): Promise<Operation>;
  multiRename(
    sources: readonly Location[],
    destinations: readonly Location[],
    signal?: AbortSignal,
  ): Promise<Operation>;
  /** Copy-with-rename in the same directory ("Duplicate", Shift+F5, TASKS/0042). */
  duplicate(sources: readonly Location[], signal?: AbortSignal): Promise<Operation>;
}

export type ConfirmableOperationKind = 'copy' | 'move' | 'trash';

export interface OperationConfirmationRequest {
  readonly kind: ConfirmableOperationKind;
  readonly sources: readonly Location[];
  readonly destination?: Location;
}

export function withOperationConfirmation(
  delegate: OperationsController,
  enabled: () => boolean,
  confirm: (request: OperationConfirmationRequest) => Promise<boolean>,
): OperationsController {
  async function confirmed(
    request: OperationConfirmationRequest,
    signal: AbortSignal | undefined,
    start: () => Promise<Operation | undefined>,
  ): Promise<Operation | undefined> {
    if (signal?.aborted) return undefined;
    if (enabled() && !(await confirm(request))) return undefined;
    if (signal?.aborted) return undefined;
    return start();
  }

  return {
    ...delegate,
    copy: (sources, destination, signal) =>
      confirmed({ kind: 'copy', sources, destination }, signal, () =>
        delegate.copy(sources, destination, signal),
      ),
    move: (sources, destination, signal) =>
      confirmed({ kind: 'move', sources, destination }, signal, () =>
        delegate.move(sources, destination, signal),
      ),
    trash: (sources, signal) =>
      confirmed({ kind: 'trash', sources }, signal, () => delegate.trash(sources, signal)),
  };
}

export function withPermanentDeleteConfirmation(
  delegate: OperationsController,
  confirm: (sources: readonly Location[]) => Promise<boolean>,
): OperationsController {
  return {
    ...delegate,
    async delete(sources, permanentDeleteConfirmed, overrideReadOnly, signal) {
      if (signal?.aborted) return undefined;
      if (!permanentDeleteConfirmed && !(await confirm(sources))) return undefined;
      if (signal?.aborted) return undefined;
      return delegate.delete(sources, true, overrideReadOnly, signal);
    },
  };
}

export function withActiveSourceGuard(
  delegate: OperationsController,
  active: () => readonly Operation[],
): OperationsController {
  const pending: (readonly Location[])[] = [];
  const submitted: { readonly operation: Operation; readonly at: number }[] = [];
  const overlaps = (left: Location, right: Location) => {
    if (left.providerId !== right.providerId) return false;
    const a = left.uri.replace(/\/+$/, '');
    const b = right.uri.replace(/\/+$/, '');
    return a === b || a.startsWith(`${b}/`) || b.startsWith(`${a}/`);
  };
  const guarded = (
    sources: readonly Location[],
    start: () => Promise<Operation | undefined>,
  ): Promise<Operation | undefined> => {
    const current = active();
    for (let index = submitted.length - 1; index >= 0; index--) {
      const candidate = submitted[index];
      if (
        candidate === undefined ||
        Date.now() - candidate.at > 5_000 ||
        current.some((operation) => operation.id === candidate.operation.id)
      ) {
        submitted.splice(index, 1);
      }
    }
    const existing = [...current, ...submitted.map(({ operation }) => operation)].find(
      (operation) =>
        ['delete', 'copy', 'move'].includes(operation.kind) &&
        !['completed', 'completedWithWarnings', 'cancelled', 'failed', 'interrupted'].includes(
          operation.state,
        ) &&
        sources.some((source) =>
          operation.sources.some((entry) => overlaps(source, entry.location)),
        ),
    );
    if (existing !== undefined) return Promise.resolve(existing);
    if (
      pending.some((locations) =>
        sources.some((source) => locations.some((location) => overlaps(source, location))),
      )
    ) {
      return Promise.resolve(undefined);
    }
    pending.push(sources);
    return start()
      .then((operation) => {
        if (operation !== undefined) submitted.push({ operation, at: Date.now() });
        return operation;
      })
      .finally(() => {
        pending.splice(pending.indexOf(sources), 1);
      });
  };
  return {
    ...delegate,
    copy: (sources, destination, signal) =>
      guarded(sources, () => delegate.copy(sources, destination, signal)),
    move: (sources, destination, signal) =>
      guarded(sources, () => delegate.move(sources, destination, signal)),
    delete: (sources, confirmed, overrideReadOnly, signal) =>
      guarded(sources, () => delegate.delete(sources, confirmed, overrideReadOnly, signal)),
  };
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

    createDirectory(location, name, createIntermediateDirectories = false, signal) {
      return client.startOperation(
        {
          type: 'createDirectory',
          sources: [],
          destination: location,
          conflictPolicy: 'ask',
          name,
          createIntermediateDirectories,
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

    createLink(target, destination, name, link, signal) {
      return client.startOperation(
        {
          type: 'createLink',
          sources: [target],
          destination,
          conflictPolicy: 'ask',
          name,
          link,
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
