# 0207 Optional knowledge answer from evidence

Status: done
Priority: medium
Subsystem: rag, frontend
Depends on: 0206

## Context

When an LLM profile exists, let users optionally consume an already inspected Knowledge Search
evidence set. Answer generation is a downstream enhancement and must not rerun or alter retrieval.

## Acceptance Criteria

- Expose answer capabilities only when configured; absence is neutral and leaves search complete.
- Capture action, context, constraints, depth, and format in `KnowledgeAnswerRequest` without adding
  them to search embeddings or retrieval plans.
- Generate from the confirmed existing evidence set and fingerprint; require an explicit refresh
  to retrieve again.
- Cite the same displayed evidence identities and open exact sources through existing authority.
- Preserve grounded-only defaults, explicit model-knowledge distinction, endpoint consent,
  cancellation, redaction, and saved-evidence deletion behavior.
- Cover no-profile use, stale evidence, profile failure, cancellation, citation stability, and
  HTTP/Tauri/mock parity.

## Implementation Notes

- Reuse the safe portions of the existing grounded Ask generation capability; do not duplicate
  retrieval or make Knowledge Search a wrapper around Ask.

## Agent Notes

- 2026-09-08 Copilot: Created from restored Structured Knowledge Query phase 10.
- 2026-09-08 Copilot: Added optional answer generation as a strictly downstream capability over
  an already displayed Knowledge Search evidence fingerprint. A bounded, expiring,
  tenant/workspace-bound cache retains the exact evidence order and content without paths; answer
  generation has no retrieval dependency, refreshes catalog authorization before prompting, and
  returns a typed refresh-required error rather than rerunning search. Reused the grounded Ask
  profile, TLS/endpoint-consent, cancellation, filename-redaction, and model-knowledge boundaries.
  Added stable evidence citations, HTTP and Tauri generate/cancel endpoints, equivalent mock
  behavior, and an optional Mithril answer panel that stays absent when no profile is configured.
  Verified generated API stability, 2,521 Rust tests, 3 Rust doctests, 2,063 frontend tests,
  TypeScript, rustfmt, warning-free Clippy, Biome (pre-existing specificity warnings only), and a
  final combined correctness/privacy review.
