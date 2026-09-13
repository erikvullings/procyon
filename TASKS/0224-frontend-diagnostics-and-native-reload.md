# 0224 Frontend diagnostics and native reload

Status: done
Priority: high
Subsystem: frontend
Depends on: 0015, 0073, 0133

## Context
Release builds need a useful recovery and troubleshooting path before Tauri's `devtools` feature
and right-click Inspect Element can be removed. Today uncaught frontend errors, rejected promises,
and explicit `console.error` calls remain inside the webview console; the Tauri diagnostics command
always returns an empty recent-errors list. Procyon also has no explicit release-safe Reload action:
native menu clicks are normally forwarded to the frontend, which does not help when JavaScript is
stuck.

## Acceptance Criteria
- Uncaught frontend errors, unhandled promise rejections, and explicit `console.error` calls are
  forwarded through `FileManagerClient`, redacted, length-bounded, and retained in the existing
  bounded recent-errors diagnostics buffer.
- Tauri writes captured frontend errors through structured tracing so they remain available in the
  rolling desktop log after a webview reload.
- Browser, mock, and Tauri adapters expose equivalent diagnostic-reporting behavior.
- Settings > Diagnostics displays and copies the captured recent frontend errors.
- View > Reload and its platform-standard shortcut reload the focused webview through the native
  host callback, without depending on the frontend event channel or DevTools.
- Reloading the webview leaves the Rust service and running backend operations alive.
- Focused frontend, transport, Tauri menu, typecheck, and lint tests pass.

## Implementation Notes
- Reuse `DiagnosticErrorDto` and the existing redaction helper instead of introducing a second
  diagnostics model.
- Keep at most 50 entries, with newest entries presented first.
- The native menu callback must consume the reload action in Rust; do not send it back through the
  JavaScript subscription first.
- Do not remove Tauri's `devtools` feature as part of this task; these capabilities make that later
  release-policy change safe but do not silently change the current alpha policy.

## Agent Notes
- 2026-09-13: Started implementation. Existing Tauri `get_diagnostics` hard-codes
  `recent_errors: []`; the server has a host-local bounded buffer, and the native menu callback
  currently forwards every action to JavaScript. The implementation will preserve errors across a
  frontend reload by retaining them in the host and will intercept Reload in the native callback.
- 2026-09-13: Completed. `frontend-diagnostics.ts` captures uncaught errors, unhandled rejections,
  and `console.error`, queues HTTP startup reports until `SessionTokenGate` is ready, and reports
  through all three clients. `DiagnosticErrorBuffer` centralizes 50-entry newest-first retention,
  redaction, and field bounds for HTTP/Tauri; Tauri also writes safe structured errors to its daily
  log. `View > Reload` uses `ui.reloadWebview`, intercepted by the Rust native-menu callback to
  reload the focused webview without frontend cooperation. Review follow-ups hardened `file://`
  and parenthesized stack-path redaction, matched mock redaction semantics, and exposed redacted
  stack context in Settings > Diagnostics and copied reports. Full frontend tests pass
  (140 files, 2,134 tests); focused Rust transport/application/Tauri tests and repository lint pass.
  Existing 13 CSS specificity warnings and the Biome schema-version notice remain unchanged.
