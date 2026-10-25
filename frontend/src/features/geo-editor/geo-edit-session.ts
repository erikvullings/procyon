import type { FileManagerClient } from '../../api/client/file-manager-client';
import { byteCount, geoMessageText, t } from '../../i18n';
import type { GeoError, GeoIssue } from '../../models';
import { GeoEditHistory } from './geo-edit-history';
import {
  addFeature,
  type DrawGeometry,
  deleteProperty,
  duplicateFeature,
  type GeoEditDocument,
  importFeatures,
  indexOfId,
  nudgeFeature,
  type PropertyType,
  parseEditDocument,
  parseTypedValue,
  removeFeature,
  renameProperty,
  replaceGeometry,
  serializeEditDocument,
  setProperty,
} from './geo-edit-model';

/** The map editor works on files up to this size, so live validation and drawing stay responsive. */
export const MAX_EDIT_BYTES = 1_500_000;
export const MAX_EDIT_FEATURES = 5_000;
const VALIDATE_DELAY_MS = 300;

export type DrawTool = 'select' | 'point' | 'linestring' | 'polygon' | 'rectangle' | 'circle';

export type GeoEditStatus = 'ready' | 'invalid' | 'tooLarge';

export interface GeoEditSessionContext {
  /** The text currently in the file editor. */
  getContent(): string;
  /** Writes the edited document back; the file editor tracks unsaved changes and saving. */
  setContent(text: string): void;
  readonly client: Pick<FileManagerClient, 'parseGeoText'>;
  /** Called after asynchronous state changes, so the view redraws. */
  changed(): void;
}

export interface GeoEditSessionState {
  readonly status: GeoEditStatus;
  readonly document: GeoEditDocument | undefined;
  /** Why the content cannot be edited on the map (`invalid` or `tooLarge`). */
  readonly reason: string | undefined;
  readonly selectedId: string | undefined;
  readonly tool: DrawTool;
  readonly canUndo: boolean;
  readonly canRedo: boolean;
  /** True when a bare Feature or geometry was wrapped in a FeatureCollection. */
  readonly wrapped: boolean;
  /** The text of the raw JSON pane. */
  readonly rawText: string;
  /** Validation result of the raw pane, shown beside it. */
  readonly rawError: GeoError | undefined;
  readonly warnings: readonly GeoIssue[];
  /** Increases on every change of the document, so views can tell when to redraw the map. */
  readonly revision: number;
}

export interface GeoEditSession {
  state(): GeoEditSessionState;
  /** Re-reads the editor text if it was changed elsewhere (text mode, reload). */
  sync(): void;
  setTool(tool: DrawTool): void;
  select(id: string | undefined): void;
  /** A shape was drawn on the map. */
  drawn(geometry: DrawGeometry): string | undefined;
  /** A vertex or whole shape was moved on the map. */
  geometryChanged(id: string, geometry: DrawGeometry): void;
  remove(id: string): void;
  duplicate(id: string): void;
  /** Moves a feature by the given degrees (keyboard nudging). */
  nudge(id: string, dLongitude: number, dLatitude: number): void;
  setProperty(id: string, key: string, text: string, type: PropertyType): string | undefined;
  addProperty(id: string, key: string): void;
  renameProperty(id: string, from: string, to: string): void;
  deleteProperty(id: string, key: string): void;
  undo(): void;
  redo(): void;
  /** Rewrites the text without changing the data: one line (`compact`) or indented. */
  reformat(compact: boolean): void;
  /** Called on every edit of the raw JSON pane. */
  setRaw(text: string): void;
  /** Merges pasted or dropped GeoJSON text into the document; returns an error message if invalid. */
  importText(text: string): Promise<string | undefined>;
  dispose(): void;
}

