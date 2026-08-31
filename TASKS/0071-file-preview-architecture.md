# 0071 Preview service and initial preview panel

Status: done
Priority: low
Owner: unassigned
Agent: unassigned
Area: backend
Depends on: 0069

## Context

`file-manager-coding-agent-spec.md` §25 (preview architecture) and §16 milestone 3 (basic file
preview).

## Acceptance Criteria

- A preview service with a renderer registry supporting the initial types: plain text, common image
  formats, file metadata, and an unsupported-file placeholder (§25).
- Preview data is delivered via ranged or streamed reads; entire large files are never loaded into
  frontend memory (§25).
- A configurable file-size limit applies, with a clear "too large to preview" state.
- Previewed files are never executed, and text previews never interpret embedded HTML/script (§25).
- Binary content shown as text is detected and refused rather than rendered as garbage.
- A collapsible preview panel in the UI updates as the cursor moves, with in-flight preview requests
  cancelled on move (§35). **Superseded 2026-08-04**: automatic cursor-driven preview loading was
  explicitly removed by product direction; preview is now triggered only via F3 (task 0088), not
  automatically as the cursor moves. See Agent Notes.
- Markdown, PDF, media metadata and syntax highlighting are done (see Agent Notes); archive summary
  and plugin previews are still only designed for, not implemented - the registry makes adding them
  additive.
- **Done 2026-08-15** (`core.calculateFolderSize`, Ctrl+. / Cmd+. - see Agent Notes for why plain
  `Space` and Ctrl+Space were both unavailable): pressing the shortcut on a **directory** entry
  computes and displays its recursive total size, matching Total Commander's "see how much space a
  folder consumes" behaviour. Distinct
  from 0097's aggregate totals (currently-listed entries only, one level, non-recursive) and 0118's
  full treemap view (visualizes the breakdown, not just a number) - this is the lightweight "just
  tell me the number, recursively, on demand" case. Fills the Size column, which is otherwise empty
  for directories.
- Tests: renderer selection by MIME/extension, size-limit enforcement, cancellation on cursor move,
  binary detection, recursive directory-size computation. Cancellation-if-the-cursor-moves-mid-walk
  is handled via the same one-shot `AbortSignal` convention as every other cancellable request in
  this app (`readFileRange`/`searchInFile`) rather than a dedicated server-side cancel endpoint - see
  Agent Notes for why, and note neither of those two precedents has a dedicated cancellation test
  either, so `calculateFolderSize` doesn't add one for parity.

## Implementation Notes

- Reuse `mime_type`/`icon_key` from `EntrySummary` where available, but sniff content rather than
  trusting the extension for the text/binary decision.
- Image previews should use a downscaled/streamed representation rather than the original bytes for
  very large images.
- **Competitive note (macOS):** macOS Quick Look (spacebar in Finder) already renders PDF and CBR
  (comic book archive) files very well out of the box — full pagination, decent performance, zero
  extra code. A from-scratch renderer for these formats in fm's own preview panel may not beat that
  experience. Worth deciding explicitly, before investing renderer effort here, whether to (a) build
  bespoke PDF/CBR renderers anyway for cross-platform parity (Windows/Linux have no equivalent to
  lean on), (b) shell out to Quick Look (`qlmanage`) on macOS specifically and accept a weaker
  experience elsewhere, or (c) deprioritize PDF/CBR preview entirely and let macOS users rely on
  Quick Look directly (fm doesn't need to duplicate an OS feature that's already excellent). No
  decision made yet — flag for product input before scoping PDF/CBR renderer work.

## Agent Notes

- Not started.
- 2026-08-04: The "collapsible preview panel that updates as the cursor moves" acceptance criterion
  above was explicitly reversed by product direction: automatic, cursor-driven preview loading is
  no longer wanted (it fetched file bytes for every entry the cursor passed over, even ones the
  user never intended to view). The renderer/content-preview architecture itself
  ([frontend/src/features/preview/content-preview.ts](../frontend/src/features/preview/content-preview.ts))
  is retained and is now exclusively surfaced through task 0088's F3 (`core.view`) Lister-style
  viewer — preview is opt-in per file, not automatic. The old cursor-driven wrapper
  (`content-preview-loader.ts`, its test, the `.fm-preview-panel` UI in
  [pane.ts](../frontend/src/features/panes/pane.ts) and its wiring in
  [app-shell.ts](../frontend/src/app/app-shell.ts)/[workspace-layout.ts](../frontend/src/features/workspace/workspace-layout.ts))
  were deleted as dead code once nothing called them automatically anymore.
