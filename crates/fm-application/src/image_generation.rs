//! Image generation (task 0325): a prompt, a bounded batch of images, and a save step.
//!
//! The person reviews the batch before anything reaches disk. Bytes stay in this module's
//! short-lived store, keyed by batch and image id; hosts fetch preview bytes by id and never
//! receive an image as JSON. Saving is the only write, and it refuses an existing destination
//! unless the person explicitly asked to replace it.

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use fm_transport_dto::{
    CancelImageGenerationRequestDto, GenerateImagesRequestDto, GeneratedImageDto, ImageBatchDto,
    ImageQualityDto, SaveGeneratedImageRequestDto, SaveGeneratedImageResponseDto,
};
use fm_vfs::{ProviderCapabilities, ProviderRegistry, WriteOptions};
use serde_json::{Value, json};
use thiserror::Error;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::llm_profiles::LlmProfileError;

/// Longest prompt the person may send.
const MAX_PROMPT_CHARS: usize = 4_000;
/// Longest negative prompt the person may send.
const MAX_NEGATIVE_CHARS: usize = 2_000;
/// Images produced by one request, at most.
pub const MAX_IMAGES: u32 = 4;
/// Smallest edge a request may name, in pixels; both edges are multiples of 8.
pub const MIN_EDGE: u32 = 64;
/// Largest edge a request may name, in pixels.
pub const MAX_EDGE: u32 = 2_048;
/// Largest accepted pixel area of one request: two megapixels.
pub const MAX_PIXELS: u64 = 2_097_152;
/// Largest seed the person may name.
pub const MAX_SEED: u64 = 4_294_967_295;
/// Largest sampler step count the person may name; above it a server is inventing detail anyway.
pub const MAX_STEPS: u32 = 64;
/// Largest file the person may ask a generation to start from.
const MAX_SOURCE_BYTES: u64 = 12 * 1024 * 1024;
/// Largest single image the provider may return.
pub(crate) const MAX_IMAGE_BYTES: usize = 24 * 1024 * 1024;
/// Largest batch the provider may return.
const MAX_BATCH_BYTES: usize = 64 * 1024 * 1024;
/// Longest edge kept for a preview; larger images are downscaled for the preview only.
const MAX_PREVIEW_EDGE: u32 = 1_024;
/// How long a batch is held before it is dropped.
const BATCH_TTL: Duration = Duration::from_secs(30 * 60);
/// Largest total size of the store; the oldest batch goes first.
const STORE_MAX_BYTES: usize = 128 * 1024 * 1024;
/// Shortest timeout allowed for one batch, whatever the profile's chat timeout says.
pub const MIN_IMAGE_TIMEOUT_SECONDS: u64 = 120;

/// Why a generation or a save did not produce a file.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ImageGenerationError {
    /// The prompt, size, count or seed violates a bounded contract.
    #[error("invalid image request: {0}")]
    InvalidRequest(&'static str),
    /// No endpoint is assigned to the image role.
    #[error("no image endpoint is assigned")]
    RoleNotAssigned,
    /// The image endpoint is a cloud host the person has not consented to. Consent is granted in
    /// Settings; the pane never prompts for it.
    #[error("the image endpoint needs consent in Settings")]
    ConsentRequired(String),
    /// The provider or the decode failed.
    #[error("image generation is unavailable")]
    Unavailable,
    /// An image was outside the accepted byte bounds.
    #[error("the generated image is too large")]
    TooLarge,
    /// The endpoint's contract has no way to take a picture to start from.
    #[error("this image endpoint cannot start from an existing picture")]
    SourceUnsupported,
    /// The file chosen to start from is not a picture the endpoints can read.
    #[error("that file cannot be used as a starting picture ({0})")]
    SourceUnusable(&'static str),
    /// The file chosen to start from is larger than a source picture may be.
    #[error("the picture to start from is too large")]
    SourceTooLarge,
    /// The batch is no longer held; the person has to generate again.
    #[error("the generated images expired")]
    Expired,
    /// The person stopped the request.
    #[error("the image generation was cancelled")]
    Cancelled,
}

impl From<LlmProfileError> for ImageGenerationError {
    fn from(error: LlmProfileError) -> Self {
        match error {
            LlmProfileError::RoleNotAssigned | LlmProfileError::NotFound => Self::RoleNotAssigned,
            LlmProfileError::ConsentRequired(host) => Self::ConsentRequired(host),
            LlmProfileError::Cancelled => Self::Cancelled,
            _ => Self::Unavailable,
        }
    }
}

/// How hard the endpoint should work at a picture, in the person's words rather than a provider's:
/// the pane offers four levels and each contract words them as it likes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageQuality {
    /// A quick look, and the cheapest call.
    Fast,
    /// A solid picture without spending much.
    Good,
    /// The level to ask for when the picture matters.
    Quality,
    /// The best the endpoint can do.
    SuperQuality,
}

impl ImageQuality {
    /// The word the OpenAI-style contract takes. That API's own scale is `low|medium|high`, so the
    /// top level asks for `high`: there is nothing above it to ask for.
    #[must_use]
    pub const fn openai_wire(self) -> &'static str {
        match self {
            Self::Fast => "low",
            Self::Good => "medium",
            Self::Quality => "high",
            Self::SuperQuality => "high",
        }
    }

    /// The word written into a saved file's metadata: the level the person chose, not the word one
    /// provider happens to use for it.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Fast => "fast",
            Self::Good => "good",
            Self::Quality => "quality",
            Self::SuperQuality => "super quality",
        }
    }
}

impl From<ImageQualityDto> for ImageQuality {
    fn from(quality: ImageQualityDto) -> Self {
        match quality {
            ImageQualityDto::Fast => Self::Fast,
            ImageQualityDto::Good => Self::Good,
            ImageQualityDto::Quality => Self::Quality,
            ImageQualityDto::SuperQuality => Self::SuperQuality,
        }
    }
}

impl From<ImageQuality> for ImageQualityDto {
    fn from(quality: ImageQuality) -> Self {
        match quality {
            ImageQuality::Fast => Self::Fast,
            ImageQuality::Good => Self::Good,
            ImageQuality::Quality => Self::Quality,
            ImageQuality::SuperQuality => Self::SuperQuality,
        }
    }
}

/// The picture a generation starts from, read from disk and inside its bounds by the time a
/// provider ever sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceImage {
    /// The file's own name, so the file it produces can say where it came from.
    pub name: String,
    /// Its media type, e.g. `image/png`.
    pub media_type: &'static str,
    /// Its encoded bytes.
    pub bytes: Vec<u8>,
}

/// A request that passed the bounds, in the shape the provider call needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedImageRequest {
    /// What to draw.
    pub prompt: String,
    /// What to avoid; sent only when the person supplied one.
    pub negative_prompt: Option<String>,
    /// Requested width in pixels.
    pub width: u32,
    /// Requested height in pixels.
    pub height: u32,
    /// How many images to produce.
    pub count: u32,
    /// Reproducibility seed; `None` lets the provider choose.
    pub seed: Option<u64>,
    /// How hard the endpoint should work, when the person asked for a level.
    pub quality: Option<ImageQuality>,
    /// Sampler step count, when one was named. A contract without a sampler simply is not sent one.
    pub steps: Option<u32>,
    /// Where the picture to start from is; its bytes are read separately, so a validated request
    /// only carries the pointer.
    pub source: Option<fm_domain::Location>,
}

