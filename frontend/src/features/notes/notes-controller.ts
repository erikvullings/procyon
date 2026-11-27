/**
 * The notes flow (task 0337): set up the notes folder (and, with the person's consent, index it
 * semantically), turn a terminal session into a draft, review drafts and the assistant's
 * proposals, and save them. A new note is created by the `createFile` operation and a changed
 * note saved through the editor, so nothing overwrites a file silently.
 */
import type { FileManagerClient } from '../../api/client/file-manager-client';
import { errorText, t } from '../../i18n';
import type { DraftNoteRequest, Location, NoteProposal, NotesSetup } from '../../models';
import { locationFromPathInput, pathInputFromLocation } from '../map/map-settings';
import { parentLocation } from '../navigation/navigation';
import type { OperationsController } from '../operations/operations-controller';
import {
  fileNameFor,
  type NoteReview,
  type NotesContinuation,
  type NotesState,
  parseTags,
  reviewForDraft,
  semanticIndexingAvailable,
  withNoteMeta,
} from './notes-state';

const OPERATION_POLL_MS = 250;
const OPERATION_POLL_LIMIT = 80;

export type NotesClientNeeds = Pick<
  FileManagerClient,
  | 'getNotesSetup'
  | 'setNotesRoot'
  | 'draftNote'
  | 'readNote'
  | 'saveEditableFile'
  | 'listOperations'
  | 'getSemanticLibraryCapabilities'
  | 'getSemanticComponentStatus'
>;

export interface NotesControllerContext {
  getClient(): NotesClientNeeds;
  getOps(): Pick<OperationsController, 'createDirectory' | 'createTextFile'>;
  getState(): NotesState;
  setState(state: NotesState): void;
  /** The active workspace, whose consent semantic indexing is recorded under. */
  getWorkspaceId(): string | undefined;
  /** Shows `location` in the active pane, so the consent step can include that folder. */
  showFolder(location: Location): Promise<void>;
  toast(message: string): void;
  redraw(): void;
}

/** What the person may change in a review. */
export interface NoteReviewEdit {
  readonly title?: string;
  readonly tags?: string;
  readonly fileName?: string;
  readonly content?: string;
  readonly view?: NoteReview['view'];
}

export interface NotesController {
  /** Opens setup; `next` continues once the folder is chosen. */
  openSetup(next?: NotesContinuation): Promise<void>;
  setSetupFolder(folder: string): void;
  setSetupIndex(index: boolean): void;
  confirmSetup(): Promise<void>;
  /** Called when the inline semantic consent step ends, confirmed or not. */
  finishEnrolment(): Promise<void>;
  /** "Save as note" in the terminal: sets notes up first when needed, then drafts. */
  saveTerminalSession(request: DraftNoteRequest): Promise<void>;
  /** A note the terminal assistant proposed. */
  showProposal(proposal: NoteProposal): Promise<void>;
  editReview(edit: NoteReviewEdit): void;
  saveReview(): Promise<void>;
  close(): void;
}

/** The text of a chat failure (`consentRequired`, …) or any other error. */
function draftFailureText(error: unknown): string {
  switch (errorText(error)) {
    case 'roleNotAssigned':
      return t('terminal', 'assistRoleNotAssigned');
    case 'consentRequired':
      return t('terminal', 'assistConsentRequired');
    case 'empty':
      return t('terminal', 'assistEmpty');
    case 'invalidRequest':
      return t('notes', 'nothingToSave');
    default:
      return t('notes', 'draftFailed');
  }
}

/** The location typed in the folder field: a known folder shown as text, or a local path. */
function folderLocation(
  text: string,
  known: readonly (Location | null | undefined)[],
): Location | null {
  const trimmed = text.trim();
  const match = known.find(
    (location) => location != null && pathInputFromLocation(location) === trimmed,
  );
  return match ?? locationFromPathInput(trimmed);
}

function lastName(location: Location): string {
  const segment = location.uri.replace(/\/+$/, '').split('/').at(-1) ?? '';
  try {
    return decodeURIComponent(segment);
  } catch {
    return segment;
  }
}

