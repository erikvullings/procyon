# 0340 fm-agent review and hardening

Status: in_progress
Priority: medium
Subsystem: backend
Depends on: 0339

## Context
0339 built the shared agent harness (`crates/fm-agent`) and moved the terminal assistant and the
chat onto it. The user asked for a thorough review and improvement: `fm-agent` is the base of
Procyon's LLM harness and should be good, fast, and reusable in other applications.

A two-axis review (coding standards and spec, `git diff 91c4cf86...HEAD`) found the following.

**Standards (hard)**
- `fm-agent` used wire DTOs as domain types: `ChatFailureDto` in `TurnHost`, `NoteProposalDto`
  in `ToolOutcome`/`TurnEnd`/`NotesAccess` (spec §3 rule 5). `architecture.rs` even described it
  as "knows only the wire contracts". This also blocked reuse outside Procyon.
- The chat imported the terminal assistant's internals
  (`terminal_assist::{FolderTools, SkillFolderDocuments, skills_for}`).

**Correctness**
- Every `ask` and chat answer was cut to 8 KB (`MAX_REPLY_BYTES` in the loop), although the chat
  promises not to cut long answers.
- `parse` ran on the raw reply but `parse_all` on the 8 KB cut. A call past 8 KB dropped the held
  lines and ended the turn with half a `TOOL {` line as the answer.
- Any JSON object anywhere in an answer that named a registered tool was run, so an answer that
  showed an example call executed it.
- A native call's argument named `tool` was overwritten, because native calls were flattened into
  `{"tool": name, ...args}`.
- `read_skill_file` passed documents to the host converter before the skill sandbox ran, and
  always at offset 0, so long documents could not be continued.
- The chat's transcript was cut from the front past 192 KB, which could drop the person's question.

**Performance**
- Each reply was parsed twice, and `parse` tried a JSON parse from every `{` in the reply (O(n²)
  on code-heavy answers).
- `LineStreamer` rescanned the whole buffered line on every streamed delta.
- Skill discovery re-read up to 16 KB of every `SKILL.md` on every request (851 skills on the
  author's machine).
- Name lookup in the catalogue was linear, and adding skills was O(n²).
- Tool schemas were rebuilt for every prompt.
- Batched read-only calls ran one at a time, although the spec allows concurrency.

**Reuse and design**
- Limits, the English "keep going" nudge and the messages were constants.
- The approval kinds (`RunCommand`, `WriteFile`) were hard-coded in the crate.
- Schemas were hand-written `json!` objects that could drift from the argument structs.
- `NoteToolOutcome` duplicated `ToolOutcome`.
- Prompt assembly, error mapping and the `ToolSpec`/`LlmToolSpec` conversion were each written
  twice, once per agent.
- `balanced_object` was public.

**Spec gaps**
- No native-path test with a fake tool.
- No deny-path test on the server route for `toolsFolder`.
- Thin chat tests.

## Acceptance Criteria
- `fm-agent` has no workspace dependencies; the architecture test places it in layer 0.
- Hosts define their own interrupt type `I` and error type; the built-in tools ask only for
  `I: From<CommandRequest>` / `I: From<NoteProposal>`.
- Tool schemas are derived from the argument types (`schemars`), with descriptions for every
  argument.
- The loop:
  - parses each reply once
  - runs consecutive read-only calls concurrently (bounded) and reports results in call order
  - takes its limits from `TurnOptions`
  - redacts results through a hook
  - never cuts an answer unless the host asks
- Loose call forms count only when they stand on their own line or end the reply; an example in a
  sentence is not run.
- Native calls are written back as `{"name":..,"arguments":..}`, so arguments may be called
  `tool` or `name`.
- `read_skill_file` checks the sandbox before any document conversion and supports `offset`.
- Skill headers are cached (`SkillIndex`) and read again only when the file changes.
- The chat keeps the person's question when old tool results drop out.
- The server refuses a `toolsFolder` outside the accessible roots (tested).
- `pnpm run lint` and the Rust suite pass.

## Implementation Notes
- `crates/fm-agent/src/`:
  - `tool.rs`: `ToolOutput`, `ToolOutcome<I>`, `ToolEffect`, the `AgentTool<I>`/`TypedTool<I>`
    traits, `ToolContext`, `schema_for`
  - `protocol.rs`: call parsing and `tool_call_line`
  - `registry.rs`: `ToolRegistry<I>`, `ToolCalling`, `compose_system_prompt`
  - `turn.rs`: `run_turn`, `TurnOptions`, `ContinueNudge`
  - `skills.rs`: `SkillIndex`, `Skill::resolve_file`/`read_checked`
- `crates/fm-application/src/agent_host/`: the host side shared by both agents (folder tools,
  sandbox, remote, documents, skills for a request, DTO mapping). It was moved out of
  `terminal_assist`.

## Agent Notes