- 2026-08-14/15: Substantial progress on the F3 (`core.view`) viewer side of this task, verified
  live against real files (not just synthetic fixtures) in the browser:
  - Mouse text/image selection and clipboard copy inside the viewer (previously blocked by the
    app-wide `user-select: none`), with toast confirmation and the shared command-bar tooltip
    style rather than native `title` attributes.
  - An Alt+Space metadata/info sub-panel (image dimensions, EXIF, GPS with a Google Maps link;
    byte size/line count/detected language for text) — works from the directory listing as well as
    from an already-open viewer, and independent of which pane the viewer is in.
  - Syntax highlighting restored for TypeScript/JavaScript, Python, Rust, CSS, Go, Ruby, SQL and
    C-like languages via CodeMirror language extensions (the highlighting itself was fine;
    `editableLanguageForExtension` just never mapped these extensions to a language).
  - A PDF renderer (`pdfjs-dist`, `frontend/src/features/preview/pdf-preview.ts`) with page
    navigation, fit-to-window scaling on both axes with resize handling, and simple full-document
    text search. Two live-only bugs were found and fixed this way (neither reproduced with
    synthetic test fixtures): (1) `.fm-file-viewer`'s CSS grid had no `grid-template-columns`, so a
    long filename's unwrapped title pushed the whole grid column (and the PDF canvas with it)
    thousands of pixels wide, clipping the page off-pane and hiding the paging controls; (2)
    pdf.js's WASM image codecs (OpenJPEG/JBIG2) were never given a `wasmUrl`, so pages containing
    JPEG2000-encoded images silently rendered blank - fixed by resolving `wasmUrl` through Vite's
    asset pipeline the same way `workerSrc` already was.
  - A CBR/CBZ comic-page renderer, paginated on demand (one page's bytes fetched at a time, never
    the whole archive) via the existing archive-nested `readFileRange` path. Also found and fixed
    live: some CBR/CBZ archives wrap their pages in a single subfolder instead of placing them at
    the archive root ("one folder per volume" scans) - `loadComic` now recurses into subdirectories
    (capped depth) rather than only listing the root.
  - EPUB chapter reading (container/OPF parsing, DOMPurify-sanitized chapter HTML), and PDF/comic/
    EPUB arrow-key page navigation that works regardless of which pane has keyboard focus (F3's own
    already-open/toggle-close behavior stays pane-scoped deliberately - see
    `frontend/src/features/keybindings/global-keydown-handler.ts`'s `findOpenViewer` comment).
  - `Space` in the directory listing now selects-and-advances (Total Commander parity); this
    exposed and fixed two backend/frontend bugs unrelated to preview (wrong default keybinding,
    and a stale-closure bug when two selection actions dispatch in the same keydown handler) - see
    `crates/fm-application/src/action.rs` and `frontend/src/features/workspace/pane-content-builder.ts`.
  - Checked the "too large to preview" size-limit question for the three new PDF/CBR/EPUB
    renderers: no gap - they intentionally share the Lister viewer's existing, deliberate "no size
    cap, full bytes always loaded" design (see `readEntireFileBytes`'s doc comment in
    `frontend/src/features/preview/content-preview.ts`), which only applies to the old, now-deleted
    cursor-driven panel, not task 0088's viewer. Nothing to fix.
