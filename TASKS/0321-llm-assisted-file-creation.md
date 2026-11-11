# 0321 LLM-assisted file creation (chat that drafts a file)

Status: in_progress (v1 implemented; see Agent Notes for what is missing)
Priority: low
Subsystem: backend + frontend
Depends on: 0184, 0186, 0126, 0086, 0099, 0312, 0045

## Context
Templates (0312) give boilerplate, but often the user wants a specific file: "a bash script that
backs up ~/notes to a dated tarball". The configured OpenAI-compatible LLM profile (0184, the one
RAG uses) can draft it. The draft must never reach disk without the user reviewing it.

## Acceptance Criteria
- From the editor of a file, an "Assistant" button opens a chat in a new tab of the opposite pane.
  If the editor is in the left pane, the chat opens in the right pane, and vice versa.
- The person writes messages; the reply is shown in the chat. "Use in editor" places the reply
  into the editor of the source pane. Nothing is written to disk by the assistant; saving uses the
  editor's normal save (conflict flow 0045).
- Only the file's name and the chat go to the model. The file's contents are never sent.
- Profile: the chat has a profile picker, defaulting to the first saved profile (as RAG does).
  No profile: the chat says so and sends nothing.
- Stop cancels the request. A reply that arrives after Stop is discarded.
- The reply is capped at 64 KiB and a single fenced code block is unwrapped.
- Cross-host parity: browser and Tauri hosts use the same backend call. Provider calls happen only
  in the Rust backend; the frontend never talks to the LLM endpoint.

## Out of scope (v1)
- Streaming (the reply arrives whole), multi-turn refinement of an existing file's contents,
  attaching the current file as context, tool use, and an entry point in the New-from-template
  picker (the assistant is reached from an open editor only).

## Implementation Notes
- Backend: `fm-application/src/file_assistant.rs` (`FileAssistantService`): validates the request,
  builds the prompts, calls `LlmProfileService::generate` (which enforces endpoint policy and cloud
  consent), unwraps a single fence, and keeps a cancellation registry keyed by request id.
- Transport: `POST /api/v1/file-assistant/reply` and `.../reply/cancel` in `fm-server`; Tauri
  commands `draft_file_assistant_reply` and `cancel_file_assistant_reply`. DTOs in
  `fm-transport-dto/src/file_assistant.rs`. OpenAPI and the Orval client regenerated.
- Frontend: `SemanticClient` methods on all adapters; the chat is a tab keyed like the Themes tab
  (`shellState.assistantByTab`), opened by `app/assistant-wiring.ts` through the workspace's
  `addTransientTab` command in the opposite pane. The editor header gets the button.

## Open questions
1. Remote profiles: the backend refuses cloud endpoints without prior consent, but the chat only
   shows a generic failure. A first-use prompt that names the host is still missing.
2. Streaming replies and Stop mid-stream (v1 is whole-reply, so Stop discards the reply).

## Agent Notes
- 2026-11-11 claude: Drafted from the discussion; not started.
- 2026-11-11 claude: v1 implemented as the spec's AC describe, with these deviations: whole reply
  instead of streaming; the chat is a tab in the other pane (not the F12 drawer), as requested;
  no picker entry point yet; consent failures show the generic "could not reply" message.
