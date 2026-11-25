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
- 2026-11-25 claude: model catalogue, family fallback, effort saved with the chat.
  - `model_thinking.json` + `model_thinking.rs`: the maintained list of model families and how
    they control thinking (`none` / `toggle` with its `chat_template_kwargs` switch / `always`,
    default on or off, `reasoning_effort` value per level, source repo, note). Pre-populated from
    the chat templates and cards on Hugging Face of the models configured or installed here:
    Inkling-2-Mini (effort none..max, `none` does not fully disable), Qwen3.8 27B/Flash-Next incl.
    ThinkingCap and Dirk (`enable_thinking`; only low/medium/xhigh, others raise a template error,
    so High sends `xhigh`), Gemma 4 (`enable_thinking`, off by default, no levels), Ornith 1.5,
    Granite 4.2 (only `low` changes anything), Qwen3 14B, Kimi K2.5 (`thinking`), MiniMax M2.x
    (always thinks), DeepSeek V4 (`thinking`; Max mode not offered), gpt-oss, Llama 3.x (none).
  - A listed model gets exactly what its template expects, then nothing; an unlisted one is still
    tried as before. Family fallbacks (`family: true`: qwen, gemma, granite, deepseek) reuse the
    family's switch and try the plain level name first, dropping it when refused. A 500 counts as
    a refusal too when the request asked for thinking (a template error may surface as 500).
  - `ThinkingSupportDto` on the role assignment; Settings shows one line under the chat role's
    effort ("Thinking can be turned off; it is on by default. Levels it distinguishes: … From the
    model card of …", or "Guessed from its model family").
  - Chat: the conversation stores the effort it was last asked with (`thinking`, optional field,
    schema 2 unchanged), restored when it is opened, copied by a fork; mock does the same.
  - Not verified against the live servers: what status oMLX returns for a template error.
  - Verified: 3 new Rust tests (catalogue validity and lookups for the 12 models, listed and family
    attempts, chat effort saved), 2 frontend (restore on open, support line); fm-application +
    fm-server 1850 and frontend 4216 pass; `pnpm lint` clean.
- 2026-11-25 claude: checked against running servers with `scripts/probe-thinking.py`.
  - oMLX 0.8.3 never refuses a value (`bogus` answers 200): unknown values are ignored, so the
    fallback never triggers there and the catalogue's values matter. Inkling: reasoning grows
    with effort. Dirk-Qwen3.8: `none` or `enable_thinking=false` turn thinking off. Gemma 4: no
    thinking unless `enable_thinking=true`; `reasoning_effort` ignored. Qwen3.8-Flash-Next does
    not fit in this Mac's memory ceiling (74 GB, 507 from oMLX).
  - DGX Spark (spark-1 and spark-2, SGLang): both serve `qwen3.8-27b`, with their own chat
    template (`lean` default, high/max → xhigh, minimal → low). Unknown values return 400, which
    the fallback handles. spark-301b serves `inkling-2-mini`. Both families were already listed;
    the results are in the entries' new `tested` field.
  - Found: `spark-cockpit` in ~/.zshrc tunnels spark-1/2's model API to remote port 8001, but the
    API listens on 30001 (nginx proxy) / 30000 (SGLang), so local 30002/30003 reset; keys are in
    ~/.config/qwen38/spark-{1,2}-api-key. Not changed (the person's shell config).

