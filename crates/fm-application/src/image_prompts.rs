//! The image pane's prompt history. Every prompt generated from is kept, with the settings it was
//! used with, in one versioned document, `image-prompts/image-prompts.json` under the settings data
//! root. Unlike chat conversations there is no file per entry: a prompt is a handful of fields, and
//! one file keeps the whole history readable and portable.

use std::fs;
use std::sync::Mutex;

use chrono::{SecondsFormat, Utc};
use fm_settings::{SettingsStore, VersionedDocument};
use fm_transport_dto::{ImagePromptDto, ImageQualityDto};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

use crate::error::ApplicationError;

const DOCUMENT_SCHEMA_VERSION: u32 = 1;
const STORE_DIRECTORY: &str = "image-prompts";
/// Longest title, in characters.
const MAX_TITLE_CHARS: usize = 80;
/// Longest subject and negative prompt; the same bounds the generation request enforces.
const MAX_SUBJECT_CHARS: usize = 4_000;
const MAX_NEGATIVE_CHARS: usize = 2_000;
/// How many prompts are kept before the least recently used one falls off.
const MAX_PROMPTS: usize = 500;
/// Most presets one prompt can carry, and the longest id of each.
const MAX_PRESETS: usize = 16;
const MAX_ID_CHARS: usize = 64;

/// Failure of a prompt-history operation.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(crate) enum ImagePromptError {
    /// No prompt with this id exists.
    #[error("image prompt not found")]
    NotFound,
    /// The prompt violates a bounded contract.
    #[error("invalid image prompt: {0}")]
    Invalid(&'static str),
    /// The history could not be read or written.
    #[error("image prompts could not be saved")]
    Persistence,
}

impl From<ImagePromptError> for ApplicationError {
    fn from(error: ImagePromptError) -> Self {
        match error {
            ImagePromptError::NotFound => Self::NotFound,
            ImagePromptError::Invalid(message) => Self::InvalidRequest(message.to_owned()),
            ImagePromptError::Persistence => Self::Internal,
        }
    }
}

/// One remembered prompt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImagePrompt {
    pub id: Uuid,
    pub title: String,
    /// RFC 3339 with millisecond precision, so text order matches time order.
    pub updated_at: String,
    pub subject: String,
    #[serde(default)]
    pub negative_prompt: String,
    #[serde(default)]
    pub preset_ids: Vec<String>,
    #[serde(default)]
    pub lens_id: String,
    #[serde(default)]
    pub aperture_id: String,
    pub width: u32,
    pub height: u32,
    pub count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<ImageQualityDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steps: Option<u32>,
}

impl ImagePrompt {
    /// Whether two prompts would produce the same request: everything but identity, title and time.
    fn same_content(&self, other: &Self) -> bool {
        self.subject == other.subject
            && self.negative_prompt == other.negative_prompt
            && self.preset_ids == other.preset_ids
            && self.lens_id == other.lens_id
            && self.aperture_id == other.aperture_id
            && self.width == other.width
            && self.height == other.height
            && self.count == other.count
            && self.seed == other.seed
            && self.quality == other.quality
            && self.steps == other.steps
    }
}

/// The versioned file that holds the whole history.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PromptsDocument {
    schema_version: u32,
    #[serde(default)]
    prompts: Vec<ImagePrompt>,
}

impl VersionedDocument for PromptsDocument {
    type MigrationError = ImagePromptError;

    const FILE_NAME: &'static str = "image-prompts.json";
    const CURRENT_SCHEMA_VERSION: u32 = DOCUMENT_SCHEMA_VERSION;

    fn migrate(value: Value, version: u32) -> Result<Value, Self::MigrationError> {
        match version {
            DOCUMENT_SCHEMA_VERSION => Ok(value),
            _ => Err(ImagePromptError::Persistence),
        }
    }

    fn validate(&self) -> Result<(), Self::MigrationError> {
        Ok(())
    }
}

/// Thread-safe prompt history, held in memory and rewritten as one file on every change.
pub(crate) struct ImagePromptService {
    store: SettingsStore,
    prompts: Mutex<Vec<ImagePrompt>>,
    /// False when the file could not be read. Nothing is written then, so a failed read can never
    /// lead to a save over history that is still on disk.
    writable: bool,
}

impl ImagePromptService {
    /// Loads the history from the settings data root; a missing file is an empty history.
    pub(crate) fn new(settings: &SettingsStore) -> Result<Self, ImagePromptError> {
        let directory = settings.directory().join(STORE_DIRECTORY);
        fs::create_dir_all(&directory).map_err(|_| ImagePromptError::Persistence)?;
        let store = SettingsStore::new(directory);
        let document = store
            .load_document::<PromptsDocument>()
            .map_err(|_| ImagePromptError::Persistence)?;
        Ok(Self {
            store,
            prompts: Mutex::new(
                document
                    .map(|document| document.prompts)
                    .unwrap_or_default(),
            ),
            writable: true,
        })
    }

    /// A read-only empty history, used when the file could not be read.
    pub(crate) fn unavailable(settings: &SettingsStore) -> Self {
        Self {
            store: SettingsStore::new(settings.directory().join(STORE_DIRECTORY)),
            prompts: Mutex::new(Vec::new()),
            writable: false,
        }
    }

