import { defaultKeymap, history, historyKeymap } from '@codemirror/commands';
import { HighlightStyle, syntaxHighlighting } from '@codemirror/language';
import { search, searchKeymap } from '@codemirror/search';
import { EditorSelection, EditorState, type Extension, StateEffect } from '@codemirror/state';
import {
  drawSelection,
  EditorView,
  highlightActiveLine,
  highlightActiveLineGutter,
  keymap,
  lineNumbers,
} from '@codemirror/view';
import { tags } from '@lezer/highlight';
import m, { type FactoryComponent } from 'mithril';

const fmHighlightStyle = HighlightStyle.define([
  { tag: [tags.keyword, tags.operatorKeyword, tags.bool, tags.null], color: 'var(--fm-accent)' },
  { tag: [tags.string, tags.regexp], color: 'var(--fm-success)' },
  { tag: [tags.number, tags.integer, tags.float], color: 'var(--fm-warning)' },
  { tag: [tags.comment, tags.lineComment, tags.blockComment], color: 'var(--fm-text-muted)' },
  { tag: [tags.heading, tags.typeName, tags.className], color: 'var(--fm-accent)' },
  { tag: [tags.link, tags.url], color: 'var(--fm-accent)', textDecoration: 'underline' },
  { tag: tags.emphasis, fontStyle: 'italic' },
  { tag: tags.strong, fontWeight: 'bold' },
  { tag: tags.invalid, color: 'var(--fm-error)' },
]);

export interface CodeMirrorEditorAttrs {
  readonly content: string;
  readonly language?: Extension;
  readonly readOnly?: boolean;
  readonly extensions?: readonly Extension[];
  readonly onChange?: (content: string) => void;
  readonly onSave?: () => void;
  readonly selection?: { readonly from: number; readonly to: number };
  /**
   * Whether CodeMirror's own find/replace panel (Mod-F, F3/Shift-F3) is active. Defaults to
   * `true`, appropriate for the F4 editor, which always has the whole file loaded. The F3
   * viewer sets this to `false`: it lazily loads large files in windows (see
   * `TEXT_WINDOW_BYTES` in `file-viewer-controller.ts`), so CodeMirror's built-in search - which
   * only searches the in-memory `EditorState.doc`, i.e. whatever's been scrolled into view so
   * far - would silently shadow the viewer's own toolbar search, which is backed by a
   * whole-file `search_in_file` backend scan.
   */
  readonly enableBuiltInSearch?: boolean;
  /** Called for Mod-F when `enableBuiltInSearch` is `false`, so the caller can redirect the
   * shortcut to its own whole-file-aware search UI instead of leaving it a dead keystroke. */
  readonly onFindShortcut?: () => void;
}

/**
 * Scrolls `pos` into view within the nearest scrollable ancestor of `view.dom` (walking up from
 * its *parent*, so `.cm-scroller` itself - a descendant, never an ancestor, of `view.dom` - is
 * never matched). `EditorView.scrollIntoView` alone only moves `.cm-scroller`'s own scroll
 * position, which does nothing when a *host* page instead relies on an ancestor wrapper (like the
 * F3 viewer's `.fm-file-viewer-body-text`, whose CSS leaves `.cm-editor` unconstrained-height so
 * the wrapper does the scrolling) - this covers that case too, while safely no-oping for hosts
 * (like the F4 editor) where `.cm-scroller` already is the real scrolling element and no ancestor
 * has real overflow to begin with.
 *
 * `coordsAtPos` triggers CodeMirror's synchronous layout-measurement pass, which throws in test
 * environments (jsdom) that don't implement `Range.getClientRects` - real browsers (and Tauri's
 * WKWebView) always support it, so this is purely a test-environment guard, not a feature check
 * that's expected to ever fail outside tests.
 */
function scrollPositionIntoView(view: EditorView, pos: number): void {
  let coords: ReturnType<EditorView['coordsAtPos']>;
  try {
    coords = view.coordsAtPos(pos);
  } catch {
    return;
  }
  if (coords === null) return;
  for (
    let ancestor = view.dom.parentElement;
    ancestor !== null;
    ancestor = ancestor.parentElement
  ) {
    if (ancestor.scrollHeight <= ancestor.clientHeight) continue;
    if (!/(auto|scroll)/.test(getComputedStyle(ancestor).overflowY)) continue;
    const rect = ancestor.getBoundingClientRect();
    const target = coords.top - rect.top + ancestor.scrollTop - ancestor.clientHeight / 2;
    ancestor.scrollTo({ top: Math.max(0, target) });
    return;
  }
}

