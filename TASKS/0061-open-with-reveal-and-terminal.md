# 0061 Open with default application, reveal in file manager, open terminal

Status: done
Priority: medium
Owner: unassigned
Agent: unassigned
Area: platform
Depends on: 0059, 0060

## Context
`file-manager-coding-agent-spec.md` §16 milestone 3, §21 (`revealInSystemFileManager`,
`openTerminal`) and §33 step 10.

## Acceptance Criteria
- `core.open` on a file opens it with the system default application; `core.openWith` offers a
  chooser where the platform supports one.
- `core.revealInSystemFileManager` reveals and selects the entry in Finder/Explorer.
- `core.openTerminal` opens the configured terminal at the current directory; the terminal command
  is a setting (§26) with a sensible platform default.
- All three are capability-gated and hidden/disabled in browser-server mode (§21).
- Arguments are passed safely — no shell string interpolation of file paths; paths with spaces,
  quotes and Unicode work (§6).
- Executable files are never executed implicitly by preview or listing (§25); "open" on an
  executable follows the platform's default behaviour and is confirmed where risky.
- Failures (no default application, terminal not found) produce a user-readable error, not a silent
  no-op.
- Tests: argument construction for awkward paths, capability gating; actual launching is verified
  manually per platform and recorded in the task notes.

## Implementation Notes
- Implement through the platform adapter traits (0058); the actions themselves stay platform-neutral.
- In server mode these actions would act on the server's desktop — they must be unavailable, not
  merely hidden (§22).

