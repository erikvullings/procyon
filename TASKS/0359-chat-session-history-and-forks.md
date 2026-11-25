# 0359 Chat conversations: tool history, compaction and forks

Status: open
Priority: medium
Subsystem: backend, agent, frontend
Depends on: 0353, 0354

## Context

Split from 0354 and 0353 (2026-11-25). Terminal sessions are saved with their full agent
history, compacted at 80 % of their budget and resumable. Chat conversations (task 0324) are
saved as text plus step lines only: a reopened chat gives the model less than it had, and chat
still drops old exchanges (`fit_messages`) instead of summarising them.

## Acceptance Criteria
- `conversation.json` schema 2 stores the agent messages (`fm_agent::Message`) of each reply
  next to the visible text; schema 1 documents load as today (migration).
- Chat compacts with `fm_agent::compact` at 80 % and stores the summary and what it covers in
  the document, so a reopened conversation does not summarise again.
- "Fork from here" on a chat message, and `/fork N` in the terminal: a new conversation/session
  with the history up to that question; the original is unchanged.
- Saved terminal sessions can be deleted (`/resume delete N`).
- Tests: migration, compaction persisted, fork shares the prefix, delete.

## Agent Notes
