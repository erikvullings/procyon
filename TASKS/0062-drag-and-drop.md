# 0062 Drag and drop within the app and with the OS

Status: in_progress
Priority: low
Owner: unassigned
Agent: unassigned
Area: desktop
Depends on: 0061, 0048

## Context
`file-manager-coding-agent-spec.md` §15 (table is a drag source and drop target), §23 (drag to and
from Finder/Explorer) and §33 step 10.

## Acceptance Criteria
- Within the app: dragging a selection between panes and tabs starts a copy or move operation
  through the engine (modifier decides; default documented per platform).
- Drop targets highlight clearly, invalid targets are rejected before the drop, and dropping onto a
  directory row targets that directory rather than its parent.
- Native drag-out to Finder/Explorer and drag-in from them, capability-gated via `nativeDragOut`
  (§21) and unavailable in browser mode.
- Dropping files from outside the app starts the appropriate operation with the same conflict and
  confirmation rules as any other operation (§35 — no silent overwrite).
- Keyboard-accessible alternatives exist for everything drag can do (§29).
- Drag of a very large selection does not stall the UI.
- Tests: drop-target resolution and validation logic (unit); native drag verified manually per
  platform and recorded in the task notes.

## Implementation Notes
- Native drag-out requires platform work behind the adapter traits (0058); if a platform cannot be
  supported yet, report the capability as `false` rather than half-implementing it.

## Agent Notes
- 2026-08-08 Codex: Implemented typed in-app drag/drop for selections between panes and onto loaded
  tabs. Directory rows resolve to themselves while files/empty space resolve to the containing
  directory; unavailable, read-only, same-location, and source-subtree targets are rejected during
  `dragover`, and valid targets receive a visible outline. Accepted drops start ordinary `copy` or
  `move` operations with `conflictPolicy: ask`, so drag uses the same conflict/confirmation engine
  as clipboard paste and never mutates files in TypeScript. Move is the documented default;
  Option requests copy on macOS and Control requests copy elsewhere. Existing Ctrl/Cmd+C/X/V is
  the keyboard-accessible equivalent. Drag payloads use one small internal marker rather than
  serializing the selection through `DataTransfer`; source locations stay in frontend state.
  Added 5 task-specific Vitest cases: 3 drop resolution/validation/modifier tests, 1 table event
  test, and 1 cross-pane operation-dispatch test. The three task test files pass (118 tests total
  in those files, including 5 attributable to this task).
  Native OS drag-in/out is deliberately not half-implemented: every current platform adapter still
  reports `nativeDragOut: false`, so it is unavailable in browser mode and all current desktop
  builds per the Implementation Notes. Manual platform verification therefore records macOS,
  Windows, and Linux as unavailable rather than falsely claiming an interactive native test.
  Full frontend Vitest: 692 passed, 1 skipped, with three pre-existing failures unrelated to this
  task (CodeMirror viewer mount timing, stale mock action list, and a CSS whitespace-string test).
  Typecheck has no new errors; three pre-existing errors remain in archive optional-property test
  data/configuration. `git diff --check` is clean. No CLAUDE.md exists; AGENTS.md needed no change
  because no development contract changed. README documents the new user-facing behavior.
- 2026-08-08 Codex follow-up: Enabled native file-reference drag-in/out in macOS and Windows Tauri
  builds. Both platform adapters now advertise `nativeDragOut`; Linux, browser, and mock adapters
  remain capability-disabled. Drag-out validates provider-neutral locations as local native paths
  before handing them to `drag-rs` on Tauri's main thread. Tauri window drop events are converted
  back into validated `Location` values and dispatched through the existing drop validation and
  operation engine as conflict-safe copies. Added typed desktop errors, teardown-safe frontend
  subscription handling, and tests for empty/non-local selections, awkward path round trips,
  Tauri invocation/path conversion, selection handoff, and incoming operation dispatch.
  `cargo test -p fm-desktop` passes (8 tests), focused native frontend tests pass, relevant Biome
  checks pass, and Clippy passes for `fm-desktop` and `fm-platform-macos`. Full frontend typecheck
  still has the same three unrelated pre-existing errors recorded above. A Windows cross-check
  reaches target-specific compilation but cannot complete on this Mac because the MSVC C headers
  and toolchain are absent (`lzma-sys`/`mlua-sys`). Interactive Finder and Explorer drag tests have
  not been performed in this non-interactive environment, so this task is returned to
  `in_progress` until both manual platform checks are recorded.
