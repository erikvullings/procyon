# 0250 Read-only fm-server attachment to the desktop semantic library

Status: in_progress
Priority: medium
Subsystem: semantic, server
Depends on: 0248

## Context

Item 3 of the Gemma non-experimental checklist qualifies Search and Ask citation navigation from
the browser host against the desktop's library. `fm-server` had no semantic capability, so the
item was blocked. User decision: "Option A: opt-in, read-only attachment to exact desktop library
on loopback. Implement as proposed, with normal authentication/root restrictions."

## Design

- `ServerConfig::semantic_desktop_profile` (`--semantic-desktop-profile`,
  `FM_SERVER_SEMANTIC_DESKTOP_PROFILE`, `semanticDesktopProfile`) is `None` by default.
  `ServerConfig::semantic_desktop_attachment` grants it only with a loopback bind, authentication
  enabled, an existing absolute profile directory, and resolvable roots; `main` refuses to start
  otherwise, and `build_router` fails closed.
- A granted attachment serves every request as `SemanticAccessContext::Host`.
- `FileManagerService::with_attached_desktop_semantic_library` composes:
  - `IpcSemanticCapability::desktop_attached`: `WorkerConnector::attached` joins the worker in
    the desktop's `worker-runtime` directory with its owner-only launch secret and never launches
    one. `shutdown`/`restart` only drop the client.
  - `SemanticLibraryComposition::AttachedDesktop`: reads the desktop's component state without
    recovery and loads the exact desktop library (default or Gemma) only when its policy file
    exists. `SemanticLibraryService::desktop_attached_read_only` allows only status operations and
    refuses internal reconciliation commits. It reports `Unavailable` while any enrolled root lies
    outside the server's accessible roots or cannot be canonicalized (fail closed).
- `fm-server` Ask citation and knowledge-source resolve routes check the
  resolved location against the current accessible roots (403), so symlink or root changes after
  indexing cannot open files outside `--root`.
- Shared vocabulary and saved-conversation writes return `PermissionDenied` while attached.

## Acceptance

- [x] No attachment by default; non-loopback, unauthenticated, or invalid configurations refused.
- [x] Attaching creates nothing in the desktop profile and never launches a worker.
- [x] The exact desktop library is visible read-only to the device principal only, follows later
  desktop commits, and is hidden while roots are outside the server's restrictions.
- [x] Shared-store writes and reconciliation commits are refused.
- [x] Docs and checklist item 3 procedure updated.
- [ ] Manual cross-host citation qualification (checklist item 3) recorded by the release owner.