/// Checks the bounded contract of one request. Nothing reaches the provider unless this passes.
pub(crate) fn validate_request(
    request: &GenerateImagesRequestDto,
) -> Result<ValidatedImageRequest, ImageGenerationError> {
    let prompt = request.prompt.trim();
    if prompt.is_empty() {
        return Err(ImageGenerationError::InvalidRequest("the prompt is empty"));
    }
    if prompt.chars().count() > MAX_PROMPT_CHARS {
        return Err(ImageGenerationError::InvalidRequest(
            "the prompt is too long",
        ));
    }
    let negative_prompt = request
        .negative_prompt
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty());
    if negative_prompt.is_some_and(|text| text.chars().count() > MAX_NEGATIVE_CHARS) {
        return Err(ImageGenerationError::InvalidRequest(
            "the negative prompt is too long",
        ));
    }
    for edge in [request.width, request.height] {
        if !(MIN_EDGE..=MAX_EDGE).contains(&edge) || edge % 8 != 0 {
            return Err(ImageGenerationError::InvalidRequest(
                "the size is outside the accepted range",
            ));
        }
    }
    if u64::from(request.width) * u64::from(request.height) > MAX_PIXELS {
        return Err(ImageGenerationError::InvalidRequest(
            "the size asks for too many pixels",
        ));
    }
    if !(1..=MAX_IMAGES).contains(&request.count) {
        return Err(ImageGenerationError::InvalidRequest(
            "the number of images is outside the accepted range",
        ));
    }
    if request.seed.is_some_and(|seed| seed > MAX_SEED) {
        return Err(ImageGenerationError::InvalidRequest(
            "the seed is too large",
        ));
    }
    if request
        .steps
        .is_some_and(|steps| steps == 0 || steps > MAX_STEPS)
    {
        return Err(ImageGenerationError::InvalidRequest(
            "the step count is outside the accepted range",
        ));
    }
    let source = request
        .source
        .as_ref()
        .map(|source| fm_domain::Location::from(source.location.clone()));
    Ok(ValidatedImageRequest {
        prompt: prompt.to_owned(),
        negative_prompt: negative_prompt.map(str::to_owned),
        width: request.width,
        height: request.height,
        count: request.count,
        seed: request.seed,
        quality: request.quality.map(Into::into),
        steps: request.steps,
        source,
    })
}

/// Builds the OpenAI-style images request body. `model` comes from the image role.
pub(crate) fn images_request_body(model: &str, request: &ValidatedImageRequest) -> Value {
    let mut body = json!({
        "model": model,
        "prompt": request.prompt,
        "n": request.count,
        "size": format!("{}x{}", request.width, request.height),
        "response_format": "b64_json",
    });
    if let Some(negative) = &request.negative_prompt {
        body["negative_prompt"] = json!(negative);
    }
    if let Some(seed) = request.seed {
        body["seed"] = json!(seed);
    }
    if let Some(quality) = request.quality {
        body["quality"] = json!(quality.openai_wire());
    }
    if let Some(steps) = request.steps {
        body["steps"] = json!(steps);
    }
    body
}

/// The form fields of an OpenAI-style image *edit*: the same ask as a generation, sent beside the
/// picture to start from. `seed` and `response_format` are left out because an edits call answers
/// with base64 on its own and takes no seed.
#[must_use]
pub(crate) fn images_edit_fields(
    model: &str,
    request: &ValidatedImageRequest,
) -> Vec<(String, String)> {
    let mut fields = vec![
        ("model".to_owned(), model.to_owned()),
        ("prompt".to_owned(), request.prompt.clone()),
        ("n".to_owned(), request.count.to_string()),
        (
            "size".to_owned(),
            format!("{}x{}", request.width, request.height),
        ),
    ];
    if let Some(negative) = &request.negative_prompt {
        fields.push(("negative_prompt".to_owned(), negative.clone()));
    }
    if let Some(quality) = request.quality {
        fields.push(("quality".to_owned(), quality.openai_wire().to_owned()));
    }
    if let Some(steps) = request.steps {
        fields.push(("steps".to_owned(), steps.to_string()));
    }
    fields
}

/// The images one provider reply carries, as raw bytes in reply order.
pub(crate) fn parse_images_response(bytes: &[u8]) -> Result<Vec<Vec<u8>>, ImageGenerationError> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| ImageGenerationError::Unavailable)?;
    let items = value
        .get("data")
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty())
        .ok_or(ImageGenerationError::Unavailable)?;
    items.iter().map(image_of_item).collect()
}

/// One `data` entry of an images reply. A reply that points at a URL instead of carrying the bytes
/// is refused rather than fetched a second time behind the person's back.
fn image_of_item(item: &Value) -> Result<Vec<u8>, ImageGenerationError> {
    use base64::Engine as _;
    let encoded = item
        .get("b64_json")
        .and_then(Value::as_str)
        .ok_or(ImageGenerationError::Unavailable)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .map_err(|_| ImageGenerationError::Unavailable)?;
    if bytes.is_empty() || bytes.len() > MAX_IMAGE_BYTES {
        return Err(ImageGenerationError::TooLarge);
    }
    Ok(bytes)
}

/// What the generation capability produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedBatch {
    /// Model the image role resolved to.
    pub model: String,
    /// Image bytes in reply order, each an encoded image.
    pub images: Vec<Vec<u8>>,
}

/// The generation capability the pane needs: turn a validated request into image bytes. Implemented
/// by [`crate::llm_profiles::LlmProfileService`] through the image role; tests supply a fake.
#[async_trait]
pub trait ImageProvider: Send + Sync {
    /// Produces up to `request.count` images, or fails. `source` carries the bytes of the picture
    /// the request asked to start from, and only a contract that can take one is ever handed it.
    async fn generate(
        &self,
        request: &ValidatedImageRequest,
        source: Option<&SourceImage>,
        cancellation: &CancellationToken,
    ) -> Result<GeneratedBatch, ImageGenerationError>;
}

/// What the person asked a batch to contain; written into the saved file where the format allows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptRecord {
    /// What was drawn.
    pub prompt: String,
    /// What was avoided; the negative prompt.
    pub negative_prompt: Option<String>,
    /// Seed, when the person named one.
    pub seed: Option<u64>,
    /// Model the image role resolved to.
    pub model: String,
    /// Requested width in pixels.
    pub width: u32,
    /// Requested height in pixels.
    pub height: u32,
    /// How hard the endpoint worked, when a level was asked for.
    pub quality: Option<ImageQuality>,
    /// Sampler step count, when one was asked for.
    pub steps: Option<u32>,
    /// The file the picture started from, when it started from one.
    pub source: Option<String>,
}

impl PromptRecord {
    /// The block stored under the PNG `parameters` keyword, in the layout image tools already read.
    #[must_use]
    pub fn parameters(&self) -> String {
        let mut text = self.prompt.clone();
        if let Some(negative) = &self.negative_prompt {
            text.push_str("\nNegative prompt: ");
            text.push_str(negative);
        }
        if let Some(seed) = self.seed {
            text.push_str(&format!("\nSeed: {seed}"));
        }
        text.push_str(&format!(
            "\nSize: {}x{}\nModel: {}\nSoftware: Procyon",
            self.width, self.height, self.model
        ));
        if let Some(quality) = self.quality {
            text.push_str(&format!("\nQuality: {}", quality.label()));
        }
        if let Some(steps) = self.steps {
            text.push_str(&format!("\nSteps: {steps}"));
        }
        if let Some(source) = &self.source {
            text.push_str(&format!("\nSource: {source}"));
        }
        text
    }
}

/// One image held for review: what the provider returned, plus what the preview needs. The preview
/// is a separate encoding, so it carries its own media type: a WebP original previews as a PNG.
#[derive(Debug, Clone)]
struct StoredImage {
    bytes: Vec<u8>,
    preview: Vec<u8>,
    media_type: &'static str,
    preview_media_type: &'static str,
    extension: &'static str,
}

/// One reviewed batch, dropped when it expires or when the store needs room.
#[derive(Debug, Clone)]
struct Batch {
    created: Instant,
    model: String,
    prompt: String,
    negative_prompt: Option<String>,
    seed: Option<u64>,
    width: u32,
    height: u32,
    quality: Option<ImageQuality>,
    steps: Option<u32>,
    source: Option<String>,
    images: HashMap<Uuid, StoredImage>,
}

impl Batch {
    fn bytes(&self) -> usize {
        self.images
            .values()
            .map(|image| image.bytes.len() + image.preview.len())
            .sum()
    }

    /// The prompt metadata a save of this batch carries.
    fn record(&self) -> PromptRecord {
        PromptRecord {
            prompt: self.prompt.clone(),
            negative_prompt: self.negative_prompt.clone(),
            seed: self.seed,
            model: self.model.clone(),
            width: self.width,
            height: self.height,
            quality: self.quality,
            steps: self.steps,
            source: self.source.clone(),
        }
    }
}

