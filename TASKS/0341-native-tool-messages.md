# 0341 Native tool messages in the agent harness

Status: done
Priority: medium
Subsystem: backend
Depends on: 0340

## Context
Since 0339, an endpoint that calls tools natively gets the tool list, but its calls come back to
the loop as `TOOL {json}` text and every result goes back as text inside one user message. Models
are trained on structured tool messages (an assistant message with `tool_calls`, then one `tool`
message per call id), call tools more reliably with them, and an endpoint can cache the unchanged
start of a structured conversation. 0340 recorded this as a decision to revisit; the user asked for
it.

## Acceptance Criteria
- `fm-agent` has a provider-neutral conversation model: `Message` (`User`, `Assistant { text,
  calls }`, `ToolResult { call_id, name, text }`, `Note`) and `ToolCallRecord { id, name,
  arguments }`.
- The loop records each reply with the calls it runs (ids from the endpoint, else generated), gives
  every recorded call exactly one result (skipped calls get the skipped message), sends loop
  messages (nudge) as notes, and reports an interrupt together with its call.
- A group-aware fit (`fit_messages`) bounds a conversation: the last user message stays, an
  assistant message is never kept without its results (or the reverse), a call without a result
  gets a "not run" result, and dropped groups are replaced by one note.
- Both hosts (terminal assistant, chat) keep their conversations as `Message`s. Natively they send
  them as structured messages; for the text protocol they render them as before.
- OpenAI-compatible (incl. Azure) bodies send `assistant.tool_calls` and `tool` messages with
  `tool_call_id`; Ollama sends `assistant.tool_calls` and `tool` messages with `tool_name`. The
  endpoint's call id is kept.
- An endpoint that turns out not to take native tools gets the follow-up messages folded into the
  user prompt as text.
- A terminal command that waits for approval keeps its call id; its result, a decline, or a new
  question answers that call.
- Tests for each of the above; `pnpm run lint` and the Rust suite pass.

## Implementation Notes
- `crates/fm-agent/src/message.rs` (new), `turn.rs`, `registry.rs` (`ParsedCall::id`),
  `protocol.rs` (`id` in the `name`/`arguments` form, `tool_call_line` with an optional id).
- `crates/fm-application/src/llm_profiles.rs`: `LlmChatGeneration::followups`, request bodies,
  stream/Ollama parsing of call ids.
- `crates/fm-application/src/terminal_assist/{mod,session}.rs`, `chat_agent.rs`,
  `agent_host/mod.rs` (`generation_parts`).

## Agent Notes
- 2026-11-23 Claude: Done.
  - `fm-agent/src/message.rs`: `Message`, `ToolCallRecord`, `fit_messages` (groups a reply with
    its results; pins the last user message and the newest group; fills a missing result with
    `NOT_RUN_RESULT`; turns an orphan result into a note; one `OMITTED_NOTE` for dropped groups),
    `render_text`, `Message::prose` (reply without `TOOL` lines, the native `content`).
  - `TurnHost` is now `push_assistant(text, calls)`, `push_result(call, text)`, `push_note(text)`;
    `TurnEnd::Interrupted { call, interrupt }`. Every recorded call gets one result: skipped calls
    after an interrupt (and a second interrupt in the same batch) get `skipped_calls_message`,
    calls left when cancelled get `NOT_RUN_RESULT`. Ids come from the endpoint
    (`ParsedCall::id`, read from `{"id","name","arguments"}`) or are `call_{step}_{index}`.
  - `tool_call_line(id, name, args)` now takes an optional id; the OpenAI stream parser keeps the
    `id` of each call (capped at 128 chars), Ollama's `id` when present.
  - `LlmChatGeneration::followups`: OpenAI-compatible/Azure send `assistant.tool_calls` (content
    `null` when there is no prose) and `tool` messages with `tool_call_id`; Ollama sends
    `tool_calls` with object arguments and `tool` messages with `tool_name`. Notes are user
    messages. A profile that does not take native tools gets the follow-ups folded into the user
    prompt with `render_text` and no `tools`.
  - Hosts: `agent_host::generation_parts` (text: render all; native: first user message is the
    prompt). The chat keeps `exchanges: Vec<Message>`; the terminal session stores
    `Turn::Agent(Message)`, holds the interrupting call (`hold_call`), answers it with the
    command output / decline / note review / write outcome (`push_tool`), or with
    "Not run: the person asked something else instead." when a new question comes first.
  - Verified: `fm-agent` 85 tests (new: message fit/render/prose, call ids, interrupt pairing);
    `fm-application` 1537 tests (new: OpenAI and Ollama bodies, null content, non-native folding,
    held command call, new question answers the call, native chat follow-ups); clippy clean.
  - Not verified against a live endpoint. Mistral-style endpoints that demand 9-character ids
    only see ids they issued themselves, since native calls always carry the endpoint's id.
