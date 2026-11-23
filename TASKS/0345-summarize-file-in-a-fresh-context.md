# 0345 Agent tool: summarize_file in a fresh context

Status: done
Priority: high
Subsystem: backend
Depends on: 0344

## Context
Asked to summarise ~30 documents, the terminal assistant read each one with `read_file`. Every
result stayed in the conversation, so the prompt grew until a local model timed out. When only a
summary is needed, the document should be read and summarised in a model call of its own, outside
the conversation, with only the summary returned. That keeps the parent small and suits small
local models, which then see one document at a time.

## Acceptance Criteria
- fm-agent has a `summarize_file(path, focus?)` tool.
  - It reads the whole text through the host: text files, and documents the host converts.
  - It summarises the text in a fresh model call with no history and no tools.
  - It returns only the summary, with the size of what was read.
- A text longer than one call can take is split into parts at paragraph boundaries. Each part is
  summarised, then the part summaries are combined. Parts are capped, and the result says when
  only the start of a long document was used.
- The capability is optional, like the others in `ToolContext`: `FileTools::read_whole` and a new
  `SideModel` trait. A host without it gets a clear error for the model.
- The terminal assistant and the chat both provide it.
  - A part's size follows the chat model's context window.
  - The turn's cancellation is honoured.
  - A failure reaches the model as a tool error, so the turn goes on.
- `write_file`'s description points to `summarize_file` for summaries.

## Follow-ups (not in this task)
- A cheap check for duplicate documents (title and first lines, or text similarity), so copies in
  PDF, EPUB and HTML are summarised once.
- A cache of summaries keyed by content hash.
- Replace tool results the model has already used with a one-line note in the conversation.
- A general `delegate(task, files)` sub-session built on `SideModel`.

## Agent Notes
- 2026-11-23 claude: Implemented.
  - fm-agent `summarize.rs`:
    - the `SideModel` trait (`input_bytes`, `complete`) and `WholeText`;
    - a `summarize_file` tool, registered in `read_only_registry`, `Exclusive` so it runs one at a
      time;
    - `split_text`, which cuts at paragraph, then line, then space, then character boundaries.
  - `FileTools::read_whole` has a default that says it is not available, and
    `ToolContext.side_model` is new.
  - A part is at most `input_bytes`, and at most `MAX_SUMMARY_PARTS` (12) parts are read. A longer
    text gives a summary of its start, and the result says so. More than one part means one
    combining call.
  - fm-application:
    - `FolderTools::read_whole` pages `read_file` using the offsets in its cut-off notes
      (`sandbox::continuation`), so local, remote and converted documents all work.
    - `ModelSideCalls` uses the chat role with no history and no tools, 1024 reply tokens, and a
      part size from the window (about 2 bytes a token, between 8 and 96 KiB; 24 KiB when unknown).
    - It honours the chat's cancellation token, and the terminal's stop flag before each call.
    - A failure is logged and goes back to the model as a tool error, so the turn goes on.
  - `write_file`'s description points to `summarize_file`.
  - Tests:
    - fm-agent: 5 new (one call, parts and combine, cap, errors, splitting);
    - fm-application: 4 new, among them an end-to-end terminal test showing the document text never
      reaches the conversation, only the summary.
    - fm-agent and fm-application, 1652 tests in all, pass; the doctests pass; clippy and fmt are
      clean.
  - Not tried with a live model. A part is summarised after the previous one, with no progress shown
    until the step line, so a long document shows nothing for a while.
- 2026-11-23 claude: progress and timeouts. Timeouts apply to each model call, not to the whole
  document: the first reply may take max(profile timeout, 300 s), then each streamed chunk resets
  the idle wait. No turn-wide deadline exists. Caveat: the Ollama preset uses `/api/chat` with
  `stream: false`, so one part (prompt + up to 1024 tokens) must finish within that first-reply
  wait; on slow CPU-only hosts a 96 KiB part may not. Progress: `ToolContext::progress`/`report`
  (fm-agent) is set per call by `run_turn` (channel drained alongside the batch, leftovers flushed
  in order) and surfaces as `TurnEvent::Progress` → `TerminalAssistEventDto::Progress` /
  `ChatReplyEventDto::Progress`; the terminal shows it in the busy status, the chat in place of the
  waiting line; never saved. `summarize_file` reports `Reading X`, `Summarising X: part i of n`,
  `combining n parts`. Also fixed: Ollama-preset requests without tools were always sent
  `format: "json"`, forcing prose (chat, terminal, summaries) into JSON; `LlmChatGeneration.reply:
  ReplyFormat` now says which callers want JSON (document_summary, rag, knowledge_answer). Parts
  and calls are still summarised one at a time (`ToolEffect::Exclusive`); parallel parts are a
  possible follow-up for remote endpoints.
