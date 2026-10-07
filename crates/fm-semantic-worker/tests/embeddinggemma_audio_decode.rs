//! Bounded native audio decoding and 16-kHz mono resampling.
#![cfg(feature = "gemma-probe")]

#[allow(unreachable_pub)]
#[path = "../src/gemma_audio_decode.rs"]
mod gemma_audio_decode;

use gemma_audio_decode::{AudioDecodeError, decode_audio_16k};

fn wav_sine(sample_rate: u32, channels: u16, frequency: f64, seconds: f64) -> Vec<u8> {
    let frames = (sample_rate as f64 * seconds) as u32;
    let data_bytes = frames * channels as u32 * 2;
    let mut wav = Vec::with_capacity(data_bytes as usize + 44);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * channels as u32 * 2).to_le_bytes());
    wav.extend_from_slice(&(channels * 2).to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_bytes.to_le_bytes());
    for i in 0..frames {
        let value = (0.6
            * (std::f64::consts::TAU * frequency * i as f64 / sample_rate as f64).sin()
            * i16::MAX as f64) as i16;
        for _ in 0..channels {
            wav.extend_from_slice(&value.to_le_bytes());
        }
    }
    wav
}

#[test]
fn resamples_44k_and_48k_sine_without_aliasing_or_rate_drift() {
    for (rate, channels) in [(44_100, 1), (48_000, 2)] {
        let pcm =
            decode_audio_16k(&wav_sine(rate, channels, 440.0, 1.0)).expect("decode supported WAV");
        assert_eq!(pcm.len(), 16_000);
        let rms_error = pcm[256..15_744]
            .iter()
            .enumerate()
            .map(|(index, sample)| {
                let phase = std::f64::consts::TAU * 440.0 * (index + 256) as f64 / 16_000.0;
                (*sample as f64 - 0.6 * phase.sin()).powi(2)
            })
            .sum::<f64>()
            / 15_488.0;
        assert!(
            rms_error.sqrt() < 0.008,
            "{rate} Hz sine resampling RMSE {}",
            rms_error.sqrt()
        );
    }
    let filtered =
        decode_audio_16k(&wav_sine(48_000, 1, 12_000.0, 1.0)).expect("decode high-frequency sine");
    let rms = (filtered[256..15_744]
        .iter()
        .map(|sample| f64::from(*sample).powi(2))
        .sum::<f64>()
        / 15_488.0)
        .sqrt();
    assert!(rms < 0.01, "12-kHz alias leaked into 16-kHz output: {rms}");
}

#[test]
fn corrupt_and_truncated_audio_returns_errors_not_partial_pcm() {
    assert!(matches!(
        decode_audio_16k(&[]),
        Err(AudioDecodeError::EncodedLength)
    ));
    assert!(matches!(
        decode_audio_16k(&vec![0; 64 * 1024 * 1024 + 1]),
        Err(AudioDecodeError::EncodedLength)
    ));
    assert!(decode_audio_16k(&[0u8; 64]).is_err());
    let mut truncated = wav_sine(44_100, 1, 440.0, 1.0);
    truncated.truncate(1000);
    assert!(decode_audio_16k(&truncated).is_err());
    let mut oversized = wav_sine(48_000, 1, 440.0, 1.0);
    oversized[24..28].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(
        decode_audio_16k(&oversized),
        Err(AudioDecodeError::UnsupportedSource)
    ));
    assert!(matches!(
        decode_audio_16k(&wav_sine(44_100, 1, 440.0, 31.0)),
        Err(AudioDecodeError::Limit)
    ));
    assert!(matches!(
        decode_audio_16k(include_bytes!("fixtures/gemma-audio-unsupported-alac.m4a")),
        Err(AudioDecodeError::UnsupportedCodec)
    ));
}

#[test]
fn native_16k_audio_preserves_pcm_samples() {
    let output = decode_audio_16k(&wav_sine(16_000, 1, 440.0, 0.3)).expect("16-kHz WAV");
    assert_eq!(output.len(), 4_800);
    let reference = 0.6 * (std::f64::consts::TAU * 440.0 / 16_000.0).sin();
    assert!((f64::from(output[1]) - reference).abs() < 0.00005);
}

#[test]
fn flac_mp3_and_m4a_aac_decode_without_external_codecs() {
    for (format, encoded) in [
        (
            "FLAC",
            include_bytes!("fixtures/gemma-audio-440hz-48k.flac").as_slice(),
        ),
        (
            "MP3",
            include_bytes!("fixtures/gemma-audio-440hz-44k.mp3").as_slice(),
        ),
        (
            "M4A/AAC",
            include_bytes!("fixtures/gemma-audio-440hz-48k.m4a").as_slice(),
        ),
    ] {
        let output = decode_audio_16k(encoded).unwrap_or_else(|error| panic!("{format}: {error}"));
        assert!(
            (7_700..=8_500).contains(&output.len()),
            "{format}: {} output samples",
            output.len()
        );
        let rms =
            (output.iter().map(|sample| sample.powi(2)).sum::<f32>() / output.len() as f32).sqrt();
        assert!(rms > 0.04 && rms < 0.2, "{format}: unexpected RMS {rms}");
    }
    let encoded = include_bytes!("fixtures/gemma-audio-440hz-48k.flac");
    assert!(decode_audio_16k(&encoded[..encoded.len() / 2]).is_err());
    let encoded = include_bytes!("fixtures/gemma-audio-440hz-44k.mp3");
    assert!(decode_audio_16k(&encoded[..encoded.len() / 2]).is_err());
    let encoded = include_bytes!("fixtures/gemma-audio-440hz-48k.m4a");
    assert!(decode_audio_16k(&encoded[..encoded.len() / 2]).is_err());
}
