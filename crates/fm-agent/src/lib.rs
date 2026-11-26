//! An agent harness: typed tools in a registry, one turn loop for endpoints that call tools
//! natively and for those that need the text protocol, and skills (`SKILL.md` folders, plus notes
//! as virtual skills) loaded on demand.
//!
//! # Adding a tool
//!
//! One [`TypedTool`] implementation and one [`ToolRegistry::register`] line. The argument schema is
//! derived from the `Args` type, the prompt section and native tool specs are generated from the
//! registry, and arguments that do not fit get a uniform error back to the model.
//!
//! ```
//! use fm_agent::{ToolContext, ToolEffect, ToolOutcome, ToolRegistry, Typed, TypedTool};
//! use schemars::JsonSchema;
//! use serde::Deserialize;
//!
//! #[derive(Deserialize, JsonSchema)]
//! struct ShoutArgs {
//!     /// The text to shout.
//!     text: String,
//! }
//!
//! struct Shout;
//!
//! #[async_trait::async_trait]
//! impl<I: Send + 'static> TypedTool<I> for Shout {
//!     type Args = ShoutArgs;
//!     fn name(&self) -> &'static str { "shout" }
//!     fn description(&self) -> &'static str { "Shouts the text." }
//!     fn effect(&self) -> ToolEffect { ToolEffect::ReadOnly }
//!     async fn run_typed(&self, _: &ToolContext<'_>, args: ShoutArgs) -> ToolOutcome<I> {
//!         ToolOutcome::output("Shouted", args.text.to_uppercase())
//!     }
//! }
//!
//! let mut registry = ToolRegistry::<()>::new();
//! registry.register(Typed(Shout));
//! assert!(registry.prompt_section().contains("shout: Shouts the text."));
//! ```
//!
//! # Running a turn
//!
//! The host implements [`TurnHost`] (its model and its conversation) and calls [`run_turn`]. A turn
//! ends with an answer, or with an *interrupt*: a host-defined value a tool returns when the person
//! must act (approve a command, review a draft). Interrupts are the registry's type parameter `I`;
//! the built-in tools that interrupt need `I: From<CommandRequest>` or `I: From<NoteProposal>`.

mod compact;
mod edit;
mod frontmatter;
mod instructions;
mod message;
mod notes;
mod paths;
mod protocol;
mod redact;
mod registry;
mod skill_tools;
mod skills;
mod store;
mod stream;
mod summarize;
mod text;
mod tool;
mod tools;
mod turn;
mod web;

pub use self::compact::{
    COMPACT_AT_PERCENT, COMPACTED_INTRO, Compaction, KEEP_AFTER_COMPACTING_PERCENT, compact,
    compaction_cut, needs_compaction, summarize_conversation,
};
pub use self::edit::{
    EditError, EditFile, EditMatch, EditRequest, MAX_EDIT_TEXT_BYTES, MAX_EDITED_FILE_BYTES,
    apply_edit, match_edit,
};
pub use self::instructions::{
    INSTRUCTION_FILE_NAMES, InstructionFile, InstructionSource, MAX_INSTRUCTION_BYTES,
    instruction_candidates, instructions_section, read_instructions,
};
pub use self::message::{
    Message, NOT_RUN_RESULT, OMITTED_NOTE, ToolCallRecord, fit_messages, render_text,
};
pub use self::notes::{NoteKind, NoteProposal, NoteToolCall, NotesAccess};
pub use self::paths::{is_credential_folder, is_secret_name};
pub use self::protocol::{repair_tool_arguments, tool_call_line};
pub use self::redact::redact_secrets;
pub use self::registry::{ParsedCall, ToolCalling, ToolRegistry, ToolSpec, compose_system_prompt};
pub use self::skills::{
    NoteSkill, Skill, SkillCatalogue, SkillFileError, SkillIndex, SkillLocation, SkillRoots,
    SkillSource,
};
pub use self::store::{
    LOCAL_STORE_ID, LocalStore, SkillStore, StoreEntry, StoreEntryKind, StoreHandle, StoreMetadata,
    file_name, join_path, relative_to,
};
pub use self::summarize::{MAX_SUMMARY_PARTS, SideModel, SideModelError, WholeText};
pub use self::text::{newest_bytes, truncate_to_bytes};
pub use self::tool::{
    AgentTool, ArgumentError, DocumentError, FileTools, SkillDocuments, ToolContext, ToolEffect,
    ToolOutcome, ToolOutput, Typed, TypedTool, schema_for,
};
pub use self::tools::CommandRequest;
pub use self::turn::{
    ContinueNudge, ModelProfile, TurnEnd, TurnEvent, TurnHost, TurnOptions, announces_more_work,
    run_turn,
};
pub use self::web::{MAX_PAGE_TEXT_BYTES, WebAccess, WebError, WebFetch, WebPage, url_problem};

use self::skill_tools::{ReadSkill, ReadSkillFile};
use self::summarize::SummarizeFile;
use self::tools::{
    Grep, ListDir, ProposeNote, ProposeNoteUpdate, ReadFile, ReadNote, RunCommand, SearchNotes,
};

/// The read-only tools: files (`list_dir`, `read_file`, `grep`, and `summarize_file`, which
/// summarises a document in a model call of its own), notes (`search_notes`,
/// `read_note`, and the `propose_note*` drafts the person reviews) and skills (`read_skill`,
/// `read_skill_file`). Nothing here writes or runs anything.
pub fn read_only_registry<I>() -> ToolRegistry<I>
where
    I: From<NoteProposal> + Send + 'static,
{
    let mut registry = ToolRegistry::new();
    registry
        .register(Typed(ListDir))
        .register(Typed(ReadFile))
        .register(Typed(SummarizeFile))
        .register(Typed(Grep))
        .register(Typed(SearchNotes))
        .register(Typed(ReadNote))
        .register(Typed(ProposeNote))
        .register(Typed(ProposeNoteUpdate))
        .register(Typed(ReadSkill))
        .register(Typed(ReadSkillFile))
        .register(Typed(WebFetch));
    registry
}

/// [`read_only_registry`] plus `run_command`, which asks the person to run one line in their
/// terminal.
pub fn command_registry<I>() -> ToolRegistry<I>
where
    I: From<NoteProposal> + From<CommandRequest> + Send + 'static,
{
    let mut registry = read_only_registry();
    registry.register(Typed(RunCommand));
    registry
}

#[cfg(test)]
mod tests;
