import type { OperationClient } from '../../api/client/operation-client.ts';
import type {
  ChangeAttributes,
  ConflictPolicy,
  GeoTransform,
  ImageMetadataEdit,
  LinkRequest,
  Location,
  Operation,
  SplitFile,
  TransferConflicts,
} from '../../models';
import { perfMark } from '../../utilities/perf';

export interface OperationsController {
  /** Copies `sources` into `destination`; `conflictPolicy` defaults to asking per conflict. */
  copy(
    sources: readonly Location[],
    destination: Location,
    signal?: AbortSignal,
    conflictPolicy?: ConflictPolicy,
  ): Promise<Operation | undefined>;
  /** Moves `sources` into `destination`; `conflictPolicy` defaults to asking per conflict. */
  move(
    sources: readonly Location[],
    destination: Location,
    signal?: AbortSignal,
    conflictPolicy?: ConflictPolicy,
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
  /** Creates a file holding `content`; like {@link createFile} it never overwrites (task 0264). */
  createTextFile(
    location: Location,
    name: string,
    content: string,
    signal?: AbortSignal,
  ): Promise<Operation>;
  /** Rewrites the metadata of image files in place; undoable when the original is kept (task 0274). */
  editImageMetadata(
    sources: readonly Location[],
    edit: ImageMetadataEdit,
    signal?: AbortSignal,
  ): Promise<Operation>;
  /**
   * Changes permissions, ownership or Windows attributes of the sources, optionally recursively, as
   * a cancellable job; undoable while the entries keep the values it set (task 0258).
   */
  changeAttributes(
    sources: readonly Location[],
    change: ChangeAttributes,
    signal?: AbortSignal,
  ): Promise<Operation>;
  /**
   * Cuts each source file into numbered parts (`name.001`, …) in `destination` (next to the source
   * when omitted), optionally with a BLAKE3 manifest; a cancellable, undoable job (task 0257).
   */
  splitFile(
    sources: readonly Location[],
    split: SplitFile,
    destination?: Location,
    signal?: AbortSignal,
  ): Promise<Operation>;
  /**
   * Joins the numbered parts the selected files belong to into one file in `destination` (next to
   * the parts when omitted), verifying the manifest when one exists (task 0257).
   */
  combineFiles(
    parts: readonly Location[],
    destination?: Location,
    name?: string,
    signal?: AbortSignal,
  ): Promise<Operation>;
  /** Converts geographic files into new files next to them (task 0281). */
  transformGeo(
    sources: readonly Location[],
    transform: GeoTransform,
    signal?: AbortSignal,
  ): Promise<Operation>;
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
  /** Sources that already exist in the destination (copy/move), when they could be determined
   * up front; the dialog then asks for the conflict policy itself. */
  readonly conflicts?: TransferConflicts;
}

/** How long the confirmation waits for the conflict lookup (e.g. a slow remote destination)
 * before opening without it; the job then still asks per conflict as before. */
export const CONFLICT_LOOKUP_TIMEOUT_MS = 1_500;

/** Resolves `lookup`, or `undefined` once `timeoutMs` passes or the lookup fails. */
async function boundedLookup(
  lookup: Promise<TransferConflicts | undefined>,
  timeoutMs: number,
): Promise<TransferConflicts | undefined> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<undefined>((resolve) => {
    timer = setTimeout(() => resolve(undefined), timeoutMs);
  });
  try {
    return await Promise.race([lookup.catch(() => undefined), timeout]);
  } finally {
    clearTimeout(timer);
  }
}

/** The user's answer: `undefined` cancels; otherwise the conflict policy the job should use. */
export type OperationConfirmationDecision = ConflictPolicy | undefined;

/** Looks up a planned transfer's existing names; resolves `undefined` when that isn't known. */
export type TransferConflictLookup = (
  sources: readonly Location[],
  destination: Location,
  signal?: AbortSignal,
) => Promise<TransferConflicts | undefined>;

/**
 * Asks before a routine copy, move or Trash (the `confirmFileOperations` setting). For a copy or
 * move it first looks up which sources already exist in the destination, so one dialog both
 * confirms the transfer and picks the conflict policy, instead of a confirmation followed by a
 * conflict prompt mid-job (task 0062). A caller-supplied policy, or a disabled confirmation,
 * passes straight through; the job still re-checks every entry while it runs.
 */
