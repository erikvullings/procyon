//! One tool the model may call: its definition, what a run produces, and what it may use.
//!
//! A tool is generic over `I`, the host's *interrupt* type: what a tool hands back when the turn
//! must stop for the person (an approval, a draft to review). The harness never looks inside an
//! interrupt; it ends the turn and gives it to the host. Built-in tools that interrupt ask for
//! `I: From<TheirRequest>`, so a host enum with one variant per request works.

use async_trait::async_trait;
use schemars::JsonSchema;
use schemars::generate::SchemaSettings;
use serde::de::DeserializeOwned;
use serde_json::Value;
use thiserror::Error;

use crate::notes::NotesAccess;
use crate::skills::SkillCatalogue;
use crate::store::StoreHandle;
use crate::summarize::{SideModel, WholeText};

/// What a tool shows the person and tells the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutput {
    /// A short line for the person ("Read src/main.rs").
    pub step: String,
    /// The result for the model.
    pub text: String,
}

impl ToolOutput {
    /// An output with its step line and its text for the model.
    pub fn new(step: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            step: step.into(),
            text: text.into(),
        }
    }

    /// An error for the model, prefixed so it reads as one.
    pub fn error(step: impl Into<String>, message: impl std::fmt::Display) -> Self {
        Self::new(step, format!("Error: {message}"))
    }
}

/// What one tool run produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolOutcome<I> {
    /// A result; the turn goes on.
    Output(ToolOutput),
    /// The turn stops for the person; the host decides what happens next.
    Interrupt(I),
}

impl<I> ToolOutcome<I> {
    /// An output with its step line and its text for the model.
    pub fn output(step: impl Into<String>, text: impl Into<String>) -> Self {
        Self::Output(ToolOutput::new(step, text))
    }

    /// An error for the model.
    pub fn error(step: impl Into<String>, message: impl std::fmt::Display) -> Self {
        Self::Output(ToolOutput::error(step, message))
    }

    /// The same outcome with its interrupt converted.
    pub fn map_interrupt<J>(self, convert: impl FnOnce(I) -> J) -> ToolOutcome<J> {
        match self {
            Self::Output(output) => ToolOutcome::Output(output),
            Self::Interrupt(interrupt) => ToolOutcome::Interrupt(convert(interrupt)),
        }
    }

    /// The text for the model, for tests of tools that only produce output.
    #[cfg(test)]
    pub(crate) fn expect_text(self) -> String
    where
        I: std::fmt::Debug,
    {
        match self {
            Self::Output(output) => output.text,
            Self::Interrupt(interrupt) => panic!("expected output, got {interrupt:?}"),
        }
    }
}

impl<I> From<ToolOutput> for ToolOutcome<I> {
    fn from(output: ToolOutput) -> Self {
        Self::Output(output)
    }
}

/// Whether a tool may run alongside others in one reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolEffect {
    /// Only looks things up: several such calls of one reply run concurrently.
    ReadOnly,
    /// Changes something or asks the person: runs alone, in order.
    Exclusive,
}

/// Files the tools may look at: a working folder the host sandboxes. The host decides what a path
/// means (local, remote) and what is allowed; every answer is a step line and text for the model.
#[async_trait]
pub trait FileTools: Send + Sync {
    /// Lists a folder, relative to the working folder.
    async fn list_dir(&self, path: &str) -> ToolOutput;
    /// Reads a file's text from byte `offset`.
    async fn read_file(&self, path: &str, offset: usize) -> ToolOutput;
    /// Finds lines containing `pattern`.
    async fn grep(&self, pattern: &str, path: Option<&str>) -> ToolOutput;
    /// The whole text of a file (a document converted), up to about `max` bytes, for a tool that
    /// hands it to a model call of its own instead of the conversation. `Err` holds the message
    /// for the model. Hosts that cannot do this keep the default.
    async fn read_whole(&self, _path: &str, _max: usize) -> Result<WholeText, String> {
        Err("reading whole files is not available here.".to_owned())
    }
}

/// Why a document inside a skill folder could not be read as text.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[error("{0}")]
pub struct DocumentError(pub String);

/// Reads a document (PDF, Word, ...) inside a skill's folder as text, when the host has a
/// converter. The harness has already checked `path` against the skill's sandbox: it is a regular
/// file inside the skill folder, not secret-looking, reached without leaving the folder.
#[async_trait]
pub trait SkillDocuments: Send + Sync {
    /// The text of the file at `path` (already checked against the skill's sandbox, in `store`'s
    /// path form) from byte `offset`, or `None` when it is not a document this host converts (the
    /// harness then reads it as plain text).
    async fn read(
        &self,
        store: &StoreHandle,
        path: &str,
        offset: usize,
    ) -> Option<Result<String, DocumentError>>;
}

/// What tools may use during one request. Every capability is optional: a tool whose capability
/// is missing tells the model so.
#[derive(Clone, Copy)]
pub struct ToolContext<'a> {
    /// The skills of this request.
    pub skills: &'a SkillCatalogue,
    /// The notes folder, when set up.
    pub notes: Option<&'a dyn NotesAccess>,
    /// The sandboxed working folder, when there is one.
    pub files: Option<&'a dyn FileTools>,
    /// The document converter for files inside skill folders, when the host has one.
    pub documents: Option<&'a dyn SkillDocuments>,
    /// Model calls outside the conversation (`summarize_file`), when the host has them.
    pub side_model: Option<&'a dyn SideModel>,
    /// Where a long-running tool reports what it is doing, as one short line such as
    /// `Summarising a.pdf: part 2 of 5`. [`run_turn`](crate::run_turn) sets it for each call and
    /// shows the person the latest report until the call's step line replaces it. Report through
    /// [`report`](Self::report).
    pub progress: Option<&'a (dyn Fn(String) + Send + Sync)>,
}

