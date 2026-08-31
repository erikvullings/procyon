import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { EntrySummary } from '../../models';
import { FileEditor } from './file-editor';
import type { FileEditorController, FileEditorState } from './file-editor-controller';

let root: HTMLElement;

const entry: EntrySummary = {
  id: 'readme',
  location: { providerId: 'local', uri: 'file:///tmp/README.md' },
  name: 'README.md',
  extension: 'md',
  kind: 'file',
  hidden: false,
  readOnly: false,
  metadataRevision: 1,
};

function controller(): FileEditorController {
  return {
    setContent: vi.fn(),
    save: vi.fn().mockResolvedValue(true),
    reload: vi.fn().mockResolvedValue(undefined),
    formatJson: vi.fn(),
    togglePreview: vi.fn(),
    requestClose: vi.fn().mockReturnValue(true),
    cancelClose: vi.fn(),
    dispose: vi.fn(),
  };
}

function state(previewVisible: boolean): Extract<FileEditorState, { readonly status: 'ready' }> {
  return {
    status: 'ready',
    entry,
    language: 'markdown',
    content: '# Source',
    savedContent: '# Source',
    revision: '1',
    dirty: false,
    saving: false,
    previewHtml: '<h1>Preview</h1>',
    previewVisible,
    conflict: false,
    closePending: false,
  };
}

beforeEach(() => {
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
});

describe('FileEditor', () => {
  it('replaces the editor with the markdown preview instead of splitting the pane', () => {
    m.mount(root, {
      view: () => m(FileEditor, { state: state(true), controller: controller(), onClose: vi.fn() }),
    });

    expect(root.querySelector('.fm-file-editor-preview')?.textContent).toBe('Preview');
    expect(root.querySelector('.cm-editor')).toBeNull();
    expect(root.querySelector('.fm-file-editor-header')?.textContent).toContain('Edit');
  });

  it('renders the editor and Preview action when preview is off', () => {
    m.mount(root, {
      view: () =>
        m(FileEditor, { state: state(false), controller: controller(), onClose: vi.fn() }),
    });

    expect(root.querySelector('.cm-editor')?.textContent).toContain('Source');
    expect(root.querySelector('.fm-file-editor-preview')).toBeNull();
    expect(root.querySelector('.fm-file-editor-header')?.textContent).toContain('Preview');
  });

  it('saves from Meta+S inside the editor', () => {
    const editorController = controller();
    m.mount(root, {
      view: () =>
        m(FileEditor, {
          state: { ...state(false), dirty: true },
          controller: editorController,
          onClose: vi.fn(),
        }),
    });

    root.querySelector<HTMLElement>('.cm-content')?.dispatchEvent(
      new KeyboardEvent('keydown', {
        key: 's',
        metaKey: true,
        bubbles: true,
        cancelable: true,
      }),
    );

    expect(editorController.save).toHaveBeenCalledOnce();
  });

  it('closes a saved editor from Meta+W without closing the pane tab', () => {
    const editorController = controller();
    const onClose = vi.fn();
    m.mount(root, {
      view: () =>
        m(FileEditor, {
          state: state(false),
          controller: editorController,
          onClose,
        }),
    });

    root.querySelector<HTMLElement>('.cm-content')?.dispatchEvent(
      new KeyboardEvent('keydown', {
        key: 'w',
        metaKey: true,
        bubbles: true,
        cancelable: true,
      }),
    );

    expect(editorController.requestClose).toHaveBeenCalledOnce();
    expect(onClose).toHaveBeenCalledOnce();
  });

  it('opens a save modal from Meta+W when the editor is dirty', () => {
    const editorController = controller();
    vi.mocked(editorController.requestClose).mockReturnValue(false);
    const onClose = vi.fn();
    m.mount(root, {
      view: () =>
        m(FileEditor, {
          state: { ...state(false), dirty: true },
          controller: editorController,
          onClose,
        }),
    });

    root.querySelector<HTMLElement>('.cm-content')?.dispatchEvent(
      new KeyboardEvent('keydown', {
        key: 'w',
        metaKey: true,
        bubbles: true,
        cancelable: true,
      }),
    );

    expect(editorController.requestClose).toHaveBeenCalledOnce();
    expect(onClose).not.toHaveBeenCalled();
  });

  it('defaults the unsaved changes modal to Yes and tabs to highlighted No', () => {
    m.mount(root, {
      view: () =>
        m(FileEditor, {
          state: { ...state(false), dirty: true, closePending: true },
          controller: controller(),
          onClose: vi.fn(),
        }),
    });
    m.redraw.sync();

    const yes = root.querySelector<HTMLButtonElement>('.fm-file-editor-close-save');
    const no = root.querySelector<HTMLButtonElement>('.fm-file-editor-close-discard');
    expect(document.activeElement).toBe(yes);
    expect(yes?.classList.contains('is-selected')).toBe(true);

    yes?.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true }),
    );
    m.redraw.sync();

    expect(document.activeElement).toBe(no);
    expect(no?.classList.contains('is-selected')).toBe(true);
  });

  it('saves on Yes, discards on No, and returns to the editor on Cancel', async () => {
    const editorController = controller();
    const onClose = vi.fn();
    m.mount(root, {
      view: () =>
        m(FileEditor, {
          state: { ...state(false), dirty: true, closePending: true },
          controller: editorController,
          onClose,
        }),
    });
    m.redraw.sync();

    root.querySelector<HTMLButtonElement>('.fm-file-editor-close-save')?.click();
    await vi.waitFor(() => expect(onClose).toHaveBeenCalledOnce());
    expect(editorController.save).toHaveBeenCalledOnce();

    onClose.mockClear();
    root.querySelector<HTMLButtonElement>('.fm-file-editor-close-discard')?.click();
    expect(onClose).toHaveBeenCalledOnce();

    root.querySelector<HTMLButtonElement>('.fm-file-editor-close-cancel')?.click();
    expect(editorController.cancelClose).toHaveBeenCalledOnce();
  });
});
