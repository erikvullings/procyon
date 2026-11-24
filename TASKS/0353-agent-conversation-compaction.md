# 0353 Agent conversation compaction by summary

Status: open
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
