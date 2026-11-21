import type {
  DraftNoteRequest,
  Location,
  Note,
  NoteDraft,
  NotesSearchResult,
  NotesSetup,
  SearchNotesRequest,
} from '../../models';

/**
 * Notes (task 0337). Setup, search and reading work in every host; drafting from a terminal
 * session follows the terminal and only exists in the desktop host.
 */
export interface NotesClient {
  /** Where notes live, whether that folder exists, and what setup suggests. */
  getNotesSetup(signal?: AbortSignal): Promise<NotesSetup>;
  /** Stores an existing folder as the notes folder; create it first with an operation. */
  setNotesRoot(location: Location, signal?: AbortSignal): Promise<NotesSetup>;
  /** Searches notes: semantically when the folder is indexed, by text otherwise. */
  searchNotes(request: SearchNotesRequest, signal?: AbortSignal): Promise<NotesSearchResult>;
  /** Reads one note inside the notes folder. */
  readNote(path: string, signal?: AbortSignal): Promise<Note>;
  /** Drafts a note from a terminal assistant session. Desktop only; nothing is written. */
  draftNote(request: DraftNoteRequest, signal?: AbortSignal): Promise<NoteDraft>;
}
