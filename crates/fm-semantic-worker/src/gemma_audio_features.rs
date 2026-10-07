//! Checkpoint-pinned Gemma4 semicausal 16-kHz PCM to 128-band audio features.

use std::fs::File;
use std::path::Path;

const SAMPLE_RATE: usize = 16_000;
const FRAME: usize = 320;
const HOP: usize = 160;
const FFT: usize = 512;
const BANDS: usize = 128;
const MAX_SAMPLES: usize = 480_000;

/// Rejected checkpoint configuration or invalid PCM.
#[derive(Debug, thiserror::Error)]
pub enum GemmaAudioFeaturesError {
    /// The owning job cancelled preprocessing.
    #[error("Gemma audio preprocessing cancelled")]
    Cancelled,
    /// Cannot read the pinned local processor configuration.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Invalid processor JSON.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// Processor parameters differ from the supported Gemma4 configuration.
    #[error("unsupported Gemma4 audio preprocessing configuration")]
    Config,
    /// PCM or padding length is outside supported bounds.
    #[error("audio must contain at least 161 and at most 480000 samples, with bounded padding")]
    InvalidLength,
    /// Audio samples cannot be NaN or infinite.
    #[error("non-finite PCM sample")]
    NonFinite,
}

/// 128-band row-major audio features and the contiguous frame validity mask.
pub struct GemmaAudioFeatures {
    /// Row-major `frame_count × 128` features, zero in invalid frames.
    pub features: Vec<f32>,
    /// Number of frames, including right padding.
    pub frame_count: usize,
    /// Number of valid leading frames; all remaining frames are masked.
    pub valid_frames: usize,
}

/// Offline Gemma4 feature extractor; the caller supplies decoded mono 16-kHz PCM.
pub struct GemmaAudioFeatureExtractor {
    window: [f32; FRAME],
    mel_filters: Vec<f64>,
}

impl GemmaAudioFeatureExtractor {
    /// Verify the pinned audio preprocessing parameters before extracting features.
    pub fn open(directory: &Path) -> Result<Self, GemmaAudioFeaturesError> {
        Self::open_file(&directory.join("preprocessor_config.json"))
    }

