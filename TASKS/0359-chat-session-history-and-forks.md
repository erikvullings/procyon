# 0359 Chat conversations: tool history, compaction and forks

Status: done
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
- 2026-11-25 claude: done.
  - Chat schema 2 (`chat_conversations.rs`): `ChatMessage.exchanges` (the reply's calls and
    redacted, bounded results as `fm_agent::Message`, at most 24 KiB, final answer excluded) and
    `ChatConversation.compaction` (`summary`, `covered` = leading saved messages it stands for,
    always ending before a person's message). Schema 1 migrates by bumping the version only;
    `validate` refuses a summary covering more than the conversation. `set_compaction` is skipped
    when the conversation changed meanwhile.
  - `chat.rs`: `history_messages` (summary + saved messages + their exchanges, with an origin
    index per message) decides and makes the summary with `fm_agent::compact` at 80 % of
    `MAX_PROMPT_BYTES`, before the model is asked, with a progress line and a step; the stored
    summary is reused, so a reopened conversation is not summarised again. Earlier calls are sent
    as "(looked up with name args)", never as `TOOL` lines the model could copy. Failure: a step,
    then the oldest messages drop out as before.
  - Fork: `fork(workspace, id, keep)` copies the first `keep` messages (and the summary if it ends
    within them) into a new conversation; HTTP `POST .../chat/conversations/{id}/fork`, Tauri
    `fork_chat_conversation`, mock, `forkChatConversation` in all three clients. Chat tab: a fork
    button per message; from a question it forks before it and puts the question in the draft,
    from a reply it keeps that reply.
  - Terminal: `/fork` lists the session's questions, `/fork N` starts a new session with the turns
    before question N (from memory, else the saved copy) and puts the question in the input;
    `/resume delete N`. Tauri `terminal_assist_questions` / `terminal_assist_fork` /
    `terminal_assist_delete_saved`.
  - Bug fixed from 0354: `/new`, `/clear` and `/resume` kept the tab's session id, so the next
    save overwrote the earlier saved session (and `/resume` overwrote the tab's own). A tab now
    gets a new id for each new conversation.
  - Also fixed: the 0352 diff style used a raw `0.8125rem`/`4px` (theme test failed on `main`);
    now `--fm-type-label` / `--fm-radius`.
  - Limits: chat's summary budget is the fixed `MAX_PROMPT_BYTES`, not scaled to the model's
    window as the terminal's is. Forking a chat conversation does not copy its images (they are
    never saved). Not tried in the running app.
  - Verified: fm-application/server/agent/DTO 2104 tests pass (new: 2 chat, 3 conversation store,
    2 terminal store); frontend 4212 pass (new: chat fork button, 3 terminal drawer); `pnpm lint`
    clean.

