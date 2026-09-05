# Semantic subsystem operations

The semantic subsystem is optional. Removing it leaves ordinary file management, filename search,
content search, and baseline document viewing available.

## Installation and enrolment

- `pnpm dev:tauri` enables the deterministic semantic lifecycle simulator. It exercises consent,
  install, pause, migration, removal, and gating UI, but does not install a real worker, model, or
  index. The Settings screen labels this state as a development simulation.
- `pnpm semantic:bundle:dev` builds a host-platform developer bundle under
  `target/semantic-developer-bundle/<platform>-<architecture>`. The bundle contains the real worker,
  native Zvec runtime, development model metadata, SHA-256 checksums, and a catalog signed by the
  repository's public development key. It is supported where Zvec 0.7 publishes a native runtime:
  Apple-silicon macOS, x86-64 Windows, and x86-64 or arm64 Linux. Intel macOS is unavailable.
- `pnpm dev:tauri:semantic` rebuilds that bundle and starts the debug Tauri app with it. Open
  **Settings > Semantic**, review the development-only disclosure, and install the offered
  components. Enrolment and indexing still require explicit consent for each local root.
- The developer bundle uses a deterministic 384-dimensional token-hashing embedder. It exercises
  managed installation, authenticated IPC, conversion, ingestion, persistent vector storage,
  restart recovery, and retrieval plumbing. It is not a trained semantic model and says nothing
  about production retrieval quality.
- The debug host performs one bounded indexing pass immediately after enrolment. If worker or
  provider indexing fails, consent remains enrolled and its reconciliation generation stays
  unchanged; inspect the development log, correct the reported problem, then repeat **Include
  folder** to retry without removing the root.
- Production desktop builds remain unavailable until the host injects a
  `ManagedSemanticComponentCapability`. This is an optional component pack, not a Lua plugin.
- Desktop-managed builds install only packages from the signed catalog. The preview reports exact
  package/model identity, download bytes, disk/RAM estimates, and the filesystem authority used.
- Before enrolment, estimate eligible files, extracted text, vectors, and any missing model bytes.
  `StorageAdmission` rejects the operation unless the configured free-space reserve remains.
- Server deployments use administrator-provisioned components, local-only enrolment policy by
  default, and hard tenant quotas. A tenant quota failure does not alter another tenant's catalog.
- Enrolment never occurs from a search. Roots require explicit recursive consent and remain
  independently excludable.

### Publishing a production component pack

1. Build worker/runtime artifacts for each supported target and publish immutable payloads through
   a catalog-ID-only artifact source. Do not accept user-supplied download URLs.
2. Evaluate the exact model revision and package set against the task-0188 baseline, then record
   license, tokenizer, dimensions, normalization, language coverage, download/disk/RAM estimates,
   protocol compatibility, and checksums in the catalog.
3. Canonicalize and sign the catalog with the release signing key. Ship only the trusted verifying
   key and catalog revision with Procyon; host adapters verify the signature and every artifact
   checksum before activation.
4. Construct `ComponentManager` with the platform app-data root and inject
   `ManagedSemanticComponentAdapters` for artifact reads, free-space checks, activation probing,
   indexing pause, worker quiescence, and authoritative index removal. Pass the resulting
   `ManagedSemanticComponentCapability` to `FileManagerService::with_semantic_component_capability`
   in the desktop host.
5. Publish the platform payloads alongside a direct-distribution release and run installed/absent,
   rollback, tamper, low-disk, and hardware smoke tests. Mac App Store builds must bundle executable
   capabilities or keep them unavailable; they may not download executable packs at runtime.

No production model pack or release catalog is currently selected or shipped. The developer bundle
is platform-specific and may be copied as a complete directory to another developer using the same
OS and architecture. The recipient must use a debug build and point
`PROCYON_SEMANTIC_DEVELOPER_BUNDLE` at that directory. Its signing key is public, so the signature
only tests catalog verification; it establishes no publisher trust. Never redistribute it as a
production component pack. Remove it by uninstalling the semantic components in Settings and
deleting the copied bundle directory after the app exits.

## Diagnostics and privacy

Default logs include only opaque IDs or hashes, stage, timing, counts, component/model/profile
identities, and redacted error categories. They exclude queries, excerpts, filenames, prompts,
responses, credentials, headers, and HTTP bodies. A sensitive capture is local, previewed,
category-scoped, and expires after at most 15 minutes.

Remote LLM requests always show their profile, locality, scope, minimized metadata, and exact
content preview before transmission. The semantic worker itself has no network authority.

## Backup and recovery

Ordinary backup includes enrolment policy/exclusions, exact model manifest identity, profile
metadata without secrets, SKOS sources/accepted edits/review decisions/attachments, and saved
conversation/citation pins. Zvec indexes, extracted chunks, embeddings, generated summaries, and
concept annotations are rebuildable and may be omitted.

