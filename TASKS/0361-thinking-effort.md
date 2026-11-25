# 0361 Thinking effort: role default, per-conversation choice

Status: done
Priority: medium
Subsystem: backend, frontend, llm
Depends on: none

## Context

Maintainer (2026-11-25): is there an option to set the thinking effort? There was none. Every
generation asked local servers not to think (Ollama `think: false`; the learned
`ReasoningSwitch` for vLLM, SGLang, oMLX, LM Studio, mlx-serve) and sent nothing to cloud APIs, so
those used the model's own default. The effort must be settable in the chat interface too; the
role setting only gives the default.

Whether a model accepts an effort, and which values, cannot be known in advance for
OpenAI-compatible servers (OpenAI rejects `reasoning_effort` for non-reasoning models; accepted
values differ by model generation: `none`, `minimal`, `low`, `medium`, `high`). Ollama documents
`think` as a boolean, with `"low" | "medium" | "high"` for gpt-oss, and refuses `think` for models
without the thinking capability.

## Acceptance Criteria
- `ThinkingEffort`: `auto` (as before: thinking off on local servers, the provider's default in the
  cloud), `off`, `low`, `medium`, `high`.
- The chat role assignment stores a default effort (Settings, next to the chat model; `/model`
  keeps it). Saving a role without an effort keeps the stored one.
- A chat conversation (chat tab) and a terminal assistant session (`/effort`) can choose an effort
  for their requests; without a choice the role default applies. Background calls (summaries,
  naming, JSON tasks) keep `auto`, so they stay fast.
- Mapping per endpoint, tried in order; a 400/422 on a request that carried a thinking parameter
  falls back to the next variant, and the variant that worked is remembered for the session:
  - local OpenAI-compatible presets: `off`/`auto` use the existing learned switch;
    `low`..`high` send `reasoning_effort`, then nothing.
  - other OpenAI-compatible endpoints (cloud, Azure, generic): `auto` sends nothing; `off` tries
    `none`, `minimal`, `low`, then nothing; `low`..`high` send that level, then nothing.
  - Ollama: `off`/`auto` send `think: false`; `low`..`high` send the level for gpt-oss models and
    `think: true` for others, then `think: false`.
- A thinking request waits as long between streamed chunks as for the first one, since a model
  may think silently.
- Browser and Tauri behave the same; API regenerated.
- Tests: mapping per preset and level, fallback on rejection, role default kept on save, request
  override, chat UI choice sent, terminal `/effort`.

## Agent Notes
- 2026-11-25 claude: done.
  - `llm_profiles.rs`: `ThinkingEffort` (auto/off/low/medium/high), `ThinkingRequest`
    (`Background` = auto, `Conversation(Option<effort>)` = the choice or the role default),
    `RoleAssignment.thinking` (serde default `auto`; a save without one keeps the stored one, so
    `/model` keeps it). `thinking_attempts(profile, effort)` is the mapping above; each attempt sets
    `reasoning_switch` / `reasoning_effort` / `ollama_think` on the request. A 400/422 to a request
    that asked anything is `Rejected` and the next attempt is tried, for Ollama too
    (`generate_ollama`). The accepted attempt per profile, model and effort is kept in memory for
    the session (not persisted; the off/auto switch is still learned and saved as before). A
    thinking request gets the first-reply wait (5 min) between chunks.
  - Conversation requests: chat replies (with and without tools) and the terminal assistant
    (ask, explain, fix, command). Background calls (summaries, side calls, naming, JSON tasks,
    compaction, file assistant) stay `auto`.
  - DTOs: `ThinkingEffortDto`; `LlmRoleAssignmentDto.thinking`, `SaveLlmRoleRequestDto.thinking`
    (optional), `ChatReplyRequestDto.thinking`, `TerminalAssistRequestDto.thinking`.
  - UI: Settings → chat role "Thinking effort" (default); chat header select "Thinking: default /
    Off / Low / Medium / High" per chat tab (not saved with the conversation); terminal
    `/effort [off|low|medium|high|default]`. i18n in 8 locales.
  - Limits: which levels a model really honours cannot be known in advance; a model that only
    switches thinking on or off (most Ollama models, Qwen templates) thinks at its own depth for
    low, medium and high. A cloud endpoint that silently ignores `reasoning_effort` cannot be
    detected. The chat's effort lasts for its tab, not with the saved conversation. Not tried
    against real endpoints.
  - Verified: 7 new Rust tests (mapping, bodies, fallback + memory + wait, role default, request
    override, chat request), 3 frontend (chat select, terminal `/effort`, settings row);
    fm-application + fm-server 1847 and frontend 4214 pass; `pnpm lint` clean; checked in the
    browser (mock): the picker in the chat header and the default in Settings.

