//! Experimental offline CPU inference for the pinned multimodal checkpoint.

use std::collections::BTreeMap;
use std::fs::File;
use std::path::{Path, PathBuf};

use fm_metadata::{VideoSamplingError, sample_video_frames_cancellable};
use tokenizers::Tokenizer;
use tokio_util::sync::CancellationToken;

use crate::gemma_audio::{GemmaAudioError, GemmaAudioTower};
use crate::gemma_audio_decode::{AudioDecodeError, decode_audio_16k_cancellable};
use crate::gemma_audio_features::{GemmaAudioFeatureExtractor, GemmaAudioFeaturesError};
use crate::gemma_compute::GemmaCompute;
use crate::gemma_fusion::{GemmaFusionEncoder, GemmaFusionError};
use crate::gemma_multimodal::{GemmaModality, GemmaProjection, GemmaProjectionError};
use crate::gemma_probe::{GemmaProbeError, GemmaTextTask};
use crate::gemma_vision::{GemmaVisionError, GemmaVisionTower};
use crate::gemma_visual::{
    GemmaVisualError, VisualKind, prepare_frame_cancellable, validate_processor_config_file,
};

/// Which media a caller has explicitly enabled for this encoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GemmaMedia {
    /// Standalone image embeddings.
    pub images: bool,
    /// Mono 16-kHz PCM embeddings.
    pub audio: bool,
    /// Sampled video-frame embeddings.
    pub video: bool,
}

/// A video embedding and the sampled source timestamps that support it.
pub struct GemmaVideoEmbedding {
    /// Unit-length vector with the selected embedding dimension.
    pub vector: Vec<f32>,
    /// Presentation timestamps, in milliseconds, for every retained frame.
    pub timestamps_ms: Vec<u64>,
}

/// Original, independently verified upstream model files; no repacking or copy is required.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GemmaNativeFiles {
    /// BF16/F32 safetensors checkpoint.
    pub weights: PathBuf,
    /// Hugging Face tokenizer JSON.
    pub tokenizer: PathBuf,
    /// EmbeddingGemma 2 model configuration.
    pub config: PathBuf,
    /// Image and video processor configuration.
    pub visual_processor: PathBuf,
    /// Audio processor configuration.
    pub audio_processor: PathBuf,
}

impl GemmaNativeFiles {
    /// Resolve the complete original-file set returned by the signed component manager.
    pub fn from_original_files(
        files: &BTreeMap<String, PathBuf>,
    ) -> Result<Self, GemmaNativeError> {
        let required = |name: &str| {
            files
                .get(name)
                .cloned()
                .ok_or(GemmaNativeError::Input("incomplete original model files"))
        };
        Ok(Self {
            weights: required("model.safetensors")?,
            tokenizer: required("tokenizer.json")?,
            config: required("config.json")?,
            visual_processor: required("processor_config.json")?,
            audio_processor: required("preprocessor_config.json")?,
        })
    }

    /// Resolve the original upstream filenames in a development cache.
    #[must_use]
    pub fn from_directory(directory: &Path) -> Self {
        Self {
            weights: directory.join("model.safetensors"),
            tokenizer: directory.join("tokenizer.json"),
            config: directory.join("config.json"),
            visual_processor: directory.join("processor_config.json"),
            audio_processor: directory.join("preprocessor_config.json"),
        }
    }
}

#[cfg(test)]
mod original_files_tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn resolves_original_files_without_repacking_and_rejects_incomplete_installation() {
        let files: BTreeMap<_, _> = [
            "model.safetensors",
            "tokenizer.json",
            "config.json",
            "processor_config.json",
            "preprocessor_config.json",
        ]
        .into_iter()
        .map(|name| (name.to_owned(), PathBuf::from("/verified").join(name)))
        .collect();
        let resolved = GemmaNativeFiles::from_original_files(&files).unwrap();
        assert_eq!(resolved.weights, files["model.safetensors"]);
        assert_eq!(resolved.audio_processor, files["preprocessor_config.json"]);
        let mut incomplete = files;
        incomplete.remove("tokenizer.json");
        assert!(GemmaNativeFiles::from_original_files(&incomplete).is_err());
    }
}

