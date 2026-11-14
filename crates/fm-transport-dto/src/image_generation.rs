//! Transport types for the image generation pane (task 0325).
//!
//! A request carries the prompt and its bounds; the reply names the generated images without
//! carrying their bytes. Those stay in a short-lived backend store and are fetched by id, so no
//! image ever travels as JSON.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::location::LocationDto;

/// Asks the image role's endpoint for images. `request_id` is the handle used to cancel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "requestId": "5b1b6b1e-9b1b-4b1b-8b1b-1b1b1b1b1b1b",
    "prompt": "a lighthouse at dusk, watercolour",
    "negativePrompt": "text, watermark",
    "width": 1024,
    "height": 1024,
    "count": 1,
    "seed": 42,
    "quality": "good",
    "steps": 30
}))]
pub struct GenerateImagesRequestDto {
    /// Identity used to cancel this request.
    pub request_id: Uuid,
    /// Workspace the pane belongs to; used for logging and event audience only.
    pub workspace_id: Uuid,
    /// What to draw.
    pub prompt: String,
    /// What to avoid; sent only when the endpoint accepts a negative prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub negative_prompt: Option<String>,
    /// Requested width in pixels.
    pub width: u32,
    /// Requested height in pixels.
    pub height: u32,
    /// How many images to produce in one batch.
    pub count: u32,
    /// Reproducibility seed; absent lets the provider choose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    /// How hard the endpoint should work; each contract words these in its own vocabulary, and a
    /// contract without such a knob leaves it out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<ImageQualityDto>,
    /// Sampler step count; absent sends no step count at all, since not every endpoint has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steps: Option<u32>,
    /// A picture already on disk the new one should start from. The backend reads its bytes; the
    /// person never uploads them, and only contracts that can take one are allowed to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ImageSourceDto>,
}

/// How hard the endpoint should work at the picture, in the person's words rather than the
/// provider's: `fast` to `superQuality`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum ImageQualityDto {
    /// Quick look; the cheapest call.
    Fast,
    /// A solid picture without spending much.
    Good,
    /// The setting to ask for when the picture matters.
    Quality,
    /// The best the endpoint can do.
    SuperQuality,
}

/// A file on disk the generation should start from, named the way a save names its destination.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImageSourceDto {
    /// The source file itself, not the folder holding it.
    pub location: LocationDto,
}

/// One generated image, as the preview pane needs it. Bytes are never part of this DTO.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedImageDto {
    /// Identity of the stored image; the host fetches its preview bytes with it.
    pub id: Uuid,
    /// Media type of the stored original, e.g. `image/png`.
    pub media_type: String,
    /// Width of the stored original in pixels.
    pub width: u32,
    /// Height of the stored original in pixels.
    pub height: u32,
    /// Size of the stored original in bytes.
    pub bytes: u64,
    /// Whether the preview bytes are a downscale of the original rather than the original itself.
    pub downscaled: bool,
    /// Whether the file name suggested for saving carries this extension.
    pub extension: String,
}

/// A batch of images the person has to review, save or discard. Nothing was written to disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImageBatchDto {
    /// Identity of the batch; needed to save or discard its images.
    pub generation_id: Uuid,
    /// Model the image role resolved to.
    pub model: String,
    /// Prompt the batch was generated from, so "regenerate" can reuse it.
    pub prompt: String,
    /// Negative prompt the batch was generated from, when one was given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub negative_prompt: Option<String>,
    /// Seed the batch was generated with, when the person supplied one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    /// Requested width and height in pixels.
    pub width: u32,
    /// Requested height in pixels.
    pub height: u32,
    /// Quality the batch asked for, when one was asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<ImageQualityDto>,
    /// Step count the batch asked for, when one was asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steps: Option<u32>,
    /// The file the batch started from, when it started from one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ImageSourceDto>,
    /// Images in request order.
    pub images: Vec<GeneratedImageDto>,
}

/// Saves one stored image to disk. Nothing is written before this request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SaveGeneratedImageRequestDto {
    /// Batch the image came from.
    pub generation_id: Uuid,
    /// Image to save.
    pub image_id: Uuid,
    /// Chosen folder and file name.
    pub destination: LocationDto,
    /// Whether the person explicitly agreed to replace an existing file.
    #[serde(default)]
    pub overwrite: bool,
}

/// What a save wrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SaveGeneratedImageResponseDto {
    /// Where the file landed.
    pub location: LocationDto,
    /// Bytes written.
    pub bytes_written: u64,
    /// Whether the prompt was written into the file's metadata; false when the format does not
    /// support it.
    pub prompt_embedded: bool,
}

/// Requests cancellation of one running generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CancelImageGenerationRequestDto {
    /// Identity supplied when the batch was requested.
    pub request_id: Uuid,
}

/// Drops a batch from the short-lived backend store without saving it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiscardImageBatchRequestDto {
    /// Batch to drop.
    pub generation_id: Uuid,
}

/// Bytes of one stored image, for a host whose commands carry JSON (the Tauri IPC host). The HTTP
/// host serves the same bytes from a route instead.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedImageBytesDto {
    /// The image bytes: the preview downscale, or the stored original.
    pub bytes: Vec<u8>,
    /// Their media type, e.g. `image/png`.
    pub media_type: String,
}
