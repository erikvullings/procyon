# 0339 Shared agent harness: tool registry, skills, notes as skills

Status: in progress (steps 3 and 4 done; 1, 2 and 5 partly or not started)
Priority: medium
Subsystem: backend, frontend, settings
Depends on: 0324, 0336, 0337, 0338

## Context
Procyon has two LLM agents on one backend LLM role (0322), and they share nothing else:
- **Terminal assistant** (0336, 0338, `crates/fm-application/src/terminal_assist/`): has a
  bounded tool loop with a text protocol (`TOOL {json}` on its own line, one call per step).
  It has 10 tools: `list_dir`, `read_file`, `grep`, `run_command` (approved), `write_file`
  (create-only, approved), and the 0337 notes tools `search_notes`, `read_note`,
  `propose_note` and `propose_note_update`.
- **Chat tab** (0324, `chat.rs`): text only, by design ("Nothing here can write to disk"). It
  has no tools, no notes and no files beyond the context the person attaches.

People expect the same abilities in both, because they talk to the same model.

Adding a tool today means editing five places: the `ToolCall` enum and its parsing and checks
(`protocol.rs`), the hand-written tool list in `ASK_PROMPT` (`mod.rs`), the dispatch in the ask
loop, the `ToolError` arms (`tools.rs`/`remote.rs`), and sometimes a new trait such as
`NotesAccess`. Harnesses like pi instead offer a `createTool`-style registry: one definition per
tool (name, description, input schema, run). The prompt or the native tool list is generated
from it.

There are two more gaps:
- Agents cannot read Agent Skills (`SKILL.md`). The person has hundreds of them under
  `~/.claude/skills`, `~/.agents/skills` and project `.claude/skills`/`.agents/skills`. The
  viewer already renders their front matter (commits `3dd0e4b3`, `c4c93c60`).
- The 0337 notes (runbooks) are a natural fit for skills. The terminal assistant already finds
  them through `search_notes`, but the chat cannot, and the agent cannot pick one up from a
  short catalogue the way it would a skill.

Decision (user, 2026-11-22): we own the harness, so **notes are treated as virtual skills inside
Procyon**. They stay plain Markdown notes in the notes folder (Obsidian-compatible, flat). They
are not converted into `<name>/SKILL.md` directories, and external agents cannot see them. Inside
the app, both agents discover and load notes exactly like skills.

## Goal
One harness in `fm-application`, used by the terminal assistant and the chat:
1. A tool registry where adding a tool means writing one type and one registration line.
2. Skills support, with progressive disclosure: a catalogue in the prompt, the body on demand.
3. Notes exposed as virtual skills through the same catalogue and tools.
4. The chat gains the read-only tools (files, notes, skills) with the same safety rules as the
   terminal assistant.

## Design

### Tool trait and registry (`crates/fm-application/src/agent/`)
```rust
#[async_trait]
pub(crate) trait AgentTool: Send + Sync {
    fn name(&self) -> &'static str;             // "read_file"
    fn description(&self) -> &'static str;      // one or two sentences for the prompt
    fn input_schema(&self) -> serde_json::Value; // JSON Schema, from a schemars-derived args type
    fn approval(&self) -> Approval;             // Auto | PerCall | Batch(kind)
    async fn run(&self, ctx: &ToolContext<'_>, args: serde_json::Value) -> ToolOutcome;
}
```
- A typed helper (`typed_tool::<Args, F>()`, or a small macro) deserialises `args` into a
  `#[derive(Deserialize, JsonSchema)]` struct. Each tool then states its contract once, and
  invalid arguments become a uniform tool error that is sent back to the model. This is our
  `createTool`.
- `ToolRegistry` holds `Arc<dyn AgentTool>`. Each agent builds its own registry: the terminal
  assistant adds `run_command` and `write_file`; the chat does not.
- `ToolContext` carries what tools need without reaching into `FileManagerService`:
  - the VFS providers and the working folder sandbox
  - the `SemanticAccessContext`
  - `NotesService`
  - the skills catalogue
  - the cancellation token
  - the host's accessible roots (server mode)
  - a redaction hook
- `ToolOutcome`: `Output { step, text }`, `NeedsApproval(..)` and `Proposal(..)`. These replace
  `NoteToolOutcome` and the ad-hoc outcomes in the terminal loop.

### The loop
- Move the step loop out of `terminal_assist/mod.rs` into `agent::run_turn`. The loop owns:
  - the maximum number of steps and the per-call output cap
  - cancellation, and stopping on a rejected approval
  - streaming of answer text
  - redaction of tool output before it is sent to the model
