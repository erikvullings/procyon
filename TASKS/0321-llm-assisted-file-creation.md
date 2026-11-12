# 0321 LLM-assisted file creation (chat that drafts a file)

Status: done (streaming and picker entry point added; see Agent Notes for what is not verified)
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
- The person writes messages; the reply streams into the chat as it arrives, and the final text
  replaces the fragments. "Use in editor" places the reply
  into the editor of the source pane. Nothing is written to disk by the assistant; saving uses the
  editor's normal save (conflict flow 0045).
- Only the file's name and the chat go to the model. The file's contents are never sent.
- From the New-from-template picker, "Draft with assistant" creates an empty file under the chosen
  name, opens it in the editor and opens the chat for it.
- Profile: the chat has a profile picker, defaulting to the first saved profile (as RAG does).
  No profile: the chat says so and sends nothing.
- Stop cancels the request. A reply that arrives after Stop is discarded.
- The reply is capped at 64 KiB and a single fenced code block is unwrapped.
- Cross-host parity: browser and Tauri hosts use the same backend call. Provider calls happen only
  in the Rust backend; the frontend never talks to the LLM endpoint.

## Out of scope
- Multi-turn refinement of an existing file's contents, attaching the current file as context,
  and tool use.

## Implementation Notes
- Backend: `fm-application/src/file_assistant.rs` (`FileAssistantService`): validates the request,
  builds the prompts, calls `LlmProfileService::generate_streamed` (which enforces endpoint policy
  and cloud consent), hides reasoning from the fragments, unwraps a single fence, and keeps a
  cancellation registry keyed by request id.
- Streaming: `LlmProbeTransport::generate_chat_streamed` reads the provider's SSE body line by line
  and reports `choices[].delta.content` fragments; the whole body is still parsed strictly at the
  end. A timeout is retried only before any fragment was sent; a rejected reasoning switch
  sends nothing.
- Transport: `POST /api/v1/file-assistant/reply/stream` (newline-delimited JSON events `delta`,
  then one `done` or `failed`) and `.../reply/cancel` in `fm-server`; Tauri command
  `stream_file_assistant_reply` with a `Channel`, plus `cancel_file_assistant_reply`. DTOs in
  `fm-transport-dto/src/file_assistant.rs`. OpenAPI and the Orval client regenerated. The browser
  reads the stream with `postNdjson` in `frontend/src/api/fetch-mutator.ts`.
- Frontend: `SemanticClient` methods on all adapters; the chat is a tab keyed like the Themes tab
  (`shellState.assistantByTab`), opened by `app/assistant-wiring.ts` through the workspace's
  `addTransientTab` command in the opposite pane. The editor header gets the button.

## Open questions
1. Remote profiles: the backend refuses cloud endpoints without prior consent, but the chat only
   shows a generic failure. A first-use prompt that names the host is still missing.

## Agent Notes
- 2026-11-11 claude: Drafted from the discussion; not started.
- 2026-11-11 claude: v1 implemented as the spec's AC describe, with these deviations: whole reply
  instead of streaming; the chat is a tab in the other pane (not the F12 drawer), as requested;
  no picker entry point yet; consent failures show the generic "could not reply" message.
- 2026-11-12 claude: Streaming and the picker entry point added. Stop cancels the request on the
  server and drops any events that arrive after it. Picker's "Draft with assistant" creates the file
  empty (the draft reaches disk only when accepted and saved). Tests: Rust (streamed fragments,
  reasoning hidden from fragments, SSE line splitting, callback path), frontend (NDJSON parsing,
  wiring stream/stop/failure, picker draft). Not checked in the browser pane or a real host.