export function createGeoEditSession(context: GeoEditSessionContext): GeoEditSession {
  let history: GeoEditHistory | undefined;
  let status: GeoEditStatus = 'ready';
  let reason: string | undefined;
  let selectedId: string | undefined;
  let tool: DrawTool = 'select';
  let wrapped = false;
  let rawText = '';
  let rawError: GeoError | undefined;
  let warnings: readonly GeoIssue[] = [];
  let revision = 0;
  let lastWritten: string | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let validation = 0;
  let disposed = false;

  function load(text: string): void {
    revision += 1;
    lastWritten = text;
    if (text.length > MAX_EDIT_BYTES) {
      status = 'tooLarge';
      reason = t('geoEdit', 'tooLargeBytes', { max: byteCount(MAX_EDIT_BYTES) });
      history = undefined;
      rawText = '';
      return;
    }
    const parsed = parseEditDocument(text);
    if (!parsed.ok) {
      status = 'invalid';
      reason = parsed.message;
      history = undefined;
      rawText = text;
      return;
    }
    if (parsed.document.features.length > MAX_EDIT_FEATURES) {
      status = 'tooLarge';
      reason = t('geoEdit', 'tooManyFeatures', { max: MAX_EDIT_FEATURES });
      history = undefined;
      return;
    }
    status = 'ready';
    reason = undefined;
    wrapped = parsed.wrapped;
    history = new GeoEditHistory(parsed.document);
    selectedId = undefined;
    rawText = wrapped ? serializeEditDocument(parsed.document) : text;
    rawError = undefined;
    warnings = [];
    if (wrapped) write(parsed.document, false);
    void validate(rawText);
  }

  function write(document: GeoEditDocument, refreshRaw = true): void {
    const text = serializeEditDocument(document);
    lastWritten = text;
    context.setContent(text);
    if (refreshRaw) {
      rawText = text;
      rawError = undefined;
    }
  }

  async function validate(text: string): Promise<void> {
    const token = ++validation;
    try {
      const result = await context.client.parseGeoText(text);
      if (disposed || token !== validation) return;
      rawError = result.error ?? undefined;
      warnings = result.warnings;
    } catch {
      if (disposed || token !== validation) return;
      rawError = undefined;
    }
    context.changed();
  }

  function apply(next: GeoEditDocument): void {
    if (history === undefined) return;
    const committed = history.push(next);
    revision += 1;
    if (indexOfId(committed, selectedId) < 0) selectedId = undefined;
    write(committed);
    void validate(rawText);
  }

  load(context.getContent());

  return {
    state: () => ({
      status,
      document: history?.document,
      reason,
      selectedId,
      tool,
      canUndo: history?.canUndo ?? false,
      canRedo: history?.canRedo ?? false,
      wrapped,
      rawText,
      rawError,
      warnings,
      revision,
    }),
    sync() {
      if (disposed) return;
      const content = context.getContent();
      if (content !== lastWritten) load(content);
    },
    setTool(next) {
      tool = next;
      if (next !== 'select') selectedId = undefined;
    },
    select(id) {
      selectedId = id;
    },
    drawn(geometry) {
      if (history === undefined) return undefined;
      const document = addFeature(history.document, geometry);
      const id = document.ids.at(-1);
      apply(document);
      selectedId = id;
      return id;
    },
    geometryChanged(id, geometry) {
      if (history !== undefined) apply(replaceGeometry(history.document, id, geometry));
    },
    remove(id) {
      if (history !== undefined) apply(removeFeature(history.document, id));
    },
    duplicate(id) {
      if (history === undefined) return;
      const next = duplicateFeature(history.document, id);
      apply(next);
      selectedId = next.ids[indexOfId(next, id) + 1];
    },
    nudge(id, dLongitude, dLatitude) {
      if (history !== undefined) apply(nudgeFeature(history.document, id, dLongitude, dLatitude));
    },
    setProperty(id, key, text, type) {
      if (history === undefined) return undefined;
      const parsed = parseTypedValue(text, type);
      if (!parsed.ok) return parsed.message;
      apply(setProperty(history.document, id, key, parsed.value));
      return undefined;
    },
    addProperty(id, key) {
      if (history !== undefined) apply(setProperty(history.document, id, key, ''));
    },
    renameProperty(id, from, to) {
      if (history !== undefined) apply(renameProperty(history.document, id, from, to));
    },
    deleteProperty(id, key) {
      if (history !== undefined) apply(deleteProperty(history.document, id, key));
    },
    undo() {
      if (history === undefined || !history.canUndo) return;
      const document = history.undo();
      revision += 1;
      if (indexOfId(document, selectedId) < 0) selectedId = undefined;
      write(document);
      void validate(rawText);
    },
    redo() {
      if (history === undefined || !history.canRedo) return;
      const document = history.redo();
      revision += 1;
      if (indexOfId(document, selectedId) < 0) selectedId = undefined;
      write(document);
      void validate(rawText);
    },
    reformat(compact) {
      if (history === undefined) return;
      const text = serializeEditDocument(history.document, !compact);
      lastWritten = text;
      rawText = text;
      context.setContent(text);
    },
    setRaw(text) {
      rawText = text;
      if (timer !== undefined) clearTimeout(timer);
      timer = setTimeout(() => {
        timer = undefined;
        const parsed = parseEditDocument(text);
        void validate(text).then(() => {
          if (disposed || rawText !== text) return;
          if (parsed.ok && rawError === undefined && history !== undefined) {
            // Keep the user's exact text (and formatting) in the pane; the document follows it.
            const committed = history.push(parsed.document);
            revision += 1;
            selectedId = indexOfId(committed, selectedId) < 0 ? undefined : selectedId;
            lastWritten = text;
            context.setContent(text);
            context.changed();
          }
        });
      }, VALIDATE_DELAY_MS);
    },
    async importText(text) {
      if (history === undefined) return t('geoEdit', 'nothingToImport');
      const result = await context.client.parseGeoText(text);
      if (result.error != null) return geoMessageText(result.error);
      const parsed = parseEditDocument(text);
      if (!parsed.ok) return parsed.message;
      apply(importFeatures(history.document, parsed.document));
      context.changed();
      return undefined;
    },
    dispose() {
      disposed = true;
      if (timer !== undefined) clearTimeout(timer);
    },
  };
}