/// Native text and optional media encoder. A single language model serves all modalities.
pub struct GemmaNativeEncoder {
    compute: GemmaCompute,
    language: GemmaFusionEncoder,
    tokenizer: Tokenizer,
    vision: Option<(GemmaVisionTower, GemmaProjection)>,
    audio: Option<(GemmaAudioFeatureExtractor, GemmaAudioTower, GemmaProjection)>,
    media: GemmaMedia,
    dimensions: usize,
    bos: u32,
    eos: u32,
    boi: u32,
    eoi: u32,
    boa: u32,
    eoa: u32,
    image_id: u32,
    video_id: u32,
    audio_id: u32,
}

impl GemmaNativeEncoder {
    /// Media towers enabled at construction.
    #[must_use]
    pub const fn media(&self) -> GemmaMedia {
        self.media
    }
    /// Selected Matryoshka embedding width.
    #[must_use]
    pub const fn dimensions(&self) -> usize {
        self.dimensions
    }

    /// Number of FP32 GEMM dispatches actually executed on the GPU.
    #[must_use]
    pub fn metal_dispatches(&self) -> usize {
        self.compute.dispatches()
    }

    /// Explicit local-only Metal probe.
    #[cfg(all(target_os = "macos", feature = "gemma-metal"))]
    pub fn open_metal(
        directory: &Path,
        dimensions: usize,
        media: GemmaMedia,
    ) -> Result<Self, GemmaNativeError> {
        Self::open_metal_files(
            &GemmaNativeFiles::from_directory(directory),
            dimensions,
            media,
        )
    }

    /// Load host-verified original files for a macOS Metal image worker.
    #[cfg(all(target_os = "macos", feature = "gemma-metal"))]
    pub fn open_metal_files(
        files: &GemmaNativeFiles,
        dimensions: usize,
        media: GemmaMedia,
    ) -> Result<Self, GemmaNativeError> {
        if !media.images || media.audio || media.video {
            return Err(GemmaNativeError::Input(
                "Metal probe supports standalone images only",
            ));
        }
        let compute = GemmaCompute::metal().ok_or(GemmaNativeError::MetalUnavailable)?;
        Self::open_files_with_compute(files, dimensions, media, compute)
    }

    /// Load the pinned local BF16/F32 checkpoint for FP32 CPU inference.
    pub fn open(
        directory: &Path,
        dimensions: usize,
        media: GemmaMedia,
    ) -> Result<Self, GemmaNativeError> {
        Self::open_files(
            &GemmaNativeFiles::from_directory(directory),
            dimensions,
            media,
        )
    }

    /// Load the original pinned model files at independent verified paths, offline.
    pub fn open_files(
        files: &GemmaNativeFiles,
        dimensions: usize,
        media: GemmaMedia,
    ) -> Result<Self, GemmaNativeError> {
        Self::open_files_with_compute(files, dimensions, media, GemmaCompute::Cpu)
    }

    fn open_files_with_compute(
        files: &GemmaNativeFiles,
        dimensions: usize,
        media: GemmaMedia,
        compute: GemmaCompute,
    ) -> Result<Self, GemmaNativeError> {
        if ![128, 256, 512, 768].contains(&dimensions) {
            return Err(GemmaNativeError::Input("unsupported embedding dimension"));
        }
        let config: serde_json::Value = serde_json::from_reader(File::open(&files.config)?)?;
        let id = |name: &str| {
            config[name]
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .ok_or(GemmaNativeError::Input("missing media token ID"))
        };
        let text_id = |name: &str| {
            config["text_config"][name]
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .ok_or(GemmaNativeError::Input("missing text token ID"))
        };
        let bos = text_id("bos_token_id")?;
        let eos = text_id("eos_token_id")?;
        let boi = id("boi_token_id")?;
        let eoi = id("eoi_token_id")?;
        let boa = id("boa_token_id")?;
        let image_id = id("image_token_id")?;
        let video_id = id("video_token_id")?;
        let audio_id = id("audio_token_id")?;
        let tokenizer =
            Tokenizer::from_file(&files.tokenizer).map_err(GemmaNativeError::Tokenizer)?;
        let eoa = tokenizer
            .token_to_id("<audio|>")
            .ok_or(GemmaNativeError::Input("missing end-of-audio token"))?;
        let language = GemmaFusionEncoder::open_files_with_compute(
            &files.config,
            &files.weights,
            compute.clone(),
        )?;
        let vision = if media.images || media.video {
            validate_processor_config_file(&files.visual_processor)?;
            Some((
                GemmaVisionTower::open_files_with_compute(
                    &files.config,
                    &files.weights,
                    compute.clone(),
                )?,
                GemmaProjection::open_files_with_compute(
                    &files.config,
                    &files.weights,
                    GemmaModality::Vision,
                    compute.clone(),
                )?,
            ))
        } else {
            None
        };
        let audio = if media.audio {
            Some((
                GemmaAudioFeatureExtractor::open_file(&files.audio_processor)?,
                GemmaAudioTower::open_files(&files.config, &files.weights)?,
                GemmaProjection::open_files(&files.config, &files.weights, GemmaModality::Audio)?,
            ))
        } else {
            None
        };
        Ok(Self {
            compute,
            language,
            tokenizer,
            vision,
            audio,
            media,
            dimensions,
            bos,
            eos,
            boi,
            eoi,
            boa,
            eoa,
            image_id,
            video_id,
            audio_id,
        })
    }

