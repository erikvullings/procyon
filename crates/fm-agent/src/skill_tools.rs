//! The tools that load skills: `read_skill` for the instructions of a skill or a note, and
//! `read_skill_file` for a file inside a skill's folder. Both are read-only and never run
//! anything.

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::notes::NoteToolCall;
use crate::redact::redact_secrets;
use crate::skills::{Skill, SkillLocation};
use crate::tool::{ToolContext, ToolEffect, ToolOutcome, ToolOutput, TypedTool};

/// Names listed when a skill is not found.
const SUGGESTED_NAMES: usize = 20;

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ReadSkillArgs {
    /// The skill's name, as the catalogue lists it.
    name: String,
}

pub(crate) struct ReadSkill;

#[async_trait]
impl<I: Send + 'static> TypedTool<I> for ReadSkill {
    type Args = ReadSkillArgs;

    fn name(&self) -> &'static str {
        "read_skill"
    }

    fn description(&self) -> &'static str {
        "Loads a skill from the catalogue (a skill folder or one of the person's notes) and \
         returns its instructions."
    }

    fn effect(&self) -> ToolEffect {
        ToolEffect::ReadOnly
    }

    async fn run_typed(&self, ctx: &ToolContext<'_>, args: ReadSkillArgs) -> ToolOutcome<I> {
        let name = args.name.trim();
        let step = format!("Loaded skill {name}");
        let Some(skill) = ctx.skills.find(name) else {
            return ToolOutcome::output(step, not_found(ctx, name));
        };
        let output = match &skill.location {
            SkillLocation::Note(path) => read_note_skill(ctx, path, step).await,
            SkillLocation::Folder { .. } => read_folder_skill(skill, step).await,
        };
        output.into()
    }
}

async fn read_folder_skill(skill: &Skill, step: String) -> ToolOutput {
    let name = &skill.name;
    let loaded = match skill.read_body().await {
        Some(body) => Some((body, skill.list_files().await)),
        None => None,
    };
    match loaded {
        Some((Ok(body), files)) => {
            let mut text = format!("Skill {name}:\n{}", redact_secrets(&body));
            if !files.is_empty() {
                text.push_str(&format!(
                    "\n\nFiles in this skill (read one with read_skill_file): {}",
                    files.join(", ")
                ));
            }
            ToolOutput::new(step, text)
        }
        Some((Err(error), _)) => ToolOutput::error(step, error),
        None => ToolOutput::error(step, "the skill cannot be read."),
    }
}

async fn read_note_skill(ctx: &ToolContext<'_>, path: &str, step: String) -> ToolOutput {
    let Some(notes) = ctx.notes else {
        return ToolOutput::error(step, "notes are not available here.");
    };
    let call = NoteToolCall::Read {
        path: path.to_owned(),
    };
    match notes.run(call).await {
        ToolOutcome::Output(output) => ToolOutput::new(step, output.text),
        ToolOutcome::Interrupt(_) => ToolOutput::error(step, "the note cannot be read."),
    }
}

fn not_found(ctx: &ToolContext<'_>, name: &str) -> String {
    let names: Vec<&str> = ctx
        .skills
        .skills()
        .iter()
        .take(SUGGESTED_NAMES)
        .map(|skill| skill.name.as_str())
        .collect();
    if names.is_empty() {
        format!("Error: there is no skill called {name}. No skills are available.")
    } else {
        format!(
            "Error: there is no skill called {name}. Available: {}.",
            names.join(", ")
        )
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ReadSkillFileArgs {
    /// The skill's name, as the catalogue lists it.
    name: String,
    /// The file, relative to the skill's folder.
    path: String,
    /// Byte offset to continue a file that an earlier read cut off.
    offset: Option<usize>,
}

pub(crate) struct ReadSkillFile;

#[async_trait]
impl<I: Send + 'static> TypedTool<I> for ReadSkillFile {
    type Args = ReadSkillFileArgs;

    fn name(&self) -> &'static str {
        "read_skill_file"
    }

    fn description(&self) -> &'static str {
        "Reads a file inside a skill's folder (references, templates, documents as text). The path \
         is relative to the skill and cannot leave it. A long file is cut off with a note: continue \
         with the offset it names. Scripts in a skill are never run by this tool."
    }

    fn effect(&self) -> ToolEffect {
        ToolEffect::ReadOnly
    }

    async fn run_typed(&self, ctx: &ToolContext<'_>, args: ReadSkillFileArgs) -> ToolOutcome<I> {
        let relative = args.path.trim().to_owned();
        let step = format!("Read {relative} in skill {}", args.name.trim());
        let Some(skill) = ctx.skills.find(&args.name) else {
            return ToolOutcome::output(step, not_found(ctx, &args.name));
        };
        let offset = args.offset.unwrap_or(0);
        // The sandbox runs first, for documents too: the host converter only sees checked paths.
        let path = match skill.resolve_file(&relative).await {
            Ok(path) => path,
            Err(error) => return ToolOutcome::error(step, error),
        };
        if let Some(documents) = ctx.documents
            && let Some((store, _)) = skill.folder()
            && let Some(result) = documents.read(store, &path, offset).await
        {
            return match result {
                Ok(text) => ToolOutcome::output(step, redact_secrets(&text)),
                Err(error) => ToolOutcome::error(step, error),
            };
        }
        let result = skill.read_checked(&path, offset).await;
        match result {
            Ok(text) => ToolOutcome::output(step, redact_secrets(&text)),
            Err(error) => ToolOutcome::error(step, error),
        }
    }
}
