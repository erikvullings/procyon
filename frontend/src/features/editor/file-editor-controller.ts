import type { FileManagerClient } from '../../api/client/file-manager-client';
import { t } from '../../i18n';
import type { EntrySummary } from '../../models';
import { type EditableLanguage, editableLanguageForExtension } from './editor-language';
import { lineRange } from './link-actions';
import { type DetectedLink, detectLinkAt } from './link-detection';
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
      /** The GeoJSON map editor replaces the text view (task 0278). */
      readonly mapMode?: boolean;
      /** Range selected when the file opened at a line (task 0271). */
      readonly initialSelection?: { readonly from: number; readonly to: number };
      readonly error?: string;
    };

export interface FileEditorController {
  setContent(content: string): void;
  save(overwrite?: boolean, destinationUri?: string): Promise<boolean>;
  reload(): Promise<void>;
  formatJson(): void;
  togglePreview(): void;
  /** Switches a GeoJSON file between the text editor and the map editor (task 0278). */
  toggleMapMode(): void;
  requestClose(): boolean;
  cancelClose(): void;
  /** Records the caret position, so actions can act on the path or URL under it (task 0271). */
  setCursor(offset: number): void;
  /** The path or URL under the caret, if any. */
  linkAtCursor(): DetectedLink | undefined;
  dispose(): void;
}

/** The stable rejection reason (or error code) a backend failure carries, when it has one. */
function failureReason(error: unknown): string | undefined {
  if (typeof error !== 'object' || error === null) return undefined;
  const { code, details } = error as { code?: unknown; details?: unknown };
  const reason = (details as { reason?: unknown } | null | undefined)?.reason;
  if (typeof reason === 'string') return reason;
  return typeof code === 'string' ? code : undefined;
}

function loadErrorMessage(error: unknown): string {
  const reason = failureReason(error);
  return reason === 'editorBinaryFile' || reason === 'editorNotUtf8'
    ? t('editor', 'binaryNotEditable')
    : t('editor', 'unableToLoad');
}

export function createFileEditorController(options: {
  client: Pick<FileManagerClient, 'loadEditableFile' | 'saveEditableFile'>;
  entry: EntrySummary;
  /** One-based line to select when the file first loads (task 0271). */
  initialLine?: number;
  update: (state: FileEditorState) => void;
}): FileEditorController {
  const language = editableLanguageForExtension(options.entry.extension, options.entry.name);
  let state: FileEditorState = { status: 'loading', entry: options.entry };
  let disposed = false;
  let request: AbortController | undefined;
  let previewTimer: ReturnType<typeof setTimeout> | undefined;
  let cursorOffset = 0;
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
  const selectionPatch = (content: string) => {
    const initialSelection =
      options.initialLine === undefined ? undefined : lineRange(content, options.initialLine);
    return initialSelection === undefined ? {} : { initialSelection };
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
        ...selectionPatch(loaded.content),
      });
    } catch (error: unknown) {
      publish({
        status: 'error',
        entry: options.entry,
        message: loadErrorMessage(error),
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
      const conflict = failureReason(error) === 'fileRevisionConflict';
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
  const toggleMapMode = () => {
    if (state.status === 'ready') publish({ ...state, mapMode: state.mapMode !== true });
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
    toggleMapMode,
    requestClose,
    cancelClose,
    setCursor: (offset) => {
      cursorOffset = offset;
    },
    linkAtCursor: () =>
      state.status === 'ready' ? detectLinkAt(state.content, cursorOffset) : undefined,
    dispose: () => {
      disposed = true;
      request?.abort();
      if (previewTimer !== undefined) clearTimeout(previewTimer);
    },
  };
}
