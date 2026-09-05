# Semantic subsystem operations

The semantic subsystem is optional. Removing it leaves ordinary file management, filename search,
content search, and baseline document viewing available.

## Installation and enrolment

- Desktop-managed builds install only packages from the signed catalog. The preview reports exact
  package/model identity, download bytes, disk/RAM estimates, and the filesystem authority used.
- Before enrolment, estimate eligible files, extracted text, vectors, and any missing model bytes.
  `StorageAdmission` rejects the operation unless the configured free-space reserve remains.
- Server deployments use administrator-provisioned components, local-only enrolment policy by
  default, and hard tenant quotas. A tenant quota failure does not alter another tenant's catalog.
- Enrolment never occurs from a search. Roots require explicit recursive consent and remain
  independently excludable.

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
