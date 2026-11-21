/**
 * Notes flow state (task 0337) and the pure helpers behind it: editing a draft's title and tags
 * in its front matter, the file name a title suggests, and the line diff shown before a note is
 * replaced. Nothing here talks to the backend.
 */
import type {
  DraftNoteRequest,
  Location,
  NoteDraft,
  NoteProposal,
  SemanticComponentStatus,
  SemanticLibraryCapabilities,
} from '../../models';

/** What happens once notes are set up: draft from the terminal, or review a proposal. */
export type NotesContinuation =
  | { readonly kind: 'draft'; readonly request: DraftNoteRequest }
  | { readonly kind: 'proposal'; readonly proposal: NoteProposal };

/** A note being reviewed. `replace` is set when it changes an existing note. */
export interface NoteReview {
  readonly root: Location;
  readonly title: string;
  readonly tags: string;
  readonly fileName: string;
  /** Whether the person typed a file name, so title edits stop changing it. */
  readonly fileNameEdited: boolean;
  readonly content: string;
  readonly redactions: number;
  readonly replace?: {
    readonly path: string;
    readonly location: Location;
    readonly revision: string;
    readonly original: string;
  };
  readonly view: 'edit' | 'preview' | 'changes';
  readonly busy: boolean;
  readonly error?: string | undefined;
}

export type NotesState =
  | { readonly phase: 'closed' }
  | {
      readonly phase: 'setup';
      readonly folder: string;
      readonly index: boolean;
      readonly semanticAvailable: boolean;
      readonly busy: boolean;
      readonly error?: string | undefined;
      readonly next?: NotesContinuation;
    }
  | { readonly phase: 'enrol'; readonly root: Location; readonly next?: NotesContinuation }
  | { readonly phase: 'drafting' }
  | ({ readonly phase: 'review' } & NoteReview);

export const INITIAL_NOTES_STATE: NotesState = { phase: 'closed' };

/** Longest slug, in characters; mirrors the backend. */
const MAX_SLUG = 60;

const FOLDED: Readonly<Record<string, string>> = { ß: 'ss', æ: 'ae', œ: 'oe', ł: 'l', ø: 'o' };

/** A file name stem for a title: lowercase ASCII words joined by hyphens, `note` when empty. */
export function slugify(title: string): string {
  const plain = title
    .toLowerCase()
    .replace(/[ßæœłø]/g, (character) => FOLDED[character] ?? character)
    .normalize('NFKD')
    .replace(/[̀-ͯ]/g, '');
  let slug = plain
    .split(/[^a-z0-9]+/)
    .filter((word) => word !== '')
    .join('-');
  if (slug.length > MAX_SLUG) slug = slug.slice(0, MAX_SLUG).replace(/-+$/, '');
  return slug === '' ? 'note' : slug;
}

/** The file name a title suggests. */
export function fileNameFor(title: string): string {
  return `${slugify(title)}.md`;
}

