# 0354 Saved, resumable and forkable agent sessions

Status: done
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
- 2026-11-25 claude: done for the terminal assistant; chat moved to 0359.
  - `fm_agent::Message`/`ToolCallRecord` are serde types now (host-neutral session format).
  - `terminal_assist/saved.rs`: `SavedSessions` under `<settings data>/terminal-sessions/`, one
    JSON document per session (schema 1: id, title = first question, folder, updated, question
    count, the stored turns incl. tool calls and results, already redacted and bounded), written
    via temp file + rename after every question, at most 50 kept (oldest removed), file names
    sanitised from the tab's session id.
  - `Sessions::persistent` (the service uses it), `saved()`, `resume(from, into)`: copies a saved
    conversation into the tab's session, so the original stays and the copy is a fork from its
    end. Service `terminal_assist_saved_sessions` / `terminal_assist_resume`, Tauri commands of the
    same name, `TerminalSavedSessionDto`.
  - Terminal: `/resume` lists up to 20 (title, questions, date, folder), `/resume N` continues one.
  - Decision: terminal sessions and chat conversations keep separate stores; they have different
    shapes (chat is per workspace, text plus steps) and 0359 extends chat's own document.
  - Not done (0359): fork from an earlier question, deleting a saved session from the UI, chat tool
    history and chat compaction.
  - Verified: 2 store tests (restart + resume-as-copy, pruning and unsafe ids); drawer `/resume`
    test; terminal_assist and message tests 115 pass; clippy clean; frontend 464 pass.
