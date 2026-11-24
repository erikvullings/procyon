# 0348 No default answer-token caps

Status: done
Priority: high
Subsystem: backend, frontend, settings
Depends on: 0345

## Context
Every LLM call was capped: profiles defaulted to 1024 answer tokens, the app clamped any cap to
16,384, and features added their own (summaries 8192, file assistant 4096, RAG and knowledge
answers 2048, the RAG planner 256, notes 2048). A reply that reached its cap ended with
`finish_reason: "length"` and was rejected as malformed, so the call failed and had to be redone,
spending more tokens than the cap saved. Live `summarize_file` runs failed this way on every part.
Separately, the 256 KB stream bound was measured on the raw SSE body (about 150-300 bytes of JSON
per token), so any streamed reply beyond roughly 1-2k tokens also failed as malformed.

## Acceptance Criteria
- No `max_tokens`/`num_predict` is sent unless the person set an answer limit in the profile.
- The profile's answer limit is optional (default none), bounded only by the context window.
- Saved profiles with the old default of 1024 migrate to no limit; other values stay.
- Feature-level caps are gone; input budgets keep room for the answer without capping it.
- A prose reply cut off at a token limit is kept with a note; a cut-off JSON reply fails; a
  cut-off reply never runs its (possibly incomplete) native calls.
- A runaway generation is still stopped, by response-size bounds instead of a token cap.

## Implementation Notes
- `crates/fm-application/src/llm_profiles.rs`:
  - `LlmChatGeneration.maximum_tokens: Option<u32>`;
  - `tightest` combines the call's and the profile's caps;
  - bodies omit the field when `None`;
  - `LlmAdvancedSettings.maximum_answer_tokens: Option<u32>` (serde default);
  - `PROFILE_SCHEMA_VERSION` 6 migrates 1024 to null;
  - `MAX_GENERATION_ANSWER_TOKENS` is removed, and the DTO's answer cap reports the 1M context cap;
  - `MAX_STREAM_BODY_BYTES` (64 MiB raw) and `MAX_REPLY_BYTES` (1 MiB of text or call arguments)
    replace `MAX_STREAM_BYTES`;
  - `ParsedReply`/`finish_reply` decide what a cut-off reply is worth by `ReplyFormat`;
  - the tool-calling probe sends no cap either (a cut-off probe recorded "no tools").
- `document_summary.rs` and `agent_host` size inputs with an 8192-token answer room (at most
  half the window) when no limit is set.
- Frontend: profile form offers "No limit" first and never fills the limit from the model's
  report; `document-summary-dialog.ts` mirrors the backend budget; i18n `noAnswerLimit` and a new
  `outputLimitHint` in 8 locales.

## Agent Notes
- 2026-11-24 claude: implemented as above.
  - Verified: fm-application/fm-agent/fm-transport-dto 1827 tests, including 4 new ones (migration,
    no cap sent, cut-off kept, long stream); frontend 4118 tests, including 1 new settings test;
    tsc, Biome, clippy and fmt clean.
  - Not tried against a live endpoint.
  - Anthropic's Messages API requires `max_tokens`, but no transport for it exists here. If one is
    added, it must send the profile limit or a provider maximum.
  - The document-summary input is still bounded at 131,072 tokens per pass (an input bound, not
    an answer cap).
