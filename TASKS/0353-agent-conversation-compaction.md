# 0353 Agent conversation compaction by summary

Status: done
Priority: medium
Subsystem: backend, agent
Depends on: none

## Context

Part of 0351. `fm_agent::fit_messages` bounds a conversation by dropping whole older exchanges and
inserting `OMITTED_NOTE`. In a long terminal or chat session the model then forgets decisions,
file names and findings from earlier questions. Pi summarises older turns instead (automatically
near the window limit, and on `/compact`).

`summarize_file` (0345, 0350) already has the pieces: `SideModel` for a fresh call, splitting by
bytes, and combining summaries in rounds.

## Acceptance Criteria
- `fm-agent` gets a compaction step: given the messages, a byte budget and a `SideModel`, it
  summarises the oldest exchanges that do not fit into one "Earlier in this conversation" message
  and keeps the recent ones verbatim.
  - The summary keeps what later turns rely on: the person's goals and decisions, files and
    paths touched, commands run and their outcome, open questions. A fixed prompt in `fm-agent`.
  - Incremental: an existing summary is extended with the newly dropped exchanges, not rebuilt
    from scratch each turn. The host stores the summary and the index it covers.
  - Tool-call groups stay whole (as `fit_messages` keeps them); secrets stay redacted.
  - If the side call fails or is cancelled, fall back to `fit_messages` and say so in a step line.
- Compaction runs before a turn when the conversation exceeds the budget, and on demand
  (`/compact` in the terminal chat input; a menu action in chat).
- The person sees a step line ("Summarised 12 earlier exchanges").
- Terminal assistant (`terminal_assist/session.rs`) and chat (`chat_agent.rs`) both use it.
- Tests: a fake `SideModel`; budget not exceeded (no call), first compaction, incremental
  extension, failure fallback, tool groups never split.

## Implementation Notes
- `fit_messages` is in `crates/fm-agent/src/message.rs`; callers are
  `chat_agent.rs:171` and `terminal_assist/session.rs:375`.
- Budget helpers: `terminal_assist::session::conversation_budget`,
  `agent_host::side_input_bytes`.
- Compaction costs a model call; trigger it at about 80 % of the budget so it is not paid every
  turn, and never more than once per turn.

## Agent Notes
- 2026-11-25 claude: done for the terminal assistant.
  - `fm-agent/src/compact.rs`: `needs_compaction` (over `COMPACT_AT_PERCENT` = 80 % of the
    budget, as Pi does: long before the window is full, where models slow down and drift),
    `compaction_cut` (earliest person message after which the rest fits
    `KEEP_AFTER_COMPACTING_PERCENT` = 40 %, so a call never loses its question), `compact` (one
    `SideModel` call with a conversation-specific prompt; a long history goes through the same
    split-and-combine-in-rounds path as `summarize_file`, now `summarize(SummaryTask, ..)`), and
    `Compaction::message` (a note "Summary of the conversation so far:"). Incremental: an earlier
    summary is the first message, so it is folded into the next one.
  - Terminal: `compact_if_needed` runs once per request before the model is asked, shows
    progress and a step ("Summarised N earlier messages"); `Sessions::messages` /
    `replace_prefix` (skipped if the conversation changed meanwhile). On failure a step says so
    and `fit_messages` leaves the oldest part out, as before. `/compact` (summarise everything)
    stays.
  - Not done: chat (`chat_agent.rs`) still only drops old exchanges with `fit_messages`; its
    conversations are rebuilt from the saved document per request, so a summary needs to be
    stored there first — left for 0354, which changes that document.
  - Verified: fm-agent 111 tests (5 compaction); terminal_assist 88 (new
    `a_long_session_is_summarised_before_the_model_is_asked`); clippy `-D warnings` clean.
