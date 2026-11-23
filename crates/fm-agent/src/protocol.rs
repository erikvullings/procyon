//! Finds tool calls in a model reply.
//!
//! The documented form is one `TOOL {json}` line per call, where the object is either
//! `{"tool": "<name>", ...arguments}` or `{"name": "<name>", "arguments": {...}}` (the form native
//! calls are written back in). Models that were not told about the protocol, or that follow their
//! own habits, also write the object alone on a line, wrapped in tags (`<tool_call>`) or a code
//! fence, or in a tag syntax with bare keys (`call:list_dir{path:"src"}`). Those forms count only
//! when the call stands on its own line, so an answer that merely mentions an example call in a
//! sentence is not run.

use serde_json::{Map, Value};

/// A call as written: the tool's name and its raw arguments.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RawCall {
    /// The endpoint's id for a native call written back with one.
    pub(crate) id: Option<String>,
    pub(crate) name: String,
    pub(crate) args: Value,
}

/// Openers a model may put before a call on its line.
const CALL_PREFIXES: [&str; 7] = [
    "<|tool_call>",
    "<|tool_call|>",
    "<tool_call>",
    "<function_call>",
    "<function>",
    "```json",
    "```",
];

/// Every call of a reply, in order, at most `max`. `TOOL` lines win; without them, the first call
/// in another form counts (models that do not batch write one).
pub(crate) fn parse_calls(reply: &str, known: &dyn Fn(&str) -> bool, max: usize) -> Vec<RawCall> {
    let tool_lines: Vec<RawCall> = reply
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("TOOL"))
        .filter(|rest| rest.starts_with([' ', '\t', '{']))
        .filter_map(|rest| object_call(rest.trim_start(), known))
        .take(max)
        .collect();
    if !tool_lines.is_empty() {
        return tool_lines;
    }
    first_loose_call(reply, known).into_iter().collect()
}

/// The first call in one of the tolerated forms, standing at the start of a line.
fn first_loose_call(reply: &str, known: &dyn Fn(&str) -> bool) -> Option<RawCall> {
    let mut offset = 0;
    for line in reply.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        let mut rest = line.trim_start();
        let mut tagged = false;
        // Peel the openers a model may put first: `<tool_call>```json {..}` and the like.
        loop {
            let before = rest;
            for prefix in CALL_PREFIXES {
                if let Some(after) = rest.strip_prefix(prefix) {
                    rest = after.trim_start();
                    tagged |= prefix.starts_with('<');
                }
            }
            if before.len() == rest.len() {
                break;
            }
        }
        // The object may continue on the next lines (a pretty-printed call).
        let from = start + (line.len() - rest.len());
        let tail = &reply[from..];
        if tail.starts_with('{') {
            if let Some(call) = object_call(tail, known) {
                return Some(call);
            }
        } else if let Some(call) = tag_call(tail, known) {
            return Some(call);
        } else if tagged && let Some(call) = garbled_call(tail, known) {
            return Some(call);
        }
    }
    trailing_call(reply, known)
}

/// After a call tag, a call object that follows a stray word on the same line: a model that mixes
/// its own call syntax with the documented one writes `<|tool_call>_call {"tool": ...} more text`.
/// The tag says a call was meant, so the object counts even with text around it.
fn garbled_call(tail: &str, known: &dyn Fn(&str) -> bool) -> Option<RawCall> {
    let line_end = tail.find('\n').unwrap_or(tail.len());
    let start = tail[..line_end].find('{')?;
    object_call(&tail[start..], known)
}

/// Closers a model may put after a call.
const CALL_SUFFIXES: [&str; 6] = [
    "</tool_call>",
    "<tool_call|>",
    "<|/tool_call|>",
    "</function_call>",
    "</function>",
    "```",
];

/// A call that ends the reply, after some prose on the same line ("Let me look: {...}").
fn trailing_call(reply: &str, known: &dyn Fn(&str) -> bool) -> Option<RawCall> {
    let mut end = reply.trim_end();
    loop {
        let before = end.len();
        for suffix in CALL_SUFFIXES {
            if let Some(stripped) = end.strip_suffix(suffix) {
                end = stripped.trim_end();
            }
        }
        if before == end.len() {
            break;
        }
    }
    if !end.ends_with('}') {
        return None;
    }
    end.match_indices('{').find_map(|(start, _)| {
        let object = balanced_object(&end[start..])?;
        (start + object.len() == end.len())
            .then(|| object_call(object, known))
            .flatten()
    })
}

/// `{"tool": "x", ...}` or `{"name": "x", "arguments": {...}}` at the start of `text`.
fn object_call(text: &str, known: &dyn Fn(&str) -> bool) -> Option<RawCall> {
    let object = balanced_object(text)?;
    // Cheap rejection before parsing: code and data in an answer rarely name either key.
    if !object.contains("\"tool\"") && !object.contains("\"name\"") {
        return None;
    }
    let Value::Object(mut fields) = serde_json::from_str::<Value>(object).ok()? else {
        return None;
    };
    let (id, name, args) = match fields.remove("tool") {
        Some(Value::String(name)) => (None, name, Value::Object(fields)),
        _ => {
            let name = fields.remove("name")?.as_str()?.to_owned();
            let id = match fields.remove("id") {
                Some(Value::String(id)) if !id.is_empty() => Some(id),
                _ => None,
            };
            let args = match fields
                .remove("arguments")
                .or_else(|| fields.remove("parameters"))
            {
                // Some endpoints send the arguments as a JSON string.
                Some(Value::String(encoded)) => serde_json::from_str(&encoded).ok()?,
                Some(args) => args,
                None => Value::Object(Map::new()),
            };
            (id, name, args)
        }
    };
    known(&name).then_some(RawCall { id, name, args })
}

/// `call:NAME{key:"value"}` at the start of `text` (Gemma writes
/// `<|tool_call>call:list_dir{path:<|"|>src<|"|>}<tool_call|>`): bare keys, and `<|"|>` as the
/// string quote.
fn tag_call(text: &str, known: &dyn Fn(&str) -> bool) -> Option<RawCall> {
    let after = text.strip_prefix("call:")?;
    let name_end = after.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))?;
    let name = &after[..name_end];
    if !known(name) {
        return None;
    }
    let body = after[name_end..].replace("<|\"|>", "\"");
    let object = balanced_object(&body)?;
    let args: Value = serde_json::from_str(&quote_bare_keys(object)).ok()?;
    args.is_object().then(|| RawCall {
        id: None,
        name: name.to_owned(),
        args,
    })
}

/// The text from the opening brace at the start of `text` to its matching closing brace, braces
/// inside strings ignored.
pub(crate) fn balanced_object(text: &str) -> Option<&str> {
    if !text.starts_with('{') {
        return None;
    }
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

/// `TOOL {"id":"<id>","name":"<name>","arguments":{..}}`: a call in the protocol's line form (the
/// id only when given). Hosts write native tool calls back like this, keeping the endpoint's call
/// id, so an argument may be called `tool` or `name` without clashing.
pub fn tool_call_line(id: Option<&str>, name: &str, arguments: &Value) -> String {
    let arguments = if arguments.is_object() {
        arguments.clone()
    } else {
        Value::Object(Map::new())
    };
    let mut object = Map::new();
    if let Some(id) = id.filter(|id| !id.is_empty()) {
        object.insert("id".to_owned(), Value::String(id.to_owned()));
    }
    object.insert("name".to_owned(), Value::String(name.to_owned()));
    object.insert("arguments".to_owned(), arguments);
    format!("TOOL {}", Value::Object(object))
}

#[cfg(test)]
mod tests;