/** Comma-separated tags as a clean list: trimmed, lowercase, without `#` or repeats. */
export function parseTags(text: string): string[] {
  const tags: string[] = [];
  for (const raw of text.split(',')) {
    const tag = raw.trim().replace(/^#+/, '').trim().toLowerCase();
    if (tag !== '' && !tags.includes(tag)) tags.push(tag);
  }
  return tags;
}

function yamlValue(value: string): string {
  return /^[\s\-?!&*|>%@`]|[:#,[\]{}"'\n]|\s$/.test(value) || value === ''
    ? `"${value.replaceAll('\\', '\\\\').replaceAll('"', '\\"')}"`
    : value;
}

/**
 * `content` with `title` and `tags` set in its front matter. Other keys stay as they were; a note
 * without front matter gets one.
 */
export function withNoteMeta(content: string, title: string, tags: readonly string[]): string {
  const titleLine = `title: ${yamlValue(title.trim())}`;
  const tagsLine = tags.length === 0 ? undefined : `tags: [${tags.map(yamlValue).join(', ')}]`;
  const match = /^---\r?\n([\s\S]*?)\r?\n---\r?\n?/.exec(content);
  if (match === null) {
    return ['---', titleLine, ...(tagsLine === undefined ? [] : [tagsLine]), '---', content].join(
      '\n',
    );
  }
  const lines = (match[1] ?? '').split(/\r?\n/);
  const kept: string[] = [];
  let skippingList = false;
  for (const line of lines) {
    if (skippingList && /^\s+-\s|^-\s/.test(line)) continue;
    skippingList = false;
    if (/^title\s*:/i.test(line)) continue;
    if (/^tags\s*:/i.test(line)) {
      skippingList = /^tags\s*:\s*$/i.test(line);
      continue;
    }
    kept.push(line);
  }
  const front = [titleLine, ...(tagsLine === undefined ? [] : [tagsLine]), ...kept].join('\n');
  return `---\n${front}\n---\n${content.slice(match[0].length)}`;
}

/** A review of a draft for a new note in `root`. */
export function reviewForDraft(root: Location, draft: NoteDraft): NoteReview {
  return {
    root,
    title: draft.title,
    tags: draft.tags.join(', '),
    fileName: draft.fileName,
    fileNameEdited: false,
    content: draft.content,
    redactions: draft.redactions,
    view: 'edit',
    busy: false,
  };
}

/** One line of a diff. */
export interface DiffLine {
  readonly kind: 'same' | 'added' | 'removed';
  readonly text: string;
}

/** Most lines compared exactly; longer notes are shown as removed-then-added. */
const MAX_DIFF_LINES = 2000;

/** The lines removed from `before` and added in `after`, in order (longest common subsequence). */
export function lineDiff(before: string, after: string): DiffLine[] {
  const a = before.split('\n');
  const b = after.split('\n');
  if (a.length * b.length > MAX_DIFF_LINES * MAX_DIFF_LINES) {
    return [
      ...a.map((text) => ({ kind: 'removed' as const, text })),
      ...b.map((text) => ({ kind: 'added' as const, text })),
    ];
  }
  const rows = a.length + 1;
  const columns = b.length + 1;
  const lengths = new Uint32Array(rows * columns);
  for (let i = a.length - 1; i >= 0; i -= 1) {
    for (let j = b.length - 1; j >= 0; j -= 1) {
      lengths[i * columns + j] =
        a[i] === b[j]
          ? (lengths[(i + 1) * columns + j + 1] ?? 0) + 1
          : Math.max(lengths[(i + 1) * columns + j] ?? 0, lengths[i * columns + j + 1] ?? 0);
    }
  }
  const diff: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < a.length && j < b.length) {
    if (a[i] === b[j]) {
      diff.push({ kind: 'same', text: a[i] ?? '' });
      i += 1;
      j += 1;
    } else if ((lengths[(i + 1) * columns + j] ?? 0) >= (lengths[i * columns + j + 1] ?? 0)) {
      diff.push({ kind: 'removed', text: a[i] ?? '' });
      i += 1;
    } else {
      diff.push({ kind: 'added', text: b[j] ?? '' });
      j += 1;
    }
  }
  for (; i < a.length; i += 1) diff.push({ kind: 'removed', text: a[i] ?? '' });
  for (; j < b.length; j += 1) diff.push({ kind: 'added', text: b[j] ?? '' });
  return diff;
}

/** Whether semantic indexing can be offered: enrolment is supported and the worker installed. */
export function semanticIndexingAvailable(
  capabilities: SemanticLibraryCapabilities | undefined,
  status: SemanticComponentStatus | undefined,
): boolean {
  if (capabilities === undefined || status === undefined) return false;
  const operations: readonly string[] = capabilities.operations;
  const installed =
    status.lifecycle.state === 'installedEnabled' || status.lifecycle.state === 'paused';
  return installed && operations.includes('previewEnrolment') && operations.includes('enrol');
}
