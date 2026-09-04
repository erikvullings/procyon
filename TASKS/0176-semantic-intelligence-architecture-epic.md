# 0176 Semantic intelligence architecture epic

Status: done
Priority: high
Subsystem: architecture, search, rag
Depends on: none

## Context

Procyon needs optional local semantic search, grounded retrieval-augmented generation (RAG),
document summaries, and SKOS-backed document labelling without making its ordinary file-manager
features depend on a large machine-learning installation. The semantic subsystem may duplicate
extracted source text and consume substantial disk, memory, and CPU, so installation, folder
enrolment, cloud disclosure, and deletion semantics must be explicit.

This epic records the agreed architecture and product requirements. Implementation is split into
0177-0189; do not implement the whole subsystem in this task or reopen it as an umbrella work item.
The subtasks depend on this completed decision record where necessary.

## Architecture And Requirements

- Semantic features are strictly optional. If components are absent, paused, incompatible, or fail
  to start, existing name/content/native-indexed search, previews, smart folders, and operations
  retain their current behaviour. Procyon never downloads components or falls back to a cloud
  service without consent.
- A first-party, pure-Rust, per-user singleton worker owns conversion, structural chunking, local
  embedding inference, Zvec writes, and retrieval. Procyon owns consent, VFS access, settings,
  credentials, search integration, RAG prompts, citations, and lifecycle. This is not a Lua plugin.
- The worker is downloaded only after opt-in, has no arbitrary filesystem or LLM credential access,
  and communicates through authenticated, bounded, versioned protobuf messages over Unix-domain
  sockets or Windows named pipes. It owns Zvec's single writer and supports multiple Procyon windows.
- Desktop manages worker/model installation. `fm-server` uses the same capability contracts but an
  administrator provisions components, data roots, enrolment, tenant isolation, and quotas. Mock
  mode uses deterministic fixtures.
- Embeddings are local-only. One exact model revision and embedding space applies to a device-local
  library; changing it is an explicit resumable full migration. Setup recommends a compact
  multilingual profile while keeping concrete models in a curated, signed manifest.
- Procyon streams provider-neutral file content to the worker. Version one enrols local roots, but
  the contract must permit later remote-provider support without giving the worker path access.
- Enrolment is recursive with explicit descendant exclusions and visible inherited state. Exclusion
  revokes consent and deletes chunks, vectors, extracted text, labels, summaries, and pinned evidence.
  Pause is a separate non-destructive operation. Symlinks never escape an enrolled root.
- A device-local SQLite catalog is authoritative; Zvec and extracted artifacts are derived. Content
  hashes verify changes, idempotent jobs recover after crashes, and document generations publish
  atomically. Filesystem events trigger work; startup and 30-minute reconciliation recover missed
  events. Temporarily unavailable roots retain searchable evidence and unavailable source links.
- Conversion starts with a bounded Rust baseline for text, code, Markdown, HTML, OOXML, and
  text-layer PDFs. A versioned converter interface permits a separately downloadable advanced
  OCR/layout pack. Unsupported and skipped content is reported rather than silently omitted.
- Chunking follows real document structure and records the strongest available page, section, slide,
  sheet/range, line, or symbol provenance. Embedding input is section hierarchy plus chunk content;
  path, filename, MIME, dates, and SKOS concepts remain replaceable scalar metadata.
- Embeddings are cached by normalized embedding input, exact model revision, and chunker version.
  Occurrence records preserve each file's provenance and access scope. Changed documents reuse all
  matching vectors and embed only new fingerprints.
- Semantic search is an explicit search mode, dense-vector-only initially, integrated with the
  existing search virtual locations and saved searches. It defaults to the current folder
  recursively. Results are files with expandable supporting chunks; partial/stale/failed coverage
  is visible, and searching never grants enrolment consent.
- RAG is separate from semantic retrieval and appears only after an LLM profile is configured.
  Profiles support local OpenAI-compatible services and Azure/OpenAI-compatible cloud endpoints;
  secrets stay in the credential service. Ask defaults visibly to the entire indexed library and
  can switch to selected files, a folder, virtual folder, or enrolled roots.
- RAG is read-only, grounded by default, and gives the model no Procyon tools or actions. Retrieved
  documents are untrusted evidence. A visible per-conversation switch may permit general model
  knowledge, whose uncited claims remain distinguishable. Cloud consent is bound to the endpoint
  host, and cloud prompts omit absolute paths and unrelated metadata.
- Document summaries reuse existing chunk embeddings: deterministic bounded k-means groups content
  chunks, the nearest real chunk to each centroid is selected, medoids are weighted and ordered by
  source position, and an LLM summarizes only that representative evidence. Summary chunks are
  embedded in Zvec, discoverable/openable, linked to supporting chunks, and excluded from recursive
  summarization.
- Named SKOS vocabularies attach to roots/workspaces. Imported and accepted concepts are
  authoritative; machine suggestions require review. Concept IDs are replaceable chunk metadata,
  update without re-embedding, and back concept-based virtual folders.
- Normal diagnostics contain no queries, excerpts, filenames, prompts, credentials, or HTTP bodies.
  Derived indexes are omitted from ordinary backup; policy, vocabularies, model identity, saved
  conversations, and an optional checksummed whole-library snapshot have explicit backup rules.

## Acceptance Criteria

- The architecture above is represented by dependency-ordered tasks 0177-0189, each independently
  resumable and testable.
- Every implementation concern is owned by exactly one primary task; cross-task dependencies are
  explicit and only point to prerequisites that must be complete first.
- The task index contains a dedicated Semantic search, RAG & knowledge section and redirects work to
  the subtasks rather than treating this epic as implementation work.

## Implementation Notes

- Extend the provider-neutral search and virtual-location seams from 0162 and 0166. Do not create a
  second unrelated result/navigation model.
- Keep the worker behind an application capability service; `FileManagerService` remains a thin
  composition/delegation facade.
- The installed Zvec skill currently documents Python and Node.js bindings. This architecture uses
  the official Rust SDK; 0181 must verify its actual API and platform behaviour rather than
  mechanically translate binding-specific examples.
- Delivery order: contracts/lifecycle, conversion and ingestion, semantic search, RAG and summaries,
  SKOS, hardening, then optional advanced packs.

## Agent Notes

- 2026-09-04: Created after a decision-tree review of privacy, enrolment, sidecar boundaries,
  incremental updates, search UX, RAG configuration, summaries, and SKOS. Architecture is settled;
  implementation proceeds through 0177-0189.