export const CodeMirrorEditor: FactoryComponent<CodeMirrorEditorAttrs> = () => {
  let view: EditorView | undefined;
  let currentContent = '';
  let currentSelection: { readonly from: number; readonly to: number } | undefined;
  const extensions = (attrs: CodeMirrorEditorAttrs): Extension[] => [
    EditorView.theme({
      '&': {
        backgroundColor: 'var(--fm-surface)',
        color: 'var(--fm-text)',
        fontFamily: 'var(--fm-font-family)',
        fontSize: '0.8em',
      },
      '.cm-content': { caretColor: 'var(--fm-accent)' },
      '.cm-cursor, .cm-dropCursor': { borderLeftColor: 'var(--fm-accent)' },
      '&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection': {
        backgroundColor: 'var(--fm-selection)',
      },
      '.cm-gutters': {
        backgroundColor: 'var(--fm-surface)',
        color: 'var(--fm-text-muted)',
        borderRightColor: 'var(--fm-border)',
      },
      '.cm-activeLine, .cm-activeLineGutter': { backgroundColor: 'var(--fm-hover)' },
      '.cm-activeLineGutter': { color: 'var(--fm-text)' },
      '.cm-panels': {
        backgroundColor: 'var(--fm-surface-elevated)',
        color: 'var(--fm-text)',
      },
      '.cm-panel input, .cm-panel button': {
        color: 'var(--fm-text)',
        backgroundColor: 'var(--fm-surface)',
        border: '1px solid var(--fm-border)',
      },
      '.cm-searchMatch': { backgroundColor: 'var(--fm-selection)' },
      '.cm-searchMatch-selected': { backgroundColor: 'var(--fm-accent)' },
    }),
    lineNumbers(),
    // Paints the selection as a decoration independent of native browser selection/focus state.
    // Without this, CodeMirror only syncs its selection into the real DOM `Selection` object
    // while the editor itself has focus (see `DocView.updateSelection` upstream) - so
    // programmatically selecting a search match (see `selection` below) rendered nothing at all
    // while the user was typing in the toolbar search input rather than the editor.
    drawSelection(),
    syntaxHighlighting(fmHighlightStyle, { fallback: true }),
    ...(attrs.enableBuiltInSearch === false ? [] : [search(), keymap.of(searchKeymap)]),
    EditorState.readOnly.of(attrs.readOnly === true),
    // Always DOM-editable (contenteditable), even in read-only mode: `readOnly` above already
    // blocks every edit transaction, so this only controls whether the content is selectable.
    // Setting `editable: false` for the F3 viewer used to make `.cm-content` non-contenteditable,
    // which silently defeated the app's `user-select: text` override and made the viewed text
    // impossible to select/copy with the mouse.
    EditorView.editable.of(true),
    ...(attrs.readOnly === true
      ? []
      : [
          history(),
          highlightActiveLine(),
          highlightActiveLineGutter(),
          keymap.of([...defaultKeymap, ...historyKeymap]),
        ]),
    ...(attrs.language === undefined ? [] : [attrs.language]),
    ...(attrs.extensions ?? []),
    EditorView.domEventHandlers({
      keydown: (event) => {
        const isMod = event.metaKey || event.ctrlKey;
        if (isMod && event.key.toLowerCase() === 'f' && attrs.enableBuiltInSearch === false) {
          event.preventDefault();
          event.stopPropagation();
          attrs.onFindShortcut?.();
          return true;
        }
        if (isMod && event.key.toLowerCase() === 's' && attrs.onSave !== undefined) {
          event.preventDefault();
          event.stopPropagation();
          attrs.onSave();
          return true;
        }
        // Keeps these chords from bubbling to the app-wide keymap, which would otherwise
        // intercept them (save/undo/redo, and Mod-F for the in-editor find panel).
        if (isMod && ['s', 'z', 'y', 'f'].includes(event.key.toLowerCase()))
          event.stopPropagation();
        return false;
      },
    }),
    EditorView.updateListener.of((update) => {
      if (!update.docChanged) return;
      currentContent = update.state.doc.toString();
      attrs.onChange?.(currentContent);
    }),
  ];
  // Applies `selection` to an already-constructed `view` if it's new (or changed) since the last
  // call - shared by `oncreate` and `onbeforeupdate` so a search-match highlight/scroll applies
  // the very first time a `CodeMirrorEditor` mounts already carrying one, not just on later
  // updates. That first-mount-with-a-selection case is real, not hypothetical: the F3 viewer
  // swaps a markdown file from rendered HTML to this component the moment a match is highlighted
  // (see `renderTextBody`), which Mithril treats as a fresh mount (`oncreate`), not an update of
  // an existing instance (`onbeforeupdate`) - so without this, the first markdown match would
  // never actually highlight or scroll into view.
  function applySelection(selection: CodeMirrorEditorAttrs['selection']): void {
    if (view === undefined || selection === undefined || selection.to > view.state.doc.length) {
      return;
    }
    // Only re-dispatch when the selection actually changed: `onbeforeupdate` runs on every
    // Mithril redraw of this component (e.g. an unrelated search-bar keystroke), and
    // unconditionally re-dispatching would re-center on the same match every time, fighting any
    // scrolling the user just did manually (Arrow/Page keys - see the F3 viewer's keybindings)
    // even though the highlighted match hadn't moved.
    if (
      currentSelection !== undefined &&
      currentSelection.from === selection.from &&
      currentSelection.to === selection.to
    ) {
      return;
    }
    currentSelection = selection;
    view.dispatch({
      selection: EditorSelection.range(selection.from, selection.to),
      effects: EditorView.scrollIntoView(selection.from, { y: 'center' }),
    });
    scrollPositionIntoView(view, selection.from);
  }
  return {
    view: () => m('.fm-code-mirror'),
    oncreate: ({ dom, attrs }) => {
      currentContent = attrs.content;
      view = new EditorView({
        state: EditorState.create({ doc: attrs.content, extensions: extensions(attrs) }),
        parent: dom as HTMLElement,
      });
      applySelection(attrs.selection);
    },
    onbeforeupdate: ({ attrs }) => {
      if (view === undefined) return false;
      view.dispatch({ effects: StateEffect.reconfigure.of(extensions(attrs)) });
      if (attrs.content !== currentContent) {
        currentContent = attrs.content;
        view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: attrs.content } });
      }
      if (attrs.selection === undefined) {
        currentSelection = undefined;
      } else {
        applySelection(attrs.selection);
      }
      return false;
    },
    onremove: () => {
      view?.destroy();
      view = undefined;
    },
  };
};
