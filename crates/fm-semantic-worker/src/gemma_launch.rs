//! Host-side launch description for the experimental Gemma worker.
//!
//! These types carry only verified paths and consent flags, so a desktop host
//! can launch a Gemma worker without linking the native inference stack, Zvec
//! or ONNX Runtime into its own binary.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Host environment variable that forces the Gemma worker onto CPU even when
/// a GPU backend such as Metal is available. Off by default; used for CPU
/// fallback qualification. The host turns it into the explicit
/// [`GEMMA_FORCE_CPU_FLAG`] worker argument.
pub const GEMMA_FORCE_CPU_ENV: &str = "PROCYON_GEMMA_FORCE_CPU";

/// Worker argument that selects the native CPU backend unconditionally.
pub const GEMMA_FORCE_CPU_FLAG: &str = "--gemma-force-cpu";

/// Whether a [`GEMMA_FORCE_CPU_ENV`] value requests CPU-only Gemma inference.
///
/// Only explicit affirmative values (`1`, `true`, `yes`, `on`, case-insensitive)
/// enable it; absent, empty, or any other value keeps automatic selection.
#[must_use]
pub fn gemma_cpu_forced(value: Option<&OsStr>) -> bool {
    value
        .and_then(OsStr::to_str)
        .map(str::trim)
        .is_some_and(|value| {
            ["1", "true", "yes", "on"]
                .iter()
                .any(|accepted| value.eq_ignore_ascii_case(accepted))
        })
}

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

/// The signed component manager returned an incomplete original-file set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("invalid native Gemma input: incomplete original model files")]
pub struct IncompleteGemmaFiles;

impl GemmaNativeFiles {
    /// Resolve the complete original-file set returned by the signed component manager.
    pub fn from_original_files(
        files: &BTreeMap<String, PathBuf>,
    ) -> Result<Self, IncompleteGemmaFiles> {
        let required = |name: &str| files.get(name).cloned().ok_or(IncompleteGemmaFiles);
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
mod tests {
    use super::*;

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
        assert_eq!(
            GemmaNativeFiles::from_original_files(&incomplete),
            Err(IncompleteGemmaFiles)
        );
    }

    #[test]
    fn only_explicit_affirmative_values_force_cpu() {
        for enabled in ["1", "true", "TRUE", " yes ", "On"] {
            assert!(gemma_cpu_forced(Some(OsStr::new(enabled))), "{enabled}");
        }
        assert!(!gemma_cpu_forced(None));
        for disabled in ["", "0", "false", "off", "no", "metal", "2"] {
            assert!(!gemma_cpu_forced(Some(OsStr::new(disabled))), "{disabled}");
        }
    }
}
