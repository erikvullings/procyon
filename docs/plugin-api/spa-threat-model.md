# Sandboxed plugin threat model (task 0226)

**Status: incomplete.** This is the reviewed boundary for the prototype, not an assurance
that every desktop host can safely run untrusted panels. On macOS, WKWebView exposes native
clipboard reads and writes to page JavaScript; only panels declaring both grants may open.
The bundled SVGO plugin now declares both grants. Windows and Linux still need native
isolation and Save-path smoke tests before this task can be closed.

## Assets and adversary

An installed plugin package, its JavaScript, its HTML/assets, and the SVG selected under the
cursor are untrusted. A plugin may try to load files outside its package, invoke Procyon's
desktop commands, spoof another plugin's bridge, exfiltrate through WebView resources, or
write a different/changed file. The trusted host owns plugin discovery, capabilities,
selection, window identity, resource delivery, and the original file's revision. A separate
same-user process capable of modifying installed plugin files is outside the current boundary.

## Enforced boundaries

| Surface | Enforcement |
| --- | --- |
| Action JavaScript | Fresh embedded QuickJS runtime; no Node, DOM, Tauri or ambient I/O bindings. Only metadata and staged clipboard-write host calls exist, each requiring its own manifest grant. Source, memory, stack, instruction count, elapsed time, input/output and cancellation are bounded; failures use the existing plugin diagnostic/auto-disable path. |
| Package assets | Manifest entrypoints reject lexical traversal. Discovery and each served asset canonicalize against the installed package; requests reject hidden paths, encoded paths, unsupported asset extensions and oversized responses. |
| WebView identity | Sixteen pre-registered scheme origins are leased one per live panel. A host-generated child WebView label is bound to that origin, plugin ID, action ID, selected file and owning trusted window; requests from a different label/slot fail. Native plugin commands are granted only to trusted `main`/`workspace-*` WebView labels, not their parent window labels; Tauri checks plugin commands before the app invoke handler, which separately denies app commands to non-app WebViews. Only the owning trusted window can position, hide/show, theme or close its child. The separate WebView occupies a transient tab in the opposite pane, never an iframe in the trusted app WebView. |
| Browser authority | Panels are incognito. CSP defaults to none, allows only packaged scripts/styles/fonts, local/data images and blob workers, and restricts connections to the window's own bridge. Popups, downloads, foreign navigation, frames and objects are blocked. WKWebView clipboard read/write on macOS bypasses the bridge, so a panel without both declared grants cannot open there. |
| Host bridge | The package calls version-1, token-bound, size-limited `save-svg`, SVGO-only `settings-change`, challenge-bound `heartbeat`, and identity-bound `close-panel` requests. The custom-scheme handler checks the calling WebView label and origin, rejects unknown fields, and revalidates enablement, package identity, `.svg` extension and relevant permissions for file/settings calls. Settings changes admit only 15 typed, range-checked optimizer fields (never an SVG, URI, layout or theme), ignore stale per-window sequence numbers, and require `settings_storage`. Close sends only the authenticated session's host-allocated label to its owning trusted WebView; the panel host accepts it only while that label and tab are active, then asks the normal tab controller to close (including the bounded settings flush). A typed response exposes no file contents or paths on error. |
| Selected file | SVGO opens only a local `file:` location, checked at trusted host open and again on every bridge request, including Save. The host loads a bounded regular UTF-8 file and binds the panel to its original `LocationDto`. Save has no destination override and uses the file editor's revision check without forced overwrite. The editor tracks its sibling temporary copy and attempts to discard it on write/commit errors and dropped save futures, cancelling the associated provider operation before cleanup. **All local whole-file editor saves**, including SVGO and the generic editor, hold a per-target process mutex and an advisory cross-process lock (in the per-user Procyon editor cache, keyed by canonical target path) through the revision recheck and commit; symlinked directory aliases share the lock, while unrelated files can save independently. Stable cache lock files are retained to avoid an unlink/recreate split-lock race. A reload closes the panel instead of replaying an outdated snapshot. Remote providers retain their existing behavior. |
| Lifecycle | Tab switches hide the child without discarding edits. Tab close, plugin disable, trusted-app reload, and parent-window close request a final settings snapshot and wait up to two seconds per panel before releasing its origin slot and cancelling pending requests; failure is logged. Disablement flushes before revoking the grant. Periodic reconciliation closes invalid children and probes the renderer through a challenge-bound authenticated bridge request. A visible child receives one retry after 20 seconds; a hidden child gets two 120-second intervals to account for background throttling. Panels that never finish loading expire after 20 seconds. JS actions use fresh runtimes. |