/// Owns the generations in flight and the short-lived store both hosts read from.
#[derive(Debug, Default)]
pub struct ImageGenerationService {
    active: Mutex<HashMap<Uuid, CancellationToken>>,
    batches: Mutex<HashMap<Uuid, Batch>>,
}

/// Bytes of one stored image, ready for a host to serve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageBytes {
    /// The bytes to serve.
    pub bytes: Vec<u8>,
    /// Their media type.
    pub media_type: &'static str,
}

impl ImageGenerationService {
    /// An empty service: nothing can be served or saved until a batch exists.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Runs one bounded batch and holds the result for review. Nothing is written to disk here.
    pub async fn generate(
        &self,
        provider: &dyn ImageProvider,
        providers: &ProviderRegistry,
        request: GenerateImagesRequestDto,
    ) -> Result<ImageBatchDto, ImageGenerationError> {
        let validated = validate_request(&request)?;
        let source_dto = request.source.clone();
        let source = match validated.source.as_ref() {
            Some(location) => Some(self.read_source(providers, location).await?),
            None => None,
        };
        let cancellation = CancellationToken::new();
        self.active_lock()
            .insert(request.request_id, cancellation.clone());
        let outcome = provider
            .generate(&validated, source.as_ref(), &cancellation)
            .await;
        self.active_lock().remove(&request.request_id);
        let batch = outcome?;
        if batch.images.is_empty()
            || batch.images.len() > usize::try_from(validated.count).unwrap_or(usize::MAX)
        {
            return Err(ImageGenerationError::Unavailable);
        }
        if batch.images.iter().map(Vec::len).sum::<usize>() > MAX_BATCH_BYTES {
            return Err(ImageGenerationError::TooLarge);
        }
        let mut held = HashMap::with_capacity(batch.images.len());
        let mut listed = Vec::with_capacity(batch.images.len());
        for bytes in batch.images {
            let (format, picture) = decode(&bytes)?;
            let (media_type, extension) =
                media_type(format).ok_or(ImageGenerationError::Unavailable)?;
            let (preview, preview_media_type, downscaled) = preview_bytes(format, &picture);
            let id = Uuid::new_v4();
            let width = picture.width();
            let height = picture.height();
            let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
            held.insert(
                id,
                StoredImage {
                    bytes,
                    preview,
                    media_type,
                    preview_media_type,
                    extension,
                },
            );
            listed.push(GeneratedImageDto {
                id,
                media_type: (*media_type).to_owned(),
                width,
                height,
                bytes: size,
                downscaled,
                extension: (*extension).to_owned(),
            });
        }
        let generation_id = Uuid::new_v4();
        let source_name = source.map(|source| source.name);
        self.hold(
            generation_id,
            Batch {
                created: Instant::now(),
                model: batch.model.clone(),
                prompt: validated.prompt.clone(),
                negative_prompt: validated.negative_prompt.clone(),
                seed: validated.seed,
                width: validated.width,
                height: validated.height,
                quality: validated.quality,
                steps: validated.steps,
                source: source_name.clone(),
                images: held,
            },
        );
        Ok(ImageBatchDto {
            generation_id,
            model: batch.model,
            prompt: validated.prompt,
            negative_prompt: validated.negative_prompt,
            seed: validated.seed,
            width: validated.width,
            height: validated.height,
            quality: validated.quality.map(Into::into),
            steps: validated.steps,
            source: source_dto,
            images: listed,
        })
    }

    /// Reads the picture a generation starts from, inside the byte bound and proved to be a picture
    /// this app can encode. Nothing is sent to an endpoint before that.
    async fn read_source(
        &self,
        providers: &ProviderRegistry,
        location: &fm_domain::Location,
    ) -> Result<SourceImage, ImageGenerationError> {
        use tokio::io::AsyncReadExt as _;

        let name = location
            .name()
            .map_err(|_| ImageGenerationError::SourceUnusable("it has no file name"))?;
        let provider = providers
            .resolve(location)
            .map_err(|_| ImageGenerationError::SourceUnusable("its location cannot be reached"))?;
        provider
            .capabilities_for(location)
            .map_err(|_| ImageGenerationError::SourceUnusable("its location cannot be reached"))?
            .require(ProviderCapabilities::READ)
            .map_err(|_| ImageGenerationError::SourceUnusable("it cannot be read from there"))?;
        let entry = fm_vfs::EntryRef {
            id: fm_domain::EntryId::new(),
            location: location.clone(),
        };
        let size = provider
            .file_size(&entry, CancellationToken::new())
            .await
            .map_err(|_| ImageGenerationError::SourceUnusable("it could not be read"))?;
        if size > MAX_SOURCE_BYTES {
            return Err(ImageGenerationError::SourceTooLarge);
        }
        let reader = provider
            .open_read(&entry, CancellationToken::new())
            .await
            .map_err(|_| {
                ImageGenerationError::InvalidRequest("the source image could not be read")
            })?;
        let mut bytes = Vec::new();
        reader
            .take(MAX_SOURCE_BYTES + 1)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| ImageGenerationError::SourceUnusable("it could not be read"))?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_SOURCE_BYTES {
            return Err(ImageGenerationError::SourceTooLarge);
        }
        // A file that will not decode is refused in the person's terms, not as a provider failure:
        // they chose this file, so it is theirs to correct.
        let (format, _) = decode(&bytes)
            .map_err(|_| ImageGenerationError::SourceUnusable("it is not a picture"))?;
        let (media_type, _) = media_type(format).ok_or(ImageGenerationError::SourceUnusable(
            "it is not a picture the image endpoints can read",
        ))?;
        Ok(SourceImage {
            name,
            media_type,
            bytes,
        })
    }

    /// Stops one running generation. Returns whether it was still running.
    pub fn cancel(&self, request: CancelImageGenerationRequestDto) -> bool {
        match self.active_lock().remove(&request.request_id) {
            Some(token) => {
                token.cancel();
                true
            }
            None => false,
        }
    }

    /// The bytes of one stored image: the downscale the preview shows, or the original.
    #[must_use]
    pub fn bytes(&self, generation_id: Uuid, image_id: Uuid, original: bool) -> Option<ImageBytes> {
        let mut batches = self.batches_lock();
        purge(&mut batches);
        let image = batches
            .get(&generation_id)
            .and_then(|batch| batch.images.get(&image_id))?;
        Some(ImageBytes {
            bytes: if original {
                image.bytes.clone()
            } else {
                image.preview.clone()
            },
            media_type: if original {
                image.media_type
            } else {
                image.preview_media_type
            },
        })
    }

    /// Drops a batch the person discarded. Returns whether it was still held.
    pub fn discard(&self, generation_id: Uuid) -> bool {
        let mut batches = self.batches_lock();
        purge(&mut batches);
        batches.remove(&generation_id).is_some()
    }

    /// Writes one stored image to disk (task 0325). This is the only write the feature performs.
    ///
    /// An existing destination is refused — the file is never replaced silently — unless the person
    /// explicitly asked to replace it, and the prompt travels into the file where the format
    /// supports it.
    ///
    /// # Errors
    ///
    /// [`ApplicationError::NotFound`] when the batch or image is no longer held,
    /// [`ApplicationError::InvalidRequest`] for a rejected file name or a provider that cannot be
    /// written, [`ApplicationError::DestinationAlreadyExists`] when the file exists and `overwrite`
    /// was not asked for, and a provider error otherwise.
    pub async fn save(
        &self,
        request: SaveGeneratedImageRequestDto,
        providers: &ProviderRegistry,
    ) -> Result<SaveGeneratedImageResponseDto, crate::error::ApplicationError> {
        use tokio::io::AsyncWriteExt as _;

        let held = {
            let mut batches = self.batches_lock();
            purge(&mut batches);
            let batch = batches
                .get(&request.generation_id)
                .ok_or(crate::error::ApplicationError::NotFound)?;
            let image = batch
                .images
                .get(&request.image_id)
                .ok_or(crate::error::ApplicationError::NotFound)?;
            (image.clone(), batch.record())
        };
        let (image, record) = held;
        let destination = fm_domain::Location::from(request.destination);
        let name = destination.name().map_err(|_| {
            crate::error::ApplicationError::InvalidRequest(
                "the destination has no file name".into(),
            )
        })?;
        fm_domain::location::validate_name(&name).map_err(|_| {
            crate::error::ApplicationError::InvalidRequest("the file name is not allowed".into())
        })?;
        let expected = format!(".{}", image.extension);
        if !name.to_ascii_lowercase().ends_with(&expected) {
            return Err(crate::error::ApplicationError::InvalidRequest(
                "the file name does not match the image format".into(),
            ));
        }
        let provider = providers
            .resolve(&destination)
            .map_err(crate::error::ApplicationError::from)?;
        provider
            .capabilities_for(&destination)
            .map_err(crate::error::ApplicationError::from)?
            .require(ProviderCapabilities::WRITE)
            .map_err(crate::error::ApplicationError::from)?;
        let (bytes, prompt_embedded) = embed_prompt_metadata(&image.bytes, &record);
        let mut writer = provider
            .open_write(
                &destination,
                WriteOptions {
                    overwrite: request.overwrite,
                },
                CancellationToken::new(),
            )
            .await
            .map_err(crate::error::ApplicationError::from)?;
        writer
            .write_all(&bytes)
            .await
            .map_err(|_| crate::error::ApplicationError::Internal)?;
        writer
            .shutdown()
            .await
            .map_err(|_| crate::error::ApplicationError::Internal)?;
        let bytes_written = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        Ok(SaveGeneratedImageResponseDto {
            location: destination.into(),
            bytes_written,
            prompt_embedded,
        })
    }

    fn active_lock(&self) -> std::sync::MutexGuard<'_, HashMap<Uuid, CancellationToken>> {
        self.active
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    fn batches_lock(&self) -> std::sync::MutexGuard<'_, HashMap<Uuid, Batch>> {
        self.batches
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    /// Stores a fresh batch, dropping expired batches first and the oldest while over budget.
    fn hold(&self, generation_id: Uuid, batch: Batch) {
        let mut batches = self.batches_lock();
        purge(&mut batches);
        batches.insert(generation_id, batch);
        while batches.values().map(Batch::bytes).sum::<usize>() > STORE_MAX_BYTES {
            let Some(oldest) = batches
                .iter()
                .min_by_key(|(_, held)| held.created)
                .map(|(id, _)| *id)
            else {
                break;
            };
            batches.remove(&oldest);
        }
    }
}

