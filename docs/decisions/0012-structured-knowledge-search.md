# 0012 Structured Knowledge Search

Status: accepted

## Context

Procyon needs a way to retrieve relevant material from intentionally indexed roots without
requiring an LLM, remote provider, or network connection. The earlier bounded multi-query RAG
experiment asked a configured LLM to rewrite a question before retrieval. That experiment remains
useful benchmark evidence, but it cannot be the primary knowledge-search experience because many
users have no generation profile and sending the question changes the privacy boundary.

The semantic worker already owns a Zvec derived vector index and an authoritative SQLite catalog.
Pinned zvec-rust 0.7.0 also provides native FTS, independent FTS queries, dense and FTS subqueries,
and reciprocal-rank fusion.

## Decision

Implement Structured Knowledge Search as a search-first, host-neutral capability:

```text
KnowledgeSearchRequest
  -> deterministic KnowledgeSearchPlan
  -> native Zvec FTS and/or dense retrieval
  -> reciprocal-rank fusion and document diversification
  -> authorized KnowledgeEvidence with optional RetrievalTrace
  -> optional KnowledgeAnswerRequest using the inspected evidence
```

`KnowledgeSearchRequest` owns retrieval inputs only: subjects, knowledge needs, explicit related
terms, authorized scopes, retrieval mode, and bounded options. Initial needs are overview,
definition, procedure, examples, evidence, arguments, comparison, limitations, and references.
The deterministic planner always searches the raw subject, applies conservative need templates,
uses explicit related terms only as lower-priority expansions, deduplicates equivalent searches,
and preserves every reason for a query.

`KnowledgeAnswerRequest` is separate. It may contain an action, application context, constraints,
depth, and output format, but it consumes an already retrieved evidence fingerprint. `do:` and
`to:` content never enters retrieval unless the user explicitly repeats it as `related:`.
Generation is optional, never automatic, and never required to parse or execute a search.

The physical retrieval layer supports `hybrid`, `fullText`, and `semantic` routes. Native Zvec FTS
and dense vectors are peers. Hybrid results use deterministic RRF over ranks; raw BM25 and cosine
scores are never added or normalized into one score. Ranking precedes per-file diversification and
bounded adjacent/section context expansion.

Every result carries stable file/chunk identities, structural provenance, the source query and
retrieval reason, route-specific ranks, final rank, availability, and authorization-safe source
navigation. An optional bounded trace explains planning and fallback decisions without exposing
filesystem paths or document content.

## Authority and capability boundaries

- `fm-semantic-worker` owns native indexing and candidate retrieval. It receives opaque identifiers
  and content already admitted by the host; it has no filesystem, consent, or LLM authority.
- `fm-application` owns authorization, scope resolution, deterministic logical planning, capability
  composition, evidence materialization, and optional answer orchestration.
- HTTP and Tauri remain thin adapters over the same application capability. The mock client
  implements the same contract.
- The Mithril frontend depends only on `FileManagerClient`; it displays explicit FTS, vector, and
  answer capabilities independently.
- SQLite remains authoritative for publication and evidence. Zvec candidates are reauthorized
  against one SQLite snapshot before leaving the worker.

Capability reporting distinguishes FTS availability, compatible query embeddings, and optional
answer generation. Hybrid requests fall back explicitly to FTS when embeddings are unavailable or
incompatible. Search remains complete and useful when answer generation is absent.

## Index migration

The current Zvec schema is vector-only and stores no searchable text. Schema version 2 will be
rebuilt non-destructively from SQLite's retained content and vectors at a staging path. The new
collection is flushed, optimized, inspected for complete indexes, and atomically published before
the old collection is eligible for reclamation. An interruption leaves version 1 readable and the
staged rebuild restartable. Runtime `add_column` and `create_index` exist, but are not used for this
migration because every row requires backfill and an in-place build has a partial-index state.

## Consequences

- Knowledge Search works offline with FTS alone and does not require a generation profile.
- Exact specialist terms, identifiers, headings, and titles can complement semantic similarity.
- Logical planning remains independently testable and is not coupled to Zvec `MultiQuery`.
- Existing grounded Ask stays separate and uses its original single-query retrieval by default.
- The developer multi-query experiment remains dormant for reproducibility but has no user-facing
  control.
- Production visibility remains gated on multilingual retrieval, migration, lifecycle, privacy,
  accessibility, and supported-platform evaluation in task 0208.

## Sequential implementation

Tasks 0202-0208 own, in order: native FTS and staged migration; hybrid retrieval and evidence;
canonical request and deterministic planner; DSL and rule parser; search-only UX and host parity;
optional answer generation from existing evidence; and evaluation/release qualification.