- Two ways to call tools, chosen per LLM profile:
  - **Native tool calling** when the endpoint supports it. Add a `ToolCalling` capability to
    `LlmApiCapability` (the Ollama probe already reads `tools`), and send the registry as the
    provider's `tools` list.
  - **Text protocol** (today's `TOOL {json}`) as the fallback. The prompt's tool section is
    generated from the registry (name, description, a compact JSON example from the schema),
    not hand-written.
- Keep one call per step for the text protocol. Native calls may batch read-only tools, which
  run concurrently up to a small limit; calls that need approval still run one at a time.

### Skills (`agent::skills`, or a `SkillsService` if it grows)
- **Sources, all read-only through the VFS:**
  - the user folders `~/.claude/skills/*/SKILL.md` and `~/.agents/skills/*/SKILL.md`
  - the project folders `.claude/skills` and `.agents/skills` under the working folder or the
    pane's folder, up to the nearest git root
  - extra folders from a new `skillsRoots` setting
- **Catalogue:** for each skill, its `name` and `description` from the front matter. This needs
  a Rust front-matter parser with the same coverage as `markdown-metadata.ts`: block scalars,
  nested maps and lists. Bounded:
  - at most N skills
  - each description capped
  - the whole catalogue capped in bytes; when over the cap, keep the closest matches to the
    request (text match, or semantic when available)
- **Prompt:** the catalogue goes into the system prompt as a short list ("Skills you can load:
  name: description").
- **Tools:**
  - `read_skill {name}` returns the `SKILL.md` body.
  - `read_skill_file {name, path}` reads a file inside that skill's folder (references,
    templates). It is sandboxed to the folder and goes through the same text/document reader
    as `read_file`.
  - Scripts in a skill are **never run** by the harness. A skill that says to run something
    becomes a `run_command` proposal that the person approves, in the terminal assistant only.
- Precedence when names collide: project, then user, then notes. The catalogue shows the
  source.
- Secret and credential file names are skipped, as in `read_file`.

### Notes as virtual skills
- Every note in the notes folder (0337 `NotesStore`) is a skill candidate:
  - **name:** the front matter `name`, else a slug of the title
  - **description:** the front matter `description`, else a short excerpt (first paragraph or
    the tags)
- 0337 capture (`draft_note`) also writes `description: Use when …`, so notes made from now on
  read like skills. Old notes still work through the fallback.
- **Catalogue for notes:** the folder is unbounded, so instead of listing every note, the
  harness adds the top notes for the current request (`NotesService::search`: semantic when
  indexed, text otherwise) to the catalogue, marked `source: note`.
- **Tools:**
  - `read_skill` resolves note names too, so one tool loads either kind.
  - `search_notes` stays, also as a way to search skills ("search skills and notes").
  - `read_note` becomes an alias of `read_skill`, or is removed if nothing still needs the
    `path` form.
- Writing stays as in 0337: `propose_note`/`propose_note_update` go to review, nothing is saved
  without the person. Optional later: an explicit "Export as skill" action that writes
  `<skills root>/<slug>/SKILL.md` for external agents. It is out of scope here.

### Chat parity
- The chat runs `agent::run_turn` with a read-only registry:
  - `list_dir`, `read_file` and `grep`, sandboxed to a folder the person chose (the pane's
    folder by default, shown as a context chip)
  - `search_notes`, `read_skill` and `read_skill_file`
  - `propose_note` and `propose_note_update`, which use the same review dialog as the terminal
- No `run_command`, and no `write_file` unless a later task adds it with 0338's approval
  model.
- **Browser/server mode (spec rule 9):** the chat runs in both hosts, so every tool honours the
  server's accessible roots (`require_within_roots`), and skills sources outside them are
  skipped. User-home skill folders are desktop only unless they lie within an accessible root.
  Notes follow 0337 (no notes in browser mode unless the notes root is accessible).
- **Streaming and UI:** the chat shows tool steps the way the terminal drawer does, as folded
  pills. Proposals use the 0337 note review dialog.
- **Consent:** cloud endpoints (0322 allow-cloud) never receive file, note or skill contents
  without the existing consent. The catalogue (names and descriptions) counts as content.

## Acceptance Criteria
- **Registry**
  - Adding a new read-only tool means one `AgentTool` implementation plus one registration line,
    with no changes to the loop, the prompt text or the error handling. A test registers a fake
    tool and sees it described in the generated prompt and called through both the native and
    text paths.
  - All existing terminal tools are ported to the registry with unchanged behaviour. The
    existing `terminal_assist` tests keep passing, adapted only where types moved.
  - Invalid tool arguments produce a uniform error that goes back to the model, never a panic or
    an aborted turn.
- **Native tool calling**
  - It is used when the profile has the `ToolCalling` capability, and the text protocol
    otherwise.
  - Both paths are covered by tests with a fake generator.
- **Skills**
  - The catalogue lists skills from every source with their name, description and source, and
    stays within its caps.
  - `read_skill` and `read_skill_file` are sandboxed: a deny test covers `..`, symlinks, and
    credential and secret names.
  - Scripts are never run.
- **Notes as skills**
  - With notes configured, a relevant note appears in the catalogue for a matching request and
    loads through `read_skill`, in both agents.
  - New captures carry `description: Use when …`.
- **Chat**
  - The chat can list, read and grep in its chosen folder, and can search and read notes and
    skills.
  - Proposals open the review dialog.
  - Server mode is restricted to the accessible roots, with deny-path tests on the routes.
  - Behaviour matches between the http and tauri clients, and mock-client fixtures exist.
- **Docs and quality**
  - README (Notes and Assistant sections) and CLAUDE.md list the new agent capability service.
  - `pnpm run lint` and the relevant tests pass.

## Implementation Notes
- Suggested order (separate commits):
  1. Registry, loop and text protocol in `agent/`, with the terminal tools ported.
  2. The `ToolCalling` capability and native tool calls in `llm_profiles.rs` (OpenAI-compatible
     `tools`/`tool_calls`, Ollama `tools`).
  3. The Rust front-matter parser and the skills catalogue, with `read_skill` and
     `read_skill_file`.
  4. Notes as virtual skills, and `description` in capture.
  5. Chat parity: backend, routes, Tauri commands, the frontend tool-step UI, and the review
     dialog reuse.
- **Services:** `FileManagerService` stays a thin facade (CLAUDE.md). The harness is a
  capability module. `ChatService` and the terminal assistant depend on it; it does not depend
  on them or on the facade.
- **Code to reuse:**
  - `terminal_assist/protocol.rs` (text parsing, tolerant JSON)
  - `tools.rs`/`remote.rs` (local and SSH file tools)
  - `documents.rs` (document text)
  - `redact.rs`
  - `notes/` (`NotesStore`, `search_notes`)
- **Schemas:** `schemars` is already in the workspace if utoipa pulls it in. Otherwise add it to
  `fm-application` only.
- **Generated code:** regenerate the HTTP client after DTO changes (`pnpm api:export`,
  `pnpm api:generate`). Never hand-edit it.
- **Out of scope:** an edit/patch tool, MCP client support, sub-agents, compaction beyond 0336's
  bounded transcript, running skill scripts, and exporting notes as real skills.

## Agent Notes
- 2026-11-22: Done in `crates/fm-application/src/agent/`: `AgentTool`/`TypedTool`/`ToolRegistry` (prompt
  section generated from the schema, uniform error for bad arguments, tolerant text-protocol parse),
  Rust front-matter parser, `SkillCatalogue` (project > user > note, caps, ranking by request),
  `read_skill`/`read_skill_file` (sandboxed; deny tests for `..`, symlinks, secrets, credential
  folders), notes as virtual skills (`NotesAccess::skills`, `name`/`description` in `NoteMeta`,
  capture writes `description: Use when …`). Wired into the terminal `ask` loop; README/CLAUDE.md updated.
- 2026-11-22: All 10 terminal tools are registry tools now (`agent/tools.rs`, `terminal_assist/
  agent_tools.rs`); `ToolCall`/`protocol.rs` are gone, `ToolOutcome` has `Output`, `Proposal` and
  `NeedsApproval`, the hand-written tool list in `ASK_PROMPT` is generated. `FileTools` in the
  `ToolContext` is how a host (terminal now, chat later) supplies sandboxed file access.
  Behaviour change: a call with bad or blank arguments now returns an error to the model instead of
  being taken as the final answer.
- Not done (open): `agent::run_turn` shared loop, native tool calling + `ToolCalling` capability, `skillsRoots`
  setting, server-mode accessible-roots filtering for skill folders (user-home skills are read in
  both hosts today), `read_skill_file` through the document reader, chat parity (backend, routes,
  Tauri, frontend, mock fixtures). Note skills found for the first step of a question are not
  re-searched on an approval continuation.