/// Drops batches older than the store's lifetime.
fn purge(batches: &mut HashMap<Uuid, Batch>) {
    let deadline = Instant::now() - BATCH_TTL;
    batches.retain(|_, batch| batch.created > deadline);
}

/// The PNG signature.
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

/// Stores the prompt in the file where the container supports it, and says whether it did.
///
/// PNG gets `prompt` and `parameters` text chunks, keeping every pixel byte untouched; JPEG and
/// WebP get the same block as their description through the metadata writer the properties panel
/// already uses. Anything else is returned unchanged, and the caller reports that plainly.
#[must_use]
pub fn embed_prompt_metadata(bytes: &[u8], record: &PromptRecord) -> (Vec<u8>, bool) {
    if bytes.starts_with(&PNG_SIGNATURE) {
        return embed_png_text(bytes, &record.parameters(), record.prompt.trim());
    }
    let photographic = bytes.starts_with(&[0xff, 0xd8])
        || (bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"));
    if photographic {
        let edit = fm_metadata::ImageEdit {
            description: fm_metadata::Change::Set(record.parameters()),
            ..fm_metadata::ImageEdit::default()
        };
        return match fm_metadata::apply_image_edit(bytes, &edit) {
            Ok(stored) => (stored, true),
            Err(_) => (bytes.to_vec(), false),
        };
    }
    (bytes.to_vec(), false)
}

/// Inserts the two text chunks behind PNG's IHDR.
fn embed_png_text(bytes: &[u8], parameters: &str, prompt: &str) -> (Vec<u8>, bool) {
    let Some(header_end) = png_header_end(bytes) else {
        return (bytes.to_vec(), false);
    };
    let mut chunks = png_text_chunk("prompt", prompt);
    chunks.extend(png_text_chunk("parameters", parameters));
    if chunks.is_empty() {
        return (bytes.to_vec(), false);
    }
    let mut stored = Vec::with_capacity(bytes.len() + chunks.len());
    stored.extend_from_slice(&bytes[..header_end]);
    stored.extend_from_slice(&chunks);
    stored.extend_from_slice(&bytes[header_end..]);
    (stored, true)
}

/// Where the IHDR chunk ends.
fn png_header_end(bytes: &[u8]) -> Option<usize> {
    if !bytes.starts_with(&PNG_SIGNATURE) || bytes.get(12..16) != Some(b"IHDR") {
        return None;
    }
    let length = usize::try_from(u32::from_be_bytes(bytes[8..12].try_into().ok()?)).ok()?;
    let end = 8usize.checked_add(12)?.checked_add(length)?;
    (bytes.len() >= end).then_some(end)
}

/// One PNG text chunk: `tEXt` when the text fits Latin-1 (what image tools already read), `iTXt`
/// when it needs the whole of Unicode. Empty for a keyword the format refuses.
fn png_text_chunk(keyword: &str, text: &str) -> Vec<u8> {
    if keyword.is_empty()
        || keyword.len() > 79
        || !keyword.is_ascii()
        || keyword.contains('\0')
        || text.is_empty()
    {
        return Vec::new();
    }
    let mut payload = Vec::with_capacity(keyword.len() + text.len() + 5);
    payload.extend_from_slice(keyword.as_bytes());
    if let Some(latin1) = latin1(text) {
        payload.push(0);
        payload.extend_from_slice(&latin1);
        png_chunk(b"tEXt", &payload)
    } else {
        // keyword NUL compression NUL compressed NUL language NUL translated keyword NUL text
        payload.extend_from_slice(&[0, 0, 0, 0, 0]);
        payload.extend_from_slice(text.as_bytes());
        png_chunk(b"iTXt", &payload)
    }
}

/// One PNG chunk with its length and CRC.
fn png_chunk(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 12);
    out.extend_from_slice(
        &u32::try_from(payload.len())
            .unwrap_or(u32::MAX)
            .to_be_bytes(),
    );
    out.extend_from_slice(kind);
    out.extend_from_slice(payload);
    out.extend_from_slice(&crc32(&[kind.as_slice(), payload].concat()));
    out
}

/// The text as Latin-1, or `None` when a character does not exist in it.
fn latin1(text: &str) -> Option<Vec<u8>> {
    text.chars()
        .map(|ch| {
            let code = u32::from(ch);
            (code <= 0xff).then_some(code as u8)
        })
        .collect()
}

/// The CRC-32 the PNG chunk trailer uses (IEEE 802.3, as RFC 2083 requires).
fn crc32(bytes: &[u8]) -> [u8; 4] {
    let mut crc = 0xff_ff_ff_ff_u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    (!crc).to_be_bytes()
}

/// Decodes a provider reply far enough to know its format, size and pixels.
fn decode(bytes: &[u8]) -> Result<(image::ImageFormat, image::DynamicImage), ImageGenerationError> {
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(ImageGenerationError::TooLarge);
    }
    let reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| ImageGenerationError::Unavailable)?;
    let format = reader.format().ok_or(ImageGenerationError::Unavailable)?;
    let picture = reader
        .decode()
        .map_err(|_| ImageGenerationError::Unavailable)?;
    Ok((format, picture))
}

/// The media type and file extension a stored image may carry; `None` refuses other formats.
fn media_type(format: image::ImageFormat) -> Option<(&'static str, &'static str)> {
    match format {
        image::ImageFormat::Png => Some(("image/png", "png")),
        image::ImageFormat::Jpeg => Some(("image/jpeg", "jpg")),
        image::ImageFormat::WebP => Some(("image/webp", "webp")),
        _ => None,
    }
}

