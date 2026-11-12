# 0324 Chat tab in a pane

Status: done (browser pane not yet checked visually; search results reach the chat through the folder listing, not a separate attach)
Priority: medium
Subsystem: backend, frontend
Depends on: 0321, 0322

## Context
The file assistant (0321) is a chat tab that opens beside an editor and drafts one file. A general
chat should work the same way: a tab in a pane, reusing the same tab mechanism, with conversations
the person can return to. It answers questions and, in its first version, can read the workspace
but not change it.

## Acceptance Criteria
- A "Chat" command opens a chat tab in the active pane, or in the opposite pane when the active pane
  shows an editor, following the 0321 placement rule.
- Multi-turn conversation with the chat role (0322). Messages stream as in 0321. Stop cancels.
- Conversations are saved locally (not sent anywhere), listed in the tab's sidebar, and can be
  renamed and deleted. Storage is per workspace and inside the settings data root.
- Read-only context on request: the person can attach the current folder listing, the selected
  entries' names and metadata, or a search result. Contents of files are attached only when the
  person selects them, and the attach step shows exactly what will be sent.
- No tool that writes to disk in this task. Actions are proposed as text only.
- Cloud endpoints follow 0322's consent rules; the chat never prompts for consent.
- Browser and Tauri hosts behave the same.
- Tests: conversation storage, attach-preview contents, stop, placement rule, consent gate.

## Implementation Notes
- Reuse the assistant tab pattern (`shellState` tab-keyed entries, `addTransientTab`), and the
  streaming endpoint from 0321.
- Conversation store: a new backend module in `fm-application` with a versioned document, following
  the profile store pattern.
- Tool use (read-only first, then confirmed actions) is a follow-up task, not part of this one.

## Agent Notes
- 2026-11-12 claude: Created from design discussion. Not started.
- 2026-11-12 claude: Implemented. Backend: `chat_conversations.rs` (versioned `chat-conversations.json`
  in the settings root, grouped by workspace; a file that cannot be read makes the store read-only so
  a save never overwrites it), `chat.rs` (reply through the chat role via
  `LlmProfileService::generate_role_streamed`; saves the person's message first and the reply on
  completion; cancel keeps the person's message only), routes `/api/v1/workspaces/{id}/chat/conversations`
  and `/api/v1/chat/reply/{stream,cancel}`, Tauri commands. Frontend: `features/chat/` (pure rules and
  attachment text, tab component), `app/chat-wiring.ts`, placement via `chatPlacementPaneId`, command
  `client.chat`. Attach: folder listing and selection of the other pane, and the contents of one selected
  file (16 KB cap, 32 KB total per message). The preview shows exactly the text sent.
- Gaps: the browser pane was not checked visually. Search results reach the chat only through the
  folder listing. Knowledge-answer conversations (`SavedRagConversation`) are a separate store with
  retrieval scope and citations; consolidating them with this store is a later question. The whole
  conversations document is rewritten on each change; fine for now, revisit if histories grow large.