    /// Lists every prompt, most recently used first.
    pub(crate) fn list(&self) -> Result<Vec<ImagePrompt>, ImagePromptError> {
        let mut prompts = self.lock()?.clone();
        prompts.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(prompts)
    }

    /// Remembers a prompt. One that matches an earlier prompt in everything but identity only
    /// refreshes that entry's time, so generating twice from the same form leaves one entry.
    pub(crate) fn save(&self, mut prompt: ImagePrompt) -> Result<ImagePrompt, ImagePromptError> {
        validate(&mut prompt)?;
        self.require_writable()?;
        let mut prompts = self.lock()?;
        let mut next = prompts.clone();
        let stamp = now();
        let saved = if let Some(existing) = next
            .iter_mut()
            .find(|existing| existing.id == prompt.id || existing.same_content(&prompt))
        {
            let title = if prompt.title.is_empty() {
                existing.title.clone()
            } else {
                prompt.title.clone()
            };
            let id = existing.id;
            *existing = ImagePrompt {
                id,
                title,
                updated_at: stamp,
                ..prompt
            };
            existing.clone()
        } else {
            prompt.updated_at = stamp;
            next.push(prompt.clone());
            prompt
        };
        if next.len() > MAX_PROMPTS {
            next.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
            next.truncate(MAX_PROMPTS);
        }
        self.persist(&next)?;
        *prompts = next;
        Ok(saved)
    }

    /// Forgets one prompt.
    pub(crate) fn delete(&self, id: Uuid) -> Result<(), ImagePromptError> {
        self.require_writable()?;
        let mut prompts = self.lock()?;
        if !prompts.iter().any(|prompt| prompt.id == id) {
            return Err(ImagePromptError::NotFound);
        }
        let next: Vec<ImagePrompt> = prompts
            .iter()
            .filter(|prompt| prompt.id != id)
            .cloned()
            .collect();
        self.persist(&next)?;
        *prompts = next;
        Ok(())
    }

    fn persist(&self, prompts: &[ImagePrompt]) -> Result<(), ImagePromptError> {
        self.store
            .save_document(&PromptsDocument {
                schema_version: DOCUMENT_SCHEMA_VERSION,
                prompts: prompts.to_vec(),
            })
            .map_err(|_| ImagePromptError::Persistence)
    }

