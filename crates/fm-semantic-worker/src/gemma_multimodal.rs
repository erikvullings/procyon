//! Experimental native multimodal projection stage; no media inference is exposed.

use std::fs::File;
use std::path::Path;

use lattice_inference::weights::{SafetensorsFile, TensorSource};

const TEXT_HIDDEN_SIZE: usize = 512;

/// The encoder whose soft tokens are projected into Gemma's text hidden space.
#[derive(Debug, Clone, Copy)]
pub enum GemmaModality {
    /// Images and video frames share the vision tower and projection.
    Vision,
    /// Audio segments use the audio tower and projection.
    Audio,
}

impl GemmaModality {
    fn name(self) -> &'static str {
        match self {
            Self::Vision => "vision",
            Self::Audio => "audio",
        }
    }

    fn width(self) -> usize {
        match self {
            Self::Vision => 768,
            Self::Audio => 1536,
        }
    }
}

/// Projects an already encoded vision or audio soft token to a text hidden state.
pub struct GemmaProjection {
    weights: Vec<f32>,
    modality: GemmaModality,
    eps: f32,
}

impl GemmaProjection {
    /// Loads the actual multimodal projection from an offline checkpoint.
    pub fn open(directory: &Path, modality: GemmaModality) -> Result<Self, GemmaProjectionError> {
        Self::open_files(
            &directory.join("config.json"),
            &directory.join("model.safetensors"),
            modality,
        )
    }

    /// Load the original projection config and weights from independent verified paths.
    pub fn open_files(
        config_path: &Path,
        weights_path: &Path,
        modality: GemmaModality,
    ) -> Result<Self, GemmaProjectionError> {
        let config: serde_json::Value = serde_json::from_reader(File::open(config_path)?)?;
        let tower = &config[format!("{}_config", modality.name())];
        let expected_width = modality.width();
        let expected_input = if matches!(modality, GemmaModality::Audio) {
            tower["output_proj_dims"].as_u64()
        } else {
            tower["hidden_size"].as_u64()
        };
        let eps = tower["rms_norm_eps"].as_f64().map(|value| value as f32);
        if config["model_type"] != "embedding_gemma2"
            || expected_input != Some(expected_width as u64)
            || !eps.is_some_and(|value| value.is_finite() && value > 0.0)
            || config["text_config"]["hidden_size"] != TEXT_HIDDEN_SIZE
        {
            return Err(GemmaProjectionError::IncompatibleConfig);
        }

        let mut checkpoint = SafetensorsFile::open(weights_path)?;
        let name = format!("embed_{}.embedding_projection.weight", modality.name());
        let dtype = TensorSource::tensor_dtype(&mut checkpoint, &name)?;
        if !matches!(dtype.as_deref(), Some("BF16" | "F32")) {
            return Err(GemmaProjectionError::UnsupportedDtype(dtype));
        }
        let (weights, shape) = checkpoint.get_f32_tensor_owned(&name)?;
        if shape != [TEXT_HIDDEN_SIZE, expected_width] {
            return Err(GemmaProjectionError::UnexpectedShape(shape));
        }
        if weights.iter().any(|weight| !weight.is_finite()) {
            return Err(GemmaProjectionError::NonFinite);
        }
        Ok(Self {
            weights,
            modality,
            eps: eps.expect("validated finite epsilon"),
        })
    }

    /// Applies the model's scale-free RMS normalization and learned projection.
    pub fn project(&self, token: &[f32]) -> Result<Vec<f32>, GemmaProjectionError> {
        let width = self.modality.width();
        if token.len() != width {
            return Err(GemmaProjectionError::InputLength {
                expected: width,
                actual: token.len(),
            });
        }
        if token.iter().any(|value| !value.is_finite()) {
            return Err(GemmaProjectionError::NonFinite);
        }
        let mean_square = token.iter().map(|value| value * value).sum::<f32>() / width as f32;
        if !mean_square.is_finite() {
            return Err(GemmaProjectionError::NonFinite);
        }
        let inverse_rms = (mean_square + self.eps).sqrt().recip();
        let output: Vec<f32> = self
            .weights
            .chunks_exact(width)
            .map(|row| {
                row.iter()
                    .zip(token)
                    .map(|(weight, value)| weight * (value * inverse_rms))
                    .sum()
            })
            .collect();
        if output.iter().any(|value| !value.is_finite()) {
            return Err(GemmaProjectionError::NonFinite);
        }
        Ok(output)
    }
}

