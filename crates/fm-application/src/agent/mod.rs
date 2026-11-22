//! The shared agent harness (task 0339): one definition per tool, a registry the prompt is
//! generated from, and skills (including notes as virtual skills) loaded on demand.
//!
//! Adding a tool means one [`TypedTool`] implementation and one [`ToolRegistry::register`] line:
//! the prompt text, the argument checks and the uniform error for bad arguments follow from it.
//! The model calls tools with the text protocol `TOOL {json}`, one call per step.

mod frontmatter;
mod skill_tools;
mod skills;
mod stream;
mod tools;
mod turn;

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

pub(crate) use self::skill_tools::{ReadSkill, ReadSkillFile};
pub(crate) use self::skills::{NoteSkill, SkillCatalogue, SkillRoots};
pub(crate) use self::tools::RunCommand;
pub(crate) use self::turn::{MAX_REPLY_BYTES, TurnEnd, TurnEvent, TurnHost, run_turn};
use fm_transport_dto::NoteProposalDto;

use crate::terminal_assist::NotesAccess;

/// What a tool run produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ToolOutcome {
    /// A line for the person and the text for the model.
    Output { step: String, text: String },
    /// A note waiting for the person's review; it ends the turn.
    Proposal(NoteProposalDto),
    /// The tool does something only the person may allow; the loop asks them.
    NeedsApproval(ApprovalRequest),
}

/// What the person is asked to approve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ApprovalRequest {
    /// Run one line in their terminal.
    RunCommand { command: String },
    /// Create one file (checked by the loop against its limits).
    WriteFile { name: String, content: String },
}

impl ToolOutcome {
    pub(crate) fn output(step: impl Into<String>, text: impl Into<String>) -> Self {
        Self::Output {
            step: step.into(),
            text: text.into(),
        }
    }

    /// The text for the model, for tests of tools that only produce output.
    #[cfg(test)]
    pub(crate) fn expect_text(self) -> String {
        match self {
            Self::Output { text, .. } => text,
            other => panic!("expected output, got {other:?}"),
        }
    }
}

/// Files the tools may look at: a working folder the host sandboxes. Each answer is a step line
/// and the text for the model.
#[async_trait]
pub(crate) trait FileTools: Send + Sync {
    async fn list_dir(&self, path: &str) -> ToolOutcome;
    async fn read_file(&self, path: &str, offset: usize) -> ToolOutcome;
    async fn grep(&self, pattern: &str, path: Option<&str>) -> ToolOutcome;
}

/// What tools may use, without reaching into the application facade.
pub(crate) struct ToolContext<'a> {
    pub(crate) skills: &'a SkillCatalogue,
    pub(crate) notes: Option<&'a dyn NotesAccess>,
    pub(crate) files: Option<&'a dyn FileTools>,
}

/// One tool the model may call.
#[async_trait]
pub(crate) trait AgentTool: Send + Sync {
    /// The name in `{"tool": "<name>"}`.
    fn name(&self) -> &'static str;
    /// One or two sentences for the prompt.
    fn description(&self) -> &'static str;
    /// JSON Schema of the arguments: an object with `properties` and `required`.
    fn input_schema(&self) -> Value;
    async fn run(&self, ctx: &ToolContext<'_>, args: Value) -> ToolOutcome;
}

/// A tool with typed arguments: invalid arguments never reach `run_typed`.
#[async_trait]
pub(crate) trait TypedTool: Send + Sync {
    type Args: DeserializeOwned + Send;
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn input_schema(&self) -> Value;
    async fn run_typed(&self, ctx: &ToolContext<'_>, args: Self::Args) -> ToolOutcome;
}

/// Adapts a [`TypedTool`] to [`AgentTool`], answering bad arguments with a uniform error that is
/// sent back to the model.
pub(crate) struct Typed<T>(pub(crate) T);

#[async_trait]
impl<T: TypedTool> AgentTool for Typed<T> {
    fn name(&self) -> &'static str {
        self.0.name()
    }

    fn description(&self) -> &'static str {
        self.0.description()
    }

    fn input_schema(&self) -> Value {
        self.0.input_schema()
    }

    async fn run(&self, ctx: &ToolContext<'_>, args: Value) -> ToolOutcome {
        match serde_json::from_value::<T::Args>(args) {
            Ok(args) => self.0.run_typed(ctx, args).await,
            Err(error) => ToolOutcome::output(
                format!("Could not call {}", self.0.name()),
                format!(
                    "Error: invalid arguments for {}: {error}. Arguments: {}",
                    self.0.name(),
                    arguments_hint(&self.0.input_schema())
                ),
            ),
        }
    }
}