impl<'a> ToolContext<'a> {
    /// A context with only a skills catalogue.
    pub fn new(skills: &'a SkillCatalogue) -> Self {
        Self {
            skills,
            notes: None,
            files: None,
            documents: None,
            side_model: None,
            progress: None,
        }
    }

    /// Reports what this call is doing now; nothing happens when no one listens.
    pub fn report(&self, text: impl Into<String>) {
        if let Some(progress) = self.progress {
            progress(text.into());
        }
    }
}

/// One tool the model may call, with raw JSON arguments. Most tools implement [`TypedTool`]
/// instead and are registered wrapped in [`Typed`].
#[async_trait]
pub trait AgentTool<I>: Send + Sync {
    /// The tool's name, as the model calls it.
    fn name(&self) -> &'static str;
    /// One or two sentences for the model.
    fn description(&self) -> &'static str;
    /// JSON Schema of the arguments: an object with `properties` and `required`.
    fn input_schema(&self) -> Value;
    /// Whether the tool may run alongside other read-only calls.
    fn effect(&self) -> ToolEffect {
        ToolEffect::Exclusive
    }
    /// Runs the tool with its raw arguments.
    async fn run(&self, ctx: &ToolContext<'_>, args: Value) -> ToolOutcome<I>;
}

/// A tool with typed arguments. The schema is derived from `Args` (doc comments become the
/// descriptions the model sees), and arguments that do not fit never reach [`run_typed`]: the
/// model gets a uniform error naming the expected arguments instead.
///
/// [`run_typed`]: TypedTool::run_typed
#[async_trait]
pub trait TypedTool<I>: Send + Sync {
    /// The arguments.
    type Args: DeserializeOwned + JsonSchema + Send;
    /// The tool's name, as the model calls it.
    fn name(&self) -> &'static str;
    /// One or two sentences for the model.
    fn description(&self) -> &'static str;
    /// Whether the tool may run alongside other read-only calls.
    fn effect(&self) -> ToolEffect {
        ToolEffect::Exclusive
    }
    /// JSON Schema of the arguments; derived from `Args` unless overridden.
    fn input_schema(&self) -> Value {
        schema_for::<Self::Args>()
    }
    /// Runs the tool with checked arguments.
    async fn run_typed(&self, ctx: &ToolContext<'_>, args: Self::Args) -> ToolOutcome<I>;
}

/// Registers a [`TypedTool`] as an [`AgentTool`].
pub struct Typed<T>(pub T);

#[async_trait]
impl<I, T> AgentTool<I> for Typed<T>
where
    T: TypedTool<I>,
    I: Send + 'static,
{
    fn name(&self) -> &'static str {
        self.0.name()
    }

    fn description(&self) -> &'static str {
        self.0.description()
    }

    fn input_schema(&self) -> Value {
        self.0.input_schema()
    }

    fn effect(&self) -> ToolEffect {
        self.0.effect()
    }

    async fn run(&self, ctx: &ToolContext<'_>, args: Value) -> ToolOutcome<I> {
        // A model that has no arguments to give often sends `null` or nothing at all.
        let args = if args.is_null() {
            Value::Object(serde_json::Map::new())
        } else {
            args
        };
        match serde_json::from_value::<T::Args>(args) {
            Ok(args) => self.0.run_typed(ctx, args).await,
            Err(error) => ToolOutcome::output(
                format!("Could not call {}", self.0.name()),
                format!(
                    "Error: invalid arguments for {}: {error}. Arguments: {}",
                    self.0.name(),
                    crate::registry::arguments_hint(&self.0.input_schema())
                ),
            ),
        }
    }
}

/// The JSON Schema of `T` as tool-calling endpoints accept it: one inline object schema, without
/// the meta keys, `"type": ["x", "null"]` collapsed to `"x"` (optional is what `required` says)
/// and without schemars' integer formats (`uint`), which strict validators reject.
pub fn schema_for<T: JsonSchema>() -> Value {
    let generator = SchemaSettings::draft2020_12()
        .with(|settings| settings.inline_subschemas = true)
        .into_generator();
    let mut schema: Value = generator.into_root_schema_for::<T>().into();
    if let Some(object) = schema.as_object_mut() {
        object.remove("$schema");
        object.remove("title");
    }
    normalize(&mut schema);
    schema
}

fn normalize(schema: &mut Value) {
    match schema {
        Value::Object(object) => {
            if let Some(Value::Array(kinds)) = object.get("type") {
                let concrete: Vec<Value> = kinds
                    .iter()
                    .filter(|kind| kind.as_str() != Some("null"))
                    .cloned()
                    .collect();
                if concrete.len() == 1 {
                    object.insert("type".to_owned(), concrete[0].clone());
                }
            }
            if object
                .get("format")
                .and_then(Value::as_str)
                .is_some_and(|format| format.starts_with("uint") || format.starts_with("int"))
            {
                object.remove("format");
            }
            for value in object.values_mut() {
                normalize(value);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(normalize),
        _ => {}
    }
}
