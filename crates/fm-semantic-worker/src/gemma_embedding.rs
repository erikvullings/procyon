//! Native Gemma text roles at the existing worker embedding boundary.

use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::embedding::{EmbeddingError, EmbeddingModelIdentity};
use crate::gemma_native::{GemmaNativeEncoder, GemmaNativeError};
use crate::gemma_probe::GemmaTextTask;
use crate::ingestion::{DocumentEmbeddingInput, EmbeddingProvider, MediaEmbedding};

/// Case-sensitive model input, distinct from E5's case-folded cache space.
pub const GEMMA_PREPROCESSING_VERSION: &str = "preserve-case/embeddinggemma-2-v1";
const MAX_TEXT_BYTES: usize = 256 * 1024;

/// An explicit text intent using one shared native multimodal encoder.
pub struct GemmaTextEmbeddingProvider {
    encoder: Arc<GemmaNativeEncoder>,
    identity: EmbeddingModelIdentity,
    task: GemmaTextTask,
}

impl GemmaTextEmbeddingProvider {
    /// Binds the signed checkpoint to one model-owned text prompt.
    #[must_use]
    pub fn new(encoder: Arc<GemmaNativeEncoder>, task: GemmaTextTask) -> Self {
        let identity = EmbeddingModelIdentity {
            model_id: "google-embeddinggemma-2".to_owned(),
            model_revision: "914f7f89142e33e77833254d9c9b90c3cef7303b".to_owned(),
            tokenizer: "google/embeddinggemma-2/tokenizer@914f7f89142e33e77833254d9c9b90c3cef7303b"
                .to_owned(),
            dimensions: encoder.dimensions(),
            max_input_tokens: 8_192,
        };
        Self {
            encoder,
            identity,
            task,
        }
    }
}

impl EmbeddingProvider for GemmaTextEmbeddingProvider {
    fn identity(&self) -> &EmbeddingModelIdentity {
        &self.identity
    }

    fn preprocessing_version(&self) -> &'static str {
        GEMMA_PREPROCESSING_VERSION
    }

    fn cache_input(&self, input: &str) -> String {
        input.to_owned()
    }

    fn cache_document_input(&self, title: Option<&str>, input: &str) -> String {
        if self.task == GemmaTextTask::Document {
            // The prompt, including its title, is the exact inference input.
            GemmaTextTask::Document
                .format(input, title)
                .expect("document prompts accept optional titles")
        } else {
            self.cache_input(input)
        }
    }

    fn embed_documents(
        &self,
        inputs: &[DocumentEmbeddingInput],
        cancellation: &CancellationToken,
    ) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        let mut vectors = Vec::with_capacity(inputs.len());
        for input in inputs {
            if cancellation.is_cancelled() {
                return Err(EmbeddingError::Cancelled);
            }

            if input.text.len() > MAX_TEXT_BYTES
                || input.title.as_ref().is_some_and(|title| title.len() > 1024)
            {
                return Err(EmbeddingError::Backend(
                    "native Gemma document input exceeds the byte limit".to_owned(),
                ));
            }

            match self.encoder.encode_text_cancellable(
                self.task,
                &input.text,
                input.title.as_deref(),
                cancellation,
            ) {
                Ok(vector) => vectors.push(vector),
                Err(GemmaNativeError::Cancelled) => return Err(EmbeddingError::Cancelled),
                Err(error) => return Err(EmbeddingError::Backend(error.to_string())),
            }
        }
        Ok(vectors)
    }

    fn embed_media(
        &self,
        media_type: &str,
        bytes: &[u8],
        cancellation: &CancellationToken,
    ) -> Result<MediaEmbedding, EmbeddingError> {
        let (vector, sampled_timestamps_ms) = match media_type {
            "image/jpeg" | "image/png" | "image/webp" => (
                self.encoder
                    .encode_image_cancellable(bytes, cancellation)
                    .map_err(map_gemma_error)?,
                vec![],
            ),
            "audio/wav" | "audio/mpeg" | "audio/flac" | "audio/aac" | "audio/mp4" => (
                self.encoder
                    .encode_audio_file_cancellable(bytes, cancellation)
                    .map_err(map_gemma_error)?,
                vec![],
            ),
            "video/mp4" | "video/quicktime" => {
                let video = self
                    .encoder
                    .encode_h264_video_cancellable(bytes, cancellation)
                    .map_err(map_gemma_error)?;
                (video.vector, video.timestamps_ms)
            }
            _ => {
                return Err(EmbeddingError::Backend(
                    "unsupported Gemma media MIME type".into(),
                ));
            }
        };
        Ok(MediaEmbedding {
            vector,
            sampled_timestamps_ms,
        })
    }

    fn media_coverage(
        &self,
        media_type: &str,
        bytes: &[u8],
        cancellation: &CancellationToken,
    ) -> Result<Vec<u64>, EmbeddingError> {
        if cancellation.is_cancelled() {
            return Err(EmbeddingError::Cancelled);
        }
        match media_type {
            "video/mp4" | "video/quicktime" if self.encoder.media().video => {
                fm_metadata::sample_video_frames_cancellable(bytes, || cancellation.is_cancelled())
                    .map(|frames| frames.into_iter().map(|frame| frame.timestamp_ms).collect())
                    .map_err(|error| match error {
                        fm_metadata::VideoSamplingError::Cancelled => EmbeddingError::Cancelled,
                        other => EmbeddingError::Backend(other.to_string()),
                    })
            }
            "image/jpeg" | "image/png" | "image/webp" if self.encoder.media().images => Ok(vec![]),
            "audio/wav" | "audio/mpeg" | "audio/flac" | "audio/aac" | "audio/mp4"
                if self.encoder.media().audio =>
            {
                Ok(vec![])
            }
            _ => Err(EmbeddingError::Backend(
                "media type was not enabled for this Gemma library".into(),
            )),
        }
    }

    fn embed(
        &self,
        inputs: &[String],
        cancellation: &CancellationToken,
    ) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        let mut vectors = Vec::with_capacity(inputs.len());
        for input in inputs {
            if cancellation.is_cancelled() {
                return Err(EmbeddingError::Cancelled);
            }
            if input.len() > MAX_TEXT_BYTES {
                return Err(EmbeddingError::Backend(format!(
                    "native Gemma input exceeds {MAX_TEXT_BYTES} bytes"
                )));
            }
            match self
                .encoder
                .encode_text_cancellable(self.task, input, None, cancellation)
            {
                Ok(vector) => vectors.push(vector),
                Err(GemmaNativeError::Cancelled) => return Err(EmbeddingError::Cancelled),
                Err(error) => return Err(EmbeddingError::Backend(error.to_string())),
            }
        }
        Ok(vectors)
    }
}

fn map_gemma_error(error: GemmaNativeError) -> EmbeddingError {
    match error {
        GemmaNativeError::Cancelled => EmbeddingError::Cancelled,
        other => EmbeddingError::Backend(other.to_string()),
    }
}
