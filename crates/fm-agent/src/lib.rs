//! The shared agent harness (task 0339): one definition per tool, a registry the prompt is
//! generated from, and skills (including notes as virtual skills) loaded on demand.
//!
//! Adding a tool means one [`TypedTool`] implementation and one [`ToolRegistry::register`] line:
//! the prompt text, the argument checks and the uniform error for bad arguments follow from it.
//! The model calls tools with the text protocol `TOOL {json}`, one call per step.

mod frontmatter;
mod notes;
mod paths;
mod redact;
mod skill_tools;
mod skills;
mod stream;
mod text;
mod tools;
mod turn;

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

pub use self::notes::{NoteToolCall, NoteToolOutcome, NotesAccess};
pub use self::paths::{is_credential_folder, is_secret_name};
pub use self::redact::redact_secrets;
use self::skill_tools::{ReadSkill, ReadSkillFile};
pub use self::skills::{NoteSkill, SkillCatalogue, SkillRoots};
pub use self::text::truncate_to_bytes;
use self::tools::RunCommand;
pub use self::turn::{MAX_REPLY_BYTES, TurnEnd, TurnEvent, TurnHost, run_turn};
use fm_transport_dto::NoteProposalDto;

/// What a tool run produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolOutcome {
    /// A line for the person and the text for the model.
    Output {
        /// A line for the person.
        step: String,
        /// The text for the model.
        text: String,
    },
    /// A note waiting for the person's review; it ends the turn.
    Proposal(NoteProposalDto),
    /// The tool does something only the person may allow; the loop asks them.
    NeedsApproval(ApprovalRequest),
}

/// What the person is asked to approve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalRequest {
    /// Run one line in their terminal.
    RunCommand {
        /// The one line to run.
        command: String,
    },
    /// Create one file (checked by the loop against its limits).
    WriteFile {
        /// The file name the model asked for.
        name: String,
        /// The text to write.
        content: String,
    },
}

impl ToolOutcome {
    /// An output with the step line and the text for the model.
    pub fn output(step: impl Into<String>, text: impl Into<String>) -> Self {
        Self::Output {
            step: step.into(),
            text: text.into(),
        }
    }

    /// The text for the model, for tests of tools that only produce output.
    #[cfg(test)]
    pub fn expect_text(self) -> String {
        match self {
            Self::Output { text, .. } => text,
            other => panic!("expected output, got {other:?}"),
        }
    }
}

/// Files the tools may look at: a working folder the host sandboxes. Each answer is a step line
/// and the text for the model.
#[async_trait]
pub trait FileTools: Send + Sync {
    /// Lists a folder, relative to the working folder.
    async fn list_dir(&self, path: &str) -> ToolOutcome;
    /// Reads a file's text from byte `offset`.
    async fn read_file(&self, path: &str, offset: usize) -> ToolOutcome;
    /// Finds lines containing `pattern`.
    async fn grep(&self, pattern: &str, path: Option<&str>) -> ToolOutcome;
}

/// What tools may use, without reaching into the application facade.
pub struct ToolContext<'a> {
    /// The skills of this request.
    pub skills: &'a SkillCatalogue,
    /// The notes folder, when set up.
    pub notes: Option<&'a dyn NotesAccess>,
    /// The sandboxed working folder, when there is one.
    pub files: Option<&'a dyn FileTools>,
    /// Converts documents (PDF, Word, ...) inside a skill's folder to text, when the host can.
    pub documents: Option<&'a dyn SkillDocuments>,
}

/// Reads a document inside a skill's folder as text. The host implements it with its document
/// converter; the folder and path have already been checked against the skill's sandbox rules by
/// the host's own sandbox.
#[async_trait]
pub trait SkillDocuments: Send + Sync {
    /// The text of `relative` inside `folder` from byte `offset`. `None` when the file is not a
    /// document this host converts, so the plain text reader handles it.
    async fn read(
        &self,
        folder: &std::path::Path,
        relative: &str,
        offset: usize,
    ) -> Option<Result<String, String>>;
}

