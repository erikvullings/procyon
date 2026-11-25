# 0360 Agent harness robustness across models

Status: open
Priority: medium
Subsystem: backend, agent
Depends on: none

## Context

Maintainer's question (2026-11-25): benchmarks increasingly show that a model's score depends on
the harness (some frontier models score better in OpenCode than in Pi). Why, and how can
`fm-agent` become more robust or adaptive?

Main reasons:
- **Post-training on specific tool shapes.** Models are trained on their vendor's own agent's tool
  names, argument names and edit format (`old_string`/`new_string`, apply-patch, `path` vs
  `file_path`, `cmd` vs `command`). A harness whose tools look like the training data gets fewer
  malformed calls.
- **Tolerance.** Strict edit matching, strict JSON and strict schemas turn small slips
  (whitespace, trailing commas, a wrong argument name) into failed rounds. Each failure costs a
  round and pushes the model into drift.
- **Error results.** "invalid arguments" vs "you passed `file_path`; this tool takes `path`, the
  file exists, try again with …". Models recover far better from errors that say what to do next.
- **Context hygiene and loop control.** Large tool outputs, repeated identical calls and long
  windows degrade weaker models first.
- **Per-model prompts.** OpenCode (as far as known) ships prompt and tool variants per model
  family; Pi uses one minimal set for all. That helps the models a harness is tuned for and hurts
  the others.

`fm-agent` already tolerates some of this: the text protocol repairs bare keys and garbled or
trailing calls (`protocol.rs`), `null` arguments count as `{}` (`tool.rs`), and invalid arguments
return an `arguments_hint`. What is missing is argument aliasing, looser edit matching, recovery
guidance in errors, loop detection and a way to measure any of it.

## Acceptance Criteria
- **Argument aliases.** `TypedTool` may declare aliases per argument (e.g. `path` ←
  `file_path`/`filePath`/`filename`; `command` ← `cmd`; `old_text` ← `old_string`/`oldString`).
  Aliases are normalised before deserialising; the schema the model sees is unchanged. Camel and
  snake case of a declared name always match. An argument that is both given and aliased is an
  error, not a silent pick.
- **JSON repair for native calls.** Native tool-call arguments that fail to parse go through the
  same repairs as the text protocol (trailing commas, bare keys, single quotes, a string holding
  JSON) before being rejected.
- **Edit matching tiers** (`edit.rs`): exact first; then ignoring trailing whitespace per line;
  then ignoring indentation differences (re-indenting the replacement by the found offset); each
  looser tier applies only if it finds exactly one match, and the result says which tier matched.
  No fuzzy (edit-distance) matching.
- **Errors that guide.** Invalid arguments name the unknown and missing fields and the closest
  declared name; an unknown tool name lists the closest tool; a failed edit gives the closest
  lines (exists) plus how many matches a looser tier would find.
- **Loop control.** The turn loop notices the same call (name + normalised arguments) repeated
  three times with the same result and tells the model so in the result, once; a fourth identical
  call ends the round with a note to the host.
- **Model profiles.** A small `ModelProfile` (prompt dialect notes, preferred calling mode,
  output-size limits, whether parallel calls are reliable) selected by model id pattern, with a
  neutral default. Hosts pass it in `TurnOptions`; no profile changes tool names or semantics, so
  both hosts and all providers keep one tool set. Start with the default and at most two profiles
  backed by measurements below; no speculative per-vendor tables.
- **Evaluation suite.** A scripted, offline suite in `fm-agent` tests (`ScriptedModel` replaying
  recorded or hand-written replies): malformed calls, aliased arguments, whitespace-off edits,
  repeated calls, oversize outputs. Plus an opt-in, not-in-CI runner against a configured real
  endpoint over a fixed task set in a temp folder, reporting per model: success, rounds, failed
  calls by cause. Results recorded in this task before adding any profile.
- Browser and Tauri hosts unaffected beyond the shared `fm-application` changes; no API change
  unless a profile setting is exposed (then regenerate with `pnpm api:export` / `api:generate`).

## Implementation Notes
- Argument decoding: `tool.rs` `Typed::run`; hint: `registry::arguments_hint`. Aliases belong to
  the tool (an associated fn or attribute), not a global table.
- Text repairs: `protocol.rs` (`quote_bare_keys`, `balanced_object`, `garbled_call`); reuse them
  for native arguments instead of a second repair path.
- Edit: `edit.rs` `apply_edit`, `EditError`; the host's diff approval (0352) must show the actual
  replaced text, whatever tier matched.
- Loop: `turn.rs` main loop; keep it host-neutral, the host only sees a `TurnEvent`.
- Measure before tuning: the eval runner is the gate for profiles and for any change to default
  prompts.

## Agent Notes
- 2026-11-25 claude: planned from the harness-correlation discussion. Out of scope: per-model tool
  renaming, fuzzy edit matching, vendor-specific edit formats (apply-patch) unless the eval shows
  a clear gain.