/// The bytes a preview shows, with their media type: the image re-encoded small when it is already
/// small, a downscale otherwise. The stored original is never touched.
fn preview_bytes(
    format: image::ImageFormat,
    picture: &image::DynamicImage,
) -> (Vec<u8>, &'static str, bool) {
    let longest = picture.width().max(picture.height());
    let target = encoded_preview_format(format);
    let target_media_type = if target == image::ImageFormat::Jpeg {
        "image/jpeg"
    } else {
        "image/png"
    };
    if longest <= MAX_PREVIEW_EDGE {
        let mut bytes = Vec::new();
        if picture
            .write_to(&mut Cursor::new(&mut bytes), target)
            .is_ok()
            && !bytes.is_empty()
        {
            return (bytes, target_media_type, false);
        }
    }
    let scale = f64::from(MAX_PREVIEW_EDGE) / f64::from(longest.max(1));
    let width = ((f64::from(picture.width()) * scale).round() as u32).max(1);
    let height = ((f64::from(picture.height()) * scale).round() as u32).max(1);
    let mut bytes = Vec::new();
    if picture
        .resize(width, height, image::imageops::FilterType::Lanczos3)
        .write_to(&mut Cursor::new(&mut bytes), target)
        .is_err()
        || bytes.is_empty()
    {
        return (Vec::new(), target_media_type, false);
    }
    (bytes, target_media_type, true)
}

