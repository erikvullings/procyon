import { describe, expect, it, vi } from 'vitest';
import type { Location, NoteDraft, NotesSetup, Operation } from '../../models';
import { createNotesController, type NotesClientNeeds } from './notes-controller';
import { INITIAL_NOTES_STATE, type NotesState } from './notes-state';

const ROOT: Location = { providerId: 'local', uri: 'file:///Users/me/Documents/Procyon%20Notes' };

const DRAFT: NoteDraft = {
  title: 'Replace nginx TLS certificate',
  fileName: 'replace-nginx-tls-certificate.md',
  tags: ['nginx', 'tls'],
  content: '---\ntitle: Replace nginx TLS certificate\ntags: [nginx, tls]\n---\n# Replace\n',
  redactions: 1,
};

function operation(id: string, state: Operation['state']): Operation {
  return { id, state } as unknown as Operation;
}

function setup(change: Partial<NotesSetup> = {}): NotesSetup {
  return { root: null, rootExists: false, suggestedRoot: ROOT, ...change };
}

function harness(options: {
  setup?: NotesSetup;
  semantic?: boolean;
  setRoot?: (location: Location) => Promise<NotesSetup>;
  workspaceId?: string | undefined;
}) {
  let state: NotesState = INITIAL_NOTES_STATE;
  const toasts: string[] = [];
  const client = {
    getNotesSetup: vi.fn(async () => options.setup ?? setup()),
    setNotesRoot: vi.fn(
      options.setRoot ??
        (async (location: Location) => setup({ root: location, rootExists: true })),
    ),
    draftNote: vi.fn(async () => DRAFT),
    readNote: vi.fn(async (path: string) => ({
      location: { providerId: 'local', uri: `${ROOT.uri}/${path}` },
      path,
      content: '# Old\n',
      revision: 'rev-1',
    })),
    saveEditableFile: vi.fn(async () => ({ revision: 'rev-2', size: 1, overwroteConflict: false })),
    listOperations: vi.fn(async () => [operation('op-1', 'completed')]),
    getSemanticLibraryCapabilities: vi.fn(async () => ({
      authority: 'desktopManaged',
      operations: options.semantic === false ? ['viewStatus'] : ['previewEnrolment', 'enrol'],
    })),
    getSemanticComponentStatus: vi.fn(async () => ({ lifecycle: { state: 'installedEnabled' } })),
  };
  const ops = {
    createDirectory: vi.fn(async () => operation('op-1', 'running')),
    createTextFile: vi.fn(async () => operation('op-1', 'running')),
  };
  const shown: Location[] = [];
  const controller = createNotesController({
    getClient: () => client as unknown as NotesClientNeeds,
    getOps: () => ops as never,
    getState: () => state,
    setState: (next) => {
      state = next;
    },
    getWorkspaceId: () => ('workspaceId' in options ? options.workspaceId : 'ws-1'),
    showFolder: async (location) => {
      shown.push(location);
    },
    toast: (message) => toasts.push(message),
    redraw: () => undefined,
  });
  return { controller, client, ops, toasts, shown: () => shown, state: () => state };
}

const REQUEST = { blocks: [], workingDirectory: 'file:///etc/nginx/', sessionId: 't1' };

describe('notes setup', () => {
  it('suggests the default folder and offers indexing only when semantic search is installed', async () => {
    const withSemantic = harness({});
    await withSemantic.controller.openSetup();
    expect(withSemantic.state()).toMatchObject({
      phase: 'setup',
      folder: '/Users/me/Documents/Procyon Notes',
      index: true,
      semanticAvailable: true,
    });

    const without = harness({ semantic: false });
    await without.controller.openSetup();
    expect(without.state()).toMatchObject({ index: false, semanticAvailable: false });
  });

  it('creates a missing folder through an operation, then stores it and asks for indexing consent', async () => {
    let attempts = 0;
    const h = harness({
      setRoot: async (location) => {
        attempts += 1;
        if (attempts === 1) throw new Error('notesRootUnavailable');
        return setup({ root: location, rootExists: true });
      },
    });
    await h.controller.openSetup();
    h.controller.setSetupFolder('/Users/me/Notes');
    await h.controller.confirmSetup();

    expect(h.ops.createDirectory).toHaveBeenCalledWith(
      { providerId: 'local', uri: 'file:///Users/me' },
      'Notes',
      true,
    );
    expect(h.client.setNotesRoot).toHaveBeenCalledTimes(2);
    expect(h.shown()).toEqual([{ providerId: 'local', uri: 'file:///Users/me/Notes' }]);
    // Indexing is never confirmed here: the inline consent step decides.
    expect(h.state()).toEqual({
      phase: 'enrol',
      root: { providerId: 'local', uri: 'file:///Users/me/Notes' },
    });
    await h.controller.finishEnrolment();
    expect(h.state()).toEqual({ phase: 'closed' });
    expect(h.toasts).toEqual(['Notes are set up.']);
  });

  it('skips the consent step when indexing is unchecked or no workspace is active', async () => {
    const unchecked = harness({});
    await unchecked.controller.openSetup();
    unchecked.controller.setSetupIndex(false);
    await unchecked.controller.confirmSetup();
    expect(unchecked.state()).toEqual({ phase: 'closed' });

    const noWorkspace = harness({ workspaceId: undefined });
    await noWorkspace.controller.openSetup();
    await noWorkspace.controller.confirmSetup();
    expect(noWorkspace.state()).toEqual({ phase: 'closed' });
  });

  it('rejects a folder that is not a path', async () => {
    const h = harness({});
    await h.controller.openSetup();
    h.controller.setSetupFolder('relative/folder');
    await h.controller.confirmSetup();
    expect(h.state()).toMatchObject({ phase: 'setup', error: 'Enter a full folder path.' });
    expect(h.client.setNotesRoot).not.toHaveBeenCalled();
  });
});

