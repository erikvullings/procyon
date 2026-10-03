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
| Host bridge | The package calls version-1, token-bound, size-limited `save-svg` and SVGO-only `settings-change` requests. The custom-scheme handler checks the calling WebView label and origin, rejects unknown fields and revalidates enablement, package identity, `.svg` extension and relevant permissions on each request. Settings changes admit only 15 typed, range-checked optimizer fields (never an SVG, URI, layout or theme), ignore stale per-window sequence numbers, and require `settings_storage`. A typed response exposes no file contents or paths on error. |
| Selected file | The host loads a bounded regular UTF-8 file and binds the panel to its original `LocationDto`. Save has no destination override and uses the file editor's revision check without forced overwrite. Same-URI panels serialize saves; a reload closes the panel instead of replaying an outdated snapshot. |
| Lifecycle | Tab switches hide the child without discarding edits. Closing the tab requests a final settings snapshot and waits up to two seconds before releasing the origin slot and cancelling pending requests; failure is logged. Parent-window close, trusted-app reload, or plugin disable releases the slot immediately. Periodic reconciliation closes child WebViews whose plugin package/permission is no longer valid. JS calls use fresh runtimes. |

The bundled SVGO preview also sanitizes imported SVG before inserting it into its DOM,
including when calculating crop bounds. CSP is defense in depth, not a substitute for SVG
sanitization.

## Outstanding risks and release gates

- **macOS clipboard authority is broad.** WKWebView can read and write the system clipboard
  independently of the selected-file bridge. The user authorized both grants for SVGO; other
  panels without either grant remain unavailable on macOS. This is not a confined clipboard
  bridge, and the host cannot apply per-operation limits or audit individual clipboard accesses.
- **macOS native behavior is not smoke-tested.** Source-level checks and unit tests do not
  prove child WKWebView positioning, custom-scheme, CSP, clipboard or Save-path behavior.
  A native denial check must also confirm a child cannot invoke updater or other Tauri plugin
  commands; the WebView-scoped ACL fix currently has only a configuration regression test.
- **Windows and Linux are not smoke-tested.** Source-level checks and desktop unit tests do
  not prove WebView2/WebKitGTK custom-scheme, CSP, bridge, or worker behavior. A Windows
  cross-build was blocked by the local `aws-lc-sys` toolchain.
- **Crash cleanup is not immediate.** Tauri has no child-WebView crash callback here; the
  parent-window close event and periodic reconciliation are available, but a crashed child
  whose parent remains open may retain an origin slot. Do not claim the crash teardown
  criterion is met.
- **Settings flush is best-effort on abrupt teardown.** Normal tab close waits for an
  acknowledged settings write, but process crashes, plugin disablement, parent-window close,
  and a timed-out flush may lose the most recent optimizer preference change. No SVG content
  is persisted through this channel.
- **Timed-out saves can leave temporary files.** The existing file editor creates a sibling
  `.fm-edit-*.tmp` before commit; dropping a save future on panel shutdown/timeout does not
  guarantee that temporary copy is discarded. A cancellation-safe file-editor transaction is
  required before claiming complete temporary-data teardown.
- **Cross-process writes remain optimistic.** Same-URI panels in one host serialize saves;
  another process or a different URI alias to the same file can race the editor's read/check/
  commit sequence. No forced overwrite is requested, but the provider operation is not an
  atomic compare-and-swap. An atomic provider revision check is needed for a stronger guarantee.

The earlier focused security review covered the previously enabled paths, before macOS panel
activation. Do not change task 0226 to `done` until the release gates are resolved and
smoke-tested on all target desktop platforms, including a fresh review of the macOS path.