The child WebView composites above trusted HTML on desktop even when HTML has a higher
CSS z-index. While a child is active opposite the file pane, the trusted context menu
and its submenu stay within the file pane instead of hiding the child. The WebView
remains visible, interactive, and mounted with its editor state. The menu backdrop
lets right-clicks reach other file rows and suppresses the main WebView's native
Reload/Inspect menu while the trusted menu is open. Native interaction near the
divider still needs macOS/Windows/Linux smoke verification.

The bundled SVGO preview also sanitizes imported SVG before inserting it into its DOM,
including when calculating crop bounds. CSP is defense in depth, not a substitute for SVG
sanitization.

## Outstanding risks and release gates

- **macOS clipboard authority is broad.** WKWebView can read and write the system clipboard
  independently of the selected-file bridge. The user authorized both grants for SVGO; other
  panels without either grant remain unavailable on macOS. This is not a confined clipboard
  bridge, and the host cannot apply per-operation limits or audit individual clipboard accesses.
- **Native smoke coverage is partial.** The feature-gated `native-spa-smoke` build opens the
  bundled SVGO panel in a real child WebView, waits for its UI, invokes the updater command
  and requires an ACL denial, then saves a disposable SVG through the revision-checked bridge.
  PR #87's CI run 37194939233 timed out on all three platforms before opening a child: a plain
  release `cargo build` omitted Tauri's `custom-protocol` feature and tried the Vite development
  URL instead of the embedded frontend. The smoke feature now enables that protocol, and the
  harness reports startup, trusted-page, child, UI, ACL, and bridge stages plus the daily-suffixed
  app log. An unbundled macOS release build passed twice without Vite, including an actual
  updater ACL rejection and Save. A separate feature-gated installer smoke now builds unsigned
  DMG/MSI/DEB artifacts with a unique smoke app identity, installs or extracts each artifact,
  verifies that the SVGO manifest and entrypoint are present in its resources, and repeats the
  same child/UI/ACL/Save assertions **without** the repository plugin-directory override.
  The copied macOS DMG app passed this test locally. Its temporary install path must be
  canonicalized (`/private/var`, not macOS's `/var` symlink), or Tauri rejects the symlinked
  starting binary and cannot resolve bundled resources. Windows MSI and Linux DEB release
  results still require CI confirmation. These feature-gated checks do not exercise the
  production installer without the smoke feature, the Linux AppImage, or the cursor/shortcut
  path; they do not establish clipboard, CSP/navigation, crash, or worker isolation.
  The smoke harness opens its child directly, outside the trusted tab controller, so it also
  cannot prove a focused child shortcut closes the corresponding UI tab. The child shortcut,
  authenticated bridge, and trusted tab routing have focused regression tests; a manual
  native shortcut check remains necessary.

  On a disposable macOS test environment, reproduce the release-mode check from the repository
  root without running Vite:

  ```bash
  node scripts/build-svgo-plugin.mjs
  pnpm exec cross-env VITE_RUNTIME=tauri pnpm run build:frontend
  cargo build -p fm-desktop --release --features native-spa-smoke
  CI=true node scripts/smoke-native-spa.mjs target/release/fm-desktop
  ```

  To qualify an installed artifact on a disposable desktop runner with the CI system
  dependencies available, use `dmg` on macOS, `msi` on Windows, or `deb` on Linux:

  ```bash
  node scripts/build-tauri.mjs --features native-spa-smoke --config tauri.native-spa-smoke.conf.json --bundles dmg
  CI=true node scripts/smoke-desktop-package.mjs --native-spa
  ```

  For the normal production app's still-unverified cursor path, use a disposable SVG in a
  separately installed package, activate the bundled SVGO action through the UI, change a Tree
  property, Save, and reopen the file to confirm the persisted edit; repeat on each platform.
  Do not treat this manual check as an ACL test: that requires the feature-gated native smoke.
-  **Lifecycle smoke extension.** The feature-gated child check now also requires a renderer
  heartbeat and settings persistence across host-driven disablement. The extended release-binary
  check passed on macOS; the extended installed-package check and Windows/Linux results remain
  unverified.
- **Crash cleanup is bounded, not immediate.** Tauri 2.11.5 does not expose a child-WebView
  crash callback. Renderer hangs/crashes are inferred after an unanswered challenge (up to
  40 seconds visible or 240 seconds hidden, plus the reconciliation interval), then the
  child and slot are closed. A suspended system or aggressively throttled but healthy hidden
  renderer may be closed after the second deadline; a malicious responsive renderer can
  answer the probe while doing nothing else. Native crash injection remains untested.
- **Settings flush cannot survive an abrupt process crash.** Normal tab close and host-driven
  disable/reload/parent close wait for an acknowledged durable settings write, but an OS kill,
  crashed renderer, inaccessible child or timed-out flush can lose the latest in-memory
  optimizer preference. For 0.4.0, loss of preferences not yet acknowledged by the host on
  whole-app crash is accepted; no crash-durable synchronous write is required. The bounded
  flush on normal tab close, disable, reload and parent-window close remains required. No SVG
  content is persisted through this channel.
- **Temporary cleanup is best-effort.** Failed or cancelled saves now attempt to discard their
  sibling `.fm-edit-*.tmp` with a fresh cancellation token; dropped futures schedule cleanup on
  the active Tokio runtime. The 0.4.0 release accepts a rare orphan after a whole-app crash:
  for local files the original SVG remains intact until the sibling temporary copy replaces it,
  while unsaved edits may need to be redone; the orphan is residual housekeeping, not a recovered
  draft. A process crash, runtime shutdown, provider deletion error, or remote upload that
  completes after cleanup can still leave a temporary copy. Some providers return a
  streaming writer before the remote upload is durable, so a successful write/shutdown is not
  proof of remote publication. Cancellation during commit can also return an error after a
  provider has already published the destination. No startup sweep is performed: `.fm-edit-*.tmp`
  entries can belong to a still-active writer in another process or provider session; deleting
  them by name or age could destroy its work. A durable journal or name/age sweep is not required
  for 0.4.0; do not claim guaranteed teardown or atomic rollback across providers.
- **Uncooperative external writes remain optimistic (accepted for 0.4.0).** Independent
  Procyon generic-editor and SVGO processes and symlinked directory aliases serialize through the same advisory
  cache lock, and each Save checks the revision while holding it. External editors that do not
  take this lock can still write between the revision check and rename: local filesystem
  operations here offer no atomic compare-and-swap against them. The user explicitly accepted
  this residual for 0.4.0; do not describe the lock as universal protection or atomic CAS.
  SVGO remote SFTP/FTP/WebDAV/etc. files cannot be opened or saved through the panel; remote
  whole-file editing remains available through the generic editor, with its existing limits.
  A remote volume mounted into the OS filesystem has a local `file:` location and is treated
  as local by Procyon; its external writers and filesystem lock semantics are not guaranteed
  by this policy.

The earlier focused security review covered the previously enabled paths, before macOS panel
activation. Do not change task 0226 to `done` until the release gates are resolved and
smoke-tested on all target desktop platforms, including a fresh review of the macOS path.
