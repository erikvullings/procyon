# 0099 In-app text file editor with Markdown preview

Status: done
Priority: medium
Owner: unassigned
Agent: codex
Area: cross-cutting
Depends on: 0088

## Context

Add safe, simple in-app editing for text-like files such as plain text, Markdown, XML, JSON and
GeoJSON. The existing `core.edit`/F4 flow from 0086 launches an external text editor, while 0088
provides an in-app, ranged-read viewer for large files. This task must decide the product entry
point explicitly: either change F4 to open the in-app editor for supported files with an external
editor fallback, or add a distinct in-app action while preserving F4's current contract.

Monaco is a candidate because it provides mature language modes and editing behaviour, but its
bundle size and worker setup may be excessive for this file manager. Evaluate it against at least
one lighter alternative before implementation. Markdown editing should offer an instant preview;
`slimdown-js` is the preferred lightweight parser, subject to its incomplete CommonMark support
and the requirement to sanitize its HTML output.

## Acceptance Criteria

- A short decision record compares Monaco with at least one lighter editor on bundle size,
  accessibility, Mithril integration, language support, worker/runtime complexity and maintenance,
  and records the chosen editor before the dependency is added.
- Supported text-like files open in an in-app editor with appropriate language mode where
  available, covering at least `.txt`, `.md`/`.markdown`, `.xml`, `.json` and `.geojson`.
  Unsupported/binary files fail safely or retain the existing external-editor fallback.
- Loading is bounded by a configurable editable-file size limit. Large files continue to use the
  0088 viewer/external editor rather than being loaded wholesale into frontend memory, with a
  clear explanation to the user.
- Add a host-agnostic application/VFS write contract and thin REST and Tauri adapters with matching
  behaviour. Saving uses safe replacement semantics (write a sibling temporary file, flush as
  appropriate, then replace) and never silently follows symlinks or overwrites a file that changed
  externally after it was loaded.
- The load response carries a revision token or equivalent metadata. Save detects stale content
  and presents an explicit reload/overwrite/save-as/cancel resolution rather than silently losing
  either version. Any explicit overwrite is auditable and tested.
- Dirty state is visible. Closing the editor, replacing its pane, navigating away or closing the
  app with unsaved changes requires a discard/save/cancel decision. Save progress and errors are
  shown without discarding the editable buffer.
- JSON and GeoJSON can be formatted and receive syntax diagnostics from the selected editor when
  supported; invalid content remains editable and is never silently rewritten on save.
- Markdown files support an instant, toggleable or split preview powered by `slimdown-js` unless
  the decision record rejects it with a documented reason. Preview updates are debounced, raw
  Markdown is treated as untrusted, rendered HTML is sanitized before DOM insertion, and links or
  images cannot execute script or bypass the server-mode security boundary.
- Editor keyboard handling coexists with global shortcuts: normal text-editing commands and undo/
  redo stay inside the editor, while intentionally global commands remain discoverable. The UI is
  keyboard accessible and respects existing theme tokens.
- Tests cover type detection, size/binary refusal, read/save parity for HTTP and Tauri, atomic-save
  failure, external-modification conflict, dirty-close decisions, editor shortcut isolation and
  safe/debounced Markdown preview rendering.

## Implementation Notes

- Reuse 0088's file viewer surface and ranged-read/type-detection infrastructure where it reduces
  duplication, but do not turn the large-file viewer into an unbounded editor. Check 0071 for the
  preview renderer boundary before adding competing preview logic.
- `slimdown-js` is a small regex-based Markdown renderer rather than a full CommonMark/GFM parser.
  Its `render(markdown)` output is HTML and it does not escape raw HTML by default, so use a proven
  sanitizer (for example DOMPurify with a restrictive policy); parser output must never be assigned
  directly to trusted HTML. Test code fences and raw HTML/script payloads specifically.
- Keep application logic in a controller/state module, not the Mithril editor component. Ensure
  long-running reads/saves and preview updates can be cancelled or superseded.
- Do not hand-edit `frontend/openapi/openapi.json` or `frontend/src/api/`; regenerate both after
  adding transport DTOs/endpoints.
