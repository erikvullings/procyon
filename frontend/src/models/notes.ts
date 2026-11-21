/**
 * Notes (task 0337): plain Markdown files in one folder the person chooses. Mirrors the backend's
 * `notes` transport types. New notes are written with the `createFile` operation and changes with
 * `saveEditableFile`; nothing here writes a file.
 */
import type { Location } from './location';

/** Where notes live and what setup would suggest. */
export interface NotesSetup {
  /** The configured notes folder; absent until notes are set up. */
  readonly root?: Location | null;
  /** Whether the configured folder currently exists as a folder. */
  readonly rootExists: boolean;
  /** The folder setup suggests on this host; absent on a server, which guesses none. */
  readonly suggestedRoot?: Location | null;
}

/** Which search produced the hits. */
export type NotesSearchStrategy = 'semantic' | 'text';

export interface SearchNotesRequest {
  /** Workspace whose semantic consent applies; without one only the text search runs. */
  readonly workspaceId?: string;
  readonly query: string;
  readonly limit?: number;
}

/** One matching note. */
export interface NoteHit {
  readonly location: Location;
  /** Path inside the notes folder, with `/` separators. */
  readonly path: string;
  readonly title: string;
  readonly tags: readonly string[];
  readonly snippet: string;
  readonly score: number;
}

export interface NotesSearchResult {
  readonly strategy: NotesSearchStrategy;
  readonly hits: readonly NoteHit[];
}

/** One note's text, with the revision `saveEditableFile` expects. */
export interface Note {
  readonly location: Location;
  readonly path: string;
  readonly content: string;
  readonly revision: string;
}

/** One finished terminal command included in a draft. */
export interface NoteTerminalBlock {
  readonly command: string;
  readonly exitCode: number;
  readonly output: string;
}

/** Turns a terminal assistant session into a draft note (desktop host only). */
export interface DraftNoteRequest {
  readonly sessionId?: string;
  readonly blocks: readonly NoteTerminalBlock[];
  readonly workingDirectory: string;
  readonly shell?: string;
  readonly host?: string;
  readonly titleHint?: string;
}

/** A note proposed for review. Nothing is written until the person saves it. */
export interface NoteDraft {
  readonly title: string;
  /** Suggested file name inside the notes folder, ending in `.md`. */
  readonly fileName: string;
  readonly tags: readonly string[];
  /** The complete Markdown file, front matter included. */
  readonly content: string;
  /** Values replaced by a `[redacted…]` mark, for the person to check. */
  readonly redactions: number;
}

/** A note the terminal assistant proposes to create (`path` absent) or replace. */
export interface NoteProposal {
  readonly path?: string;
  readonly draft: NoteDraft;
}
