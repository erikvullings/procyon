//! Transport types for the image pane's prompt history.
//!
//! Every prompt the person generated from is kept, with the settings that went with it, so one
//! click on the pane's `Image > Prompts` list restores the whole form.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::image_generation::ImageQualityDto;

/// One remembered prompt and the settings it was used with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImagePromptDto {
    /// Identity of this entry.
    pub id: Uuid,
    /// Short name for the list. Blank on a save asks the backend to derive one from the subject.
    #[serde(default)]
    #[schema(required = true)]
    pub title: String,
    /// RFC 3339 time of the last use; assigned by the backend, ignored on a save.
    #[serde(default)]
    #[schema(required = true)]
    pub updated_at: String,
    /// What to draw.
    pub subject: String,
    /// What to avoid, on top of what the chosen presets exclude.
    #[serde(default)]
    #[schema(required = true)]
    pub negative_prompt: String,
    /// Chosen style preset ids.
    #[serde(default)]
    #[schema(required = true)]
    pub preset_ids: Vec<String>,
    /// Chosen lens id; empty when none.
    #[serde(default)]
    #[schema(required = true)]
    pub lens_id: String,
    /// Chosen aperture id; empty when none.
    #[serde(default)]
    #[schema(required = true)]
    pub aperture_id: String,
    /// Requested width in pixels.
    pub width: u32,
    /// Requested height in pixels.
    pub height: u32,
    /// How many images one batch produces.
    pub count: u32,
    /// Reproducibility seed, when one was set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    /// Quality level, when one was chosen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<ImageQualityDto>,
    /// Sampler step count, when one was chosen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steps: Option<u32>,
}
