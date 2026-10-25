import { describe, expect, it, vi } from 'vitest';
import { ApiError } from '../../api/fetch-mutator';
import type { EntrySummary } from '../../models';
import { createFileEditorController, type FileEditorState } from './file-editor-controller';

const entry: EntrySummary = {
  id: 'note',
  location: { providerId: 'local', uri: 'file:///tmp/note.json' },
  name: 'note.json',
  extension: 'json',
  kind: 'file',
  hidden: false,
  readOnly: false,
  metadataRevision: 1,
};

describe('file editor controller', () => {
  it('opens Markdown in source-editing mode before preview is toggled', async () => {
    const states: FileEditorState[] = [];
    const controller = createFileEditorController({
      client: {
        loadEditableFile: vi
          .fn()
          .mockResolvedValue({ content: '# Title', revision: 'r1', size: 7 }),
        saveEditableFile: vi.fn(),
      },
      entry: { ...entry, name: 'README.md', extension: 'md' },
      update: (state) => states.push(state),
    });

    await vi.waitFor(() => expect(states.at(-1)?.status).toBe('ready'));
    expect(states.at(-1)).toMatchObject({ language: 'markdown', previewVisible: false });
    controller.dispose();
  });

  it('toggles the GeoJSON map editor without touching the content or the dirty flag', async () => {
    const states: FileEditorState[] = [];
    const controller = createFileEditorController({
      client: {
        loadEditableFile: vi
          .fn()
          .mockResolvedValue({ content: '{"a":1}', revision: 'r1', size: 7 }),
        saveEditableFile: vi.fn(),
      },
      entry,
      update: (state) => states.push(state),
    });
    await vi.waitFor(() => expect(states.at(-1)?.status).toBe('ready'));
    controller.toggleMapMode();
    expect(states.at(-1)).toMatchObject({ mapMode: true, dirty: false, content: '{"a":1}' });
    controller.toggleMapMode();
    expect(states.at(-1)).toMatchObject({ mapMode: false });
    controller.dispose();
  });

  it('tracks dirtiness, formats JSON, and saves with the loaded revision', async () => {
    const states: FileEditorState[] = [];
    const client = {
      loadEditableFile: vi.fn().mockResolvedValue({ content: '{"a":1}', revision: 'r1', size: 7 }),
      saveEditableFile: vi
        .fn()
        .mockResolvedValue({ revision: 'r2', size: 13, overwroteConflict: false }),
    };
    const controller = createFileEditorController({
      client,
      entry,
      update: (state) => states.push(state),
    });
    await vi.waitFor(() => expect(states.at(-1)?.status).toBe('ready'));
    controller.formatJson();
    expect(states.at(-1)).toMatchObject({ dirty: true, content: '{\n  "a": 1\n}\n' });
    await controller.save();
    expect(client.saveEditableFile).toHaveBeenCalledWith(
      expect.objectContaining({ expectedRevision: 'r1' }),
    );
    expect(states.at(-1)).toMatchObject({ dirty: false, revision: 'r2' });
  });

  it('requires a close decision when dirty and exposes revision conflicts', async () => {
    const states: FileEditorState[] = [];
    const client = {
      loadEditableFile: vi.fn().mockResolvedValue({ content: '{}', revision: 'r1', size: 2 }),
      saveEditableFile: vi.fn().mockRejectedValue(
        new ApiError(409, {
          code: 'fileRevisionConflict',
          message: 'The file changed after it was loaded.',
        }),
      ),
    };
    const controller = createFileEditorController({
      client,
      entry,
      update: (state) => states.push(state),
    });
    await vi.waitFor(() => expect(states.at(-1)?.status).toBe('ready'));
    controller.setContent('{"x":1}');
    expect(controller.requestClose()).toBe(false);
    await controller.save();
    expect(states.at(-1)).toMatchObject({ conflict: true, dirty: true });
  });

  it('localizes binary-file load failures', async () => {
    const states: FileEditorState[] = [];
    const controller = createFileEditorController({
      client: {
        loadEditableFile: vi.fn().mockRejectedValue(
          new ApiError(400, {
            code: 'invalidRequest',
            message: 'binary files cannot be edited in-app; use the external editor',
            details: { reason: 'editorBinaryFile' },
          }),
        ),
        saveEditableFile: vi.fn(),
      },
      entry,
      update: (state) => states.push(state),
    });

    await vi.waitFor(() => expect(states.at(-1)?.status).toBe('error'));
    expect(states.at(-1)).toMatchObject({
      status: 'error',
      message: 'Binary files cannot be edited in Procyon.',
    });
    controller.dispose();
  });
});

describe('editor links (task 0271)', () => {
  const entry = {
    id: 'e',
    name: 'log.txt',
    extension: 'txt',
    kind: 'file',
    hidden: false,
    readOnly: false,
    metadataRevision: 1,
    location: { providerId: 'local', uri: 'file:///log.txt' },
  } as const;

  function create(initialLine?: number) {
    const states: FileEditorState[] = [];
    const controller = createFileEditorController({
      client: {
        loadEditableFile: vi.fn().mockResolvedValue({
          content: 'first\nsee /tmp/a.txt:3 here\nthird',
          revision: 'r1',
        }),
        saveEditableFile: vi.fn(),
      },
      entry,
      ...(initialLine === undefined ? {} : { initialLine }),
      update: (state) => states.push(state),
    });
    return { controller, states };
  }

  it('selects the requested line when the file opens', async () => {
    const { states } = create(2);
    await vi.waitFor(() => expect(states.at(-1)?.status).toBe('ready'));
    const ready = states.at(-1);
    expect(ready?.status === 'ready' && ready.initialSelection).toEqual({ from: 6, to: 27 });
  });

  it('has no initial selection without a line or for a line past the end', async () => {
    for (const line of [undefined, 99]) {
      const { states } = create(line);
      await vi.waitFor(() => expect(states.at(-1)?.status).toBe('ready'));
      const ready = states.at(-1);
      expect(ready?.status === 'ready' && ready.initialSelection).toBeFalsy();
    }
  });

  it('reports the path under the caret', async () => {
    const { controller, states } = create();
    await vi.waitFor(() => expect(states.at(-1)?.status).toBe('ready'));
    controller.setCursor(12);
    expect(controller.linkAtCursor()).toMatchObject({ kind: 'path', path: '/tmp/a.txt', line: 3 });
    controller.setCursor(1);
    expect(controller.linkAtCursor()).toBeUndefined();
  });
});