describe('saving a terminal session', () => {
  it('sets notes up first, then drafts and opens the review', async () => {
    const h = harness({ semantic: false });
    await h.controller.saveTerminalSession(REQUEST);
    expect(h.state()).toMatchObject({ phase: 'setup', next: { kind: 'draft' } });

    await h.controller.confirmSetup();
    expect(h.client.draftNote).toHaveBeenCalledWith(REQUEST);
    expect(h.state()).toMatchObject({
      phase: 'review',
      title: DRAFT.title,
      tags: 'nginx, tls',
      fileName: DRAFT.fileName,
      redactions: 1,
      view: 'edit',
    });
  });

  it('drafts straight away when notes are set up and reports model failures', async () => {
    const h = harness({ setup: setup({ root: ROOT, rootExists: true }) });
    h.client.draftNote.mockRejectedValueOnce(new Error('consentRequired'));
    await h.controller.saveTerminalSession(REQUEST);
    expect(h.state()).toEqual({ phase: 'closed' });
    expect(h.toasts[0]).toMatch(/consent/i);
  });
});

describe('reviewing and saving', () => {
  it('keeps title, tags, front matter and file name in step until the file name is typed', async () => {
    const h = harness({ setup: setup({ root: ROOT, rootExists: true }) });
    await h.controller.saveTerminalSession(REQUEST);
    h.controller.editReview({ title: 'Renew certificates', tags: 'tls, certbot' });
    expect(h.state()).toMatchObject({ fileName: 'renew-certificates.md' });
    const state = h.state();
    expect(state.phase === 'review' && state.content).toContain(
      'title: Renew certificates\ntags: [tls, certbot]',
    );
    h.controller.editReview({ fileName: 'mine.md' });
    h.controller.editReview({ title: 'Other' });
    expect(h.state()).toMatchObject({ fileName: 'mine.md' });
  });

  it('creates a new note with the createFile operation, which asks before replacing a file', async () => {
    const h = harness({ setup: setup({ root: ROOT, rootExists: true }) });
    await h.controller.saveTerminalSession(REQUEST);
    h.controller.editReview({ fileName: 'renew' });
    await h.controller.saveReview();
    expect(h.ops.createTextFile).toHaveBeenCalledWith(ROOT, 'renew.md', DRAFT.content);
    expect(h.state()).toEqual({ phase: 'closed' });
    expect(h.toasts).toEqual(['Saved renew.md.']);
  });

  it('refuses a file name with a folder in it', async () => {
    const h = harness({ setup: setup({ root: ROOT, rootExists: true }) });
    await h.controller.saveTerminalSession(REQUEST);
    h.controller.editReview({ fileName: '../x.md' });
    await h.controller.saveReview();
    expect(h.ops.createTextFile).not.toHaveBeenCalled();
    expect(h.state()).toMatchObject({ phase: 'review', error: expect.any(String) });
  });

  it('shows the changes of a proposed update and saves it with the revision it was read at', async () => {
    const h = harness({ setup: setup({ root: ROOT, rootExists: true }) });
    await h.controller.showProposal({
      path: 'ops/nginx.md',
      draft: { ...DRAFT, content: '# New\n' },
    });
    expect(h.client.readNote).toHaveBeenCalledWith('ops/nginx.md');
    expect(h.state()).toMatchObject({
      phase: 'review',
      view: 'changes',
      fileName: 'ops/nginx.md',
      replace: { path: 'ops/nginx.md', original: '# Old\n', revision: 'rev-1' },
    });
    await h.controller.saveReview();
    expect(h.client.saveEditableFile).toHaveBeenCalledWith({
      location: { providerId: 'local', uri: `${ROOT.uri}/ops/nginx.md` },
      content: '# New\n',
      expectedRevision: 'rev-1',
      overwriteConflict: false,
    });
    expect(h.state()).toEqual({ phase: 'closed' });
  });

  it('keeps the review open with the error when the note changed meanwhile', async () => {
    const h = harness({ setup: setup({ root: ROOT, rootExists: true }) });
    await h.controller.showProposal({ path: 'ops/nginx.md', draft: DRAFT });
    h.client.saveEditableFile.mockRejectedValueOnce(
      new Error('The file changed after it was loaded.'),
    );
    await h.controller.saveReview();
    expect(h.state()).toMatchObject({
      phase: 'review',
      busy: false,
      error: 'The file changed after it was loaded.',
    });
  });
});