Whole-library exports use schema version 1, stable entry names, SHA-256 checksums, and an explicit
plaintext-content warning. Import rejects unknown schema versions, unsafe names, and any checksum
mismatch before applying data. Encrypt exported files with the platform's storage controls.

After a crash, reopen the authoritative catalogs first, recover or roll back incomplete journal
transactions, then resume deletion and ingestion jobs. A damaged or incompatible derived index is
quarantined and rebuilt from authoritative state; never reinterpret vectors under a different
model-space fingerprint.

## Evaluation and change control

The local evaluation suite stores query/relevance judgments on device and computes file recall@k,
chunk recall@k, mean reciprocal rank, and binary nDCG@k. It performs no telemetry. The repository
fixture covers multilingual recall, near duplicates, boilerplate diversity, structural citations,
incremental edits, summaries, scope isolation, unavailable sources, and concept labels.

Changes to model, chunker, converter, index, grouping, summary selection, or labelling threshold
require a before/after `EvaluationChangeReport` with distinct fingerprints, the same cases/cutoff,
an explicit migration description, and signed storage impact. Generated-answer fluency is not an
evaluation metric.

## Storage and unsupported formats

Storage diagnostics aggregate authoritative measurements by enrolled root and detected format,
separating active bytes from bytes pending cleanup. They do not trust caller-supplied totals.
Baseline conversion supports plain text, source code, Markdown, HTML, DOCX, PPTX, XLSX, CSV, and
PDFs with an extractable text layer. Unsupported, encrypted, malformed, over-budget, and image-only
documents remain visible as typed skip or omission reasons; install an optional converter only
after its capability and resource disclosure has been reviewed.

## Optional advanced packs

Advanced converters, acceleration backends, and rerankers are independent packs rather than
baseline dependencies. Each pack has its own signed manifest, artifact checksum, target triples,
worker-protocol and index-schema range, download/disk/RAM estimate, and baseline comparison. A
server accepts only capability families on its administrator allow-list. Installation verifies the
signature, compatibility, policy, and payload before activation; activation retains one rollback
version per capability family. Removing one family does not alter the others.

Advanced converters receive bounded bytes and trusted metadata, never paths, credentials, provider
handles, or network authority. Baseline conversion runs first and remains authoritative for every
format it supports. Advanced output must preserve the task-0180 structure, report provenance
precision and omissions, and pass the same source, expansion, output, timeout, and cancellation
limits. Removing the pack therefore returns unsupported scanned or complex documents to their
typed baseline outcome without affecting baseline-readable documents.

Acceleration starts in CPU mode. Before sharing an existing index, the accelerated backend must
match CPU vectors within the configured tolerance. A different embedding space is exposed as an
explicit baseline/candidate fingerprint migration and requires building and activating a separate
index generation. Driver or runtime failure returns CPU vectors; it never publishes partial or
unlabelled accelerated vectors.

Reranking is local, quality-gated, bounded to the configured candidate count and text size, and
cancellable. Diagnostics report only model fingerprint, count, input characters, and elapsed time.
Without an active reranker, dense retrieval remains unchanged. `Semantic` remains dense-only;
`Hybrid` is a separately selected mode that combines independently ranked dense and lexical
results using deterministic weighted reciprocal-rank fusion. Fusion uses ranks rather than
incomparable raw scores, applies structured filters first, and rejects a request if any candidate
has a different tenant or library scope.

Every signed advanced-pack report contains comparable baseline and candidate measurements for
multilingual, OCR, exact-term, code, structured-document, duplicate, latency, memory, and storage
fixtures. Rerankers additionally require at least a 0.02 absolute nDCG improvement. Release
packaging tests verify all declared macOS, Windows, and Linux target triples and reject undeclared
targets; platform hardware smoke tests remain mandatory before publishing an accelerated pack.

## Troubleshooting and deletion

1. Check component/model compatibility and available disk reserve.
2. Inspect root coverage for pending, stale, excluded, skipped, failed, and unavailable counts.
3. Restart the worker; authenticated clients reconnect and incomplete staging generations remain
   invisible.
4. If the derived index is corrupt, preserve authoritative settings, remove only the identified
   derived data directory, and rebuild.
5. For exclusion, monitor every deletion category until complete. Shared vectors remain only while
   another authorized occurrence references them.
6. For complete removal, delete saved conversations/pins, vocabulary attachments and annotations,
   semantic catalog/extracted data, Zvec data, models/runtimes, then verify the deletion inventory.

Manual release verification covers keyboard and screen-reader operation for installation consent,
the enrolment tree, progress/errors, semantic results/evidence, summaries, Ask/citations, profile
setup, and SKOS review in both browser and desktop hosts. Platform packaging smoke tests run with
the optional subsystem installed and absent.
