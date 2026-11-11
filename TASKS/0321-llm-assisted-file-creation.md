# 0321 LLM-assisted file creation (draft a new file from a prompt)

Status: planned
Priority: low
Subsystem: backend + frontend
Depends on: 0184, 0186, 0126, 0086, 0099, 0312, 0045

## Context
Templates (0312) give boilerplate, but often the user wants a specific file: "a bash script that
backs up ~/notes to a dated tarball". The configured OpenAI-compatible LLM profile (0184, the one
RAG uses) can draft it. The draft must never reach disk without the user reviewing it.

## Acceptance Criteria
- Entry point: "New file with assistant…" in the New-from-template dialog and the palette. It opens
  an "Assistant" tab in the F12 terminal drawer (0126) for the current folder.
- The user types a prompt and an optional file name. The model streams a draft into a new,
  unsaved buffer in the F4 editor (0086/0099). The chat tab shows progress and the profile in use.
- Nothing is written to disk by the assistant. Saving uses the ordinary create-file operation
  (never overwrites; conflict flow 0045). Discarding the buffer leaves no file.
- No file contents from disk are sent. Only the prompt and the chosen folder name go to the model.
- The header shows the profile name and its endpoint locality (`LlmEndpointLocalityDto`). The first
  use of a remote profile asks the user to confirm that prompts leave the machine.
- No profile configured: the action stays visible but disabled, with a hint that links to LLM
  settings. Nothing is called.
- Stop cancels the stream. Output is capped (64 KiB, the template body limit) and the request has a
  timeout. If the whole reply is one fenced code block, the fence lines are removed.
- Cross-host parity: browser and Tauri hosts behave the same. Provider calls happen only in the Rust
  backend; the frontend never talks to the LLM endpoint.

## Out of scope (v1)
- Multi-turn refinement, editing an existing file, attaching files or folders as context, tool use.
  Attaching the current file is a natural v2 with its own privacy prompt.

## Implementation Notes
- Backend: a new `fm-application` capability service (e.g. `file_assist`) that resolves the active
  profile and reuses the streaming chat path in `llm_profiles.rs` (`stream_chat`, which already takes
  a `CancellationToken`). Keep `FileManagerService` a thin delegate (CLAUDE.md).
- Transport: a streaming endpoint (SSE in `fm-server`, a channel in Tauri), like the other long-running
  jobs. DTOs in `fm-transport-dto`; regenerate OpenAPI and the Orval client.
- Frontend: the drawer gets an Assistant tab; the editor opens the draft as an unsaved buffer.
  The state lives in the Meiosis tree, not in the component.

## Open questions
1. Chat in the drawer (proposed) or a docked side panel next to the editor?
2. Confirm-on-first-use for remote profiles, or a per-profile "allow prompts to leave this machine"
   setting?

## Agent Notes
- 2026-11-11 claude: Drafted from the discussion; not started.
