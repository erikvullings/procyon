import type { FileManagerClient } from '../../api/client/file-manager-client';
import { t } from '../../i18n';
import type { EntrySummary } from '../../models';
import { type EditableLanguage, editableLanguageForExtension } from './editor-language';
import { safeMarkdownHtml } from './markdown-preview';

export type FileEditorState =
  | { readonly status: 'loading'; readonly entry: EntrySummary }
  | { readonly status: 'error'; readonly entry: EntrySummary; readonly message: string }
  | {
      readonly status: 'ready';
      readonly entry: EntrySummary;
      readonly language: EditableLanguage;
      readonly content: string;
      readonly savedContent: string;
      readonly revision: string;
      readonly dirty: boolean;
      readonly saving: boolean;
      readonly previewHtml?: string;
      readonly previewVisible: boolean;
      readonly conflict: boolean;
      readonly closePending: boolean;
      readonly error?: string;
    };

export interface FileEditorController {
  setContent(content: string): void;
  save(overwrite?: boolean, destinationUri?: string): Promise<boolean>;
  reload(): Promise<void>;
  formatJson(): void;
  togglePreview(): void;
  requestClose(): boolean;
  cancelClose(): void;
  dispose(): void;
}

export function createFileEditorController(options: {
  client: Pick<FileManagerClient, 'loadEditableFile' | 'saveEditableFile'>;
  entry: EntrySummary;
  update: (state: FileEditorState) => void;
}): FileEditorController {
  const language = editableLanguageForExtension(options.entry.extension, options.entry.name);
  let state: FileEditorState = { status: 'loading', entry: options.entry };
  let disposed = false;
  let request: AbortController | undefined;
  let previewTimer: ReturnType<typeof setTimeout> | undefined;
  const publish = (next: FileEditorState) => {
    state = next;
    if (!disposed) options.update(next);
  };
  const schedulePreview = () => {
    if (previewTimer !== undefined) clearTimeout(previewTimer);
    if (state.status !== 'ready' || state.language !== 'markdown') return;
    previewTimer = setTimeout(() => {
      if (state.status === 'ready')
        publish({ ...state, previewHtml: safeMarkdownHtml(state.content) });
    }, 150);
  };
  const reload = async () => {
    request?.abort();
    request = new AbortController();
    publish({ status: 'loading', entry: options.entry });
    try {
      const loaded = await options.client.loadEditableFile(
        { location: options.entry.location },
        request.signal,
      );
      publish({
        status: 'ready',
        entry: options.entry,
        language,
        content: loaded.content,
        savedContent: loaded.content,
        revision: loaded.revision,
        dirty: false,
        saving: false,
        previewVisible: false,
        ...(language === 'markdown' ? { previewHtml: safeMarkdownHtml(loaded.content) } : {}),
        conflict: false,
        closePending: false,
      });
    } catch (error: unknown) {
      publish({
        status: 'error',
        entry: options.entry,
        message: error instanceof Error ? error.message : t('editor', 'unableToLoad'),
      });
    }
  };
  const setContent = (content: string) => {
    if (state.status !== 'ready') return;
    const { error: _error, ...current } = state;
    publish({ ...current, content, dirty: content !== state.savedContent });
    schedulePreview();
  };
  const save = async (overwrite = false, destinationUri?: string) => {
    if (state.status !== 'ready' || state.saving) return false;
    const snapshot = state;
    const { error: _error, ...current } = state;
    publish({ ...current, saving: true });
    try {
      const result = await options.client.saveEditableFile({
        location: state.entry.location,
        ...(destinationUri === undefined
          ? {}
          : { destination: { ...state.entry.location, uri: destinationUri } }),
        content: state.content,
        expectedRevision: state.revision,
        overwriteConflict: overwrite,
      });
      publish({
        ...snapshot,
        revision: result.revision,
        savedContent: snapshot.content,
        dirty: false,
        saving: false,
        conflict: false,
        closePending: false,
      });
      return true;
    } catch (error: unknown) {
      const conflict =
        error instanceof Error &&
        (error.message.includes('changed after') || error.message.includes('fileRevisionConflict'));
      const { error: _previousError, ...current } = snapshot;
      publish({
        ...current,
        saving: false,
        conflict,
        ...(conflict
          ? {}
          : { error: error instanceof Error ? error.message : t('editor', 'saveFailed') }),
      });
      return false;
    }
  };
  const formatJson = () => {
    if (state.status !== 'ready') return;
    try {
      setContent(`${JSON.stringify(JSON.parse(state.content), null, 2)}\n`);
    } catch {
      publish({ ...state, error: t('editor', 'invalidJson') });
    }
  };
  const togglePreview = () => {
    if (state.status === 'ready') publish({ ...state, previewVisible: !state.previewVisible });
  };
  const requestClose = () => {
    if (state.status !== 'ready' || !state.dirty) return true;
    publish({ ...state, closePending: true });
    return false;
  };
  const cancelClose = () => {
    if (state.status === 'ready') publish({ ...state, closePending: false });
  };
  void reload();
  return {
    setContent,
    save,
    reload,
    formatJson,
    togglePreview,
    requestClose,
    cancelClose,
    dispose: () => {
      disposed = true;
      request?.abort();
      if (previewTimer !== undefined) clearTimeout(previewTimer);
    },
  };
}
