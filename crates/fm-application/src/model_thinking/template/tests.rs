//! Templates shaped like the real ones read on 2026-11-25 (trimmed to their thinking logic).

use super::*;

const HEAD: &str =
    "{%- for message in messages %}<|{{ message.role }}|>{{ message.content }}{% endfor %}";

/// Qwen3.8 upstream: `enable_thinking` (default on), efforts low/medium/xhigh, others raise.
const QWEN38: &str = "{%- if enable_thinking is undefined or enable_thinking is true %}{%- set e = reasoning_effort|default('xhigh') %}{%- if e not in ('xhigh', 'medium', 'low') %}{{- raise_exception('Unexpected reasoning effort ' ~ e) }}{%- endif %}[effort {{ e }}]{%- endif %}{%- if add_generation_prompt %}<|assistant|>{%- if enable_thinking is defined and enable_thinking is false %}<think></think>{%- else %}<think>{%- endif %}{%- endif %}";

/// The DGX Spark's Qwen3.8 template: `lean` default, high and max mapped to xhigh, minimal to low.
const SPARK: &str = "{%- set e = reasoning_effort|default('lean') %}{%- if e in ('max', 'high') %}{%- set e = 'xhigh' %}{%- elif e == 'minimal' %}{%- set e = 'low' %}{%- endif %}{%- if e not in ('xhigh', 'medium', 'low', 'lean') %}{{- raise_exception('Unexpected reasoning effort ' ~ e) }}{%- endif %}{%- if enable_thinking is undefined or enable_thinking is true %}[effort {{ e }}]{%- endif %}{%- if add_generation_prompt %}<|assistant|>{%- if enable_thinking is defined and enable_thinking is false %}<think></think>{%- else %}<think>{%- endif %}{%- endif %}";

/// Inkling: no switch, a map of efforts to numbers, unknown keys raise.
const INKLING: &str = "{%- set effort_map = {'none': 0.0, 'minimal': 0.1, 'low': 0.2, 'medium': 0.7, 'high': 0.9, 'xhigh': 0.99, 'max': 0.99} -%}{%- set eff = reasoning_effort if reasoning_effort is defined and reasoning_effort is not none else 0.9 -%}{%- if eff is string -%}{%- if eff not in effort_map -%}{{- raise_exception('Unknown reasoning_effort: ' ~ eff) -}}{%- endif -%}{%- set num = effort_map[eff] -%}{%- else -%}{%- set num = eff -%}{%- endif -%}<|system|>Thinking effort level: {{ num }}{%- if add_generation_prompt %}<|model|>{%- endif %}";

/// Gemma 4: `enable_thinking` defaults to false; no efforts.
const GEMMA4: &str = "{%- set enable_thinking = enable_thinking | default(false) -%}{%- if enable_thinking %}<|think|>{%- endif %}{%- if add_generation_prompt %}<|model|>{%- endif %}";

/// Kimi K2.5: a `thinking` switch, on unless false.
const KIMI: &str = "{%- if add_generation_prompt -%}<|assistant|>{%- if thinking is defined and thinking is false -%}<think></think>{%- else -%}<think>{%- endif -%}{%- endif -%}";

/// Granite 4.2: `enable_thinking`, and `reasoning_effort == "low"` shortens thinking.
const GRANITE: &str = "{%- set enable_thinking = enable_thinking if enable_thinking is defined else true %}{%- set low_effort = reasoning_effort is defined and reasoning_effort == 'low' %}{%- if enable_thinking %}<think>{% if low_effort %}briefly{% endif %}{%- endif %}{%- if add_generation_prompt %}<|assistant|>{%- endif %}";

fn analysed(body: &str) -> ModelThinking {
    analyse(&format!("{HEAD}{body}")).expect("the template shows thinking controls")
}

fn levels(entry: &ModelThinking) -> Vec<(LevelName, &str)> {
    entry
        .levels
        .iter()
        .map(|(level, value)| (*level, value.as_str()))
        .collect()
}

#[test]
fn qwen38_has_a_switch_and_only_its_three_efforts() {
    let entry = analysed(QWEN38);
    assert_eq!(entry.thinking, ThinkingKind::Toggle);
    assert_eq!(entry.switch.as_deref(), Some("enable_thinking"));
    assert!(entry.default_on);
    // `high` raises, so High is sent as `xhigh`.
    assert_eq!(
        levels(&entry),
        [
            (LevelName::Low, "low"),
            (LevelName::Medium, "medium"),
            (LevelName::High, "xhigh")
        ]
    );
}