    /// Embed a model-formatted text query or titled document.
    pub fn encode_text(
        &self,
        task: GemmaTextTask,
        text: &str,
        title: Option<&str>,
    ) -> Result<Vec<f32>, GemmaNativeError> {
        self.encode_text_cancellable(task, text, title, &CancellationToken::new())
    }

    /// Embed a text query or document while honoring cancellation.
    pub fn encode_text_cancellable(
        &self,
        task: GemmaTextTask,
        text: &str,
        title: Option<&str>,
        cancellation: &CancellationToken,
    ) -> Result<Vec<f32>, GemmaNativeError> {
        if cancellation.is_cancelled() {
            return Err(GemmaNativeError::Cancelled);
        }
        let formatted = task.format(text, title)?;
        let tokens = self
            .tokenizer
            .encode(formatted, true)
            .map_err(GemmaNativeError::Tokenizer)?;
        Ok(self.language.encode_cancellable(
            tokens.get_ids(),
            &[],
            self.dimensions,
            cancellation,
        )?)
    }

    /// Embed a standalone bounded PNG or JPEG image.
    pub fn encode_image(&self, encoded: &[u8]) -> Result<Vec<f32>, GemmaNativeError> {
        self.encode_image_cancellable(encoded, &CancellationToken::new())
    }

    /// Embed an image while honoring the owning job's cancellation.
    pub fn encode_image_cancellable(
        &self,
        encoded: &[u8],
        cancellation: &CancellationToken,
    ) -> Result<Vec<f32>, GemmaNativeError> {
        if cancellation.is_cancelled() {
            return Err(GemmaNativeError::Cancelled);
        }
        if !self.media.images {
            return Err(GemmaNativeError::Input("image embedding is disabled"));
        }
        let mut ids = vec![self.bos];
        let mut replacements = Vec::new();
        self.append_frame(
            encoded,
            VisualKind::Image,
            &mut ids,
            &mut replacements,
            cancellation,
        )?;
        ids.push(self.eos);
        self.fuse(&ids, &replacements, cancellation)
    }

    /// Embed up to 32 timestamped, already decoded video frames. The caller
    /// owns source-authoritative sampling, decoding, and temporal evidence.
    pub fn encode_video_frames(
        &self,
        frames: &[(&[u8], u64)],
    ) -> Result<Vec<f32>, GemmaNativeError> {
        self.encode_video_frames_cancellable(frames, &CancellationToken::new())
    }

    /// Embed timestamped frames while honoring the owning job's cancellation.
    pub fn encode_video_frames_cancellable(
        &self,
        frames: &[(&[u8], u64)],
        cancellation: &CancellationToken,
    ) -> Result<Vec<f32>, GemmaNativeError> {
        if cancellation.is_cancelled() {
            return Err(GemmaNativeError::Cancelled);
        }
        if !self.media.video {
            return Err(GemmaNativeError::Input("video embedding is disabled"));
        }
        if frames.is_empty()
            || frames.len() > 32
            || frames.iter().map(|(bytes, _)| bytes.len()).sum::<usize>() > 64 * 1024 * 1024
            || frames
                .windows(2)
                .any(|pair| pair[1].1 / 1_000 <= pair[0].1 / 1_000)
        {
            return Err(GemmaNativeError::Input("invalid sampled video frames"));
        }
        let mut ids = vec![self.bos];
        let mut replacements = Vec::new();
        for &(encoded, _) in frames {
            self.append_frame(
                encoded,
                VisualKind::VideoFrame,
                &mut ids,
                &mut replacements,
                cancellation,
            )?;
        }
        ids.push(self.eos);
        self.fuse(&ids, &replacements, cancellation)
    }