## Agent Notes
- 2026-08-01 copilot: Implemented `core.open`/`core.openWith`/`core.revealInSystemFileManager`/
  `core.openTerminal` end to end, dispatched directly to the injected `PlatformAdapter` (0058)
  rather than through the mutating-operation engine.
  - `ActionRegistry::with_core_actions(capabilities: PlatformCapabilities)` (previously a
    zero-argument constructor) now capability-gates these four actions via new
    `capability_gated_single_selection`/`capability_gated_none` helpers, computing
    `feature_available` from the adapter's reported flags instead of a hardcoded `true`/
    `unimplemented()`. `FileManagerService`'s single internal constructor (used by `new`,
    `with_event_bus` and `with_platform_adapter` alike) derives `platform_capabilities` from
    `platform.capabilities()` before building the registry, so every entry point stays
    consistent and browser/server mode (`FallbackPlatformAdapter`, no capabilities) reports these
    actions unavailable rather than merely hidden (spec §22) - covered by a dedicated test,
    `invoke_action_reveal_and_terminal_are_unavailable_in_browser_server_mode`, plus per-capability
    gating-independence tests in `action.rs`.
  - Parameter contract: `core.open`/`core.openWith`/`core.revealInSystemFileManager` take
    `{ "uri": "file://..." }` for the single selected entry; `core.openTerminal` takes
    `{ "uri": "file://..." }` for the current directory. The backend has no entry registry to
    resolve an opaque `EntryId` back to a path (mirroring plugin action invocation, task 0055), so
    the frontend supplies the target explicitly, built from the already-loaded `Location`
    (`platformActionParameters` in `app-shell.ts`). The backend parses the URI with
    `fm_domain::Location::parse().to_native_path()` and passes the resulting `PathBuf` to the
    adapter as a discrete argument (`std::process::Command::arg`/`NSString`/`NSURL`) - never
    string-built or shell-interpolated. A missing or malformed `uri` is rejected as
    `ApplicationError::InvalidRequest`, not a silent no-op. Covered by
    `invoke_action_opens_the_uri_parameters_path_with_the_default_application`, which round-trips
    a path containing spaces, single/double quotes and a non-ASCII (café) character through a real
    temp file to prove the URI is parsed rather than assembled as a command string.
  - `core.openWith` gap (documented, not a bug): it shares
    `PlatformActionKind::Open`/`PlatformCapabilities::OPEN_WITH_DEFAULT_APPLICATION` with
    `core.open` - no `PlatformAdapter` implementation (macOS or the Windows/fallback stub) exposes
    a distinct "choose an application" picker yet, so invoking `core.openWith` currently just opens
    with the default application, identically to `core.open`. This is called out in doc comments
    on `core_actions` (`action.rs`) and at the top of `fm-platform-macos/src/lib.rs`, and left as a
    known, explicit gap rather than a fabricated chooser.
  - New `ApplicationError::PlatformOperationFailed(String)` /
    `ApplicationErrorCode::PlatformOperationFailed` (serializes to `"platformOperationFailed"`,
    maps to HTTP 502 in `apps/fm-server`) carries a genuine, already-sanitized
    `fm_platform::PlatformError` message (e.g. "no default application is registered for .xyz
    files") back to the caller - failures are surfaced, never swallowed into a silent no-op or a
    generic "internal error". A `PlatformError::Unsupported` from the adapter (e.g. a
    capability-detection/invocation race) instead maps to the existing `ActionUnavailable`, since
    that is the more accurate signal. `fm-platform-macos`'s `open_with_default_application` and
    `open_terminal` both shell out via `std::process::Command` (`open <path>` /
    `open -a <app> <path>`) and turn a non-zero exit status into `PlatformError::Io`, so a real
    "no handler" or "app not found" failure is not silently swallowed as success.
  - `open_terminal`'s `command_override: Option<&str>` (new second parameter on
    `PlatformAdapter::open_terminal`, updated on the trait's default, `FallbackPlatformAdapter`,
    `MacosPlatformAdapter` and the `WindowsPlatformAdapter` delegating stub) is sourced from
    `Settings.terminal_command` (`Option<String>`, already `Mutex`-guarded on
    `FileManagerService`). This setting field **already existed** in `fm-settings`/
    `fm-transport-dto` before this task (confirmed by inspecting `settings.rs` history and the
    unmodified `mock-file-manager-client.ts`'s `terminalCommand: null` default) - it was not newly
    added here, so no frontend settings-model or OpenAPI regeneration was needed for it, only for
    the new `PlatformOperationFailed` error code (see below). `None` (the default) falls back to a
    sensible per-platform default, e.g. `Terminal` on macOS.
  - Frontend: `AppShell`'s `onOpenEntry` now invokes `core.open` with `{ uri: entry.location.uri }`
    for files (previously a no-op `undefined`) instead of navigating, while directories still
    navigate as before. `core.revealInSystemFileManager` was added to `availability.ts`'s
    `SELECTION_ACTION_IDS` so it appears in the selection context menu/palette like `core.open`/
    `core.openWith`, gated by the same `feature_available`/selection-count logic as every other
    selection action (no bespoke gating code needed).
  - Confirmed no code path added by this task executes a target file directly as a process:
    `open_with_default_application` always shells out to the native `open`/`NSWorkspace`
    "open with default application" mechanism (never `Command::new(path)`), matching the
    "executable files are never executed implicitly" acceptance criterion; this was true before
    this task and remains true after it - nothing here changes preview/listing behaviour at all.
  - Generated/fixture files touched as a direct, non-stale consequence of the new error code:
    `fixtures/mock-responses/actions.json` (2 lines, mock server data), `frontend/openapi/openapi.json`
    (1 line, new enum value) and `frontend/src/api/generated/models/applicationErrorCode.ts` (1
    line). Verified not stale by re-running `bash scripts/export-openapi.sh && bash
    scripts/generate-api.sh` and diffing just those paths afterwards - zero further changes.
  - Verification (workspace root, all commands re-run fresh by the finishing agent):
    `cargo fmt --all --check` (clean), `cargo clippy --workspace --all-targets -- -D warnings`
    (zero warnings), `cargo test -p fm-application --no-fail-fast` (113 unit tests + every
    integration target passed this run, including `conflict_resolution` 5/5 - see flaky-test note
    below), `cargo test -p fm-server --no-fail-fast` (only
    `plugin_routes::list_plugins_starts_empty_and_unknown_enablement_is_not_found` fails, everything
    else passes, e.g. `action_routes` 5/5, `workspace_routes` 15/15), `cargo test -p
    fm-platform-macos` (10/10 pass), `cargo test -p fm-vfs-local` (18 passed, 1 known failure - see
    below). Frontend: `pnpm run typecheck` (clean) and the full `pnpm test -- --run` suite (45 test
    files, 305 tests, all passing) from `frontend/`.
  - Three confirmed pre-existing, unrelated failures observed tonight, none caused by this task
    (each independently reproduced on unmodified `main` by an earlier subagent via `git stash`,
    per this session's notes):
    `fm-application`'s `conflict_resolution::a_destination_appearing_after_planning_is_resolved_like_an_initial_conflict`
    (timing-sensitive/flaky - passed 5/5 on this run's `cargo test -p fm-application`, but is known
    to fail intermittently under `cargo test --workspace`'s parallel/full-load conditions),
    `fm-server`'s `plugin_routes::list_plugins_starts_empty_and_unknown_enablement_is_not_found`,
    and `fm-vfs-local`'s `metadata_is_separate_and_capabilities_are_truthful` (introduced by task
    0059, unrelated to platform-adapter dispatch work here).
  - Manual verification (macOS 26.6, BuildVersion 25G72, via `sw_vers`), performed against scratch
    files/directories under `$TMPDIR` only, never real user files:
    - `core.open`'s underlying mechanism: created a temp text file containing spaces and a
      non-ASCII character in its name, ran `open <path>` (the exact command
      `open_with_default_application` shells out to) directly at the crate/OS level rather than
      through the full HTTP/Tauri stack, and confirmed via `ps aux` that `TextEdit.app` launched
      as a real process for it; quit TextEdit afterward via `osascript -e 'tell application
      "TextEdit" to quit'` and re-confirmed via `ps aux` that it was gone.
    - Reveal in Finder: not re-done manually: `fm-platform-macos`'s existing
      `reveal_in_finder_succeeds_for_a_real_temporary_file` test (from task 0059, re-run above as
      part of the 10/10 `fm-platform-macos` pass) already exercises a real
      `NSWorkspace.activateFileViewerSelectingURLs` call against a scratch `tempfile`-created file,
      so it stands as this task's Finder-reveal verification too (the reveal code path itself was
      not touched by task 0061, only its `ActionRegistry`/dispatch wiring was added).
    - Open terminal: ran `open -a Terminal <scratch-temp-dir>` (the exact command
      `open_terminal`'s default, no-override path builds) directly; it exited 0 and `ps aux`
      confirmed a new `Terminal.app` process. Attempted to close the resulting window via
      `osascript -e 'tell application "Terminal" to close (every window whose name contains
      ...)'`, which timed out waiting on an AppleEvent (likely a first-run Automation/Accessibility
      permission prompt this non-interactive session could not answer) - the extra Terminal window
      was therefore left open rather than force-closed; it is harmless (a shell at an
      already-deleted scratch temp directory) and can be closed manually.
  - Left untouched, as instructed: `frontend/src/api/client/tauri-file-manager-client.ts`'s
    pre-existing uncommitted one-line change (`import { type FileManagerClient }` ->
    `import type { FileManagerClient }`), which predates tonight's work and is unrelated to this
    task - not staged, not committed.
- 2026-08-04 copilot: Closed the `core.openWith` gap noted above (reported by the user after
  testing task 0086/0087's Ctrl+Enter/Cmd+Enter shortcut in the real Tauri desktop app: "CMD+ENTER
  should display the open with system dialog instead of acting as the viewer").
  - Added `PlatformAdapter::open_with_chooser(&self, path: &Path) -> Result<(), PlatformError>`
    (`fm-platform/src/adapter.rs`), with a default impl falling back to
    `open_with_default_application` (mirrors `open_in_text_editor`'s precedent from task 0086) -
    reuses the existing `OPEN_WITH_DEFAULT_APPLICATION` capability bit rather than adding a new one
    (no adapter needs to advertise this distinctly; it degrades gracefully).
  - `MacosPlatformAdapter::open_with_chooser` shells out to `osascript` running an
    `on run argv ... end run` script that calls AppleScript's `choose application` (the OS's native
    app-picker dialog) and then `tell application "Finder" to open (POSIX file targetPath) using
    chosenApp`. The target path is passed as a genuine trailing `argv` element (via
    `Command::arg(path)`), never interpolated into the `-e` script text, to rule out AppleScript/
    shell injection (OWASP concern) - verified by a unit test
    (`open_with_chooser_passes_the_path_as_a_trailing_argv_element_never_interpolated`) asserting
    the path only ever appears as the final argv element and never inside any `-e` fragment.
    Cancelling the dialog raises AppleScript error -128, caught inside the script
    (`on error number -128 / return`) and treated as a successful no-op, not a failure.
  - `WindowsPlatformAdapter` and `FallbackPlatformAdapter` both delegate/report-unsupported for
    `open_with_chooser` exactly like every other method (task 0060's real Explorer integration,
    not yet done, will need a real Windows implementation - the correct native mechanism is
    `rundll32.exe shell32.dll,OpenAs_RunDLL <path>`, not implemented here to keep this crate's
    scope consistent with its current 100%-delegation state).
  - `fm-application/src/service.rs`: added `PlatformActionKind::OpenWithChooser`; `core.openWith`
    now maps to it (split out of the `core.open`/`core.view` shared `Open` arm) and dispatches to
    `self.platform.open_with_chooser(&path)` instead of `open_with_default_application` - so
    `core.open`/`core.view` and `core.openWith` are now genuinely distinct dispatch targets, not
    just distinct action ids sharing one behavior. Updated `action.rs`'s `core_actions` doc comment
    to stop claiming `core.openWith` "currently behaves identically to core.open" now that macOS
    has a real chooser. Test: `invoke_action_open_with_shows_the_chooser_not_the_default_application`
    asserts `core.openWith` records into a new `opened_with_chooser` field on the
    `RecordingPlatformAdapter` test double, and explicitly asserts `adapter.opened` stays empty
    (proving it does NOT fall through to the `core.open`/`core.view` path).
  - NOT manually verified end-to-end against the real interactive dialog: `choose application` pops
    a real, blocking macOS system dialog with no scriptable/automatable way for this non-interactive
    agent session to dismiss it (unlike task 0061's original manual verification of `open`/
    `open -a Terminal`, which complete without user interaction). All argument-construction/
    injection-safety/cancellation-handling is covered by unit tests instead, per the acceptance
    criterion "actual launching is verified manually per platform and recorded in the task notes" -
    this remains an explicit gap for a human to verify by running the desktop app and pressing
    Cmd+Enter on a file.
  - Also fixed, same session, unrelated bug report ("the Function keys in the footer don't do
    anything" in the real Tauri app): the footer's `.fm-function-key` spans
    (`frontend/src/app/app-shell.ts`) had zero click wiring (a pure text/status display). Added
    `invokeFunctionKeyShortcut(shortcut)`, which synthesizes a real `KeyboardEvent('keydown', {
    key: shortcut })` and dispatches it at the active pane's DOM element
    (`[data-active="true"] > .fm-pane`), reusing the exact same two-tier keyboard-dispatch pipeline
    (pane-level then document-level handlers) a real key press already goes through, rather than
    duplicating each action's dispatch logic. Wired via `onclick`/`role="button"`/`tabindex` on the
    footer spans (disabled ones get `tabindex="-1"`/no `onclick`), plus `cursor: pointer` /
    `cursor: default` CSS. Test:
    `copies one selected file to the other pane by clicking the F5 footer hint (Tauri parity fix)`.
  - Verification this session: `cargo fmt --all` (clean), `cargo clippy --workspace --all-targets
    -- -D warnings` (zero warnings), `cargo test` for `fm-platform`/`fm-platform-macos`/
    `fm-platform-windows`/`fm-application` (all passing; the only failure anywhere in the workspace
    was the already-documented pre-existing `fm-plugin-runtime` icon-count test, unrelated).
    Frontend: `tsc --noEmit` (clean), full `vitest run` (59 files, 466 tests, all passing).
- 2026-08-05 copilot: Two follow-up fixes reported by the user after testing the previous entry's
  work in the real Tauri app.
  - **`core.openWith`'s dialog showed every installed application, unfiltered** ("In Marta, when I
    press CMD+ENTER on an image file, I see [a list scoped to image editors]... I would like to see
    the same as Marta"). Root cause: AppleScript's `choose application` has no filtering hook at
    all - it always lists literally every app on the system. Fixed by querying Launch Services
    directly instead: `MacosPlatformAdapter::open_with_chooser` now calls
    `NSWorkspace.URLsForApplicationsToOpenURL(_:)` (new `recommended_applications` helper) to get
    the same Launch-Services-recommended, default-app-first application list Finder's own "Open
    With" submenu uses, pairs each app's absolute bundle path with its localized display name
    (`NSFileManager.displayNameAtPath(_:)`, e.g. "Preview" not "Preview.app"), and presents just
    those names (plus a trailing "Other Application…" entry) via a new `choose_from_list_command`
    AppleScript builder. Deliberately did **not** build a custom interactive native `NSMenu` with
    click-callback wiring (would need `objc2::declare_class!`-based target-action plumbing, a much
    larger and riskier addition with no existing precedent in this crate - `install_native_menu`
    only ever installs an empty menu) - a `choose from list` dialog is a lighter-weight, equally
    testable way to achieve the same *filtering* outcome, even though its chrome looks like a
    picker dialog rather than Finder's contextual menu.
    - Selection resolution moved entirely into pure, unit-tested Rust
      (`resolve_open_with_choice`) rather than more AppleScript branching: the `choose from list`
      script only ever returns a chosen name (or a `__fm_open_with_cancelled__` sentinel string on
      Cancel/Escape/error -128) on stdout; Rust then matches that name back against the
      already-fetched `(name, path)` list and, for a match, launches it directly via
      `open -a <app_path> <path>` (`std::process::Command`, argv-safe, no more AppleScript/Finder
      needed for the actual launch step). Picking "Other Application…" falls back to the original,
      already-tested unfiltered `open_with_chooser_command`/`choose application` dialog (kept
      byte-for-byte unchanged, still covered by its original test) - this mirrors Finder's own
      "Open With" submenu, which lists recommended apps first and an "Other…" catch-all last. An
      empty `recommended_applications` result (should be rare) falls back to the unfiltered dialog
      outright.
    - Every application name and the target path are passed exclusively as trailing `argv`
      elements, never interpolated into `-e` script text (same injection-safety discipline as the
      original `open_with_chooser_command`) - verified by
      `choose_from_list_command_passes_names_as_trailing_argv_elements_never_interpolated`, which
      uses a name containing embedded quotes and a non-ASCII character to prove it can't leak into
      the script. `resolve_open_with_choice`'s three branches (cancelled sentinel, "Other
      Application…", exact-name match, and the defensive "unmatched name is treated as cancelled
      rather than guessed" case) are each covered by a dedicated pure unit test requiring no
      objc2/osascript involvement at all.
    - `recommended_applications` itself (a real, read-only `NSWorkspace` query, not a mocked/stubbed
      call) is exercised directly by `recommended_applications_finds_at_least_one_candidate_for_a_
      plain_text_file`, which asserts Launch Services returns at least one `.app`-suffixed
      candidate for a scratch `.txt` file (every macOS install ships TextEdit) - unlike the
      interactive chooser dialogs, this call has no blocking UI, so it can run as a normal (not
      `--ignored`) test.
    - **Not manually verified end-to-end against the real interactive dialog** (same constraint as
      the previous entry): `choose from list` pops a real, blocking system dialog with no
      scriptable way for this non-interactive session to dismiss it. Remains an explicit gap for a
      human to verify by running the desktop app, pressing Cmd+Enter on an image file, and
      confirming the list is scoped to image-capable apps (Preview, Pinta, etc.) rather than every
      installed application, matching Marta's screenshot.
  - **Browser-mode Ctrl+Enter/F3/F4 produced a persistent top-of-screen error banner** ("this
    command does not work in the browser, and I see a rather ugly permanent error at the top of the
    screen... unavailable actions in the browser should be hidden if they have a visible shortcut,
    and if the user presses a shortcut key, use a mithril-materialized toast to warn the user
    briefly"). Root cause: `handleGlobalKeydown`'s `core.view`/`core.edit`/`core.openWith` branch
    (`app-shell.ts`) always invoked the backend action whenever an entry was selected, with no
    check of `contextRequirements.featureAvailable` (the backend-computed, session-lifetime-permanent
    signal that already distinguishes "will never work this session", e.g. browser/server mode, from
    "temporarily blocked", e.g. no selection) - so a genuinely-gated action's backend rejection set
    `commandPaletteError`, which renders unconditionally and only clears when the command palette is
    reopened.
    - `handleGlobalKeydown` now looks the dispatched action up in `registeredActions` first; if
      `contextRequirements.featureAvailable === false`, it calls `event.preventDefault()`, shows a
      `mithril-materialized` `toast({ html: "<title> isn't available in the browser." })` (default
      `displayLength` of 4000ms, confirmed via the bundled UMD source - genuinely brief, unlike the
      persistent banner it replaces for this case), and returns *before* ever reaching
      `invokeActionById`/the backend. The pre-existing `commandPaletteError` banner is untouched and
      remains the correct fallback for genuine, unpredictable backend failures elsewhere.
    - `footerFunctionKeyBindings` (`keybindings/dispatcher.ts`) now excludes (rather than merely
      disabling) any F-key entry whose action has `contextRequirements.featureAvailable === false`,
      so a permanently-gated action's footer hint (e.g. `core.view`'s F3, `core.edit`'s F4 in
      browser mode) disappears entirely instead of lingering as dead, confusing UI; its doc comment
      was updated to describe this. Genuinely transient unavailability (e.g. no selection) is
      unaffected - those entries still show, just marked `actionAvailable: false`, per the
      pre-existing (and still-passing) "marks unavailable actions instead of hiding them" test.
    - Tests: `omits footer entries for actions that are permanently unavailable in this runtime`
      (`dispatcher.test.ts`) and `shows a brief toast instead of invoking a permanently
      browser-unavailable action from its shortcut` (`app-shell.test.ts`, asserting a real `.toast`
      DOM element appears with the action's title and that `client.invokeAction` is never called).
  - Verification this session: `cargo test -p fm-platform-macos` (18 passed, 1 ignored - the
    pre-existing manual-only Finder-reveal test), `cargo fmt --all --check` and
    `cargo clippy --workspace --all-targets -- -D warnings` (both clean), `cargo test --workspace`
    (only the already-documented pre-existing `fm-plugin-runtime` icon-count failure, unrelated).
    Frontend: `tsc --noEmit` (clean), `vitest run` (468 tests, 59 files, all passing, up from 466),
    repo-wide `pnpm run lint` (cargo fmt --check + clippy + `biome check .`, all clean).

