import { describe, expect, it, vi } from 'vitest';
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
      saveEditableFile: vi
        .fn()
        .mockRejectedValue(new Error('fileRevisionConflict: changed after opening')),
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
});
