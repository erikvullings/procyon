# 0179 Semantic library enrolment and consent policy

Status: open
Priority: high
Subsystem: backend, frontend, settings
Depends on: 0020, 0030, 0177

## Context

Semantic indexing persists embeddings and normalized excerpts derived from user files. Folder
selection therefore grants durable processing and storage consent, not merely a search scope.
Consent must remain understandable across nested roots, exclusions, folder moves, removable media,
multiple workspaces, and eventual remote-provider support.

Version one exposes local roots but the policy and worker feed must remain provider-neutral. A
device-local library is shared across workspaces so overlapping folders are embedded once; each
query still applies its active workspace/root/tenant scope.

## Acceptance Criteria

- A versioned semantic-library policy persists enrolled roots, stable root identities, recursive
  inclusion, explicit descendant exclusions, eligibility overrides, attached vocabularies, resource
  profile, reconciliation interval, and model/library identity without credentials.
- Folder UI shows Included here, Inherited from parent, and Excluded states. Enabling a root previews
  estimated file count, source bytes, extracted text/vector storage, unsupported/skipped content,
  model download if missing, and the fact that normalized excerpts will be retained locally.
- Excluding a root or descendant requires destructive confirmation and queues deletion of all
  occurrences, extracted text, summaries, labels, vectors no longer referenced elsewhere, and
  saved-conversation evidence pins within that scope. Completion/failure is visible and resumable.
- Pause is a separate library operation that preserves consent and indexed data. Queries can still
  use the last complete generations while ingestion is paused.
- Enrolment follows a renamed/moved directory only when stable filesystem identity proves it is the
  same root on the same volume. A new directory at an old path never inherits consent. Ambiguous or
  cross-volume moves require new confirmation.
- Symlinks are not followed outside an enrolled root. Curated defaults skip hidden/system content,
  application/package bundles, dependency/build/cache directories, `.gitignore` matches, unsupported
  MIME types, and over-budget files; reasons and counts remain visible and per-root overrides are
  possible.
- Temporarily unavailable roots retain searchable/RAG-usable indexed evidence and show Source
  unavailable links. Absence is not deletion; only a successful complete reconciliation or explicit
  exclusion may remove missing documents.
- One device-local desktop library deduplicates content across workspaces. Workspace deletion does
  not revoke globally enrolled roots. Query scopes are enforced by occurrence records, never only
  by frontend filtering.
- Server mode supports administrator-defined tenant libraries, enrolment policies, user-to-library
  authorization, hard quotas, and retrieval-side isolation. Until tenant administration exists, it
  permits only one private/admin library and disables the capability for other users.
- Tests cover inheritance/exclusions, overlapping roots, path reuse, proven rename, symlink escape,
  unavailable volumes, explicit deletion, shared occurrences, workspace deletion, policy migration,
  server tenant isolation, and authorization bypass attempts.

## Implementation Notes

- Use the existing settings migration machinery for global policy metadata, but keep high-volume
  document/catalog state under the configurable semantic-data root.
- Procyon enumerates entries and streams bytes through VFS capabilities. The worker must not crawl
  local paths itself; this preserves the boundary needed for remote providers later.
- Explicit exclusion overrides all retention features. A saved conversation is not permission to
  retain revoked source material.

## Agent Notes

- 2026-09-04: Split from 0176. Agreed semantics are recursive roots with descendant exclusions,
  destructive exclusion, non-destructive pause, and retained evidence for temporarily unavailable
  sources.