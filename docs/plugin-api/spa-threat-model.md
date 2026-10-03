# Sandboxed plugin threat model (task 0226)

**Status: incomplete.** This is the reviewed boundary for the prototype, not an assurance
that every desktop host can safely run untrusted panels. On macOS panel opening fails closed:
WKWebView exposes clipboard reads to page JavaScript, and Wry does not provide a reliable
per-WebView read-denial switch. Granting only clipboard write would not fix that. Windows and
Linux still need native isolation and Save-path smoke tests before this task can be closed.

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
| Window identity | Sixteen pre-registered scheme origins are leased one per live panel. A host-generated WebView label is bound to that origin, plugin ID, action ID and selected file; requests from a different label/slot fail. All app commands are denied at the central invoke handler for non-app window labels, independent of Tauri's default ACL behavior. |
| Browser authority | Panels are incognito. CSP defaults to none, allows only packaged scripts/styles/fonts, local/data images and blob workers, and restricts connections to the window's own bridge. Popups, downloads, foreign navigation, frames and objects are blocked. Clipboard cannot be made deny-by-default on macOS, so panels do not open there. |
| Host bridge | The package calls a version-1, token-bound, size-limited `save-svg` request. The custom-scheme handler checks the calling WebView label and origin, rejects unknown fields and revalidates enablement, package identity, `.svg` extension and selected-content permissions on each request. A typed response exposes no file contents or paths on error. |
| Selected file | The host loads a bounded regular UTF-8 file and binds the panel to its original `LocationDto`. Save has no destination override and uses the file editor's revision check without forced overwrite. Same-URI panels serialize saves; a reload closes the panel instead of replaying an outdated snapshot. |
| Lifecycle | Closing or disabling releases the origin slot and cancels pending bridge requests; a periodic reconciliation closes windows whose plugin package/permission is no longer valid. JS calls use fresh runtimes. |

The bundled SVGO preview also sanitizes imported SVG before inserting it into its DOM,
including when calculating crop bounds. CSP is defense in depth, not a substitute for SVG
sanitization.

## Outstanding risks and release gates

- **macOS is unavailable.** WKWebView can read the system clipboard independently of the
  selected-file bridge. The user declined a clipboard-read grant for SVGO, so the host must
  continue to reject activation until an enforceable process/WebView isolation design exists.
- **Windows and Linux are not smoke-tested.** Source-level checks and desktop unit tests do
  not prove WebView2/WebKitGTK custom-scheme, CSP, bridge, or worker behavior. A Windows
  cross-build was blocked by the local `aws-lc-sys` toolchain.
- **Crash cleanup is not immediate.** Tauri has no WebView-crash callback here; the window
  close event and periodic reconciliation are available, but a crashed WebView that leaves its
  window open may retain an origin slot. Do not claim the crash teardown criterion is met.
- **Timed-out saves can leave temporary files.** The existing file editor creates a sibling
  `.fm-edit-*.tmp` before commit; dropping a save future on panel shutdown/timeout does not
  guarantee that temporary copy is discarded. A cancellation-safe file-editor transaction is
  required before claiming complete temporary-data teardown.
- **Cross-process writes remain optimistic.** Same-URI panels in one host serialize saves;
  another process or a different URI alias to the same file can race the editor's read/check/
  commit sequence. No forced overwrite is requested, but the provider operation is not an
  atomic compare-and-swap. An atomic provider revision check is needed for a stronger guarantee.

The focused security review found no exploitable vulnerability in the reviewed **enabled**
paths, subject to these explicit availability and platform gaps. Do not change task 0226 to
`done` until the release gates are resolved and smoke-tested on all target desktop platforms.
