//! What model families support for thinking (task 0361), so a request asks for an effort in the
//! form the model's chat template understands instead of finding it by trial. The list is
//! maintained in `model_thinking.json`, read from each model's chat template and card; a model
//! that is not listed is asked by trial, as before.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Deserialize;

/// Whether and how a model thinks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThinkingKind {
    /// The model never thinks.
    None,
    /// A chat-template switch turns thinking on or off.
    Toggle,
    /// The model always thinks; at most its effort can be lowered.
    Always,
}

/// An effort a model may map to a `reasoning_effort` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LevelName {
    /// Thinking off, or as little as the model allows.
    Off,
    /// A little thinking.
    Low,
    /// Moderate thinking.
    Medium,
    /// Thorough thinking.
    High,
}

/// One model family's thinking controls.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelThinking {
    /// Texts of which one occurs in the model id (case-insensitive).
    #[serde(rename = "match")]
    pub matches: Vec<String>,
    /// The Hugging Face repository the entry was read from.
    pub source: String,
    /// Whether and how the model thinks.
    pub thinking: ThinkingKind,
    /// The `chat_template_kwargs` name of the toggle.
    #[serde(default)]
    pub switch: Option<String>,
    /// Whether the model thinks when nothing is asked.
    pub default_on: bool,
    /// The `reasoning_effort` value per effort; a missing effort sends none.
    #[serde(default)]
    pub levels: BTreeMap<LevelName, String>,
    /// What else the card says about thinking.
    #[serde(default)]
    pub note: Option<String>,
    /// A fallback for a whole family: its switch is shared, its levels are not verified.
    #[serde(default)]
    pub family: bool,
    /// Where and how the entry was checked against a running server.
    #[serde(default)]
    pub tested: Vec<String>,
}

impl ModelThinking {
    /// The `reasoning_effort` value for `level`, when the model has one.
    pub fn level(&self, level: LevelName) -> Option<&str> {
        self.levels.get(&level).map(String::as_str)
    }
}

#[derive(Debug, Deserialize)]
struct Catalogue {
    models: Vec<ModelThinking>,
}

fn catalogue() -> &'static [ModelThinking] {
    static CATALOGUE: OnceLock<Vec<ModelThinking>> = OnceLock::new();
    CATALOGUE.get_or_init(|| {
        // Checked by `the_catalogue_is_valid`, so a broken file fails the tests, not the app.
        serde_json::from_str::<Catalogue>(include_str!("model_thinking.json"))
            .map(|catalogue| catalogue.models)
            .unwrap_or_default()
    })
}

/// The thinking controls of `model`, when its family is listed.
pub fn lookup(model: &str) -> Option<&'static ModelThinking> {
    let model = model.to_ascii_lowercase();
    catalogue().iter().find(|entry| {
        entry
            .matches
            .iter()
            .any(|text| model.contains(&text.to_ascii_lowercase()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalogue_is_valid() {
        let models = serde_json::from_str::<Catalogue>(include_str!("model_thinking.json"))
            .unwrap()
            .models;
        assert!(!models.is_empty());
        for entry in &models {
            assert!(!entry.matches.is_empty(), "{}", entry.source);
            assert_eq!(
                entry.switch.is_some(),
                entry.thinking == ThinkingKind::Toggle,
                "a toggle names its switch, and only a toggle: {}",
                entry.source
            );
            assert!(
                entry.thinking != ThinkingKind::None || entry.levels.is_empty(),
                "a model that does not think has no levels: {}",
                entry.source
            );
        }
    }

    #[test]
    fn models_in_use_find_their_family() {
        let cases = [
            ("Inkling-2-Mini-oQ4e-mtp", "thinkingmachines/Inkling-2-Mini"),
            ("ThinkingCap-Qwen3.8-27B-MLX-4bit-DWQ", "Qwen/Qwen3.8-27B"),
            ("Dirk-Qwen3.8-27B-MLX-oQ4e", "Qwen/Qwen3.8-27B"),
            ("Qwen3.8-Flash-Next-oQ4e-mtp", "Qwen/Qwen3.8-27B"),
            (
                "gemma-4-26B-A4B-it-QAT-MLX-4bit",
                "google/gemma-4-26B-A4B-it",
            ),
            (
                "ornith-1.5-122b-mlx-mixed-4.46bpw",
                "ornith-ai/Ornith-1.5-122B",
            ),
            ("granite4.2:8b", "ibm-granite/granite-4.2-8b"),
            ("qwen3:14b", "Qwen/Qwen3-14B"),
            ("llama3.2:3b", "meta-llama/Llama-3.2-3B-Instruct"),
            ("kimi-k2.5:cloud", "moonshotai/Kimi-K2.5"),
            ("minimax-m2.7:cloud", "MiniMaxAI/MiniMax-M2.7"),
            ("deepseek-v4-flash:cloud", "deepseek-ai/DeepSeek-V4-Pro"),
        ];
        for (model, source) in cases {
            assert_eq!(
                lookup(model).map(|entry| entry.source.as_str()),
                Some(source),
                "{model}"
            );
        }
        assert!(lookup("some-unknown-model").is_none());
        // A model of a listed family without its own entry falls back to the family.
        let family = lookup("Qwen3.6-35B-A3B-MLX").unwrap();
        assert!(family.family);
        assert_eq!(family.switch.as_deref(), Some("enable_thinking"));
        assert!(!lookup("Qwen3.8-27B").unwrap().family);
        // Qwen3.8 refuses `high`; its highest level is `xhigh`.
        assert_eq!(
            lookup("Qwen3.8-27B").unwrap().level(LevelName::High),
            Some("xhigh")
        );
    }
}
