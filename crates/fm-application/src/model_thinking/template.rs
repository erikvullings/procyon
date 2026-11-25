//! Reads a model's thinking controls from its chat template (task 0362), by rendering the
//! template once per switch and effort and comparing the prompts. No model is called: what the
//! template does with `enable_thinking` and `reasoning_effort` is what the server sends the model.

use std::collections::BTreeMap;

use minijinja::{Environment, Error, ErrorKind, Value, context};

use super::{EFFORT_VALUES, LevelName, ModelThinking, SWITCH_NAMES, ThinkingKind};

/// Longest template analysed, in bytes.
pub const MAX_TEMPLATE_BYTES: usize = 1024 * 1024;
/// Rendering budget per render, so a template cannot loop for long.
const FUEL: u64 = 5_000_000;
/// A value no template knows, to tell values it uses from values it ignores.
const UNKNOWN_EFFORT: &str = "procyon-unknown-effort";

/// The thinking controls `template` shows, or `None` when it cannot be rendered or shows none.
pub fn analyse(template: &str) -> Option<ModelThinking> {
    if template.len() > MAX_TEMPLATE_BYTES {
        return None;
    }
    let template = without_generation_tags(template);
    let template = template.as_str();
    let env = environment();
    let render = |name: Option<&str>, value: Value| render(&env, template, name, value);
    let plain = render(None, Value::UNDEFINED).ok()?;

    // The switch: the first kwarg whose on and off prompts differ.
    let mut switch = None;
    let mut default_on = true;
    for name in SWITCH_NAMES {
        if let (Ok(on), Ok(off)) = (
            render(Some(name), Value::from(true)),
            render(Some(name), Value::from(false)),
        ) && on != off
        {
            switch = Some(*name);
            // Plain equal to off means thinking is off unless asked for.
            default_on = plain != off;
            break;
        }
    }

    // Efforts: values the template accepts and does something with. When an unknown value raises,
    // every value that renders is accepted; when it is ignored, only values that change the prompt.
    let unknown = render(Some("reasoning_effort"), Value::from(UNKNOWN_EFFORT));
    let accepted: Vec<&'static str> = EFFORT_VALUES
        .iter()
        .copied()
        .filter(|value| {
            match (
                render(Some("reasoning_effort"), Value::from(*value)),
                &unknown,
            ) {
                (Ok(_), Err(_)) => true,
                (Ok(prompt), Ok(ignored)) => prompt != *ignored,
                (Err(_), _) => false,
            }
        })
        .collect();
    let takes = |value: &str| accepted.contains(&value);
    let first = |values: &[&'static str]| values.iter().copied().find(|value| takes(value));

    let mut levels = BTreeMap::new();
    if switch.is_none()
        && let Some(none) = first(&["none"])
    {
        levels.insert(LevelName::Off, none.to_owned());
    }
    for (level, values) in [
        (LevelName::Low, &["low", "minimal"][..]),
        (LevelName::Medium, &["medium"][..]),
        (LevelName::High, &["high", "xhigh", "max"][..]),
    ] {
        if let Some(value) = first(values) {
            levels.insert(level, value.to_owned());
        }
    }

    let thinking = match (switch, levels.is_empty()) {
        (Some(_), _) => ThinkingKind::Toggle,
        (None, false) => ThinkingKind::Always,
        // Nothing to switch and no levels: whether it thinks cannot be told from the template.
        (None, true) => return None,
    };
    Some(ModelThinking {
        matches: Vec::new(),
        source: "the chat template served with the model".to_owned(),
        thinking,
        switch: switch.map(str::to_owned),
        default_on,
        levels,
        note: (!accepted.is_empty())
            .then(|| format!("Efforts the template takes: {}.", accepted.join(", "))),
        family: false,
        tested: Vec::new(),
    })
}

/// `template` without transformers' `{% generation %}` / `{% endgeneration %}` tags, which only
/// mark the assistant's text for training masks and are not Jinja.
fn without_generation_tags(template: &str) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{%") {
        let Some(end) = rest[start..].find("%}") else {
            break;
        };
        let tag = &rest[start + 2..start + end];
        let word = tag.trim_matches(|c: char| c == '-' || c.is_whitespace());
        out.push_str(&rest[..start]);
        if word != "generation" && word != "endgeneration" {
            out.push_str(&rest[start..start + end + 2]);
        }
        rest = &rest[start + end + 2..];
    }
    out.push_str(rest);
    out
}

fn environment() -> Environment<'static> {
    let mut env = Environment::new();
    env.set_fuel(Some(FUEL));
    minijinja_contrib::add_to_environment(&mut env);
    env.set_unknown_method_callback(minijinja_contrib::pycompat::unknown_method_callback);
    env.add_function(
        "raise_exception",
        |message: String| -> Result<Value, Error> {
            Err(Error::new(ErrorKind::InvalidOperation, message))
        },
    );
    // Some templates print today's date; a fixed one keeps renders comparable.
    env.add_function("strftime_now", |_format: String| "01 Jan 2026".to_owned());
    env
}

/// One render with a user message and the generation prompt; `name` set to `value` when given.
fn render(
    env: &Environment<'_>,
    template: &str,
    name: Option<&str>,
    value: Value,
) -> Result<String, Error> {
    let messages = Value::from_serialize([serde_json::json!({"role": "user", "content": "Hello"})]);
    let base = context! {
        messages => messages,
        add_generation_prompt => true,
        bos_token => "",
        eos_token => "",
    };
    let ctx = match name {
        Some(name) => {
            let mut extra = BTreeMap::new();
            extra.insert(name.to_owned(), value);
            context! { ..Value::from(extra), ..base }
        }
        None => base,
    };
    env.render_str(template, ctx)
}

#[cfg(test)]
mod tests;
