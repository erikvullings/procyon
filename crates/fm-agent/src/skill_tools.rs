//! The tools that load skills (task 0339): `read_skill` for the body of a skill or a note, and
//! `read_skill_file` for a file inside a skill's folder. Both are read-only and never run
//! anything.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use super::skills::{Skill, SkillFileError, SkillLocation};
use super::{ToolContext, ToolOutcome, TypedTool};
use crate::notes::{NoteToolCall, NoteToolOutcome};
use crate::redact::redact_secrets;

/// Names listed when a skill is not found.
const SUGGESTED_NAMES: usize = 20;

#[derive(Debug, Deserialize)]
pub(crate) struct ReadSkillArgs {
    name: String,
}

pub(crate) struct ReadSkill;

#[async_trait]
impl TypedTool for ReadSkill {
    type Args = ReadSkillArgs;

    fn name(&self) -> &'static str {
        "read_skill"
    }

    fn description(&self) -> &'static str {
        "loads a skill from the catalogue (a skill folder or one of the person's notes) and \
         returns its instructions."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "name": { "type": "string" } },
            "required": ["name"],
        })
    }

    async fn run_typed(&self, ctx: &ToolContext<'_>, args: ReadSkillArgs) -> ToolOutcome {
        let step = format!("Loaded skill {}", args.name.trim());
        let Some(skill) = ctx.skills.find(&args.name) else {
            return ToolOutcome::output(step, not_found(ctx, &args.name));
        };
        match &skill.location {
            SkillLocation::Note(path) => read_note_skill(ctx, skill, path, step).await,
            SkillLocation::Folder(_) => {
                let skill = skill.clone();
                let loaded = tokio::task::spawn_blocking(move || {
                    let body = skill.read_body()?;
                    Some((body, skill.list_files()))
                })
                .await
                .ok()
                .flatten();
                let text = match loaded {
                    Some((Ok(body), files)) => {
                        let mut text =
                            format!("Skill {}:\n{}", args.name.trim(), redact_secrets(&body));
                        if !files.is_empty() {
                            text.push_str(&format!(
                                "\n\nFiles in this skill (read one with read_skill_file): {}",
                                files.join(", ")
                            ));
                        }
                        text
                    }
                    Some((Err(error), _)) => format!("Error: {error}"),
                    None => "Error: the skill cannot be read.".to_owned(),
                };
                ToolOutcome::output(step, text)
            }
        }
    }
}

async fn read_note_skill(
    ctx: &ToolContext<'_>,
    skill: &Skill,
    path: &str,
    step: String,
) -> ToolOutcome {
    let Some(notes) = ctx.notes else {
        return ToolOutcome::output(step, "Error: notes are not available here.");
    };
    match notes
        .run(NoteToolCall::Read {
            path: path.to_owned(),
        })
        .await
    {
        NoteToolOutcome::Output { text, .. } => ToolOutcome::output(step, text),
        NoteToolOutcome::Proposal(_) => ToolOutcome::output(
            step,
            format!("Error: the note {} cannot be read.", skill.name),
        ),
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
        format!(
            "Error: there is no skill called {}. No skills are available.",
            name.trim()
        )
    } else {
        format!(
            "Error: there is no skill called {}. Available: {}.",
            name.trim(),
            names.join(", ")
        )
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct ReadSkillFileArgs {
    name: String,
    path: String,
}

pub(crate) struct ReadSkillFile;

#[async_trait]
impl TypedTool for ReadSkillFile {
    type Args = ReadSkillFileArgs;

    fn name(&self) -> &'static str {
        "read_skill_file"
    }

    fn description(&self) -> &'static str {
        "reads a text file inside a skill's folder (references, templates). The path is relative \
         to the skill and cannot leave it. Scripts in a skill are never run by this tool."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "path": { "type": "string" },
            },
            "required": ["name", "path"],
        })
    }

    async fn run_typed(&self, ctx: &ToolContext<'_>, args: ReadSkillFileArgs) -> ToolOutcome {
        let step = format!("Read {} in skill {}", args.path.trim(), args.name.trim());
        let Some(skill) = ctx.skills.find(&args.name) else {
            return ToolOutcome::output(step, not_found(ctx, &args.name));
        };
        let skill = skill.clone();
        let path = args.path.clone();
        let result = tokio::task::spawn_blocking(move || skill.read_file(&path))
            .await
            .unwrap_or_else(|_| Err(SkillFileError::NotAvailable(args.path.clone())));
        let text = match result {
            Ok(text) => redact_secrets(&text),
            Err(error) => format!("Error: {error}"),
        };
        ToolOutcome::output(step, text)
    }
}