#[test]
fn the_spark_template_maps_high_and_minimal_itself() {
    let entry = analysed(SPARK);
    assert_eq!(entry.switch.as_deref(), Some("enable_thinking"));
    assert_eq!(
        levels(&entry),
        [
            (LevelName::Low, "low"),
            (LevelName::Medium, "medium"),
            (LevelName::High, "high")
        ]
    );
    assert!(entry.note.as_deref().unwrap().contains("lean"));
}

#[test]
fn inkling_has_no_switch_but_an_effort_from_none_to_max() {
    let entry = analysed(INKLING);
    assert_eq!(entry.thinking, ThinkingKind::Always);
    assert_eq!(entry.switch, None);
    assert_eq!(
        levels(&entry),
        [
            (LevelName::Off, "none"),
            (LevelName::Low, "low"),
            (LevelName::Medium, "medium"),
            (LevelName::High, "high")
        ]
    );
}

#[test]
fn gemma4_switches_and_is_off_by_default() {
    let entry = analysed(GEMMA4);
    assert_eq!(entry.switch.as_deref(), Some("enable_thinking"));
    assert!(!entry.default_on);
    assert!(entry.levels.is_empty());
}

#[test]
fn kimi_switches_with_thinking() {
    let entry = analysed(KIMI);
    assert_eq!(entry.switch.as_deref(), Some("thinking"));
    assert!(entry.default_on);
}

#[test]
fn granite_takes_only_low() {
    let entry = analysed(GRANITE);
    assert_eq!(entry.switch.as_deref(), Some("enable_thinking"));
    assert_eq!(levels(&entry), [(LevelName::Low, "low")]);
}

#[test]
fn generation_tags_are_not_jinja_and_are_dropped() {
    let template = format!(
        "{HEAD}{{%- generation -%}}x{{%- endgeneration -%}}{{% set e = reasoning_effort|default('low') %}}{{% if e not in ('low', 'high') %}}{{{{ raise_exception('no') }}}}{{% endif %}}[{{{{ e }}}}]"
    );
    let entry = analyse(&template).unwrap();
    assert_eq!(
        levels(&entry),
        [(LevelName::Low, "low"), (LevelName::High, "high")]
    );
}

#[test]
fn a_template_without_controls_or_that_does_not_render_yields_nothing() {
    assert!(
        analyse(&format!(
            "{HEAD}{{% if add_generation_prompt %}}<|assistant|>{{% endif %}}"
        ))
        .is_none()
    );
    assert!(analyse("{% for %}").is_none());
    assert!(analyse(&"x".repeat(MAX_TEMPLATE_BYTES + 1)).is_none());
}

/// Prints the analysis of every `*.jinja` (or `tokenizer_config.json` with a template) below
/// `PROCYON_TEMPLATES`, to check new models by hand:
/// `PROCYON_TEMPLATES=~/.omlx/models cargo test -p fm-application real_templates -- --ignored --nocapture`
#[test]
#[ignore = "reads templates from PROCYON_TEMPLATES"]
fn real_templates() {
    let Some(root) = std::env::var_os("PROCYON_TEMPLATES") else {
        return;
    };
    let mut stack = vec![std::path::PathBuf::from(root)];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for path in entries.filter_map(Result::ok).map(|entry| entry.path()) {
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            let template = match name {
                "tokenizer_config.json" => std::fs::read(&path)
                    .ok()
                    .and_then(|bytes| crate::llm_profiles::template_from_tokenizer_config(&bytes)),
                _ if name.ends_with(".jinja") || name.ends_with(".tpl") => {
                    std::fs::read_to_string(&path).ok()
                }
                _ => None,
            };
            let Some(template) = template else {
                continue;
            };
            let entry = analyse(&template);
            println!(
                "{}\n    {:?}",
                path.display(),
                entry.map(|entry| (
                    entry.thinking,
                    entry.switch,
                    entry.default_on,
                    entry.levels,
                    entry.note
                ))
            );
        }
    }
}

#[test]
#[ignore = "debugging aid: renders PROCYON_TEMPLATE once and prints the error"]
fn render_one() {
    let Some(path) = std::env::var_os("PROCYON_TEMPLATE") else {
        return;
    };
    let template = std::fs::read_to_string(path).unwrap();
    let env = environment();
    match render(&env, &template, None, Value::UNDEFINED) {
        Ok(text) => println!("OK: {}", text.chars().take(400).collect::<String>()),
        Err(error) => println!("ERR: {error:#}"),
    }
}
