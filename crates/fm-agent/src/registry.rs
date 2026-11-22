//! The tools one agent offers, and what the model is told about them: either a generated prompt
//! section (text protocol) or a list of specs for an endpoint that calls tools natively.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{Value, json};

use crate::protocol::{RawCall, parse_calls};
use crate::skills::SkillCatalogue;
use crate::tool::AgentTool;

/// How the model calls tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCalling {
    /// The endpoint takes the tools as a list and returns structured calls.
    Native,
    /// The prompt describes the tools and the model writes `TOOL {json}` lines.
    Text,
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

struct Entry<I> {
    tool: Arc<dyn AgentTool<I>>,
    /// Built once at registration: the schema is needed for every prompt and request.
    spec: ToolSpec,
}

impl<I> Clone for Entry<I> {
    fn clone(&self) -> Self {
        Self {
            tool: Arc::clone(&self.tool),
            spec: self.spec.clone(),
        }
    }
}

/// The tools one agent offers, by name.
pub struct ToolRegistry<I> {
    tools: BTreeMap<&'static str, Entry<I>>,
}

impl<I> Default for ToolRegistry<I> {
    fn default() -> Self {
        Self {
            tools: BTreeMap::new(),
        }
    }
}

impl<I> Clone for ToolRegistry<I> {
    fn clone(&self) -> Self {
        Self {
            tools: self.tools.clone(),
        }
    }
}

/// A call the model wrote to a registered tool.
pub struct ParsedCall<I> {
    /// The tool that was named.
    pub tool: Arc<dyn AgentTool<I>>,
    /// Its arguments as the model wrote them.
    pub args: Value,
}

/// The instruction for the text protocol, ahead of the tool list.
const TEXT_PROTOCOL: &str = "To use a tool, write a line `TOOL {json}` (one line per call) and \
     nothing after it, then wait for the result.";

impl<I> ToolRegistry<I> {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a tool, replacing one of the same name.
    pub fn register(&mut self, tool: impl AgentTool<I> + 'static) -> &mut Self {
        let spec = ToolSpec {
            name: tool.name(),
            description: tool.description(),
            schema: tool.input_schema(),
        };
        self.tools.insert(
            spec.name,
            Entry {
                tool: Arc::new(tool),
                spec,
            },
        );
        self
    }

    /// Replaces what the model is told a tool does, for a host that knows more (say, which
    /// document formats its file reader converts); `false` when no such tool is registered.
    pub fn describe(&mut self, name: &str, description: &'static str) -> bool {
        match self.tools.get_mut(name) {
            Some(entry) => {
                entry.spec.description = description;
                true
            }
            None => false,
        }
    }

    /// Removes a tool; `true` when there was one.
    pub fn remove(&mut self, name: &str) -> bool {
        self.tools.remove(name).is_some()
    }

    /// Whether a tool of that name is registered.
    pub fn contains(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    /// The registered tool names, sorted.
    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.tools.keys().copied()
    }

    /// Whether no tool is registered.
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// The calls of a model reply, in order, at most `max` (see the protocol module for the forms
    /// that count).
    pub fn parse(&self, reply: &str, max: usize) -> Vec<ParsedCall<I>> {
        let known = |name: &str| self.tools.contains_key(name);
        parse_calls(reply, &known, max)
            .into_iter()
            .filter_map(|RawCall { name, args }| {
                let entry = self.tools.get(name.as_str())?;
                Some(ParsedCall {
                    tool: Arc::clone(&entry.tool),
                    args,
                })
            })
            .collect()
    }

    /// The registered tools as specs, for an endpoint that calls tools natively.
    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools
            .values()
            .map(|entry| entry.spec.clone())
            .collect()
    }

    /// The prompt section that teaches the text protocol and describes every tool, generated from
    /// the registered definitions. Empty without tools.
    pub fn prompt_section(&self) -> String {
        if self.tools.is_empty() {
            return String::new();
        }
        let mut text = format!("{TEXT_PROTOCOL}\nTools:");
        for entry in self.tools.values() {
            let ToolSpec {
                name,
                description,
                schema,
            } = &entry.spec;
            text.push_str(&format!(
                "\n- {name}: {description} Arguments: {}. Example: TOOL {}",
                arguments_hint(schema),
                example_call(name, schema)
            ));
        }
        text
    }

    /// The tool part of a system prompt for `calling`: the text protocol and the tools for
    /// [`ToolCalling::Text`]; nothing for [`ToolCalling::Native`], where the endpoint gets
    /// [`specs`](Self::specs) instead.
    pub fn prompt_for(&self, calling: ToolCalling) -> String {
        match calling {
            ToolCalling::Native => String::new(),
            ToolCalling::Text => self.prompt_section(),
        }
    }

    /// The specs to send for `calling`: all of them natively, none for the text protocol.
    pub fn specs_for(&self, calling: ToolCalling) -> Vec<ToolSpec> {
        match calling {
            ToolCalling::Native => self.specs(),
            ToolCalling::Text => Vec::new(),
        }
    }
}

/// A system prompt from its parts: the host's base text, the tool section for `calling`, and the
/// skills catalogue for `request`. Empty sections are left out.
pub fn compose_system_prompt<I>(
    base: &str,
    registry: &ToolRegistry<I>,
    calling: ToolCalling,
    skills: &SkillCatalogue,
    request: &str,
) -> String {
    let mut prompt = base.trim_end().to_owned();
    for section in [registry.prompt_for(calling), skills.prompt_section(request)] {
        if !section.is_empty() {
            prompt.push_str("\n\n");
            prompt.push_str(&section);
        }
    }
    prompt
}

fn type_of(property: &Value) -> &str {
    match property.get("type") {
        Some(Value::String(kind)) => kind,
        Some(Value::Array(kinds)) => kinds
            .iter()
            .filter_map(Value::as_str)
            .find(|kind| *kind != "null")
            .unwrap_or("string"),
        _ => "string",
    }
}

fn required_of(schema: &Value) -> Vec<&str> {
    schema
        .get("required")
        .and_then(Value::as_array)
        .map(|names| names.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

/// `path (string), offset (integer, optional)` from a schema.
pub(crate) fn arguments_hint(schema: &Value) -> String {
    let required = required_of(schema);
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return "none".to_owned();
    };
    if properties.is_empty() {
        return "none".to_owned();
    }
    properties
        .iter()
        .map(|(name, property)| {
            let optional = if required.contains(&name.as_str()) {
                ""
            } else {
                ", optional"
            };
            format!("{name} ({}{optional})", type_of(property))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// A compact call from a schema: the required arguments with placeholder values.
fn example_call(name: &str, schema: &Value) -> String {
    let mut call = json!({ "tool": name });
    let required = required_of(schema);
    if let (Some(properties), Some(object)) = (
        schema.get("properties").and_then(Value::as_object),
        call.as_object_mut(),
    ) {
        for (key, property) in properties {
            if !required.contains(&key.as_str()) {
                continue;
            }
            let placeholder = match type_of(property) {
                "integer" | "number" => json!(0),
                "boolean" => json!(true),
                "array" => json!([]),
                _ => json!("..."),
            };
            object.insert(key.clone(), placeholder);
        }
    }
    call.to_string()
}
