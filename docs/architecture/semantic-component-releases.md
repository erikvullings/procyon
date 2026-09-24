# Independent Semantic Component Releases

## Context

Semantic workers, native runtimes, models, and signed catalogs have a different build, signing,
qualification, and update cadence from the Procyon desktop application. Building them again from a
desktop release tag changes source-revision and public-URL identity, while macOS notarization and
Windows packaging can also change artifact bytes. A byte-for-byte comparison with artifacts from
an earlier private qualification must therefore fail even when both builds are healthy.

The `v0.1.0-33` public workflow demonstrated this coupling: four payload and four catalog jobs
passed, but collection rejected the rebuilt aggregate because it could not equal the reviewed
private report.

## Decision

Publish semantic components through `.github/workflows/release-semantic-components.yml`, separately
from desktop releases.

1. A qualification dispatch receives the intended immutable `semantic-v*` release tag. It builds,
   signs, tests, and retains all four target payload/catalog sets using that final public asset URL.
2. Qualification generates a reviewable component lock containing the exact run ID, source
   revision, evaluation fingerprint, catalog revisions, and catalog/signature SHA-256 values.
3. After review, the lock is committed as
   `docs/evaluations/semantic-component-release-v1.json`.
4. A publication dispatch names the exact qualification run. It downloads those retained bytes
   instead of rebuilding them, reproduces the reviewed evaluation, verifies every catalog,
   signature, payload checksum, payload size, source revision, and release URL against the lock,
   then creates the immutable semantic component release.
5. Desktop releases never build or publish semantic components. When semantic support is enabled,
   each platform downloads only its approved catalog and detached signature from the locked
   component release, verifies their hashes and identity, and embeds them with the production
   verification key.

`SEMANTIC_COMPONENTS_RELEASE_QUALIFIED` authorizes component publication.
`SEMANTIC_RELEASE_QUALIFIED` independently controls whether a Procyon desktop build embeds the
approved component catalog. Structured Knowledge Search keeps its existing independent gate.

## Consequences

- Component build/signing failures cannot fail a Procyon desktop release.
- Desktop packaging failures cannot invalidate or rebuild an approved component set.
- Multiple Procyon versions may consume one component release.
- Component improvements can ship under a new `semantic-v*` tag without changing the desktop
  version.
- Both paths remain fail closed: an unapproved lock, missing target, changed catalog/signature,
  mismatched source revision, changed payload, or non-immutable download URL stops publication or
  embedding.
- Qualification artifacts must remain available until the reviewed lock is merged and the exact
  publication dispatch completes.