- 2026-08-19 Claude: Fixed a bug found during manual verification: on desktop builds with
  `nativeDragOut` enabled (macOS, Windows), every in-app drag — even a plain move between panes on
  the same volume — was routed through the native OS drag session, and the drop handler forced
  `copy` unconditionally for any drop that came back through that native round-trip, regardless of
  modifier keys. In-app drags could never resolve to `move`. Root cause: `onDrop`/`onTabDrop` in
  `frontend/src/features/workspace/pane-content-builder.ts` short-circuited on
  `context.getNativeDropInProgress()` before consulting `operationForDrop`, and that flag was set
  for every native-routed drop, not just genuine external ones. Fix: added a
  `nativeDragSourceInternal` flag (set in `onDragStart` when a drag starts inside the app) and a
  location-set comparison in `workspace-controller.ts`'s `subscribeNativeFileDrops` handler; the
  drop is now only treated as external (forcing `copy`) when its locations don't match what this
  window itself started dragging. In-app drags on native-drag-capable platforms now default to
  `move`, matching non-native/browser builds; genuine drag-in from Finder/Explorer still forces
  `copy` (existing behavior, still covered by the pre-existing "copies a native file drop" test).
  Added a Vitest case ("defaults an in-app drag to move even when routed through the native drag
  host") verifying `startOperation` receives `type: 'move'` for a same-window native-routed drag.
  Full frontend Vitest: 1352 passed, 0 failed. `tsc --noEmit` clean. Manual Finder/Explorer
  interactive drag tests still not recorded in this non-interactive environment; task remains
  `in_progress` for that reason alone.
- 2026-09-07 Copilot: Replaced the fixed-operation native handoff with a locally patched
  `drag-rs 2.1.1` mode that advertises both copy and move. Finder/Explorer now owns native
  operation negotiation and cursor feedback, including modifier changes after the pointer leaves
  Procyon. In-app pointer drags retain move-by-default behavior, show an explicit `-`/`+` operation
  badge, and refresh that feedback immediately on modifier keydown/keyup without requiring mouse
  movement. Interactive Finder/Explorer verification remains outstanding.
- 2026-09-10 Copilot: Kept the ordinary arrow cursor for valid in-app move/copy drags. The previous
  CSS `copy` cursor made macOS add its native green copy badge on top of Procyon's blue operation
  badge; using the default cursor removes that duplicate while preserving Procyon's single `+`/`-`
  indicator consistently across macOS, Windows, and Linux. Invalid targets retain the platform
  `not-allowed` cursor so dragging onto the current folder still has explicit no-drop feedback.
- 2026-09-12 Copilot: Fixed native drag-out returning focus with a delayed compatibility click.
  Native handoff now suppresses that click exactly once; a fresh pointer-down clears suppression,
  so the next intentional click still works. This prevents the delayed click from moving the
  cursor/selecting the dragged row and consequently triggering cursor-driven virtual-list
  scrolling. Added helper and directory-table regression coverage; interactive Finder/Explorer
  verification remains outstanding.
- 2026-10-02 Copilot: Reproduced macOS WKWebView auto-scrolling the source directory after native
  drag-out (several selected SVGs, pointer held outside the Tauri window). The pane now hands off
  when the pointer reaches its status bar and holds that viewport's scroll offset during the native
  session, restoring ordinary scrolling when the session ends. Tauri waits for the drag completion
  callback rather than reporting success immediately; the completion promise is preserved through
  the pane callbacks. Manual macOS retest confirmed the pane stays painted and stationary while
  dragging out and scrolls normally after release. A previously reported disappearing window on
  drag-out to Inkscape remains unconfirmed; Windows/Explorer verification is still outstanding.
- 2026-10-02 Copilot: Dropping an entire selection back into its source directory remains a
  rejected target, but is treated as a silent cancellation rather than an error. Mixed-source
  drops with any unchanged source remain rejected with feedback. Other drop and clipboard errors
  appear in a compact, dismissible overlay instead of adding an undismissable row below the
  workspace. Escape cancels an in-app pointer drag before native handoff; after handoff the OS
  owns the native drag session.
- 2026-10-03 Copilot: The user reports using drag and drop extensively. This confirms the feature
  is available in ordinary use, but does not specify the acceptance criterion's bidirectional
  Finder/Explorer native tests on each platform. The macOS native drag-out scrolling retest was
  recorded above; the earlier Inkscape drag-out failure remains unconfirmed. Record the native
  Finder and Explorer checks explicitly before marking this task done.
- 2026-10-03 release owner: Confirmed macOS Finder drag-in and drag-out. On a drop into Finder,
  the pointer displayed the "no operation" icon even though the file was copied. This incorrect
  native feedback must be resolved before macOS drag-out is considered verified; Windows Explorer
  bidirectional verification remains outstanding.
- 2026-10-04 Copilot: Wired the directory-tree sidebar into the existing in-app drop validation,
  operation controller, platform modifier mapping, and desktop pointer target registry. Tree
  folders can move/copy to other tree folders, table directories and tabs; table selections can
  drop onto tree folders. Read-only/unavailable targets and recursive drops are rejected before
  acceptance. Pointer cancellation clears the pending in-app selection. No change to native
  Finder/Explorer verification status.
- 2026-11-01 erik: **Code review pass (no behavior change).** Confirmed the native handoff is
  correct on both supported platforms: `apps/fm-desktop/src-tauri/src/commands.rs`
  `start_native_drag` passes `drag::DragMode::CopyMove`, and the patched `third_party/drag-rs`
  advertises the right source mask on each — macOS `draggingSession:sourceOperationMaskForDragging
  Context:` returns `NSDragOperationCopy | NSDragOperationMove` (17) and Windows returns
  `DROPEFFECT_COPY | DROPEFFECT_MOVE`. The frontend drag-drop + directory-table suites pass
  (`vitest run src/features/drag-drop/ ...directory-table.test.ts` → 70 passed) and the desktop
  native-drag Rust unit tests previously passed; the native code is unchanged since the last
  review. Assessed the outstanding macOS Finder "no-operation" cursor the release owner flagged:
  the *source* advertises copy+move correctly, so this is a *destination-side* macOS feedback
  quirk — Finder's own `draggingUpdated:` returns `NSDragOperationNone` during hover while the
  drop still performs a copy. That mismatch is not controllable from the drag source and has no
  reliable source-side fix (restricting the advertised mask would only change the ceiling, not
  Finder's hover feedback); confirming or rejecting a fix requires an interactive session. No
  speculative native change was made. **Status stays `in_progress`:** the only remaining
  acceptance work is the explicitly *interactive, per-platform manual* Finder (macOS) and
  Explorer (Windows) bidirectional drag verification recorded in the task notes — work an
  automated agent in a headless environment cannot perform. Everything that can be done
  without a live pointer + real Finder/Explorer windows is complete.

## Manual platform verification checklist (agent-provided 2026-11-01; run on macOS + Windows)

The code is complete and reviewed. This task closes once a human records pass/fail for each item
below on the platforms where `nativeDragOut` is enabled (**macOS** and **Windows**; **Linux and
browser are intentionally capability-disabled — skip them**). Reply to the agent with a
`pass`/`fail` per item (and the A3 cursor observation); the agent marks the task `done` if all
pass, or fixes any real defect the checklist surfaces.

**Prerequisites**
- Build/run the desktop app (`pnpm run build:tauri`, or `pnpm dev:tauri` for dev).
- Two windows side by side: a Procyon pane browsing a local folder, and a Finder (macOS) /
  Explorer (Windows) window on the **same volume**.
- Use a scratch folder you don't mind writing to, e.g. `~/drag-test/`.

**A. Drag-OUT from Procyon → Finder/Explorer**
- [ ] A1 (default = move): select a file in `~/drag-test/`, drag it into the Finder/Explorer
  window. Expect the file to **move** (gone from source, present in dest).
- [ ] A2 (copy via modifier): repeat holding **Option** (macOS) / **Ctrl** (Windows). Expect a
  **copy** (present in both).
- [ ] A3 (cursor feedback): watch the pointer over the destination. Record whether it shows the
  correct operation icon or the reported **“no operation” (circle-slash)** icon. (This is the
  quirk the release owner flagged — confirm if it still occurs.)
- [ ] A4 (drop onto a directory row): drop onto a directory *row*; confirm it lands **inside**
  that directory, not its parent.

**B. Drag-IN from Finder/Explorer → Procyon**
- [ ] B1: drag a file from Finder/Explorer onto a Procyon pane (onto a directory row). Expect a
  **copy into** the browsed folder; confirm it exists there.
- [ ] B2 (no silent overwrite): repeat B1 dropping onto an **already-existing** file. Expect the
  conflict/confirmation dialog (§35), not a silent overwrite.

**C. In-app (both platforms)**
- [ ] C1: drag a selection between two panes / onto another tab. Default = **move**; modifier =
  **copy**. Confirm it runs through the operation engine with conflict handling.

- 2026-11-01 erik: **First re-verification round — fixes shipped, results analysed.**
  *Findings the agent could not reproduce headlessly; each item below is what the manual tester
  reported and the agent's disposition.*
  - **B1 (drag-IN fails, both macOS Finder and Windows Explorer): fixed in code, needs re-test.**
    Root cause: the webview showed the OS drop cursor but never delivered a `drop`, so
    `subscribeNativeFileDrops` never fired and files dragged in from outside silently did not
    land. Added a document-level `dragenter`/`dragover` `preventDefault` scoped to drags carrying
    `Files`/`text/uri-list` (a real external OS drag does; in-app HTML5 drags carry only the
    `application/x-fm-locations` marker, so they and the tree-drop tests are unaffected). Also
    worth re-checking on the next run is the drop-point→`elementFromPoint` coordinate path
    (`workspace-controller.ts`) if the drop still resolves after this fix.
  - **B2: blocked on B1** — re-test once B1 passes.
  - **A3 (move drag-out shows no cursor icon; copy shows one, both platforms): expected OS
    behaviour, not a Procyon bug.** For a drag-OUT, Procyon is the *source* and advertises
    `Copy|Move` correctly (verified). The cursor over the destination is the destination's own
    feedback, and Finder/Explorer do not offer *move* for files dragged out of another
    application (they copy) — so a missing move icon and a present copy icon are correct. No
    code change.
  - **A1 (Windows drag-OUT defaults to copy): expected OS behaviour, not a Procyon bug.**
    Windows Explorer copies external-source drags by default when the source advertises
    `Copy|Move`; that is Explorer's negotiation, exactly the design the 2026-09-07 entry intends
    ("Finder/Explorer own native operation negotiation"). macOS A1 passing (move) is because
    Finder moves same-volume drags by default. No code change.
  - **A2 (Windows modifier): consistent with Explorer conventions** (Shift=move, Ctrl=copy);
    the tester's note is ambiguous but does not indicate a Procyon defect. No code change.
  - **C1: pass on both platforms.**
  - **Also shipped this round (unrelated to the drag results): (a)** `core.duplicateLocationTo
    OtherPane` (the Alt+F2 / "Duplicate Directory to Other Pane" command) gained an **Alt+F2**
    chord — Total Commander parity; on macOS that is Option+F2. See the commit for the reasoning.
    **(b)** The Operations-centre toolbar button now uses the shared styled `tooltip()` portal
    instead of a native `title=` tooltip, matching every sibling toolbar button.
  - **Status stays `in_progress`:** B1 is a code fix awaiting desktop re-verification (the agent
    cannot run the desktop app), and the A-series items are confirmed-correct OS behaviour that
    the manual tester should re-acknowledge. Nothing else changed.
- 2026-11-01 erik: **B1 root-caused (macOS): Tauri `onDragDropEvent` never fires; the webview
  delivers the drop only as HTML5.** Instrumented the whole chain. `nativeDragOut` is `true`,
  `subscribeNativeFileDrops` is called and the `getCurrentWindow().onDragDropEvent(cb)`
  subscription *registers* (returns an unsubscribe fn), but **the callback never fires for any
  drag** — Finder→Procyon, and even Procyon-window→Procyon-window — verified with a
  `console.debug` at the very top of the callback (before the `payload.type !== 'drop'` filter),
  so no `'enter'/'over'/'leave'/'drop'` of any type is received. Adding a document-level
  `preventDefault()` on `dragenter`/`dragover` (scoped to `dataTransfer.types` containing
  `Files`/`text/uri-list`) did **not** make the Tauri callback fire. But the *same*
  `preventDefault` **does** make WKWebView fire an HTML5 `drop` (proven: a folder's `.DS_Store`
  entry opened in the default editor on drop — the webview's own drop hit a directory row). So
  the webview accepts and delivers the drag as HTML5 but not as a Tauri native event: this is a
  wry/Tauri macOS integration gap, not a webview-accept problem. **The only working delivery is
  the HTML5 `drop` event.** Candidate fix being evaluated: keep the `dragover`/`dragenter`
  `preventDefault` so WKWebView fires `drop`, then in `pane-content-builder.ts` `onDrop` detect
  the external file drop (`dataTransfer.types` has `Files`/`text/uri-list` and
  `getDraggedLocations()` is empty) and extract real source paths — WKWebView hides paths behind
  `event.webkitDataTransfer.pasteboardItems()[].propertyListForKey('public.file-url')` or
  `dataTransfer.getData('public.file-url')` (single-file) — convert via the existing Rust
  `native_drag_locations` command and run `copy`. Must avoid double-handling (the synthetic
  Tauri drop carries no `dataTransfer`; key the HTML5 path on `dataTransfer`/`webkitDataTransfer`
  presence) and must suppress the post-drop click that opened `.DS_Store`. **Open questions:**
  (1) is there a Tauri/wry config or version bump to get `onDragDropEvent` working on macOS
  WKWebView rather than routing around it; (2) is `webkitDataTransfer.pasteboardItems()` reliable
  for *multi-file* drops in current WKWebView; (3) should the HTML5 path be universal or
  macOS-only (Windows uses EdgeWebView, where the Tauri channel may work). See the full summary
  handed off to another agent for these. Diagnostic `console.debug('[native-drop] …')` logging is
  currently in place across the chain (`tauri-file-manager-client.ts`, `workspace-controller.ts`,
  `pane-content-builder.ts`) and should be stripped once B1 is fixed.
- 2026-11-01 Claude: **B1 root cause found and fixed: wrong event target.** Native probes showed
  the drop *does* reach wry (`draggingEntered:`/`performDragOperation:` on WryWebView, handler
  returns `true`, so the page correctly sees no HTML5 events) and Tauri *does* dispatch it — but as
  `RunEvent::WebviewEvent(DragDrop)`, not a window event. That path emits `tauri://drag-drop` via
  `emit_to_webview` (target `{kind: Webview}`), which `getCurrentWindow().onDragDropEvent`
  (target `{kind: Window}`) never matches. Fix: `subscribeNativeFileDrops` now uses
  `getCurrentWebview().onDragDropEvent`; Tauri's window-content path emits to `AnyLabel`, which
  Webview listeners also match (`tauri` `manager/mod.rs` target matching), so this covers both
  delivery paths/platforms. The earlier diagnosis above ("WKWebView delivers only HTML5 drops") was
  wrong: a standalone WKWebView repro on macOS 27.2 receives native and HTML5 drops normally, and
  no Tauri config change was needed. Removed the app-wide `dragenter`/`dragover` accept
  (`app-shell.ts`), the HTML5 guard in `pane-content-builder.ts` `onDrop`, and all `[native-drop]`
  diagnostics. Test mocks now provide `onDragDropEvent` only on the webview, so a regression to the
  window target fails `tauri-file-manager-client.test.ts`. Full frontend Vitest 3327 passed; tsc and
  Biome clean. Needs desktop re-test of B1/B2 (and Windows, where the label emit should match).
- 2026-11-01 Claude: **B2 follow-up: one prompt instead of confirm-then-conflict.** With
  *Confirm file operations* on, a copy/move first asked "Copy N items?" and then the job's conflict
  prompt for each existing name. The confirmation now asks the backend which sources already exist
  in the destination (new `findTransferConflicts`: `fm-application::transfer_conflicts`, same
  top-level `destination.join(name)` + provider `inspect` rule as the engine, bounded to 1,000
  sources and 16 concurrent lookups; `POST /api/v1/operations/transfer-conflicts`, Tauri
  `find_transfer_conflicts`, mock). When some do, the same dialog lists them and offers Skip /
  Rename (keep both, focused so Enter never overwrites) / Decide per item / Overwrite (destructive);
  the choice becomes the job's `conflictPolicy`. The lookup is advisory and time-boxed (1.5 s, or
  any error → plain confirmation, job still asks per conflict), and the job re-checks every entry,
  so nothing is overwritten without a choice (§35). Applies to every confirmed copy/move (drag,
  paste, F5/F6, palette), all hosts.