/// Invalid checkpoint, input, or native projection output.
#[derive(Debug, thiserror::Error)]
pub enum GemmaProjectionError {
    /// Local checkpoint could not be opened.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Local configuration is not valid JSON.
    #[error(transparent)]
    Config(#[from] serde_json::Error),
    /// A checkpoint from another architecture or shape was supplied.
    #[error("unsupported EmbeddingGemma 2 multimodal configuration")]
    IncompatibleConfig,
    /// FP16 weights are unsafe for this model.
    #[error("unsupported multimodal weight dtype: {0:?}; expected BF16 or F32")]
    UnsupportedDtype(Option<String>),
    /// The projection weight matrix is the wrong size.
    #[error("unexpected multimodal projection tensor shape: {0:?}")]
    UnexpectedShape(Vec<usize>),
    /// The supplied encoder token is the wrong width.
    #[error("expected a {expected}-element soft token, got {actual}")]
    InputLength {
        /// Required number of encoder dimensions.
        expected: usize,
        /// Number supplied by the caller.
        actual: usize,
    },
    /// Inference cannot proceed on non-finite weights, input or output.
    #[error("non-finite multimodal projection value")]
    NonFinite,
    /// Checkpoint tensor loading failed.
    #[error(transparent)]
    Inference(#[from] lattice_inference::InferenceError),
}

#[cfg(test)]
mod tests {
    use super::{GemmaModality, GemmaProjection, GemmaProjectionError};

    #[test]
    fn fp16_checkpoint_is_rejected_before_inference() {
        let directory = tempfile::tempdir().expect("temporary model directory");
        std::fs::write(
            directory.path().join("config.json"),
            r#"{"model_type":"embedding_gemma2","text_config":{"hidden_size":512},"vision_config":{"hidden_size":768,"rms_norm_eps":0.000001}}"#,
        )
        .expect("config");
        let bytes = 512 * 768 * 2;
        let header = serde_json::json!({
            "embed_vision.embedding_projection.weight": {
                "dtype": "F16",
                "shape": [512, 768],
                "data_offsets": [0, bytes],
            }
        })
        .to_string();
        let mut file = Vec::with_capacity(8 + header.len() + bytes);
        file.extend_from_slice(&(header.len() as u64).to_le_bytes());
        file.extend_from_slice(header.as_bytes());
        file.resize(file.len() + bytes, 0);
        std::fs::write(directory.path().join("model.safetensors"), file).expect("weights");

        assert!(matches!(
            GemmaProjection::open(directory.path(), GemmaModality::Vision),
            Err(GemmaProjectionError::UnsupportedDtype(Some(dtype))) if dtype == "F16"
        ));
    }

    #[test]
    fn malformed_soft_tokens_fail_explicitly() {
        let projection = GemmaProjection {
            weights: vec![0.0; 512 * 768],
            modality: GemmaModality::Vision,
            eps: 1e-6,
        };
        assert!(matches!(
            projection.project(&[0.0; 2]),
            Err(GemmaProjectionError::InputLength {
                expected: 768,
                actual: 2
            })
        ));
        let mut token = vec![0.0; 768];
        token[0] = f32::NAN;
        assert!(matches!(
            projection.project(&token),
            Err(GemmaProjectionError::NonFinite)
        ));
        token[0] = f32::MAX;
        assert!(matches!(
            projection.project(&token),
            Err(GemmaProjectionError::NonFinite)
        ));
    }
}