    /// Decode and embed a bounded H.264-in-MP4/MOV clip at up to one frame per second.
    pub fn encode_h264_video(&self, bytes: &[u8]) -> Result<GemmaVideoEmbedding, GemmaNativeError> {
        self.encode_h264_video_cancellable(bytes, &CancellationToken::new())
    }

    /// Decode, sample, and embed a bounded clip while honoring cancellation.
    pub fn encode_h264_video_cancellable(
        &self,
        bytes: &[u8],
        cancellation: &CancellationToken,
    ) -> Result<GemmaVideoEmbedding, GemmaNativeError> {
        if cancellation.is_cancelled() {
            return Err(GemmaNativeError::Cancelled);
        }
        if !self.media.video {
            return Err(GemmaNativeError::Input("video embedding is disabled"));
        }
        let frames = sample_video_frames_cancellable(bytes, || cancellation.is_cancelled())?;
        let references: Vec<_> = frames
            .iter()
            .map(|frame| (frame.bytes.as_slice(), frame.timestamp_ms))
            .collect();
        let vector = self.encode_video_frames_cancellable(&references, cancellation)?;
        Ok(GemmaVideoEmbedding {
            vector,
            timestamps_ms: frames.iter().map(|frame| frame.timestamp_ms).collect(),
        })
    }

    /// Embed at most 30 seconds of decoded, normalized mono 16-kHz PCM.
    pub fn encode_audio_pcm16k(&self, pcm: &[f32]) -> Result<Vec<f32>, GemmaNativeError> {
        self.encode_audio_pcm16k_cancellable(pcm, &CancellationToken::new())
    }

    /// Embed bounded PCM while honoring cancellation throughout preprocessing and inference.
    pub fn encode_audio_pcm16k_cancellable(
        &self,
        pcm: &[f32],
        cancellation: &CancellationToken,
    ) -> Result<Vec<f32>, GemmaNativeError> {
        if cancellation.is_cancelled() {
            return Err(GemmaNativeError::Cancelled);
        }
        let (extractor, tower, projection) = self
            .audio
            .as_ref()
            .ok_or(GemmaNativeError::Input("audio embedding is disabled"))?;
        let features =
            extractor.extract_pcm16k_cancellable(pcm, pcm.len(), || cancellation.is_cancelled())?;
        let tokens = tower.encode_features_cancellable(
            &features.features,
            features.frame_count,
            features.valid_frames,
            cancellation,
        )?;
        let mut ids = vec![self.bos, self.boa];
        let mut replacements = Vec::with_capacity(tokens.len());
        for token in tokens {
            if cancellation.is_cancelled() {
                return Err(GemmaNativeError::Cancelled);
            }
            replacements.push(projection.project(&token)?);
            ids.push(self.audio_id);
        }
        ids.extend([self.eoa, self.eos]);
        self.fuse(&ids, &replacements, cancellation)
    }

    /// Decode a bounded WAV, FLAC, MP3, or M4A/AAC file before embedding.
    pub fn encode_audio_file(&self, encoded: &[u8]) -> Result<Vec<f32>, GemmaNativeError> {
        self.encode_audio_file_cancellable(encoded, &CancellationToken::new())
    }

    /// Decode and embed bounded audio while honoring cancellation.
    pub fn encode_audio_file_cancellable(
        &self,
        encoded: &[u8],
        cancellation: &CancellationToken,
    ) -> Result<Vec<f32>, GemmaNativeError> {
        if cancellation.is_cancelled() {
            return Err(GemmaNativeError::Cancelled);
        }
        if !self.media.audio {
            return Err(GemmaNativeError::Input("audio embedding is disabled"));
        }
        let pcm = decode_audio_16k_cancellable(encoded, || cancellation.is_cancelled())?;
        self.encode_audio_pcm16k_cancellable(&pcm, cancellation)
    }