- 2026-08-15: Implemented the recursive directory-size-on-a-key acceptance criterion (`core.
  calculateFolderSize`). Plain `Space` already means "select and advance" per Total Commander
  parity work landed earlier the same day, so this needed its own chord - first tried Ctrl+Space,
  which turned out to be unusable: the frontend dispatcher maps any `ctrl: true` chord to the
  platform's primary modifier (`hasPrimaryModifier`), which is **Cmd** on macOS, so "Ctrl+Space"
  was actually Cmd+Space there - Spotlight's system-wide shortcut, intercepted by the OS before any
  app (browser or Tauri) ever sees the keystroke. Found live via user report ("Ctrl+space does not
  compute the size of a folder") rather than anything a unit test could catch, since jsdom-based
  tests dispatch synthetic `KeyboardEvent`s directly and have no OS-level shortcut interception to
  reproduce. Tried **Ctrl+Shift+Space** next (unreserved everywhere), but the user preferred not
  to give up plain Space's row as a modifier-heavy chord; there's also no per-chord way to request
  literal Control on macOS instead of the Cmd translation (only Tab gets that special case, see the
  dispatcher's comment on it, and adding a second one-off for this single action wasn't worth it).
  Settled on **Ctrl+. (Cmd+. on macOS)** by explicit user preference instead. Full stack:
  - Backend: `crates/fm-application/src/folder_size.rs` walks a directory with the same
    stack/pagination/cancellation idiom as `operation_planner.rs`'s `CopyExecutor::plan` (provider-
    agnostic - only needs `ProviderCapabilities::LIST`), summing every descendant file/symlink's
    size (symlinks counted as their own size, never followed - matches the copy/delete planners'
    cycle-safety convention). Exposed as a single `POST /api/v1/directories/size` request/response
    endpoint (`FileManagerService::calculate_folder_size`) plus a matching Tauri command, mirroring
    `search_in_file`/`read_file_range` exactly rather than `start_search`/`cancel_search`'s
    background-task-plus-event-stream shape.
  - Deliberately **not** wired through `fm_events::DirectoryDeltaPayload` (the per-pane, revision-
    tracked delta channel real directory watches use) - that channel requires the emitted delta's
    revision to be exactly `current + 1` or the frontend discards it and does a full refetch, which
    would mean either faking a revision bump for a purely-local, non-authoritative computed value
    (risking a mismatch with the *next real* server delta) or building real cross-request
    cancellation-registry state to synchronize with it. Simpler and just as correct for a single
    on-demand number: a plain awaited request, applied to the frontend's own in-memory pane state
    directly (`app-shell.ts`'s new `calculateFolderSize` closure patches the one row's `EntrySummary.
    size` in the `directories` map), discarded if the result arrives after `AbortController.abort()`
    (fired when a newer calculation starts - only one is ever tracked at a time, mirroring the
    single-viewer-at-a-time convention elsewhere in this file).
  - `formatEntrySize` (`frontend/src/features/entry-formatting/entry-formatting.ts`) used to always
    blank a directory's size unconditionally; narrowed to only blank when `size` is actually
    `undefined`, so the explicit post-calculation value renders normally. Directories still never
    get a size from the backend on an ordinary listing, so this is a no-op everywhere except the row
    that was just calculated.
  - Frontend action interception lives in `global-keydown-handler.ts` alongside the existing F3/
    Alt+Space special cases, gated on `cursorEntry.kind === 'directory'` client-side (no backend
    "cursor entry must be a directory" predicate exists in `ActionContextRequirements` - noted as
    acceptable in `action.rs`'s registration comment rather than adding one for a single action).
  - New tests: Rust DTO round-trip (`fm-transport-dto`), a `FileManagerService` integration test
    walking a nested fixture tree plus a not-found case, and frontend tests for the mock client
    (recursive sum across a real fixture subtree) and the action-registry/keybinding-context wiring.
  - Regenerated the OpenAPI-derived frontend client (`pnpm run api:export && pnpm run api:generate`)
    after adding the route - required for the pre-push `api:check` determinism gate to pass.
  - `Status` moves to `done`: every acceptance criterion above is now met or explicitly, correctly
    scoped out (the superseded auto-preview-panel criterion, and the still-open archive-summary/
    plugin-preview items, which are lower-priority extensibility work, not core preview
    functionality - split out below as their own backlog items rather than blocking this task
    indefinitely on open-ended scope).
- 2026-08-15 (later same day): Two keybinding fixes reported live, plus a real regression found and
  fixed for the "Space selects and advances" behaviour claimed done above:
  - `core.calculateFolderSize`'s shortcut changed twice more after user testing. Ctrl+Space
    (`primary(" ")`) turned out to be Cmd+Space on macOS (see the entry above) - Ctrl+Shift+Space
    was tried next, but the user preferred not to spend Space as a modifier-heavy chord and asked
    whether the *literal* Control key could be requested on macOS instead of the Cmd translation.
    Answer: not without adding a new one-off concept to `KeyChord`/`matches()` (currently only Tab
    gets a literal-Ctrl special case, see the dispatcher's comment on it) - not worth it for a
    single action. Settled on **Ctrl+. / Cmd+.** by explicit user preference.
  - Added **Cmd+, / Ctrl+,** to open Settings (the standard desktop "Preferences" shortcut) -
    handled the same way Ctrl+P opens the command palette (`global-keydown-handler.ts`, a direct
    `event.key` check ahead of `dispatchKeybinding`, since it's a pure UI toggle with no backend
    action). New `GlobalKeydownContext.openSettingsDialog()` mirrors the settings toolbar button's
    "open" branch (not toggle, so pressing it again while already open is a harmless no-op).
  - **Real bug, not a misunderstanding**: the user reported Space still wasn't selecting the row it
    toggled (only advancing), even after the earlier "stale-closure" fix and a hard refresh, but
    that Shift+Down (`extendRange`) worked correctly. That was the key diagnostic: `extendRange`
    dispatches a single `onSelectionAction` call; `toggleCursorSelectionAndAdvance` (Insert/Space)
    dispatched **two** back-to-back (`{type:'toggle'}` then `{type:'moveCursor', offset:1}`).
    Rather than continue debugging the two-dispatch sequence blind (live reproduction wasn't
    reliable this session - the Claude Browser tool's synthetic key events for Space/ArrowDown came
    back with empty `key`/`code` when checked with a raw listener), collapsed it into one atomic
    reducer transition: added `SelectionAction`'s `toggleAndAdvance` case
    (`frontend/src/features/selection/selection.ts`) that toggles the given entry and moves the
    cursor in a single `reduceSelection` call, and changed `pane.ts` to dispatch that once instead
    of `toggle` + `moveCursor` separately. This removes the two-dispatch sequence entirely rather
    than patching around whatever was dropping the first call's effect - root cause not fully
    confirmed (couldn't reproduce reliably to bisect further), but the fix is verified via new
    reducer unit tests (toggle-on, toggle-off, and clamped-at-end cases) and updated
    `pane.test.ts` expectations. If this recurs, the next thing to check is whether Mithril's own
    event-handler re-entrancy (calling a prop function twice within one native event handler)
    was ever actually safe here, versus this collapse just being the more robust design regardless.
- 2026-08-15 (later still): Found the *actual* root cause behind "first Space after a mouse click
  doesn't select" plus two related symptoms the user reported together - "clicking a row doesn't
  seem to set the cursor" and "switching back to the app leaves the cursor row highlighted but
  arrow keys do nothing until I click a row first". All three trace to the same gap: mouse row
  clicks were never moving real DOM keyboard focus onto the pane (`onCursorChange` in `pane.ts`
  only ever dispatched selection *state* changes), and nothing restored focus when the OS window
  regained it either. `section.fm-pane` is the actual `onkeydown` target (`tabindex="-1"`, only
  focusable programmatically) - the existing Tab-pane-switch focus fix
  (`workspace-layout.ts`'s `focusAndActivate`/`registerFocusPane`) covered keyboard-driven pane
  switches, but nothing equivalent ran on a plain row click or on the window's own `focus` event.
  This went unnoticed by the test suite because jsdom tests dispatch keydown events directly on the
  target element (`element.dispatchEvent(...)`), which invokes its listener regardless of real
  `document.activeElement` state - so the tests never exercised whether a *click* had actually
  moved focus there first, only whether the listener behaves correctly once invoked.
  - `pane.ts`: captured the pane's own DOM node (`sectionElement`, via `oncreate`/`onremove` on the
    `section.fm-pane` vnode) and call `.focus()` on it from `onCursorChange` - i.e. every mouse row
    interaction now grabs real keyboard focus the same way clicking anywhere else in the app does,
    not just an eventual keyboard action.
  - `app-shell.ts`: added a `window` `focus` listener (`handleWindowFocus`) that calls the existing
    `focusPane`/`activatePane` fallback (same pair `toggleTerminal` already uses) for the current
    `workspace.activePaneId`, so alt-tabbing back into the app restores keyboard focus without
    requiring an extra click.
  - New tests: `pane.test.ts` asserts a row click moves `document.activeElement` onto `.fm-pane`;
    `app-shell.test.ts` asserts a real `window` `focus` event restores focus to the previously-
    focused pane after an explicit `.blur()` (simulating the OS taking focus away and back).