export function withOperationConfirmation(
  delegate: OperationsController,
  enabled: () => boolean,
  confirm: (request: OperationConfirmationRequest) => Promise<OperationConfirmationDecision>,
  findConflicts?: TransferConflictLookup,
): OperationsController {
  async function confirmed(
    request: OperationConfirmationRequest,
    signal: AbortSignal | undefined,
    start: (conflictPolicy: ConflictPolicy | undefined) => Promise<Operation | undefined>,
  ): Promise<Operation | undefined> {
    if (signal?.aborted) return undefined;
    if (!enabled()) return start(undefined);
    const conflicts =
      request.destination === undefined || findConflicts === undefined
        ? undefined
        : await boundedLookup(
            findConflicts(request.sources, request.destination, signal),
            CONFLICT_LOOKUP_TIMEOUT_MS,
          );
    if (signal?.aborted) return undefined;
    const decision = await confirm(
      conflicts === undefined || conflicts.names.length === 0 ? request : { ...request, conflicts },
    );
    if (decision === undefined || signal?.aborted) return undefined;
    return start(decision);
  }

  return {
    ...delegate,
    copy: (sources, destination, signal, conflictPolicy) =>
      conflictPolicy === undefined
        ? confirmed({ kind: 'copy', sources, destination }, signal, (policy) =>
            delegate.copy(sources, destination, signal, policy),
          )
        : delegate.copy(sources, destination, signal, conflictPolicy),
    move: (sources, destination, signal, conflictPolicy) =>
      conflictPolicy === undefined
        ? confirmed({ kind: 'move', sources, destination }, signal, (policy) =>
            delegate.move(sources, destination, signal, policy),
          )
        : delegate.move(sources, destination, signal, conflictPolicy),
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
    copy: (sources, destination, signal, conflictPolicy) =>
      guarded(sources, () => delegate.copy(sources, destination, signal, conflictPolicy)),
    move: (sources, destination, signal, conflictPolicy) =>
      guarded(sources, () => delegate.move(sources, destination, signal, conflictPolicy)),
    delete: (sources, confirmed, overrideReadOnly, signal) =>
      guarded(sources, () => delegate.delete(sources, confirmed, overrideReadOnly, signal)),
  };
}

export function createOperationsController(rawClient: OperationClient): OperationsController {
  const client: Pick<OperationClient, 'startOperation'> = {
    async startOperation(request, signal) {
      perfMark('op-start-request', { type: request.type });
      const operation = await rawClient.startOperation(request, signal);
      perfMark('op-start-returned', { type: request.type, state: operation.state });
      return operation;
    },
  };
  return {
    copy(sources, destination, signal, conflictPolicy = 'ask') {
      return client.startOperation({ type: 'copy', sources, destination, conflictPolicy }, signal);
    },

    move(sources, destination, signal, conflictPolicy = 'ask') {
      return client.startOperation({ type: 'move', sources, destination, conflictPolicy }, signal);
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

    transformGeo(sources, transform, signal) {
      return client.startOperation(
        { type: 'transformGeo', sources, conflictPolicy: 'ask', geoTransform: transform },
        signal,
      );
    },

    splitFile(sources, split, destination, signal) {
      return client.startOperation(
        {
          type: 'splitFile',
          sources,
          conflictPolicy: 'ask',
          splitFile: split,
          ...(destination === undefined ? {} : { destination }),
        },
        signal,
      );
    },

    combineFiles(parts, destination, name, signal) {
      return client.startOperation(
        {
          type: 'combineFiles',
          sources: parts,
          conflictPolicy: 'ask',
          ...(destination === undefined ? {} : { destination }),
          ...(name === undefined ? {} : { name }),
        },
        signal,
      );
    },

    changeAttributes(sources, change, signal) {
      return client.startOperation(
        { type: 'changeAttributes', sources, conflictPolicy: 'ask', attributeChange: change },
        signal,
      );
    },

    editImageMetadata(sources, edit, signal) {
      return client.startOperation(
        { type: 'editImageMetadata', sources, conflictPolicy: 'ask', imageMetadataEdit: edit },
        signal,
      );
    },

    createTextFile(location, name, content, signal) {
      return client.startOperation(
        {
          type: 'createFile',
          sources: [],
          destination: location,
          conflictPolicy: 'ask',
          name,
          initialContent: content,
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