    fn append_frame(
        &self,
        encoded: &[u8],
        kind: VisualKind,
        ids: &mut Vec<u32>,
        replacements: &mut Vec<Vec<f32>>,
        cancellation: &CancellationToken,
    ) -> Result<(), GemmaNativeError> {
        if cancellation.is_cancelled() {
            return Err(GemmaNativeError::Cancelled);
        }
        let (tower, projection) = self
            .vision
            .as_ref()
            .ok_or(GemmaNativeError::Input("vision encoder is disabled"))?;
        let prepared = prepare_frame_cancellable(encoded, kind, || cancellation.is_cancelled())?;
        let tokens = tower.encode_patches_cancellable(
            &prepared.pixels,
            &prepared.positions,
            prepared.valid_patches,
            cancellation,
        )?;
        if tokens.len() != prepared.soft_tokens {
            return Err(GemmaNativeError::Input("vision soft-token count mismatch"));
        }
        ids.push(self.boi);
        for token in tokens {
            if cancellation.is_cancelled() {
                return Err(GemmaNativeError::Cancelled);
            }
            replacements.push(projection.project(&token)?);
            ids.push(match kind {
                VisualKind::Image => self.image_id,
                VisualKind::VideoFrame => self.video_id,
            });
        }
        ids.push(self.eoi);
        Ok(())
    }

    fn fuse(
        &self,
        ids: &[u32],
        tokens: &[Vec<f32>],
        cancellation: &CancellationToken,
    ) -> Result<Vec<f32>, GemmaNativeError> {
        if cancellation.is_cancelled() {
            return Err(GemmaNativeError::Cancelled);
        }
        let replacements: Vec<_> = ids
            .iter()
            .enumerate()
            .filter(|(_, id)| [self.image_id, self.video_id, self.audio_id].contains(id))
            .zip(tokens)
            .map(|((position, _), token)| (position, token.as_slice()))
            .collect();
        if replacements.len() != tokens.len() {
            return Err(GemmaNativeError::Input("media token count mismatch"));
        }
        Ok(self
            .language
            .encode_cancellable(ids, &replacements, self.dimensions, cancellation)?)
    }
}

/// Rejected native checkpoint, media input, or inference result.
#[derive(Debug, thiserror::Error)]
pub enum GemmaNativeError {
    /// The owning job no longer permits inference.
    #[error("Gemma inference cancelled")]
    Cancelled,
    /// No Metal device supports the required FP32 GEMM path.
    #[cfg(all(target_os = "macos", feature = "gemma-metal"))]
    #[error("Metal FP32 GEMM is unavailable")]
    MetalUnavailable,
    /// Checkpoint file could not be read.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Checkpoint configuration is invalid JSON.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// Media or dimension is disabled or invalid.
    #[error("invalid native Gemma input: {0}")]
    Input(&'static str),
    /// Source video could not be safely decoded and sampled.
    #[error(transparent)]
    Video(VideoSamplingError),
    /// Local tokenizer could not be read or used.
    #[error("Gemma tokenization failed: {0}")]
    Tokenizer(Box<dyn std::error::Error + Send + Sync>),
    /// Text prompt cannot be formatted for this task.
    #[error(transparent)]
    Prompt(#[from] GemmaProbeError),
    /// Visual decoding or preprocessing failed.
    #[error(transparent)]
    Visual(GemmaVisualError),
    /// Vision inference failed.
    #[error(transparent)]
    Vision(GemmaVisionError),
    /// Audio preprocessing failed.
    #[error(transparent)]
    AudioFeatures(GemmaAudioFeaturesError),
    /// Audio source decoding or resampling failed.
    #[error(transparent)]
    AudioDecode(AudioDecodeError),
    /// Audio inference failed.
    #[error(transparent)]
    Audio(GemmaAudioError),
    /// Projection failed.
    #[error(transparent)]
    Projection(#[from] GemmaProjectionError),
    /// Language-model fusion failed.
    #[error(transparent)]
    Fusion(GemmaFusionError),
}

macro_rules! map_stage_cancellation {
    ($($error:ty => $variant:ident),+ $(,)?) => {
        $(
            impl From<$error> for GemmaNativeError {
                fn from(error: $error) -> Self {
                    match error {
                        <$error>::Cancelled => Self::Cancelled,
                        other => Self::$variant(other),
                    }
                }
            }
        )+
    };
}

map_stage_cancellation!(
    VideoSamplingError => Video,
    GemmaVisualError => Visual,
    GemmaVisionError => Vision,
    GemmaAudioFeaturesError => AudioFeatures,
    AudioDecodeError => AudioDecode,
    GemmaAudioError => Audio,
    GemmaFusionError => Fusion,
);
