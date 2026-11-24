# 0354 Saved, resumable and forkable agent sessions

Status: open
Priority: medium
Subsystem: backend, agent, frontend
Depends on: 0353

## Context

Part of 0351. Terminal assistant sessions live in memory (`terminal_assist/session.rs`, 32
sessions, 40 turns) and end with the tab or the app. Chat conversations are saved (task 0324), but
only as person/assistant text plus step lines: the tool calls and results the model saw are
lost, so a resumed chat starts with less context than it had. Pi saves every session as a tree
that can be resumed and forked from any earlier message.

## Acceptance Criteria
- A host-neutral session record in `fm-agent`: the `Message` sequence (with tool calls and
  results), the compaction summary from 0353, and a parent pointer per entry so a fork shares its
  prefix. Serialisable with serde, versioned.
- Storage stays in `fm-application`, under the settings data root, one file per session,
  bounded (entries, bytes per entry, sessions per workspace, oldest pruned), written atomically.
  Secrets are redacted before writing, as for prompts.
- Terminal assistant:
  - sessions survive a restart; a closed tab's session can be resumed from a list (title, folder,
    time), and is resumed in a terminal at the same working folder;
  - "fork from here" on an earlier question starts a new session with the history up to it.
- Chat: saved conversations keep tool history (schema migration of `conversation.json`; old
  documents load as today) and gain "fork from here".
- Browser and Tauri hosts behave the same; the API is regenerated with `pnpm api:export` /
  `api:generate`.
- Tests: save/load round trip, migration of a v1 chat conversation, fork shares the prefix and
  diverges, pruning bounds, redaction on write.

## Implementation Notes
- Reuse `fm_settings::VersionedDocument` as `chat_conversations.rs` does.
- Decide whether terminal sessions and chat conversations share one store; record it here.
- Privacy: sessions may hold file contents the assistant read. Store them like chat
  conversations (local only), and offer delete.

## Agent Notes
