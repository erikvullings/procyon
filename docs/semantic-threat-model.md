# Semantic subsystem threat model

This document is the release-gate threat model for Procyon's optional semantic subsystem. The
filesystem host remains authoritative: workers receive opaque tenant, library, document, source,
and chunk identities plus bounded bytes, never provider credentials or unrestricted filesystem or
network access.

## Assets and trust boundaries

Authoritative assets are enrolment consent and exclusions, exact component/model manifests,
credential-free generation-profile metadata, SKOS sources and accepted edits, and saved
conversation/citation identities. Extracted text, embeddings, Zvec indexes, summaries, concept
annotations, and evaluation observations are sensitive derived data. Provider credentials and LLM
API keys remain in the host credential store.

The boundaries are:

1. VFS providers to the application host, which validates roots and reads source bytes.
2. The authenticated local IPC channel to the semantic worker.
3. Signed component catalogs and downloaded package verification.
4. Explicit, previewed generation requests from the host to configured remote LLM endpoints.
5. Explicit backup, evaluation, feedback, and diagnostic exports.

## Threats and controls

| Threat | Required control and evidence |
| --- | --- |
| Component supply-chain compromise | Signed trusted catalogs, digest verification before activation, version compatibility checks, atomic activation, and rollback to the last valid component. |
| Local IPC impersonation or replay | Owner-only Unix socket/named pipe, per-launch secret, authenticated expiring sessions, request IDs, bounded frames, deadlines, and connection/request limits. |
| Malicious documents and archive/converter bombs | Rust baseline conversion, bounded source/message/stream sizes, archive expansion limits, structural chunk limits, cancellation, timeouts, and isolated optional converters. |
| Prompt injection | Retrieved text is delimited as untrusted evidence, cannot alter authority, receives no tools, credentials, paths, or provider metadata, and cannot broaden scope. |
| Tenant/filter bypass | Tenant and library are mandatory IPC scope; SQLite re-applies tenant, library, root, workspace, availability, and generation filters to untrusted vector candidates. |
| Path or metadata disclosure | Workers use opaque IDs; cloud prompts use the configured metadata-redaction policy; default diagnostics contain only IDs/hashes, stages, timings, counts, versions, and error categories. |
| SSRF | LLM profiles accept only validated HTTP(S) endpoints under host policy; semantic workers have no network capability and documents cannot supply endpoints. |
| Credential leakage | Credentials remain in the credential store, are write-only over DTOs, never enter worker messages, backups, logs, prompts, or exported profile metadata. |
| Stale or unavailable evidence | Results carry content hash, generation, stale/available state, provenance, and coverage. Citations resolve locally and report when the original cannot open. |
| Denial of service | Hard per-tenant budgets, pre-enrolment estimates, free-space reserve admission, bounded queues/batches/candidates/tokens, cooperative cancellation, and graceful shutdown. |
| Deletion or retention failure | Exclusion uses a durable category inventory and resumable deletion plan; SQLite foreign keys cascade summaries/concepts; shared vectors remain until the final occurrence reference is removed. |
| Corruption or interrupted publication | Versioned manifests, atomic temp-file replacement, staged/complete generations, schema checks, checksummed exports, and rebuildable derived indexes. |

## Logging and diagnostic capture

Normal semantic events are represented by `SemanticDiagnosticEvent`, which has no query, excerpt,
filename, prompt, response, header, body, or credential fields. Identifiers must use a bounded safe
alphabet. Sensitive capture requires a `DiagnosticCaptureGrant`: the user previews the categories
and warning, grants at most 15 minutes, and the grant authorizes only named categories until its
expiry. Capture data is never uploaded automatically.

## End-to-end deletion proof

An exclusion begins by preventing new reads and ingestion for the scope. The durable deletion
inventory then covers catalog occurrences, extracted artifacts, derived-index records, embedding
references, summaries, concept evidence, conversation pins, and snapshots/backups in progress.
Publication and cleanup are generation-based. Removing a source record cascades summary and concept
rows; the vector reference count is decremented and the vector is deleted only at zero. In-flight
jobs observe cancellation and may not publish after exclusion. A completed deletion plan is the
proof that every catalog category reached zero; `SemanticDeletionProof` additionally requires zero
temporary backup/snapshot items and zero in-flight jobs. An interrupted plan resumes from its
recorded category. User-owned exports outside Procyon's data root are not silently modified or
deleted and remain subject to the warning shown when they were created.

## Residual and deployment risks

Desktop-managed components execute with the user's account but remain isolated from provider
authority. Administrator-managed servers must additionally enforce tenant quotas, TLS, backup
access controls, and per-tenant storage placement. Mac App Store builds cannot download executable
runtime components and therefore require bundled or administrator-provisioned capabilities.
Plaintext whole-library exports contain sensitive metadata and retained conversation data; the UI
must show the warning before writing them, and operators should encrypt them at rest.
