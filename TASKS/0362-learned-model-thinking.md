# 0362 Learn a model's thinking controls from the chat template it is served with

Status: done
Priority: medium
Subsystem: backend, llm, frontend
Depends on: 0361

## Context

0361 added a curated list of model families and how they control thinking
(`crates/fm-application/src/model_thinking.json`), plus trial with fallback for models that are
not listed. What trial finds is kept for the session only, and it is weak evidence: oMLX answers
200 to any `reasoning_effort`, even `bogus`, so "accepted" does not mean "used".

The rules that matter (which switch, which levels, the default) live in the chat template, and a
template can be checked without running the model: render it with each switch and level and see
which values change the prompt or raise an error. The template must be the one the server uses:
the DGX Spark serves Qwen3.8 with its own template (`lean` default, `high` mapped to `xhigh`),
so the upstream template on Hugging Face would give the wrong answer. Hugging Face lookup was
therefore considered and dropped (maintainer, 2026-11-25: only if it helps; it does not).

oMLX lists each model's folder (`model_path` in `GET /v1/models/status`) and reports its own
reading of the template (`reasoning_efforts`, `reasoning_levels`, `thinking_default`), which is
incomplete (`reasoning_efforts` is empty for Dirk-Qwen3.8 and Inkling).

## Acceptance Criteria
- Template analysis (`model_thinking`): render a Jinja chat template with `minijinja` (Python
  string methods via `minijinja-contrib`, `raise_exception`, `strftime_now`) for: nothing set,
  `enable_thinking` and `thinking` true/false, and `reasoning_effort` in none, minimal, low,
  medium, high, xhigh, max, lean and an unknown value. From the renders derive the switch (the
  kwarg whose true/false renders differ), the default (which of those the plain render equals),
  the accepted efforts (render without error and differ from the unknown value's render when that
  one does not raise), and from those the value per effort (off: none when accepted and no
  switch; high: high, else xhigh, else max; low: low, else minimal). A template that does not
  render, or shows no thinking control, yields nothing.
- Template source: oMLX endpoints on this machine (loopback). The model's `model_path` from
  `/v1/models/status`, then `chat_template.jinja`, or `chat_template` in
  `tokenizer_config.json` (a string, or the `default` of a list). Bounded size (1 MiB). Other
  servers do not expose their template; they keep the curated list and trial.
- Learned list: `model-thinking-learned.json` under the settings data root, one entry per
  endpoint and model, with the template's SHA-256 and the date. Lookup order: learned (exact
  endpoint and model) → curated → family → trial. Analysis runs at the first generation of a
  model in a session; a changed template hash re-analyses, an unchanged one reuses the entry.
- The chat role's `thinkingSupport` says where its answer came from (template on this server,
  curated card, family guess); for a learned entry Settings offers "Copy as list entry" (the
  JSON for `model_thinking.json`).
- Tests: analysis of templates shaped like Qwen3.8 (switch + strict levels), the Spark variant
  (lean default, mapped values), Inkling (effort only, map), Gemma 4 (switch, default off),
  Kimi (`thinking` switch), a template without controls; learned store round trip and hash
  change; lookup order; oMLX template discovery with a fake transport.

## Implementation Notes
- `minijinja` and `minijinja-contrib` 2.24 are in the local cargo cache.
- The analysis renders only; no model is called. Keep rendering bounded (template size, fuel).
- Reading the template file is app-internal configuration of a local server, like settings, so
  it uses `std::fs` with a size bound rather than the VFS.

## Agent Notes
- 2026-11-25 claude: done.
  - `model_thinking/template.rs`: `analyse(template)` renders with minijinja (+ contrib pycompat,
    `raise_exception`, fixed `strftime_now`, fuel bound, 1 MiB cap) for plain, `enable_thinking` /
    `thinking` true/false, and the efforts none..lean plus an unknown one; transformers'
    `{% generation %}` tags are dropped first (Inkling's template uses them).
  - Learned values are limited to a fixed vocabulary (`SWITCH_NAMES`, `EFFORT_VALUES`); the
    curated list is checked against it in a test, and a learned file with anything else is not
    loaded (nor saved over), so a request never carries an arbitrary parameter.
  - `model_thinking/learned.rs`: `model-thinking-learned.json` (schema 1) per endpoint and model
    with SHA-256 and date, at most 200 entries.
  - `LlmProfileService::model_thinking`: oMLX on loopback → `GET /v1/models/status` →
    `model_path` → `chat_template.jinja` or `tokenizer_config.json`; once per session per model;
    unchanged hash reuses the entry. Lookup: learned → curated → family → trial
    (`thinking_attempts_with`). `roles_dto` gives `thinkingSupport.origin` (template / card /
    family), `learnedAt`, and `listEntry` (JSON for `model_thinking.json`); Settings shows the
    origin and a "Copy as list entry" button.
  - Live check against oMLX 0.8.3 (`live_omlx_templates`, ignored): every text model's template
    found and analysed. Dirk-Qwen3.8 has its own template (none, minimal, low, high, xhigh, max;
    no medium), so the Qwen3.8 family entry would have sent it `medium`; Inkling none..max, no
    switch; Gemma 4 switch, off by default; Ornith switch, no levels; ThinkingCap, Flash-Next and
    Underdog low/medium/xhigh.
  - Hugging Face lookup dropped: the served template can differ from the published one (the
    Spark's `lean` template), and other servers do not expose theirs.
  - Also fixed (found while the maintainer added an SGLang connection): the Settings dialog's
    Save also saved the connection editor, whose new draft had no model when discovery found
    nothing, so the whole dialog failed with "settings are not valid" (the mock accepted an empty
    model, hiding it). Now the dialog skips an unfinished new connection, "Save connection" says
    a model is missing, and the mock refuses an empty model like the backend.
  - Verified: 18 Rust tests (template shapes, generation tags, learned store, discovery on disk,
    end to end with a fake transport, vocabulary), 3 frontend; fm-application + fm-server and
    frontend suites pass; `pnpm lint` clean.

