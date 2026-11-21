import type {
  DraftNoteRequest,
  Location,
  Note,
  NoteDraft,
  NoteHit,
  NotesSearchResult,
  NotesSetup,
  SearchNotesRequest,
} from '../../models';
import { desktopHostRequiredError } from '../fetch-mutator';
import type { MockContext } from './mock-context';
import { MockClientError } from './mock-shared';
import type { NotesClient } from './notes-client';

const SUGGESTED_ROOT: Location = { providerId: 'mock', uri: 'mock:///Procyon%20Notes' };

/** Sample notes for the mock host; saving a note there does not add to them. */
const SAMPLE_NOTES: Readonly<Record<string, string>> = {
  'ops/replace-nginx-certificate.md':
    '---\ntitle: Replace nginx TLS certificate\ntags: [nginx, tls]\n---\n' +
    '# Replace nginx TLS certificate\n\n## Steps\n1. Copy the chain:\n' +
    '```bash\nsudo cp fullchain.pem /etc/nginx/certs/\n```\n2. Check and reload:\n' +
    '```bash\nsudo nginx -t && sudo systemctl reload nginx\n```\n',
  'create-a-csr.md':
    '---\ntitle: Create a CSR\ntags: [openssl, tls]\n---\n# Create a CSR\n\n' +
    '```bash\nopenssl req -new -newkey rsa:2048 -nodes -keyout <domain>.key -out <domain>.csr\n```\n',
};

function frontMatterValue(text: string, key: string): string | undefined {
  const match = new RegExp(`^${key}:\\s*(.+)$`, 'm').exec(text);
  return match?.[1]?.trim();
}

/** In-memory notes for `pnpm dev:mock`: a fixed folder, sample notes and a plain text search. */
export class MockNotesClient implements NotesClient {
  private root: Location | undefined;

  constructor(private readonly ctx: MockContext) {}

  private setup(): NotesSetup {
    return {
      root: this.root ?? null,
      rootExists: this.root !== undefined,
      suggestedRoot: SUGGESTED_ROOT,
    };
  }

  private noteLocation(path: string): Location {
    const base = (this.root ?? SUGGESTED_ROOT).uri.replace(/\/$/, '');
    return {
      providerId: 'mock',
      uri: `${base}/${path.split('/').map(encodeURIComponent).join('/')}`,
    };
  }

  getNotesSetup(signal?: AbortSignal): Promise<NotesSetup> {
    return this.ctx.perform('getNotesSetup', signal, () => this.setup());
  }

  setNotesRoot(location: Location, signal?: AbortSignal): Promise<NotesSetup> {
    return this.ctx.perform('setNotesRoot', signal, () => {
      this.root = location;
      return this.setup();
    });
  }

  searchNotes(request: SearchNotesRequest, signal?: AbortSignal): Promise<NotesSearchResult> {
    return this.ctx.perform('searchNotes', signal, () => {
      if (this.root === undefined) {
        throw new MockClientError('invalidRequest', 'Notes are not set up yet.');
      }
      const terms = request.query
        .toLowerCase()
        .split(/[^\p{L}\p{N}]+/u)
        .filter((term) => term.length >= 2);
      const hits: NoteHit[] = [];
      for (const [path, text] of Object.entries(SAMPLE_NOTES)) {
        const lower = text.toLowerCase();
        const score = terms.filter((term) => lower.includes(term)).length;
        if (score === 0) continue;
        const tags =
          frontMatterValue(text, 'tags')
            ?.replace(/^\[|\]$/g, '')
            .split(/,\s*/) ?? [];
        hits.push({
          location: this.noteLocation(path),
          path,
          title: frontMatterValue(text, 'title') ?? path,
          tags,
          snippet:
            text.split('\n').find((line) => terms.some((t) => line.toLowerCase().includes(t))) ??
            '',
          score,
        });
      }
      hits.sort((a, b) => b.score - a.score || a.path.localeCompare(b.path));
      return { strategy: 'text', hits: hits.slice(0, request.limit ?? 8) };
    });
  }

  readNote(path: string, signal?: AbortSignal): Promise<Note> {
    return this.ctx.perform('readNote', signal, () => {
      const content = SAMPLE_NOTES[path];
      if (content === undefined) {
        throw new MockClientError(
          'invalidRequest',
          `${path} is not available in the notes folder.`,
        );
      }
      return {
        location: this.noteLocation(path),
        path,
        content,
        revision: String(content.length),
      };
    });
  }

  /** Drafting follows the terminal, which the mock host does not have. */
  draftNote(_request: DraftNoteRequest, _signal?: AbortSignal): Promise<NoteDraft> {
    return Promise.reject(desktopHostRequiredError());
  }
}