/// Keeps transparency by re-encoding everything that is not a photograph as PNG.
const fn encoded_preview_format(format: image::ImageFormat) -> image::ImageFormat {
    match format {
        image::ImageFormat::Jpeg => image::ImageFormat::Jpeg,
        _ => image::ImageFormat::Png,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex as StdMutex;

    use base64::Engine as _;

    use super::*;

    fn request() -> GenerateImagesRequestDto {
        GenerateImagesRequestDto {
            request_id: Uuid::new_v4(),
            workspace_id: Uuid::new_v4(),
            prompt: "a lighthouse at dusk".into(),
            negative_prompt: None,
            width: 1_024,
            height: 1_024,
            count: 1,
            seed: None,
            quality: None,
            steps: None,
            source: None,
        }
    }

    /// Encodes a real image, the way a provider's reply would carry it.
    fn encoded(format: image::ImageFormat, width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        image::DynamicImage::new_rgb8(width, height)
            .write_to(&mut Cursor::new(&mut bytes), format)
            .expect("encode");
        bytes
    }

    #[test]
    fn a_plain_request_passes_the_bounds() {
        let validated = validate_request(&request()).expect("accepted");
        assert_eq!(validated.prompt, "a lighthouse at dusk");
        assert_eq!(validated.width, 1_024);
        assert_eq!(validated.count, 1);
        assert_eq!(validated.seed, None);
    }

    #[test]
    fn an_empty_prompt_is_refused() {
        for prompt in ["", "   ", "\n\t"] {
            let dto = GenerateImagesRequestDto {
                prompt: prompt.into(),
                ..request()
            };
            assert_eq!(
                validate_request(&dto),
                Err(ImageGenerationError::InvalidRequest("the prompt is empty"))
            );
        }
    }

    #[test]
    fn an_over_long_prompt_or_negative_prompt_is_refused() {
        let long = "p".repeat(MAX_PROMPT_CHARS + 1);
        assert_eq!(
            validate_request(&GenerateImagesRequestDto {
                prompt: long,
                ..request()
            }),
            Err(ImageGenerationError::InvalidRequest(
                "the prompt is too long"
            ))
        );
        let long_negative = "n".repeat(MAX_NEGATIVE_CHARS + 1);
        assert_eq!(
            validate_request(&GenerateImagesRequestDto {
                negative_prompt: Some(long_negative),
                ..request()
            }),
            Err(ImageGenerationError::InvalidRequest(
                "the negative prompt is too long"
            ))
        );
        // A blank negative prompt is simply not sent.
        assert_eq!(
            validate_request(&GenerateImagesRequestDto {
                negative_prompt: Some("   ".into()),
                ..request()
            })
            .expect("accepted")
            .negative_prompt,
            None
        );
    }

    #[test]
    fn sizes_outside_the_accepted_range_are_refused() {
        for (width, height) in [
            (MIN_EDGE - 8, 1_024),
            (1_024, MAX_EDGE + 8),
            (1_023, 1_024), // not a multiple of 8
            (4_096, 64),    // over the largest edge
        ] {
            let dto = GenerateImagesRequestDto {
                width,
                height,
                ..request()
            };
            assert_eq!(
                validate_request(&dto),
                Err(ImageGenerationError::InvalidRequest(
                    "the size is outside the accepted range"
                )),
                "{width}x{height} should be refused"
            );
        }
        // Both edges inside the range but too many pixels together.
        assert_eq!(
            validate_request(&GenerateImagesRequestDto {
                width: 2_048,
                height: 2_048,
                ..request()
            }),
            Err(ImageGenerationError::InvalidRequest(
                "the size asks for too many pixels"
            ))
        );
        // A tall portrait at the largest sensible size is still accepted.
        assert!(
            validate_request(&GenerateImagesRequestDto {
                width: 1_536,
                height: 1_024,
                ..request()
            })
            .is_ok()
        );
    }

    #[test]
    fn count_and_seed_have_bounds() {
        for count in [0, MAX_IMAGES + 1] {
            assert_eq!(
                validate_request(&GenerateImagesRequestDto { count, ..request() }),
                Err(ImageGenerationError::InvalidRequest(
                    "the number of images is outside the accepted range"
                )),
                "count {count} should be refused"
            );
        }
        assert_eq!(
            validate_request(&GenerateImagesRequestDto {
                seed: Some(MAX_SEED + 1),
                ..request()
            }),
            Err(ImageGenerationError::InvalidRequest(
                "the seed is too large"
            ))
        );
        assert_eq!(
            validate_request(&GenerateImagesRequestDto {
                seed: Some(MAX_SEED),
                ..request()
            })
            .expect("accepted")
            .seed,
            Some(MAX_SEED)
        );
    }

    #[test]
    fn the_request_body_carries_only_what_the_person_asked_for() {
        let body = images_request_body(
            "flux-schnell",
            &validate_request(&request()).expect("accepted"),
        );
        assert_eq!(body["model"], "flux-schnell");
        assert_eq!(body["prompt"], "a lighthouse at dusk");
        assert_eq!(body["n"], 1);
        assert_eq!(body["size"], "1024x1024");
        assert_eq!(body["response_format"], "b64_json");
        assert!(body.get("negative_prompt").is_none());
        assert!(body.get("seed").is_none());

        let with_extras = images_request_body(
            "flux",
            &validate_request(&GenerateImagesRequestDto {
                negative_prompt: Some("text, watermark".into()),
                count: 3,
                seed: Some(42),
                width: 832,
                height: 1_216,
                ..request()
            })
            .expect("accepted"),
        );
        assert_eq!(with_extras["negative_prompt"], "text, watermark");
        assert_eq!(with_extras["n"], 3);
        assert_eq!(with_extras["seed"], 42);
        assert_eq!(with_extras["size"], "832x1216");
    }

    #[test]
    fn a_reply_is_read_as_images_in_reply_order() {
        let png = b"fake-png-bytes".to_vec();
        let encoded = base64::engine::general_purpose::STANDARD.encode(&png);
        let body = json!({ "data": [{ "b64_json": encoded }, { "b64_json": encoded }] });
        assert_eq!(
            parse_images_response(body.to_string().as_bytes()).expect("parsed"),
            vec![png.clone(), png]
        );
    }

    #[test]
    fn a_reply_without_usable_images_is_refused() {
        for body in [
            "{\"data\":[]}",
            "{\"data\":[{\"url\":\"https://x.test/a.png\"}]}",
            "not json",
            "{}",
        ] {
            assert_eq!(
                parse_images_response(body.as_bytes()),
                Err(ImageGenerationError::Unavailable),
                "{body} should be refused"
            );
        }
        let huge =
            base64::engine::general_purpose::STANDARD.encode(vec![0_u8; MAX_IMAGE_BYTES + 1]);
        assert_eq!(
            parse_images_response(
                json!({ "data": [{ "b64_json": huge }] })
                    .to_string()
                    .as_bytes()
            ),
            Err(ImageGenerationError::TooLarge)
        );
    }

    /// A hook a test runs while a generation is in flight, e.g. to cancel it.
    type GenerateHook = Box<dyn Fn(&CancellationToken) + Send + Sync>;

    /// Records what was asked and answers with prepared images, or with a failure.
    struct FakeProvider {
        model: &'static str,
        outcome: Result<Vec<Vec<u8>>, ImageGenerationError>,
        seen: StdMutex<Vec<ValidatedImageRequest>>,
        /// The picture each request asked to start from, as the provider was handed it.
        seen_sources: StdMutex<Vec<Option<SourceImage>>>,
        on_generate: Option<GenerateHook>,
    }

    #[async_trait]
    impl ImageProvider for FakeProvider {
        async fn generate(
            &self,
            request: &ValidatedImageRequest,
            source: Option<&SourceImage>,
            cancellation: &CancellationToken,
        ) -> Result<GeneratedBatch, ImageGenerationError> {
            self.seen_sources.lock().unwrap().push(source.cloned());
            if let Some(on_generate) = &self.on_generate {
                on_generate(cancellation);
            }
            self.seen.lock().unwrap().push(request.clone());
            self.outcome.clone().map(|images| GeneratedBatch {
                model: self.model.to_owned(),
                images,
            })
        }
    }

    fn provider(images: Vec<Vec<u8>>) -> FakeProvider {
        FakeProvider {
            model: "flux-schnell",
            outcome: Ok(images),
            seen: StdMutex::new(Vec::new()),
            seen_sources: StdMutex::new(Vec::new()),
            on_generate: None,
        }
    }

    #[tokio::test]
    async fn a_generated_batch_is_listed_without_its_bytes_and_held_for_review() {
        let service = ImageGenerationService::new();
        let original = encoded(image::ImageFormat::Png, 96, 64);
        let batch = service
            .generate(
                &provider(vec![original.clone()]),
                &registry(),
                GenerateImagesRequestDto {
                    negative_prompt: Some("text".into()),
                    seed: Some(7),
                    ..request()
                },
            )
            .await
            .expect("generated");
        assert_eq!(batch.model, "flux-schnell");
        assert_eq!(batch.prompt, "a lighthouse at dusk");
        assert_eq!(batch.negative_prompt.as_deref(), Some("text"));
        assert_eq!(batch.seed, Some(7));
        assert_eq!(batch.images.len(), 1);
        let image = &batch.images[0];
        assert_eq!(image.media_type, "image/png");
        assert_eq!(image.extension, "png");
        assert_eq!((image.width, image.height), (96, 64));
        assert_eq!(image.bytes, u64::try_from(original.len()).unwrap());
        assert!(!image.downscaled);
        // The reply carries no bytes: only the store holds them.
        assert!(
            serde_json::to_string(&batch)
                .unwrap()
                .find("b64_json")
                .is_none()
        );
        let served = service
            .bytes(batch.generation_id, image.id, false)
            .expect("held");
        assert_eq!(served.media_type, "image/png");
        assert_eq!(served.bytes, original);
    }

    #[tokio::test]
    async fn a_very_large_image_is_downscaled_for_the_preview_only() {
        let service = ImageGenerationService::new();
        let original = encoded(image::ImageFormat::Png, 2_048, 1_536);
        let batch = service
            .generate(&provider(vec![original.clone()]), &registry(), request())
            .await
            .expect("generated");
        let image = &batch.images[0];
        assert!(image.downscaled);
        assert_eq!(image.width, 2_048);
        assert_eq!(image.bytes, u64::try_from(original.len()).unwrap());
        let preview = service
            .bytes(batch.generation_id, image.id, false)
            .expect("preview");
        let stored = service
            .bytes(batch.generation_id, image.id, true)
            .expect("original");
        assert_eq!(stored.bytes, original, "the original is kept as returned");
        assert!(preview.bytes.len() < original.len());
        let picture = image::load_from_memory(&preview.bytes).expect("preview decodes");
        assert_eq!(
            picture.width().max(picture.height()),
            MAX_PREVIEW_EDGE,
            "the preview fits the preview bound"
        );
        // What a host is told to serve is what it is handed.
        assert_eq!(preview.media_type, "image/png");
        assert_eq!(format_of(&preview.bytes), image::ImageFormat::Png);
        assert_eq!(stored.media_type, "image/png");
        assert_eq!(format_of(&stored.bytes), image::ImageFormat::Png);
    }

    /// The container one served blob actually is.
    fn format_of(bytes: &[u8]) -> image::ImageFormat {
        image::ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()
            .expect("guessable")
            .format()
            .expect("a known format")
    }

    #[tokio::test]
    async fn a_jpeg_preview_stays_a_jpeg_and_an_unknown_format_is_refused() {
        let service = ImageGenerationService::new();
        let batch = service
            .generate(
                &provider(vec![encoded(image::ImageFormat::Jpeg, 320, 240)]),
                &registry(),
                request(),
            )
            .await
            .expect("generated");
        assert_eq!(batch.images[0].media_type, "image/jpeg");
        assert_eq!(batch.images[0].extension, "jpg");
        let served = service
            .bytes(batch.generation_id, batch.images[0].id, false)
            .expect("preview");
        assert_eq!(
            format_of(&served.bytes),
            image::ImageFormat::Jpeg,
            "a photographic preview stays a JPEG"
        );
        assert_eq!(served.media_type, "image/jpeg");
        assert_eq!(
            service
                .generate(
                    &provider(vec![b"not an image".to_vec()]),
                    &registry(),
                    request()
                )
                .await,
            Err(ImageGenerationError::Unavailable)
        );
    }

    #[tokio::test]
    async fn a_provider_answer_outside_the_bounds_is_refused_and_holds_nothing() {
        let service = ImageGenerationService::new();
        // More images than were asked for.
        assert_eq!(
            service
                .generate(
                    &provider(vec![
                        encoded(image::ImageFormat::Png, 32, 32),
                        encoded(image::ImageFormat::Png, 32, 32)
                    ]),
                    &registry(),
                    request()
                )
                .await,
            Err(ImageGenerationError::Unavailable)
        );
        // Nothing at all.
        assert_eq!(
            service
                .generate(&provider(vec![]), &registry(), request())
                .await,
            Err(ImageGenerationError::Unavailable)
        );
        // A reply that cannot be decoded holds no batch either.
        assert_eq!(service.bytes(Uuid::new_v4(), Uuid::new_v4(), false), None);
        assert!(service.active_lock().is_empty());
    }

    #[tokio::test]
    async fn a_bad_request_never_reaches_the_provider() {
        let service = ImageGenerationService::new();
        let provider = provider(vec![encoded(image::ImageFormat::Png, 32, 32)]);
        for (dto, reason) in [
            (
                GenerateImagesRequestDto {
                    prompt: " ".into(),
                    ..request()
                },
                "the prompt is empty",
            ),
            (
                GenerateImagesRequestDto {
                    count: 9,
                    ..request()
                },
                "the number of images is outside the accepted range",
            ),
            (
                GenerateImagesRequestDto {
                    width: 1_001,
                    height: 1_001,
                    ..request()
                },
                "the size is outside the accepted range",
            ),
        ] {
            assert_eq!(
                service
                    .generate(&provider, &registry(), dto)
                    .await
                    .expect_err("refused"),
                ImageGenerationError::InvalidRequest(reason)
            );
        }
        assert!(provider.seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn stop_cancels_a_running_generation() {
        let service = ImageGenerationService::new();
        let started = tokio::sync::Notify::new();
        let provider = BlockingProvider { started: &started };
        let dto = request();
        let request_id = dto.request_id;
        let run = async { service.generate(&provider, &registry(), dto).await };
        let stop = async {
            started.notified().await;
            assert!(service.cancel(CancelImageGenerationRequestDto { request_id }));
        };
        let (outcome, ()) = tokio::join!(run, stop);
        assert_eq!(outcome, Err(ImageGenerationError::Cancelled));
        assert!(!service.cancel(CancelImageGenerationRequestDto { request_id }));
        assert!(service.active_lock().is_empty());
    }

    struct BlockingProvider<'a> {
        started: &'a tokio::sync::Notify,
    }

    #[async_trait]
    impl ImageProvider for BlockingProvider<'_> {
        async fn generate(
            &self,
            _request: &ValidatedImageRequest,
            _source: Option<&SourceImage>,
            cancellation: &CancellationToken,
        ) -> Result<GeneratedBatch, ImageGenerationError> {
            self.started.notify_one();
            cancellation.cancelled().await;
            Err(ImageGenerationError::Cancelled)
        }
    }

    #[tokio::test]
    async fn a_discarded_batch_can_no_longer_be_served() {
        let service = ImageGenerationService::new();
        let batch = service
            .generate(
                &provider(vec![encoded(image::ImageFormat::Png, 32, 32)]),
                &registry(),
                request(),
            )
            .await
            .expect("generated");
        let id = batch.images[0].id;
        assert!(service.bytes(batch.generation_id, id, false).is_some());
        assert!(service.discard(batch.generation_id));
        assert!(service.bytes(batch.generation_id, id, false).is_none());
        assert!(!service.discard(batch.generation_id));
    }

    /// Walks the PNG chunks of a saved file and returns the text stored under `keyword`.
    fn png_text(bytes: &[u8], keyword: &str) -> Option<String> {
        assert!(bytes.starts_with(&PNG_SIGNATURE));
        let mut cursor = PNG_SIGNATURE.len();
        while cursor + 8 <= bytes.len() {
            let length = usize::try_from(u32::from_be_bytes(
                bytes[cursor..cursor + 4].try_into().ok()?,
            ))
            .ok()?;
            let kind = &bytes[cursor + 4..cursor + 8];
            let payload = bytes.get(cursor + 8..cursor + 8 + length)?;
            if let Some(text) = chunk_text(kind, payload, keyword) {
                return Some(text);
            }
            cursor += 12 + length;
        }
        None
    }

    /// Splits at the first NUL: the bytes before it, and the bytes after it.
    fn split_at_nul(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
        let index = bytes.iter().position(|byte| *byte == 0)?;
        Some((&bytes[..index], &bytes[index + 1..]))
    }

    /// Reads the value of one `tEXt` or `iTXt` chunk when its keyword matches.
    fn chunk_text(kind: &[u8], payload: &[u8], keyword: &str) -> Option<String> {
        let (found, text) = match kind {
            b"tEXt" => split_at_nul(payload)?,
            b"iTXt" => {
                let (found, flags) = split_at_nul(payload)?;
                let after_flags = flags.get(2..)?; // compression and compressed flags
                let (_, after_language) = split_at_nul(after_flags)?;
                let (_, text) = split_at_nul(after_language)?;
                (found, text)
            }
            _ => return None,
        };
        (found == keyword.as_bytes()).then(|| match kind {
            b"tEXt" => text.iter().map(|byte| char::from(*byte)).collect(),
            _ => String::from_utf8_lossy(text).into_owned(),
        })
    }

    /// The level the person chose is worded the way the OpenAI-style contract wants it, and a level
    /// or step count nobody chose is not sent at all - an endpoint without those knobs then sees
    /// exactly the request it saw before they existed.
    #[test]
    fn quality_and_steps_are_worded_for_the_endpoint_and_omitted_when_untouched() {
        let body = images_request_body(
            "flux-schnell",
            &validate_request(&GenerateImagesRequestDto {
                quality: Some(ImageQualityDto::SuperQuality),
                steps: Some(30),
                ..request()
            })
            .expect("within bounds"),
        );
        // That API's own scale stops at "high", so the top level the person can choose asks for it.
        assert_eq!(body["quality"], "high");
        assert_eq!(body["steps"], 30);
        for level in [
            ImageQualityDto::Fast,
            ImageQualityDto::Good,
            ImageQualityDto::Quality,
        ] {
            let body = images_request_body(
                "flux-schnell",
                &validate_request(&GenerateImagesRequestDto {
                    quality: Some(level),
                    ..request()
                })
                .expect("within bounds"),
            );
            assert!(
                ["low", "medium", "high"].contains(&body["quality"].as_str().expect("a word")),
                "{level:?} should ask in the endpoint's own words"
            );
        }
        let plain = images_request_body("flux-schnell", &validate_request(&request()).expect("ok"));
        assert!(plain.get("quality").is_none());
        assert!(plain.get("steps").is_none());
    }

    #[test]
    fn a_step_count_outside_the_accepted_range_is_refused() {
        for steps in [0, MAX_STEPS + 1] {
            assert_eq!(
                validate_request(&GenerateImagesRequestDto {
                    steps: Some(steps),
                    ..request()
                }),
                Err(ImageGenerationError::InvalidRequest(
                    "the step count is outside the accepted range"
                )),
                "{steps} steps should be refused"
            );
        }
    }

    #[tokio::test]
    async fn the_quality_and_step_count_asked_for_come_back_with_the_batch() {
        let service = ImageGenerationService::new();
        let batch = service
            .generate(
                &provider(vec![encoded(image::ImageFormat::Png, 64, 48)]),
                &registry(),
                GenerateImagesRequestDto {
                    quality: Some(ImageQualityDto::Good),
                    steps: Some(20),
                    ..request()
                },
            )
            .await
            .expect("generated");
        assert_eq!(batch.quality, Some(ImageQualityDto::Good));
        assert_eq!(batch.steps, Some(20));
    }

    #[test]
    fn the_saved_metadata_names_the_quality_the_steps_and_the_source() {
        let record = PromptRecord {
            prompt: "a lighthouse at dusk".to_owned(),
            negative_prompt: None,
            seed: None,
            model: "flux-schnell".to_owned(),
            width: 1_024,
            height: 1_024,
            quality: Some(ImageQuality::SuperQuality),
            steps: Some(30),
            source: Some("harbour.png".to_owned()),
        };
        let text = record.parameters();
        assert!(text.contains("Quality: super quality"), "{text}");
        assert!(text.contains("Steps: 30"), "{text}");
        assert!(text.contains("Source: harbour.png"), "{text}");
    }

    #[tokio::test]
    async fn a_source_picture_is_read_from_disk_and_handed_to_the_endpoint() {
        use fm_transport_dto::ImageSourceDto;

        let directory = tempfile::tempdir().expect("temp directory");
        let bytes = encoded(image::ImageFormat::Png, 96, 96);
        let path = directory.path().join("harbour.png");
        std::fs::write(&path, &bytes).expect("the source is written");
        let location = fm_domain::Location::from_native_path(&path).expect("location");
        let service = ImageGenerationService::new();
        let provider = provider(vec![encoded(image::ImageFormat::Png, 64, 48)]);
        let batch = service
            .generate(
                &provider,
                &registry(),
                GenerateImagesRequestDto {
                    source: Some(ImageSourceDto {
                        location: location.clone().into(),
                    }),
                    ..request()
                },
            )
            .await
            .expect("generated");
        let seen = provider.seen_sources.lock().expect("record");
        let source = seen[0]
            .as_ref()
            .expect("the endpoint was given the picture");
        assert_eq!(source.name, "harbour.png");
        assert_eq!(source.media_type, "image/png");
        assert_eq!(source.bytes, bytes);
        // "Regenerate with this prompt" has to be able to ask for the same picture again.
        assert_eq!(
            batch
                .source
                .expect("the batch says where it started")
                .location,
            location.into()
        );
    }

    #[tokio::test]
    async fn a_source_the_app_cannot_use_is_refused_before_the_endpoint_is_asked() {
        use fm_transport_dto::ImageSourceDto;

        let directory = tempfile::tempdir().expect("temp directory");
        let notes = directory.path().join("notes.txt");
        std::fs::write(&notes, b"not a picture at all").expect("the file is written");
        let missing = directory.path().join("gone.png");
        let cases = [
            (&notes, "it is not a picture"),
            (&missing, "it could not be read"),
        ];
        for (path, expected) in cases {
            let service = ImageGenerationService::new();
            let provider = provider(vec![encoded(image::ImageFormat::Png, 64, 48)]);
            assert_eq!(
                service
                    .generate(
                        &provider,
                        &registry(),
                        GenerateImagesRequestDto {
                            source: Some(ImageSourceDto {
                                location: fm_domain::Location::from_native_path(path)
                                    .expect("location")
                                    .into(),
                            }),
                            ..request()
                        },
                    )
                    .await,
                Err(ImageGenerationError::SourceUnusable(expected)),
                "{} should be refused",
                path.display()
            );
            assert!(
                provider.seen.lock().expect("record").is_empty(),
                "the endpoint is not asked about a source nobody can use"
            );
        }
    }

    fn registry() -> ProviderRegistry {
        let mut providers = ProviderRegistry::new();
        providers.register(std::sync::Arc::new(
            fm_vfs_local::LocalFileSystemProvider::new(),
        ));
        providers
    }

    fn save_request(
        batch: &ImageBatchDto,
        destination: &fm_domain::Location,
        overwrite: bool,
    ) -> SaveGeneratedImageRequestDto {
        SaveGeneratedImageRequestDto {
            generation_id: batch.generation_id,
            image_id: batch.images[0].id,
            destination: destination.clone().into(),
            overwrite,
        }
    }

    async fn one_png(service: &ImageGenerationService) -> ImageBatchDto {
        service
            .generate(
                &provider(vec![encoded(image::ImageFormat::Png, 64, 48)]),
                &registry(),
                GenerateImagesRequestDto {
                    negative_prompt: Some("text, watermark".into()),
                    seed: Some(42),
                    ..request()
                },
            )
            .await
            .expect("generated")
    }

    #[tokio::test]
    async fn saving_writes_the_chosen_file_and_carries_the_prompt() {
        let directory = tempfile::tempdir().expect("temp directory");
        let service = ImageGenerationService::new();
        let batch = one_png(&service).await;
        let path = directory.path().join("lighthouse.png");
        let destination = fm_domain::Location::from_native_path(&path).expect("location");
        let saved = service
            .save(save_request(&batch, &destination, false), &registry())
            .await
            .expect("saved");
        assert_eq!(saved.location, destination.into());
        assert!(saved.prompt_embedded);
        let written = std::fs::read(&path).expect("the file is on disk");
        assert!(written.len() >= batch.images[0].bytes as usize);
        assert_eq!(saved.bytes_written, u64::try_from(written.len()).unwrap());
        // The picture is intact and the prompt travels with it.
        let picture = image::load_from_memory(&written).expect("the saved image decodes");
        assert_eq!((picture.width(), picture.height()), (64, 48));
        assert_eq!(
            png_text(&written, "prompt").as_deref(),
            Some("a lighthouse at dusk")
        );
        let parameters = png_text(&written, "parameters").expect("parameters chunk");
        assert_eq!(
            parameters,
            "a lighthouse at dusk\nNegative prompt: text, watermark\nSeed: 42\nSize: 1024x1024\nModel: flux-schnell\nSoftware: Procyon"
        );
    }

    #[tokio::test]
    async fn a_prompt_outside_latin1_is_stored_and_survives_the_round_trip() {
        let directory = tempfile::tempdir().expect("temp directory");
        let service = ImageGenerationService::new();
        let batch = service
            .generate(
                &provider(vec![encoded(image::ImageFormat::Png, 32, 32)]),
                &registry(),
                GenerateImagesRequestDto {
                    prompt: "a café at dusk, 灯台".into(),
                    ..request()
                },
            )
            .await
            .expect("generated");
        let destination =
            fm_domain::Location::from_native_path(&directory.path().join("cafe.png")).expect("loc");
        service
            .save(save_request(&batch, &destination, false), &registry())
            .await
            .expect("saved");
        let written = std::fs::read(directory.path().join("cafe.png")).expect("written");
        assert_eq!(
            png_text(&written, "prompt").as_deref(),
            Some("a café at dusk, 灯台")
        );
    }

    #[tokio::test]
    async fn an_existing_file_survives_until_the_person_asks_to_replace_it() {
        let directory = tempfile::tempdir().expect("temp directory");
        let path = directory.path().join("keep.png");
        std::fs::write(&path, b"someone else's file").expect("write");
        let service = ImageGenerationService::new();
        let batch = one_png(&service).await;
        let destination = fm_domain::Location::from_native_path(&path).expect("location");
        assert_eq!(
            service
                .save(save_request(&batch, &destination, false), &registry())
                .await
                .expect_err("refused"),
            crate::error::ApplicationError::DestinationAlreadyExists
        );
        assert_eq!(
            std::fs::read(&path).expect("still there"),
            b"someone else's file"
        );
        service
            .save(save_request(&batch, &destination, true), &registry())
            .await
            .expect("replaced on request");
        assert!(
            std::fs::read(&path)
                .expect("replaced")
                .starts_with(&PNG_SIGNATURE)
        );
    }

    #[tokio::test]
    async fn a_jpeg_save_keeps_its_format_and_stores_the_prompt() {
        let directory = tempfile::tempdir().expect("temp directory");
        let service = ImageGenerationService::new();
        let batch = service
            .generate(
                &provider(vec![encoded(image::ImageFormat::Jpeg, 96, 64)]),
                &registry(),
                request(),
            )
            .await
            .expect("generated");
        let path = directory.path().join("photo.jpg");
        let destination = fm_domain::Location::from_native_path(&path).expect("location");
        let saved = service
            .save(save_request(&batch, &destination, false), &registry())
            .await
            .expect("saved");
        assert!(saved.prompt_embedded, "JPEG stores a description");
        let written = std::fs::read(&path).expect("written");
        assert!(written.starts_with(&[0xff, 0xd8]));
        image::load_from_memory(&written).expect("the saved jpeg decodes");
        assert!(
            written
                .windows(b"Software: Procyon".len())
                .any(|window| window == b"Software: Procyon"),
            "the prompt block travels with the photo"
        );
    }

    #[tokio::test]
    async fn an_unknown_batch_or_image_is_not_found_and_writes_nothing() {
        let directory = tempfile::tempdir().expect("temp directory");
        let service = ImageGenerationService::new();
        let batch = one_png(&service).await;
        let destination = fm_domain::Location::from_native_path(&directory.path().join("a.png"))
            .expect("location");
        for (generation_id, image_id, what) in [
            (Uuid::new_v4(), batch.images[0].id, "no such batch"),
            (batch.generation_id, Uuid::nil(), "no such image"),
        ] {
            let mut request = save_request(&batch, &destination, false);
            request.generation_id = generation_id;
            request.image_id = image_id;
            assert_eq!(
                service.save(request, &registry()).await.expect_err(what),
                crate::error::ApplicationError::NotFound,
                "{what}"
            );
        }
        assert!(!directory.path().join("a.png").exists());
    }

    #[tokio::test]
    async fn a_file_name_that_does_not_match_the_image_is_refused() {
        let directory = tempfile::tempdir().expect("temp directory");
        let service = ImageGenerationService::new();
        let batch = one_png(&service).await;
        for name in ["lighthouse.jpg", "lighthouse", "lighthouse.PNGX.png.txt"] {
            let destination = fm_domain::Location::from_native_path(&directory.path().join(name))
                .expect("location");
            let error = service
                .save(save_request(&batch, &destination, false), &registry())
                .await
                .expect_err("refused");
            assert!(
                matches!(error, crate::error::ApplicationError::InvalidRequest(_)),
                "{name}: {error:?}"
            );
        }
        // The uppercase name that does carry the extension is accepted.
        let destination =
            fm_domain::Location::from_native_path(&directory.path().join("LIGHT.PNG"))
                .expect("location");
        service
            .save(save_request(&batch, &destination, false), &registry())
            .await
            .expect("accepted");
    }

    #[test]
    fn the_prompt_block_reads_the_way_image_tools_expect() {
        let record = PromptRecord {
            quality: None,
            steps: None,
            source: None,
            prompt: "a lighthouse at dusk".into(),
            negative_prompt: Some("text, watermark".into()),
            seed: Some(42),
            model: "flux-schnell".into(),
            width: 1_024,
            height: 768,
        };
        assert_eq!(
            record.parameters(),
            "a lighthouse at dusk\nNegative prompt: text, watermark\nSeed: 42\nSize: 1024x768\nModel: flux-schnell\nSoftware: Procyon"
        );
        assert_eq!(
            PromptRecord {
                quality: None,
                steps: None,
                source: None,
                prompt: "a cat".into(),
                negative_prompt: None,
                seed: None,
                model: "m".into(),
                width: 64,
                height: 64,
            }
            .parameters(),
            "a cat\nSize: 64x64\nModel: m\nSoftware: Procyon"
        );
    }
}