    fn require_writable(&self) -> Result<(), ImagePromptError> {
        if self.writable {
            Ok(())
        } else {
            Err(ImagePromptError::Persistence)
        }
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Vec<ImagePrompt>>, ImagePromptError> {
        self.prompts
            .lock()
            .map_err(|_| ImagePromptError::Persistence)
    }
}

/// Trims and bounds a prompt, and gives it a title when it has none.
fn validate(prompt: &mut ImagePrompt) -> Result<(), ImagePromptError> {
    prompt.subject = prompt.subject.trim().to_owned();
    prompt.negative_prompt = prompt.negative_prompt.trim().to_owned();
    if prompt.subject.is_empty() {
        return Err(ImagePromptError::Invalid("the subject is empty"));
    }
    if prompt.subject.chars().count() > MAX_SUBJECT_CHARS {
        return Err(ImagePromptError::Invalid("the subject is too long"));
    }
    if prompt.negative_prompt.chars().count() > MAX_NEGATIVE_CHARS {
        return Err(ImagePromptError::Invalid("the negative prompt is too long"));
    }
    if prompt.preset_ids.len() > MAX_PRESETS
        || prompt
            .preset_ids
            .iter()
            .chain([&prompt.lens_id, &prompt.aperture_id])
            .any(|id| id.chars().count() > MAX_ID_CHARS)
    {
        return Err(ImagePromptError::Invalid("a style choice is not valid"));
    }
    prompt.title = prompt.title.trim().chars().take(MAX_TITLE_CHARS).collect();
    if prompt.title.is_empty() {
        prompt.title = derive_title(&prompt.subject);
    }
    Ok(())
}

/// A cheap, local title: the subject's first clause, cut at a word boundary. No model is asked,
/// so saving a prompt never costs a request and never leaves the machine.
pub(crate) fn derive_title(subject: &str) -> String {
    let first_line = subject.lines().find(|line| !line.trim().is_empty());
    let line = first_line.unwrap_or_default().trim();
    let clause = line
        .split(['.', ',', ';', ':', '!', '?'])
        .map(str::trim)
        .find(|clause| !clause.is_empty())
        .unwrap_or(line);
    if clause.chars().count() <= MAX_TITLE_CHARS {
        return clause.to_owned();
    }
    let mut title = String::new();
    for word in clause.split_whitespace() {
        let next = title.chars().count() + usize::from(!title.is_empty()) + word.chars().count();
        if next > MAX_TITLE_CHARS - 1 {
            break;
        }
        if !title.is_empty() {
            title.push(' ');
        }
        title.push_str(word);
    }
    if title.is_empty() {
        title = clause.chars().take(MAX_TITLE_CHARS - 1).collect();
    }
    title.push('…');
    title
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub(crate) fn to_dto(prompt: ImagePrompt) -> ImagePromptDto {
    ImagePromptDto {
        id: prompt.id,
        title: prompt.title,
        updated_at: prompt.updated_at,
        subject: prompt.subject,
        negative_prompt: prompt.negative_prompt,
        preset_ids: prompt.preset_ids,
        lens_id: prompt.lens_id,
        aperture_id: prompt.aperture_id,
        width: prompt.width,
        height: prompt.height,
        count: prompt.count,
        seed: prompt.seed,
        quality: prompt.quality,
        steps: prompt.steps,
    }
}

pub(crate) fn from_dto(dto: ImagePromptDto) -> ImagePrompt {
    ImagePrompt {
        id: dto.id,
        title: dto.title,
        updated_at: dto.updated_at,
        subject: dto.subject,
        negative_prompt: dto.negative_prompt,
        preset_ids: dto.preset_ids,
        lens_id: dto.lens_id,
        aperture_id: dto.aperture_id,
        width: dto.width,
        height: dto.height,
        count: dto.count,
        seed: dto.seed,
        quality: dto.quality,
        steps: dto.steps,
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    fn prompt(subject: &str) -> ImagePrompt {
        ImagePrompt {
            id: Uuid::new_v4(),
            title: String::new(),
            updated_at: String::new(),
            subject: subject.to_owned(),
            negative_prompt: String::new(),
            preset_ids: vec!["oil".to_owned()],
            lens_id: String::new(),
            aperture_id: String::new(),
            width: 1_024,
            height: 1_024,
            count: 1,
            seed: Some(7),
            quality: Some(ImageQualityDto::Good),
            steps: None,
        }
    }

    fn service(directory: &std::path::Path) -> ImagePromptService {
        ImagePromptService::new(&SettingsStore::new(directory)).unwrap()
    }

    #[test]
    fn saving_derives_a_title_and_keeps_every_setting() {
        let directory = tempdir().unwrap();
        let saved = service(directory.path())
            .save(prompt("A lighthouse at dusk, watercolour"))
            .unwrap();
        assert_eq!(saved.title, "A lighthouse at dusk");
        assert_eq!(saved.seed, Some(7));
        assert_eq!(saved.quality, Some(ImageQualityDto::Good));
        assert_eq!(saved.preset_ids, vec!["oil".to_owned()]);
    }

    #[test]
    fn history_survives_a_restart_in_a_single_file() {
        let directory = tempdir().unwrap();
        let first = service(directory.path());
        first.save(prompt("one")).unwrap();
        first.save(prompt("two")).unwrap();
        let reloaded = service(directory.path()).list().unwrap();
        assert_eq!(reloaded.len(), 2);
        let files: Vec<_> = fs::read_dir(directory.path().join(STORE_DIRECTORY))
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(files.len(), 1, "one file holds every prompt");
    }

    #[test]
    fn the_same_form_twice_leaves_one_entry_and_refreshes_its_time() {
        let directory = tempdir().unwrap();
        let service = service(directory.path());
        let first = service.save(prompt("a fox")).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let second = service.save(prompt("a fox")).unwrap();
        assert_eq!(service.list().unwrap().len(), 1);
        assert_eq!(second.id, first.id);
        assert!(second.updated_at > first.updated_at);
    }

    #[test]
    fn a_changed_setting_is_a_new_entry_and_the_newest_comes_first() {
        let directory = tempdir().unwrap();
        let service = service(directory.path());
        service.save(prompt("a fox")).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let mut other = prompt("a fox");
        other.seed = Some(8);
        let newest = service.save(other).unwrap();
        let listed = service.list().unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id, newest.id);
    }

    #[test]
    fn delete_removes_and_reports_a_missing_entry() {
        let directory = tempdir().unwrap();
        let service = service(directory.path());
        let saved = service.save(prompt("a fox")).unwrap();
        service.delete(saved.id).unwrap();
        assert!(service.list().unwrap().is_empty());
        assert_eq!(service.delete(saved.id), Err(ImagePromptError::NotFound));
    }

    #[test]
    fn an_empty_or_oversized_subject_is_refused() {
        let directory = tempdir().unwrap();
        let service = service(directory.path());
        assert!(service.save(prompt("   ")).is_err());
        assert!(
            service
                .save(prompt(&"x".repeat(MAX_SUBJECT_CHARS + 1)))
                .is_err()
        );
    }

    #[test]
    fn long_subjects_get_a_word_boundary_title() {
        let title = derive_title(&"lighthouse ".repeat(20));
        assert!(title.ends_with('…'));
        assert!(title.chars().count() <= MAX_TITLE_CHARS);
        assert!(!title.trim_end_matches('…').ends_with(' '));
    }

    #[test]
    fn a_failed_read_leaves_the_history_untouched() {
        let directory = tempdir().unwrap();
        let service = ImagePromptService::unavailable(&SettingsStore::new(directory.path()));
        assert_eq!(
            service.save(prompt("a fox")),
            Err(ImagePromptError::Persistence)
        );
    }
}