- Decide whether save participates in the operation engine/history or is a focused content-write
  command. Whichever is chosen must preserve browser/Tauri parity and the repository's rule against
  silent overwrite.

## Agent Notes

- 2026-08-05 codex: Created as a follow-up to 0086 (external F4 edit) and 0088 (in-app ranged
  viewer). Dependency is only 0088 because its final UI/controller contract should be settled
  before the editor reuses or extends it. The editor-library and F4-entry-point choices are
  intentionally left as recorded design decisions rather than prematurely fixed in this task.
- 2026-08-08 gemini: Decision Record: Selected CodeMirror 6 (CM6) over Monaco for the in-app editor and replacing highlight.js in the 0088 viewer.
  - Rationale: CM6 provides a lightweight bundle (~120 KB vs Monaco's ~2–4 MB), zero web worker runtime/bundler configuration for Tauri/web environments, and direct integration with Mithril's imperative lifecycle (`oncreate`/`onremove`). Core language packages (`@codemirror/lang-json`, `@codemirror/lang-markdown`, `@codemirror/lang-xml`) satisfy formatting and diagnostic needs for targeted config and Markdown files without heavy LSP dependencies.
  - Standardized Syntax Engine & Language Parity: Replacing `highlight.js` in 0088 with read-only CM6 (`EditorState.readOnly.of(true)`) eliminates duplicate grammar bundles, ensures 100% theme/token parity between viewer and editor modes, and allows seamless transitions from viewing to editing. Full language parity with `highlight.js` is maintained by combining first-class `@codemirror/lang-*` packages with `@codemirror/legacy-modes` (`StreamLanguage.define()`) for shell, Dockerfile, TOML, INI, and other config formats.
  - Entry Point & Shortcut Policy: F4 remains the primary "Edit" action, using 0088 type and size inspection to route supported files <= limit (e.g. 3Mb) to the CM6 in-app editor, with external editor fallback for large or binary files. Shift+Alt+F4 always routes to the external editor, and Shift+F4 creates a new file (opens a dialog to enter the filename, same as in Total Commander). Ctrl/Cmd+F4 remains Total Commander's "Sort by Extension".
  - For an example, see below.

## Example using CodeMirror

File: `CodeMirrorEditor.ts`

```ts
import m, { Component, Vnode } from 'mithril';
import { EditorState, Extension, StateEffect } from '@codemirror/state';
import { EditorView, keymap, lineNumbers, highlightActiveLineGutter, highlightActiveLine } from '@codemirror/view';
import { defaultKeymap, history, historyKeymap } from '@codemirror/commands';
import { syntaxHighlighting, defaultHighlightStyle, StreamLanguage, LanguageSupport } from '@codemirror/language';

export interface CodeMirrorAttrs {
  /** Document text content */
  content: string;
  /** Active language support (e.g. json(), markdown(), or StreamLanguage.define(shell)) */
  languageExtension?: LanguageSupport | Extension;
  /** When true, disables editing and hides cursor/active-line highlights */
  readOnly?: boolean;
  /** Called when editor content changes in edit mode */
  onChange?: (newContent: string) => void;
  /** Optional additional extensions (e.g. theme, keymaps) */
  extensions?: Extension[];
}

/** Helper to wrap legacy CodeMirror 5 modes (e.g. shell, dockerfile, toml) */
export function createLegacyLanguageSupport(mode: Parameters<typeof StreamLanguage.define>[0]): Extension {
  return StreamLanguage.define(mode);
}

export const CodeMirrorEditor: Component<CodeMirrorAttrs> = () => {
  let editorView: EditorView | null = null;
  let currentContent = '';
  let currentReadOnly = false;
  let currentLanguage: LanguageSupport | Extension | undefined = undefined;

  const buildExtensions = (attrs: CodeMirrorAttrs): Extension[] => {
    const isReadOnly = attrs.readOnly ?? false;

    const baseExtensions: Extension[] = [
      lineNumbers(),
      syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
      EditorState.readOnly.of(isReadOnly),
      EditorView.editable.of(!isReadOnly),
    ];

    if (!isReadOnly) {
      baseExtensions.push(
        history(),
        highlightActiveLine(),
        highlightActiveLineGutter(),
        keymap.of([...defaultKeymap, ...historyKeymap]),
        EditorView.updateListener.of((update) => {
          if (update.docChanged && attrs.onChange) {
            currentContent = update.state.doc.toString();
            attrs.onChange(currentContent);
          }
        })
      );
    }

    if (attrs.languageExtension) {
      baseExtensions.push(attrs.languageExtension);
    }

    if (attrs.extensions) {
      baseExtensions.push(...attrs.extensions);
    }

    return baseExtensions;
  };

  const initView = (domNode: HTMLElement, attrs: CodeMirrorAttrs) => {
    currentContent = attrs.content;
    currentReadOnly = attrs.readOnly ?? false;
    currentLanguage = attrs.languageExtension;

    const state = EditorState.create({
      doc: currentContent,
      extensions: buildExtensions(attrs),
    });

    editorView = new EditorView({
      state,
      parent: domNode,
    });
  };

  return {
    oncreate(vnode: Vnode<CodeMirrorAttrs>) {
      initView(vnode.dom as HTMLElement, vnode.attrs);
    },

    onbeforeupdate(vnode: Vnode<CodeMirrorAttrs>) {
      const nextContent = vnode.attrs.content;
      const nextReadOnly = vnode.attrs.readOnly ?? false;
      const nextLanguage = vnode.attrs.languageExtension;

      // Reconfigure state extensions if read-only mode OR language mode changes
      if (nextReadOnly !== currentReadOnly || nextLanguage !== currentLanguage) {
        currentReadOnly = nextReadOnly;
        currentLanguage = nextLanguage;

        if (editorView) {
          editorView.dispatch({
            effects: StateEffect.reconfigure.of(buildExtensions(vnode.attrs)),
          });
        }
      }

      // External content update check (e.g., file reloaded or swapped)
      if (nextContent !== currentContent && editorView) {
        currentContent = nextContent;
        editorView.dispatch({
          changes: {
            from: 0,
            to: editorView.state.doc.length,
            insert: nextContent,
          },
        });
      }

      // Instruct Mithril NEVER to re-render or diff CodeMirror's DOM tree
      return false;
    },

    onremove() {
      if (editorView) {
        editorView.destroy();
        editorView = null;
      }
    },

    view() {
      return m('.codemirror-wrapper', {
        style: { height: '100%', width: '100%', overflow: 'hidden' },
      });
    },
  };
};
```

Example of using it:

```ts
import m from 'mithril';
import { json } from '@codemirror/lang-json';
import { shell } from '@codemirror/legacy-modes/mode/shell';
import { CodeMirrorEditor, createLegacyLanguageSupport } from './CodeMirrorEditor';

// Native Lezer Language (JSON)
m(CodeMirrorEditor, {
  content: jsonString,
  readOnly: true,
  languageExtension: json(),
});

// Legacy Mode Language (Shell / Bash / Dockerfile)
m(CodeMirrorEditor, {
  content: scriptString,
  readOnly: false,
  languageExtension: createLegacyLanguageSupport(shell),
  onChange: (updated) => console.log(updated),
});
```

## Agent Notes

- 2026-08-08: Implemented the CodeMirror 6 decision above using registry-current pinned CM6
  packages. F4 opens supported UTF-8 text files in the opposite pane; Shift+Alt+F4 and unsupported
  extensions retain the external-editor path. The editor supports text, Markdown, XML, JSON and
  GeoJSON, dirty-state close decisions, JSON diagnostics/formatting, and a debounced
  slimdown-js/DOMPurify Markdown preview.
- Added bounded 3 MiB application-service load/save contracts, generated REST client routes and
  matching Tauri commands. Saves use a sibling temporary file and VFS commit, reject non-files,
  compare opaque content revisions, preserve externally changed content by default, and expose
  reload/overwrite/save-as/cancel conflict choices. Explicit stale overwrites are appended to the
  application audit log. Focused Rust editor tests and frontend editor/app-shell tests pass.
