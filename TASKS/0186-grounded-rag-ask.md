# 0186 Grounded RAG Ask experience

Status: open
Priority: medium
Subsystem: frontend, backend, rag
Depends on: 0183, 0184

## Context

Semantic retrieval should remain useful without an LLM. When a profile exists, Procyon can add a
read-only Ask experience that retrieves local evidence and streams a cited answer. Procyon, not the
semantic worker, owns prompts, credentials, provider calls, conversation state, and citation
resolution.

Ask must not become a filesystem agent. Indexed documents are untrusted evidence and cannot expand
scope, change instructions, request secrets, or invoke Procyon actions.

## Acceptance Criteria

- Ask appears only when semantic retrieval is available and an LLM profile is configured. Failure
  or removal of either capability leaves ordinary semantic search unchanged.
- Every conversation shows its profile and evidence scope. New conversations default visibly to
  Entire indexed library and can switch in one action to selected files, current folder recursively,
  a semantic result set/virtual folder, or named enrolled roots.
- Retrieval is local dense search with filters, score thresholds, per-document diversity, and
  adjacent structural chunk expansion under a strict token budget. No reranker or query-time LLM
  rewrite is required in this task.
- Before generation the UI exposes retrieved files/excerpts and coverage status. Pending, stale,
  failed, excluded, or unavailable scope portions remain visible; unavailable retained chunks may
  answer questions while their citations report that the original cannot currently open.
- Prompt assembly assigns opaque citation labels and includes only bounded excerpt text, useful
  title/filename according to profile redaction, section/page provenance, question, and bounded
  conversation history. Absolute paths, account/provider details, credentials, unrelated metadata,
  and stable internal IDs never go to a cloud model.
- System and user prompts delimit retrieved text as untrusted evidence that cannot alter authority.
  The LLM receives no action/tool schema and RAG cannot mutate files, enrol roots, broaden scope, or
  read additional content autonomously.
- Answers stream through the semantic client with retrieval, token, done, cancellation, and error
  states. Citations resolve locally to exact/best-available source navigation or retained evidence.
- Grounded-only is the default and asks the model to state when evidence is insufficient. A visible
  per-conversation Allow model knowledge toggle may permit general knowledge, but uncited/model-only
  claims remain distinguishable from library-backed claims.
- Conversations are ephemeral by default. Explicit Save stores question/answer, profile/scope
  metadata, and citation identities without duplicating all chunks. It may pin cited evidence and
  reports storage use; deleting the conversation releases pins, while source exclusion overrides
  pins immediately.
- RAG retrieval may use 0185 summaries for selection/compression, but source claims link to original
  supporting chunks. The UI distinguishes generated summaries from source excerpts.
- Local diagnostics and feedback record metadata/timing/status only. Export/share previews exactly
  what conversation and source metadata will leave the app.
- Tests cover all scopes, diversity/context budgets, insufficient evidence, citation mapping,
  unavailable/stale sources, prompt-injection fixtures, metadata minimization, grounded/model-
  knowledge modes, streaming/cancellation/failure, saved pins/deletion, cloud consent, tenant
  isolation, adapter parity, keyboard use, and screen-reader semantics.

## Implementation Notes

- Keep retrieval and answer generation as separate typed stages so retrieved evidence is inspectable
  and testable without a provider.
- Chat Completions streaming is supplied by 0184. Do not build a second provider client here.
- Natural-language file operations are explicitly out of scope and belong to a future typed,
  previewable operation-planning feature.

## Agent Notes

- 2026-09-04: Split from 0176. Ask defaults to the entire library but always displays scope; it is
  grounded and read-only by default with an explicit model-knowledge toggle.
