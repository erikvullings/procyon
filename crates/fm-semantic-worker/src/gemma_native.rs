//! Experimental offline CPU inference for the pinned multimodal checkpoint.

use std::fs::File;
use std::path::Path;

use fm_metadata::{VideoSamplingError, sample_video_frames};
use tokenizers::Tokenizer;

use crate::gemma_audio::{GemmaAudioError, GemmaAudioTower};
use crate::gemma_audio_decode::{AudioDecodeError, decode_audio_16k};
use crate::gemma_audio_features::{GemmaAudioFeatureExtractor, GemmaAudioFeaturesError};
use crate::gemma_fusion::{GemmaFusionEncoder, GemmaFusionError};
use crate::gemma_multimodal::{GemmaModality, GemmaProjection, GemmaProjectionError};
use crate::gemma_probe::{GemmaProbeError, GemmaTextTask};
use crate::gemma_vision::{GemmaVisionError, GemmaVisionTower};
use crate::gemma_visual::{GemmaVisualError, VisualKind, prepare_frame, validate_processor_config};

/// Which media a caller has explicitly enabled for this encoder.
#[derive(Debug, Clone, Copy)]
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

/// Native text and optional media encoder. A single language model serves all modalities.
pub struct GemmaNativeEncoder {
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
    /// Load the pinned local BF16/F32 checkpoint for FP32 CPU inference.
    pub fn open(
        directory: &Path,
        dimensions: usize,
        media: GemmaMedia,
    ) -> Result<Self, GemmaNativeError> {
        if ![128, 256, 512, 768].contains(&dimensions) {
            return Err(GemmaNativeError::Input("unsupported embedding dimension"));
        }
        let config: serde_json::Value =
            serde_json::from_reader(File::open(directory.join("config.json"))?)?;
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
        let tokenizer = Tokenizer::from_file(directory.join("tokenizer.json"))
            .map_err(GemmaNativeError::Tokenizer)?;
        let eoa = tokenizer
            .token_to_id("<audio|>")
            .ok_or(GemmaNativeError::Input("missing end-of-audio token"))?;
        let language = GemmaFusionEncoder::open(directory)?;
        let vision = if media.images || media.video {
            validate_processor_config(directory)?;
            Some((
                GemmaVisionTower::open(directory)?,
                GemmaProjection::open(directory, GemmaModality::Vision)?,
            ))
        } else {
            None
        };
        let audio = if media.audio {
            Some((
                GemmaAudioFeatureExtractor::open(directory)?,
                GemmaAudioTower::open(directory)?,
                GemmaProjection::open(directory, GemmaModality::Audio)?,
            ))
        } else {
            None
        };
        Ok(Self {
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
        let formatted = task.format(text, title)?;
        let tokens = self
            .tokenizer
            .encode(formatted, true)
            .map_err(GemmaNativeError::Tokenizer)?;
        Ok(self
            .language
            .encode(tokens.get_ids(), &[], self.dimensions)?)
    }

    /// Embed a standalone bounded PNG or JPEG image.
    pub fn encode_image(&self, encoded: &[u8]) -> Result<Vec<f32>, GemmaNativeError> {
        if !self.media.images {
            return Err(GemmaNativeError::Input("image embedding is disabled"));
        }
        let mut ids = vec![self.bos];
        let mut replacements = Vec::new();
        self.append_frame(encoded, VisualKind::Image, &mut ids, &mut replacements)?;
        ids.push(self.eos);
        self.fuse(&ids, &replacements)
    }

    /// Embed up to 32 timestamped, already decoded video frames. The caller
    /// owns source-authoritative sampling, decoding, and temporal evidence.
    pub fn encode_video_frames(
        &self,
        frames: &[(&[u8], u64)],
    ) -> Result<Vec<f32>, GemmaNativeError> {
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
            self.append_frame(encoded, VisualKind::VideoFrame, &mut ids, &mut replacements)?;
        }
        ids.push(self.eos);
        self.fuse(&ids, &replacements)
    }

    /// Decode and embed a bounded H.264-in-MP4/MOV clip at up to one frame per second.
    pub fn encode_h264_video(&self, bytes: &[u8]) -> Result<GemmaVideoEmbedding, GemmaNativeError> {
        if !self.media.video {
            return Err(GemmaNativeError::Input("video embedding is disabled"));
        }
        let frames = sample_video_frames(bytes)?;
        let references: Vec<_> = frames
            .iter()
            .map(|frame| (frame.bytes.as_slice(), frame.timestamp_ms))
            .collect();
        let vector = self.encode_video_frames(&references)?;
        Ok(GemmaVideoEmbedding {
            vector,
            timestamps_ms: frames.iter().map(|frame| frame.timestamp_ms).collect(),
        })
    }

    /// Embed at most 30 seconds of decoded, normalized mono 16-kHz PCM.
    pub fn encode_audio_pcm16k(&self, pcm: &[f32]) -> Result<Vec<f32>, GemmaNativeError> {
        let (extractor, tower, projection) = self
            .audio
            .as_ref()
            .ok_or(GemmaNativeError::Input("audio embedding is disabled"))?;
        let features = extractor.extract_pcm16k(pcm, pcm.len())?;
        let tokens = tower.encode_features(
            &features.features,
            features.frame_count,
            features.valid_frames,
        )?;
        let mut ids = vec![self.bos, self.boa];
        let mut replacements = Vec::with_capacity(tokens.len());
        for token in tokens {
            replacements.push(projection.project(&token)?);
            ids.push(self.audio_id);
        }
        ids.extend([self.eoa, self.eos]);
        self.fuse(&ids, &replacements)
    }

    /// Decode a bounded WAV, FLAC, MP3, or M4A/AAC file before embedding.
    pub fn encode_audio_file(&self, encoded: &[u8]) -> Result<Vec<f32>, GemmaNativeError> {
        if !self.media.audio {
            return Err(GemmaNativeError::Input("audio embedding is disabled"));
        }
        let pcm = decode_audio_16k(encoded)?;
        self.encode_audio_pcm16k(&pcm)
    }

    fn append_frame(
        &self,
        encoded: &[u8],
        kind: VisualKind,
        ids: &mut Vec<u32>,
        replacements: &mut Vec<Vec<f32>>,
    ) -> Result<(), GemmaNativeError> {
        let (tower, projection) = self
            .vision
            .as_ref()
            .ok_or(GemmaNativeError::Input("vision encoder is disabled"))?;
        let prepared = prepare_frame(encoded, kind)?;
        let tokens = tower.encode_patches(
            &prepared.pixels,
            &prepared.positions,
            prepared.valid_patches,
        )?;
        if tokens.len() != prepared.soft_tokens {
            return Err(GemmaNativeError::Input("vision soft-token count mismatch"));
        }
        ids.push(self.boi);
        for token in tokens {
            replacements.push(projection.project(&token)?);
            ids.push(match kind {
                VisualKind::Image => self.image_id,
                VisualKind::VideoFrame => self.video_id,
            });
        }
        ids.push(self.eoi);
        Ok(())
    }

    fn fuse(&self, ids: &[u32], tokens: &[Vec<f32>]) -> Result<Vec<f32>, GemmaNativeError> {
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
        Ok(self.language.encode(ids, &replacements, self.dimensions)?)
    }
}

/// Rejected native checkpoint, media input, or inference result.
#[derive(Debug, thiserror::Error)]
pub enum GemmaNativeError {
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
    Video(#[from] VideoSamplingError),
    /// Local tokenizer could not be read or used.
    #[error("Gemma tokenization failed: {0}")]
    Tokenizer(Box<dyn std::error::Error + Send + Sync>),
    /// Text prompt cannot be formatted for this task.
    #[error(transparent)]
    Prompt(#[from] GemmaProbeError),
    /// Visual decoding or preprocessing failed.
    #[error(transparent)]
    Visual(#[from] GemmaVisualError),
    /// Vision inference failed.
    #[error(transparent)]
    Vision(#[from] GemmaVisionError),
    /// Audio preprocessing failed.
    #[error(transparent)]
    AudioFeatures(#[from] GemmaAudioFeaturesError),
    /// Audio source decoding or resampling failed.
    #[error(transparent)]
    AudioDecode(#[from] AudioDecodeError),
    /// Audio inference failed.
    #[error(transparent)]
    Audio(#[from] GemmaAudioError),
    /// Projection failed.
    #[error(transparent)]
    Projection(#[from] GemmaProjectionError),
    /// Language-model fusion failed.
    #[error(transparent)]
    Fusion(#[from] GemmaFusionError),
}
