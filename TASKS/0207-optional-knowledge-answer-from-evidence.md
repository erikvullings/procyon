# 0207 Optional knowledge answer from evidence

Status: open
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
