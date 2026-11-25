//! Thinking controls learned from the chat templates servers on this machine use (task 0362), kept
//! under the settings data root so a later session does not analyse an unchanged template again.
//! One entry per endpoint and model, with the template's hash: a changed template is analysed anew.

use std::sync::Mutex;

use chrono::{SecondsFormat, Utc};
use fm_settings::{SettingsStore, VersionedDocument};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

use super::ModelThinking;

const SCHEMA_VERSION: u32 = 1;
/// Most entries kept; the oldest are dropped beyond this.
const MAX_ENTRIES: usize = 200;

/// The learned list could not be read.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("the learned model thinking list is not readable")]
pub struct LearnedError;

/// What was learned for one model on one endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LearnedEntry {
    /// The endpoint's normalised base URL.
    pub endpoint: String,
    /// The model id the endpoint serves.
    pub model: String,
    /// SHA-256 of the template, hex.
    pub template_sha256: String,
    /// When it was analysed, RFC 3339.
    pub learned_at: String,
    /// What the template shows.
    pub thinking: ModelThinking,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LearnedDocument {
    schema_version: u32,
    #[serde(default)]
    entries: Vec<LearnedEntry>,
}

impl VersionedDocument for LearnedDocument {
    type MigrationError = LearnedError;

    const FILE_NAME: &'static str = "model-thinking-learned.json";
    const CURRENT_SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn migrate(value: Value, version: u32) -> Result<Value, Self::MigrationError> {
        match version {
            SCHEMA_VERSION => Ok(value),
            _ => Err(LearnedError),
        }
    }

    fn validate(&self) -> Result<(), Self::MigrationError> {
        // Only switches and efforts a request may carry; anything else is not loaded.
        let valid = self.entries.iter().all(|entry| {
            entry
                .thinking
                .switch
                .as_deref()
                .is_none_or(|name| super::known_switch(name).is_some())
                && entry
                    .thinking
                    .levels
                    .values()
                    .all(|value| super::known_effort(value).is_some())
        });
        if valid { Ok(()) } else { Err(LearnedError) }
    }
}

/// SHA-256 of a template, hex.
pub fn template_hash(template: &str) -> String {
    Sha256::digest(template.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The learned list. A list that cannot be read starts empty and is not saved over.
pub struct LearnedThinking {
    store: SettingsStore,
    entries: Mutex<Vec<LearnedEntry>>,
    writable: bool,
}

impl LearnedThinking {
    /// Loads the list kept in `store`.
    pub fn new(store: SettingsStore) -> Self {
        let (entries, writable) = match store.load_document::<LearnedDocument>() {
            Ok(document) => (
                document
                    .map(|document| document.entries)
                    .unwrap_or_default(),
                true,
            ),
            Err(_) => (Vec::new(), false),
        };
        Self {
            store,
            entries: Mutex::new(entries),
            writable,
        }
    }

    /// What was learned for `model` on `endpoint`.
    pub fn get(&self, endpoint: &str, model: &str) -> Option<LearnedEntry> {
        self.lock()
            .iter()
            .find(|entry| entry.endpoint == endpoint && entry.model == model)
            .cloned()
    }

    /// Records what `template` (hashed) shows for `model` on `endpoint`, replacing an earlier
    /// entry. A failed save keeps it for this session only.
    pub fn put(&self, endpoint: &str, model: &str, template: &str, mut thinking: ModelThinking) {
        thinking.matches = vec![model.to_owned()];
        let entry = LearnedEntry {
            endpoint: endpoint.to_owned(),
            model: model.to_owned(),
            template_sha256: template_hash(template),
            learned_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
            thinking,
        };
        let document = {
            let mut entries = self.lock();
            entries.retain(|existing| !(existing.endpoint == endpoint && existing.model == model));
            entries.push(entry);
            let excess = entries.len().saturating_sub(MAX_ENTRIES);
            entries.drain(..excess);
            LearnedDocument {
                schema_version: SCHEMA_VERSION,
                entries: entries.clone(),
            }
        };
        if self.writable {
            let _ = self.store.save_document(&document);
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<LearnedEntry>> {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::model_thinking::{LevelName, ThinkingKind};

    fn thinking(high: &str) -> ModelThinking {
        ModelThinking {
            matches: Vec::new(),
            source: "the chat template served with the model".into(),
            thinking: ThinkingKind::Toggle,
            switch: Some("enable_thinking".into()),
            default_on: true,
            levels: BTreeMap::from([(LevelName::High, high.to_owned())]),
            note: None,
            family: false,
            tested: Vec::new(),
        }
    }

    #[test]
    fn an_entry_survives_a_restart_and_is_replaced_when_learned_again() {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = "http://127.0.0.1:8000";
        let learned = LearnedThinking::new(SettingsStore::new(dir.path()));
        learned.put(endpoint, "dirk", "template one", thinking("high"));
        let again = LearnedThinking::new(SettingsStore::new(dir.path()));
        let entry = again.get(endpoint, "dirk").unwrap();
        assert_eq!(entry.template_sha256, template_hash("template one"));
        assert_eq!(entry.thinking.matches, ["dirk"]);
        assert!(again.get("http://other", "dirk").is_none());

        again.put(endpoint, "dirk", "template two", thinking("xhigh"));
        let entry = again.get(endpoint, "dirk").unwrap();
        assert_eq!(entry.template_sha256, template_hash("template two"));
        assert_eq!(entry.thinking.level(LevelName::High), Some("xhigh"));
    }

    #[test]
    fn a_list_with_an_unknown_parameter_is_not_loaded_or_saved_over() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("model-thinking-learned.json");
        let mut entry = serde_json::to_value(LearnedEntry {
            endpoint: "e".into(),
            model: "m".into(),
            template_sha256: "x".into(),
            learned_at: "2026-11-25T00:00:00Z".into(),
            thinking: thinking("high"),
        })
        .unwrap();
        entry["thinking"]["switch"] = "temperature".into();
        let text = serde_json::json!({"schemaVersion": 1, "entries": [entry]}).to_string();
        std::fs::write(&path, &text).unwrap();
        let learned = LearnedThinking::new(SettingsStore::new(dir.path()));
        assert!(learned.get("e", "m").is_none());
        learned.put("e", "other", "t", thinking("high"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    }
}