export function createNotesController(context: NotesControllerContext): NotesController {
  let knownSetup: NotesSetup | undefined;

  function set(state: NotesState): void {
    context.setState(state);
    context.redraw();
  }

  function patchSetup(change: Partial<Extract<NotesState, { phase: 'setup' }>>): void {
    const state = context.getState();
    if (state.phase === 'setup') set({ ...state, ...change });
  }

  function patchReview(change: Partial<NoteReview>): void {
    const state = context.getState();
    if (state.phase === 'review') set({ ...state, ...change });
  }

  async function semanticAvailable(): Promise<boolean> {
    try {
      const client = context.getClient();
      const [capabilities, status] = await Promise.all([
        client.getSemanticLibraryCapabilities(),
        client.getSemanticComponentStatus(),
      ]);
      return semanticIndexingAvailable(capabilities, status);
    } catch {
      return false;
    }
  }

  async function waitForOperation(id: string): Promise<boolean> {
    for (let attempt = 0; attempt < OPERATION_POLL_LIMIT; attempt += 1) {
      const operation = (await context.getClient().listOperations()).find((item) => item.id === id);
      if (operation?.state === 'completed') return true;
      if (operation?.state === 'failed' || operation?.state === 'cancelled') return false;
      await new Promise((resolve) => setTimeout(resolve, OPERATION_POLL_MS));
    }
    return false;
  }

  /** Stores the folder, creating it first through the operation engine when it is missing. */
  async function storeRoot(location: Location): Promise<NotesSetup> {
    try {
      return await context.getClient().setNotesRoot(location);
    } catch (error) {
      const operation = await context
        .getOps()
        .createDirectory(parentLocation(location), lastName(location), true);
      if (!(await waitForOperation(operation.id))) throw error;
      return context.getClient().setNotesRoot(location);
    }
  }

  async function continueWith(root: Location, next: NotesContinuation | undefined): Promise<void> {
    if (next === undefined) {
      set({ phase: 'closed' });
      context.toast(t('notes', 'setupDone'));
      return;
    }
    if (next.kind === 'draft') await draft(root, next.request);
    else await review(root, next.proposal);
  }

  async function draft(root: Location, request: DraftNoteRequest): Promise<void> {
    set({ phase: 'drafting' });
    try {
      const result = await context.getClient().draftNote(request);
      if (context.getState().phase !== 'drafting') return;
      set({ phase: 'review', ...reviewForDraft(root, result) });
    } catch (error) {
      if (context.getState().phase !== 'drafting') return;
      set({ phase: 'closed' });
      context.toast(draftFailureText(error));
    }
  }

  async function review(root: Location, proposal: NoteProposal): Promise<void> {
    const base = reviewForDraft(root, proposal.draft);
    if (proposal.path === undefined) {
      set({ phase: 'review', ...base });
      return;
    }
    try {
      const note = await context.getClient().readNote(proposal.path);
      set({
        phase: 'review',
        ...base,
        fileName: note.path,
        view: 'changes',
        replace: {
          path: note.path,
          location: note.location,
          revision: note.revision,
          original: note.content,
        },
      });
    } catch (error) {
      set({ phase: 'closed' });
      context.toast(errorText(error));
    }
  }

  /** The configured folder, or `undefined` when setup is needed first. */
  async function readyRoot(): Promise<Location | undefined> {
    knownSetup = await context.getClient().getNotesSetup();
    return knownSetup.root != null && knownSetup.rootExists ? knownSetup.root : undefined;
  }

  const controller: NotesController = {
    async openSetup(next) {
      try {
        knownSetup = await context.getClient().getNotesSetup();
      } catch (error) {
        context.toast(errorText(error));
        return;
      }
      const available = await semanticAvailable();
      const folder = knownSetup.root ?? knownSetup.suggestedRoot;
      set({
        phase: 'setup',
        folder: folder == null ? '' : pathInputFromLocation(folder),
        index: available,
        semanticAvailable: available,
        busy: false,
        ...(next === undefined ? {} : { next }),
      });
    },

    setSetupFolder(folder) {
      patchSetup({ folder, error: undefined });
    },

    setSetupIndex(index) {
      patchSetup({ index });
    },

    async confirmSetup() {
      const state = context.getState();
      if (state.phase !== 'setup' || state.busy) return;
      const location = folderLocation(state.folder, [knownSetup?.root, knownSetup?.suggestedRoot]);
      if (location === null) {
        patchSetup({ error: t('notes', 'folderInvalid') });
        return;
      }
      patchSetup({ busy: true, error: undefined });
      let stored: NotesSetup;
      try {
        stored = await storeRoot(location);
      } catch (error) {
        patchSetup({ busy: false, error: errorText(error) });
        return;
      }
      knownSetup = stored;
      const root = stored.root ?? location;
      const workspaceId = context.getWorkspaceId();
      if (state.index && state.semanticAvailable && workspaceId !== undefined) {
        // Consent is only accepted for the folder the active pane shows.
        await context.showFolder(root);
        set({ phase: 'enrol', root, ...(state.next === undefined ? {} : { next: state.next }) });
        return;
      }
      await continueWith(root, state.next);
    },

    async finishEnrolment() {
      const state = context.getState();
      if (state.phase !== 'enrol') return;
      await continueWith(state.root, state.next);
    },

    async saveTerminalSession(request) {
      try {
        const root = await readyRoot();
        if (root === undefined) await controller.openSetup({ kind: 'draft', request });
        else await draft(root, request);
      } catch (error) {
        context.toast(errorText(error));
      }
    },

    async showProposal(proposal) {
      try {
        const root = await readyRoot();
        if (root === undefined) await controller.openSetup({ kind: 'proposal', proposal });
        else await review(root, proposal);
      } catch (error) {
        context.toast(errorText(error));
      }
    },

    editReview(edit) {
      const state = context.getState();
      if (state.phase !== 'review') return;
      const title = edit.title ?? state.title;
      const tags = edit.tags ?? state.tags;
      let content = edit.content ?? state.content;
      if (edit.title !== undefined || edit.tags !== undefined) {
        content = withNoteMeta(content, title, parseTags(tags));
      }
      const fileNameEdited = state.fileNameEdited || edit.fileName !== undefined;
      const fileName =
        edit.fileName ??
        (state.replace === undefined && !fileNameEdited && edit.title !== undefined
          ? fileNameFor(title)
          : state.fileName);
      set({
        ...state,
        title,
        tags,
        content,
        fileName,
        fileNameEdited,
        ...(edit.view === undefined ? {} : { view: edit.view }),
        error: undefined,
      });
    },

    async saveReview() {
      const state = context.getState();
      if (state.phase !== 'review' || state.busy) return;
      const fileName = state.fileName.trim();
      if (state.replace === undefined && (fileName === '' || /[\\/]/.test(fileName))) {
        patchReview({ error: t('notes', 'fileNameInvalid') });
        return;
      }
      patchReview({ busy: true, error: undefined });
      try {
        if (state.replace !== undefined) {
          await context.getClient().saveEditableFile({
            location: state.replace.location,
            content: state.content,
            expectedRevision: state.replace.revision,
            overwriteConflict: false,
          });
          set({ phase: 'closed' });
          context.toast(t('notes', 'saved', { name: state.replace.path }));
          return;
        }
        const name = /\.(md|markdown|txt)$/i.test(fileName) ? fileName : `${fileName}.md`;
        const operation = await context.getOps().createTextFile(state.root, name, state.content);
        set({ phase: 'closed' });
        // An existing file of that name raises the usual conflict question; nothing is replaced
        // without the person's answer.
        if (await waitForOperation(operation.id)) context.toast(t('notes', 'saved', { name }));
      } catch (error) {
        patchReview({ busy: false, error: errorText(error) });
      }
    },

    close() {
      set({ phase: 'closed' });
    },
  };
  return controller;
}