/// The tools one agent offers, by name.
#[derive(Default, Clone)]
pub(crate) struct ToolRegistry {
    tools: BTreeMap<&'static str, Arc<dyn AgentTool>>,
}

/// A call the model wrote: a registered tool and its raw arguments.
pub(crate) struct ParsedCall {
    pub(crate) tool: Arc<dyn AgentTool>,
    pub(crate) args: Value,
}

impl ToolRegistry {
    pub(crate) fn register(&mut self, tool: impl AgentTool + 'static) -> &mut Self {
        self.tools.insert(tool.name(), Arc::new(tool));
        self
    }

    /// The tool call in a model reply, when the first JSON object that names a registered tool
    /// is one. The documented form is a `TOOL {json}` line; models also wrap the object in tags
    /// or a code fence, or use `{"name":..,"arguments":{..}}`.
    pub(crate) fn parse(&self, reply: &str) -> Option<ParsedCall> {
        reply
            .match_indices('{')
            .find_map(|(start, _)| self.call_at(&reply[start..]))
    }

    fn call_at(&self, text: &str) -> Option<ParsedCall> {
        let object = balanced_object(text)?;
        let value: Value = serde_json::from_str(object).ok()?;
        let (name, args) = match value.get("tool").and_then(Value::as_str) {
            Some(name) => {
                let mut flat = value.as_object()?.clone();
                flat.remove("tool");
                (name.to_owned(), Value::Object(flat))
            }
            None => (
                value.get("name")?.as_str()?.to_owned(),
                value.get("arguments")?.clone(),
            ),
        };
        let tool = self.tools.get(name.as_str())?;
        Some(ParsedCall {
            tool: Arc::clone(tool),
            args,
        })
    }

    /// The prompt section that describes every tool, generated from the registered definitions.
    pub(crate) fn prompt_section(&self) -> String {
        if self.tools.is_empty() {
            return String::new();
        }
        let mut text =
            String::from("Tools (one `TOOL {json}` line per call, then wait for the result):");
        for tool in self.tools.values() {
            let schema = tool.input_schema();
            text.push_str(&format!(
                "\n- {} {} Arguments: {}",
                tool.name(),
                tool.description(),
                arguments_hint(&schema)
            ));
            text.push_str(&format!(" Example: {}", example_call(tool.name(), &schema)));
        }
        text
    }
}

/// `path (string), offset (integer, optional)` from a schema.
fn arguments_hint(schema: &Value) -> String {
    let required: Vec<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|names| names.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return "none".to_owned();
    };
    if properties.is_empty() {
        return "none".to_owned();
    }
    properties
        .iter()
        .map(|(name, property)| {
            let kind = property
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("string");
            let optional = if required.contains(&name.as_str()) {
                ""
            } else {
                ", optional"
            };
            format!("{name} ({kind}{optional})")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// A compact call from a schema: the required arguments with placeholder values.
fn example_call(name: &str, schema: &Value) -> String {
    let mut call = json!({ "tool": name });
    if let (Some(properties), Some(object)) = (
        schema.get("properties").and_then(Value::as_object),
        call.as_object_mut(),
    ) {
        let required: Vec<&str> = schema
            .get("required")
            .and_then(Value::as_array)
            .map(|names| names.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        for (key, property) in properties {
            if !required.contains(&key.as_str()) {
                continue;
            }
            let placeholder = match property.get("type").and_then(Value::as_str) {
                Some("integer" | "number") => json!(0),
                Some("boolean") => json!(true),
                _ => json!("..."),
            };
            object.insert(key.clone(), placeholder);
        }
    }
    call.to_string()
}

/// The text from the opening brace to its matching closing brace, braces in strings ignored.
pub(crate) fn balanced_object(text: &str) -> Option<&str> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (index, character) in text.char_indices() {
        if in_string {
            match character {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match character {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(&text[..=index]);
                }
            }
            _ => {}
        }
    }
    None
}

/// The read-only tools every agent offers: files, notes and skills. An agent that may also run
/// commands or create files adds those itself.
pub(crate) fn read_only_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::default();
    registry
        .register(Typed(tools::ListDir))
        .register(Typed(tools::ReadFile))
        .register(Typed(tools::Grep))
        .register(Typed(tools::SearchNotes))
        .register(Typed(tools::ReadNote))
        .register(Typed(tools::ProposeNote))
        .register(Typed(tools::ProposeNoteUpdate))
        .register(Typed(ReadSkill))
        .register(Typed(ReadSkillFile));
    registry
}

#[cfg(test)]
mod tests;