/// One tool the model may call.
#[async_trait]
pub trait AgentTool: Send + Sync {
    /// The name in `{"tool": "<name>"}`.
    fn name(&self) -> &'static str;
    /// One or two sentences for the prompt.
    fn description(&self) -> &'static str;
    /// JSON Schema of the arguments: an object with `properties` and `required`.
    fn input_schema(&self) -> Value;
    /// Runs the tool with its raw arguments.
    async fn run(&self, ctx: &ToolContext<'_>, args: Value) -> ToolOutcome;
}

/// A tool with typed arguments: invalid arguments never reach `run_typed`.
#[async_trait]
pub trait TypedTool: Send + Sync {
    /// The arguments, checked when the call is made.
    type Args: DeserializeOwned + Send;
    /// The name in `{"tool": "<name>"}`.
    fn name(&self) -> &'static str;
    /// One or two sentences for the prompt.
    fn description(&self) -> &'static str;
    /// JSON Schema of the arguments, for the prompt.
    fn input_schema(&self) -> Value;
    /// Runs the tool with checked arguments.
    async fn run_typed(&self, ctx: &ToolContext<'_>, args: Self::Args) -> ToolOutcome;
}

/// Adapts a [`TypedTool`] to [`AgentTool`], answering bad arguments with a uniform error that is
/// sent back to the model.
pub struct Typed<T>(pub T);

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
pub struct ToolRegistry {
    tools: BTreeMap<&'static str, Arc<dyn AgentTool>>,
}

/// One tool as an endpoint's `tools` list needs it.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolSpec {
    /// The tool's name.
    pub name: &'static str,
    /// What it does.
    pub description: &'static str,
    /// JSON Schema of its arguments.
    pub schema: Value,
}

/// Most tool calls run from one model reply.
const MAX_CALLS_PER_REPLY: usize = 8;

/// A call the model wrote: a registered tool and its raw arguments.
pub struct ParsedCall {
    /// The tool that was named.
    pub tool: Arc<dyn AgentTool>,
    /// Its arguments as the model wrote them.
    pub args: Value,
}

impl ToolRegistry {
    /// Adds a tool, replacing one of the same name.
    pub fn register(&mut self, tool: impl AgentTool + 'static) -> &mut Self {
        self.tools.insert(tool.name(), Arc::new(tool));
        self
    }

    /// The tool call in a model reply, when the first JSON object that names a registered tool
    /// is one. The documented form is a `TOOL {json}` line; models also wrap the object in tags
    /// or a code fence, or use `{"name":..,"arguments":{..}}`.
    pub fn parse(&self, reply: &str) -> Option<ParsedCall> {
        reply
            .match_indices('{')
            .find_map(|(start, _)| self.call_at(&reply[start..]))
            .or_else(|| self.native_text_call(reply))
    }

    /// A call in a model's own tag syntax, `call:NAME{key:"value"}` (Gemma writes
    /// `<|tool_call>call:list_dir{path:""}<tool_call|>`): unquoted keys, and `<|"|>` as the string
    /// delimiter.
    fn native_text_call(&self, reply: &str) -> Option<ParsedCall> {
        let reply = reply.replace("<|\"|>", "\"");
        reply.match_indices("call:").find_map(|(start, marker)| {
            let after = &reply[start + marker.len()..];
            let name_end = after.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))?;
            let tool = self.tools.get(&after[..name_end])?;
            let args = after[name_end..]
                .starts_with('{')
                .then(|| balanced_object(&after[name_end..]))
                .flatten()?;
            let args: Value = serde_json::from_str(&quote_bare_keys(args)).ok()?;
            Some(ParsedCall {
                tool: Arc::clone(tool),
                args,
            })
        })
    }

    /// Every tool call of a reply, in order: each `TOOL {json}` line is one call (a model or
    /// endpoint may batch several in one step). A reply without such lines falls back to the first
    /// call [`parse`](Self::parse) finds.
    pub fn parse_all(&self, reply: &str) -> Vec<ParsedCall> {
        let calls: Vec<ParsedCall> = reply
            .lines()
            .filter_map(|line| line.trim_start().strip_prefix("TOOL"))
            .filter_map(|rest| self.call_at(rest.trim_start()))
            .take(MAX_CALLS_PER_REPLY)
            .collect();
        if calls.is_empty() {
            self.parse(reply).into_iter().collect()
        } else {
            calls
        }
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

    /// The registered tools as specs, for an endpoint that calls tools natively.
    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools
            .values()
            .map(|tool| ToolSpec {
                name: tool.name(),
                description: tool.description(),
                schema: tool.input_schema(),
            })
            .collect()
    }

    /// The prompt section that describes every tool, generated from the registered definitions.
    pub fn prompt_section(&self) -> String {
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

/// `{path:"a",n:2}` as JSON: wraps each bare key (an identifier after `{` or `,`) in quotes.
fn quote_bare_keys(object: &str) -> String {
    let mut out = String::with_capacity(object.len() + 8);
    let mut in_string = false;
    let mut escaped = false;
    let mut expects_key = false;
    let mut chars = object.chars().peekable();
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                expects_key = false;
                out.push(c);
            }
            '{' | ',' => {
                expects_key = true;
                out.push(c);
            }
            c if expects_key && (c.is_ascii_alphabetic() || c == '_') => {
                out.push('"');
                out.push(c);
                while let Some(&next) = chars.peek() {
                    if !(next.is_ascii_alphanumeric() || next == '_') {
                        break;
                    }
                    out.push(next);
                    chars.next();
                }
                out.push('"');
                expects_key = false;
            }
            c => {
                if !c.is_whitespace() {
                    expects_key = false;
                }
                out.push(c);
            }
        }
    }
    out
}

/// The text from the opening brace to its matching closing brace, braces in strings ignored.
pub fn balanced_object(text: &str) -> Option<&str> {
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

/// The read-only tools every agent offers: files, notes and skills. An agent that may also create
/// files adds that itself.
pub fn read_only_registry() -> ToolRegistry {
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

/// [`read_only_registry`] plus `run_command`, which asks the person to run one line in their
/// terminal.
pub fn command_registry() -> ToolRegistry {
    let mut registry = read_only_registry();
    registry.register(Typed(RunCommand));
    registry
}

#[cfg(test)]
mod tests;