    /// Open an independently verified original audio processor file.
    pub fn open_file(path: &Path) -> Result<Self, GemmaAudioFeaturesError> {
        let config: serde_json::Value = serde_json::from_reader(File::open(path)?)?;
        if config["feature_extractor_type"] != "Gemma4AudioFeatureExtractor"
            || config["feature_size"] != BANDS
            || config["sampling_rate"] != SAMPLE_RATE
            || config["frame_length"] != FRAME
            || config["hop_length"] != HOP
            || config["fft_length"] != FFT
            || config["min_frequency"] != 0.0
            || config["max_frequency"] != 8000.0
            || config["preemphasis"] != 0.0
            || config["dither"] != 0.0
            || config["input_scale_factor"] != 1.0
            || config["mel_floor"] != 0.001
            || config["fft_overdrive"] != false
            || config["preemphasis_htk_flavor"] != true
            || config["padding_value"] != 0.0
            || config["padding_side"] != "right"
            || config["return_attention_mask"] != true
            || !config["per_bin_mean"].is_null()
            || !config["per_bin_stddev"].is_null()
        {
            return Err(GemmaAudioFeaturesError::Config);
        }
        let window = std::array::from_fn(|i| {
            (0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / FRAME as f64).cos()) as f32
        });
        let mel_max = 2595.0 * (1.0_f64 + 8000.0 / 700.0).log10();
        let frequencies: Vec<_> = (0..BANDS + 2)
            .map(|band| {
                let mel = mel_max * band as f64 / (BANDS + 1) as f64;
                700.0 * (10.0_f64.powf(mel / 2595.0) - 1.0)
            })
            .collect();
        let mut mel_filters = vec![0.0; (FFT / 2 + 1) * BANDS];
        for bin in 0..=FFT / 2 {
            let frequency = bin as f64 * SAMPLE_RATE as f64 / FFT as f64;
            for band in 0..BANDS {
                let rising =
                    (frequency - frequencies[band]) / (frequencies[band + 1] - frequencies[band]);
                let falling = (frequencies[band + 2] - frequency)
                    / (frequencies[band + 2] - frequencies[band + 1]);
                mel_filters[bin * BANDS + band] = rising.min(falling).max(0.0);
            }
        }
        Ok(Self {
            window,
            mel_filters,
        })
    }

    /// Transform mono 16-kHz normalized PCM. `padded_samples` is the longest
    /// unpadded sample count in its batch (use `pcm.len()` for a single item).
    /// As upstream does, right padding rounds that count up to 128 samples.
    pub fn extract_pcm16k(
        &self,
        pcm: &[f32],
        padded_samples: usize,
    ) -> Result<GemmaAudioFeatures, GemmaAudioFeaturesError> {
        self.extract_pcm16k_cancellable(pcm, padded_samples, || false)
    }

    /// Extract features while checking for cancellation between frames.
    pub fn extract_pcm16k_cancellable(
        &self,
        pcm: &[f32],
        padded_samples: usize,
        is_cancelled: impl Fn() -> bool,
    ) -> Result<GemmaAudioFeatures, GemmaAudioFeaturesError> {
        if is_cancelled() {
            return Err(GemmaAudioFeaturesError::Cancelled);
        }
        if pcm.len() <= FRAME / 2 || padded_samples < pcm.len() || padded_samples > MAX_SAMPLES {
            return Err(GemmaAudioFeaturesError::InvalidLength);
        }
        let padded = padded_samples.next_multiple_of(128);
        if padded > MAX_SAMPLES {
            return Err(GemmaAudioFeaturesError::InvalidLength);
        }
        if pcm.iter().any(|sample| !sample.is_finite()) {
            return Err(GemmaAudioFeaturesError::NonFinite);
        }

        // The prepended 160 zero samples center the first semicausal frame at t=0.
        // The additional sample in each 321-sample unfold is excluded when
        // preemphasis is disabled, but still determines the number of frames.
        let frame_count = (padded - FRAME / 2 - 1) / HOP + 1;
        let valid_frames = (pcm.len() - FRAME / 2 - 1) / HOP + 1;
        let mut features = vec![0.0; frame_count * BANDS];
        let mut real = [0.0_f64; FFT];
        let mut imaginary = [0.0_f64; FFT];
        for frame in 0..valid_frames {
            if is_cancelled() {
                return Err(GemmaAudioFeaturesError::Cancelled);
            }
            real.fill(0.0);
            imaginary.fill(0.0);
            for (sample, value) in real[..FRAME].iter_mut().enumerate() {
                let input_index = frame * HOP + sample;
                if input_index >= FRAME / 2 {
                    let index = input_index - FRAME / 2;
                    if index < pcm.len() {
                        *value = (pcm[index] * self.window[sample]) as f64;
                    }
                }
            }
            fft(&mut real, &mut imaginary);
            let output = &mut features[frame * BANDS..(frame + 1) * BANDS];
            let mut mel = [0.0_f64; BANDS];
            for bin in 0..=FFT / 2 {
                let magnitude = real[bin].hypot(imaginary[bin]);
                for (band, value) in mel.iter_mut().enumerate() {
                    *value += magnitude * self.mel_filters[bin * BANDS + band];
                }
            }
            for (value, spectral) in output.iter_mut().zip(mel) {
                *value = (spectral + 0.001).ln() as f32;
            }
        }
        Ok(GemmaAudioFeatures {
            features,
            frame_count,
            valid_frames,
        })
    }
}

fn fft(real: &mut [f64; FFT], imaginary: &mut [f64; FFT]) {
    for index in 1..FFT {
        let swapped =
            index.reverse_bits() >> (usize::BITS as usize - FFT.trailing_zeros() as usize);
        if index < swapped {
            real.swap(index, swapped);
            imaginary.swap(index, swapped);
        }
    }
    let mut span = 2;
    while span <= FFT {
        for start in (0..FFT).step_by(span) {
            for offset in 0..span / 2 {
                let angle = -std::f64::consts::TAU * offset as f64 / span as f64;
                let (sine, cosine) = angle.sin_cos();
                let other = start + offset + span / 2;
                let a = real[other] * cosine - imaginary[other] * sine;
                let b = real[other] * sine + imaginary[other] * cosine;
                let current = start + offset;
                real[other] = real[current] - a;
                imaginary[other] = imaginary[current] - b;
                real[current] += a;
                imaginary[current] += b;
            }
        }
        span *= 2;
    }
}
