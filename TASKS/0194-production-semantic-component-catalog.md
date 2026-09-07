# 0194 Production semantic component catalog

Status: open
Priority: high
Subsystem: backend, release
Depends on: 0178, 0188, 0191, 0192

## Context

The completed semantic implementation currently ships only through a development bundle signed by
a public development key. Release builds intentionally reject that bundle, so merging the semantic
branch does not make local indexing, search, summaries, or Ask available to ordinary desktop
users. Establish the production supply-chain contract before producing platform artifacts.

Do not start implementation until the completed semantic branch has been merged into `main` and
its integration CI is green. The branch merge itself is not part of this task.

## Acceptance Criteria

- Define immutable production artifact identities for the semantic worker, Zvec runtime, and
  pinned multilingual-E5 model, including versioning and compatibility with the protocol, index
  schema, converter, chunker, tokenizer, and model revision.
- Build the production catalog from an auditable manifest containing source, license, target,
  size, resource estimates, and SHA-256 for every payload; no runtime URL or checksum is supplied
  by the user.
- Keep the release signing private key outside the repository and CI logs. Release jobs sign the
  canonical catalog, while applications contain only the trusted public verification key.
- Verify catalog signatures and payload checksums through the existing managed-component
  installer, including tampered, truncated, unknown, and incompatible artifact failures.
- Document catalog rotation, artifact retention, rollback, emergency revocation, and reproducible
  local verification.
- Add automated tests for deterministic catalog generation, signature verification, compatibility
  rejection, and key/credential redaction.

## Implementation Notes

- Extend the contracts in `crates/fm-semantic-components`; do not introduce a second installer.
- Reuse the model and worker manifests produced by the developer-bundle tooling, but never reuse
  its public signing key or development artifact IDs.
- Deterministic Docling is compiled into the worker and does not require a separate model package.
- This task defines the common catalog and signing pipeline. Target-specific payload production is
  tracked by 0195.

## Agent Notes

- 2026-09-07 Copilot: Created after confirming that merge-to-main alone cannot enable semantic
  features in release builds. Start only after the semantic implementation branch is merged and
  green on `main`.
