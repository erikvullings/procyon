//! Bounded, pure-Rust audio decoding and antialiased mono 16-kHz resampling.

use std::io::{Cursor, ErrorKind};

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{
    CODEC_TYPE_AAC, CODEC_TYPE_FLAC, CODEC_TYPE_MP3, CODEC_TYPE_PCM_F32LE, CODEC_TYPE_PCM_F64LE,
    CODEC_TYPE_PCM_S16LE, CODEC_TYPE_PCM_S24LE, CODEC_TYPE_PCM_S32LE, CODEC_TYPE_PCM_U8,
    DecoderOptions,
};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

const MAX_ENCODED_BYTES: usize = 64 * 1024 * 1024;
const MAX_DURATION_SECONDS: usize = 30;
const MAX_OUTPUT_SAMPLES: usize = 480_000;
const MAX_PACKET_BYTES: usize = 1024 * 1024;
const MAX_FRAMES_PER_PACKET: usize = 65_536;
const MAX_PACKETS: usize = 100_000;
const SINC_RADIUS: isize = 48;
const OUTPUT_RATE: u32 = 16_000;

/// Audio cannot be decoded within the supported model and resource limits.
#[derive(Debug, thiserror::Error)]
pub enum AudioDecodeError {
    /// Encoded input exceeds the fixed resource limit.
    #[error("encoded audio must contain between 1 and 67108864 bytes")]
    EncodedLength,
    /// File is invalid or a frame could not be decoded completely.
    #[error("invalid or truncated audio: {0}")]
    Corrupt(String),
    /// Container or codec lacks a supported pure-Rust decoder.
    #[error("unsupported audio codec or container")]
    UnsupportedCodec,
    /// Track format, channel count, or source sample rate cannot be safely processed.
    #[error("unsupported audio channel count, sample rate, or frame geometry")]
    UnsupportedSource,
    /// Audio exceeds the 30-second, packet, or decoded-sample limit.
    #[error("audio exceeds duration, packet, or decoded-sample limits")]
    Limit,
}

/// Decode bounded WAV/FLAC/MP3/M4A-AAC bytes to finite mono 16-kHz PCM.
///
/// The caller retains authority over file access and may reject the source
/// before reading it. Inputs longer than 30 seconds are rejected, not clipped.
pub fn decode_audio_16k(encoded: &[u8]) -> Result<Vec<f32>, AudioDecodeError> {
    if encoded.is_empty() || encoded.len() > MAX_ENCODED_BYTES {
        return Err(AudioDecodeError::EncodedLength);
    }
    let stream =
        MediaSourceStream::new(Box::new(Cursor::new(encoded.to_vec())), Default::default());
    let probed = symphonia::default::get_probe()
        .format(
            &Hint::new(),
            stream,
            &FormatOptions {
                enable_gapless: true,
                ..Default::default()
            },
            &MetadataOptions::default(),
        )
        .map_err(probe_error)?;
    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or(AudioDecodeError::UnsupportedCodec)?;
    let codec = track.codec_params.codec;
    if !matches!(
        codec,
        CODEC_TYPE_AAC
            | CODEC_TYPE_FLAC
            | CODEC_TYPE_MP3
            | CODEC_TYPE_PCM_F32LE
            | CODEC_TYPE_PCM_F64LE
            | CODEC_TYPE_PCM_S16LE
            | CODEC_TYPE_PCM_S24LE
            | CODEC_TYPE_PCM_S32LE
            | CODEC_TYPE_PCM_U8
    ) {
        return Err(AudioDecodeError::UnsupportedCodec);
    }
    let rate = track
        .codec_params
        .sample_rate
        .ok_or(AudioDecodeError::UnsupportedSource)?;
    let mut channels = track.codec_params.channels.map(|channels| channels.count());
    if !(8_000..=96_000).contains(&rate) || channels.is_some_and(|count| !(1..=2).contains(&count))
    {
        return Err(AudioDecodeError::UnsupportedSource);
    }
    let expected_frames = track.codec_params.n_frames.and_then(|count| {
        track.codec_params.time_base.map_or(Some(count), |base| {
            (u128::from(count) * u128::from(base.numer) * u128::from(rate))
                .checked_div(u128::from(base.denom))
                .and_then(|frames| u64::try_from(frames).ok())
        })
    });
    let sample_timebase = track
        .codec_params
        .time_base
        .is_some_and(|base| u64::from(base.numer) * u64::from(rate) == u64::from(base.denom));
    if expected_frames.is_some_and(|count| count > u64::from(rate) * MAX_DURATION_SECONDS as u64) {
        return Err(AudioDecodeError::Limit);
    }
    if track
        .codec_params
        .max_frames_per_packet
        .is_some_and(|count| count > MAX_FRAMES_PER_PACKET as u64)
    {
        return Err(AudioDecodeError::Limit);
    }
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions { verify: true })
        .map_err(probe_error)?;
    let mut mono = Vec::new();
    let mut packet_count = 0;
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(io)) if io.kind() == ErrorKind::UnexpectedEof => break,
            Err(error) => return Err(AudioDecodeError::Corrupt(error.to_string())),
        };
        packet_count += 1;
        if packet_count > MAX_PACKETS || packet.data.len() > MAX_PACKET_BYTES {
            return Err(AudioDecodeError::Limit);
        }
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = decoder
            .decode(&packet)
            .map_err(|error| AudioDecodeError::Corrupt(error.to_string()))?;
        if decoded.capacity() > MAX_FRAMES_PER_PACKET || decoded.frames() > MAX_FRAMES_PER_PACKET {
            return Err(AudioDecodeError::Limit);
        }
        let spec = *decoded.spec();
        let decoded_channels = spec.channels.count();
        if spec.rate != rate
            || !(1..=2).contains(&decoded_channels)
            || channels.is_some_and(|count| count != decoded_channels)
        {
            return Err(AudioDecodeError::UnsupportedSource);
        }
        channels = Some(decoded_channels);
        let start = (packet.trim_start as usize).min(decoded.frames());
        let end = decoded.frames().saturating_sub(packet.trim_end as usize);
        let end = if sample_timebase {
            end.min(start.saturating_add(packet.dur as usize))
        } else {
            end
        };
        if end < start {
            return Err(AudioDecodeError::Corrupt("invalid packet trim".into()));
        }
        if mono.len() + (end - start) > rate as usize * MAX_DURATION_SECONDS {
            return Err(AudioDecodeError::Limit);
        }
        let mut buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        buffer.copy_interleaved_ref(decoded);
        for frame in buffer
            .samples()
            .chunks_exact(decoded_channels)
            .skip(start)
            .take(end - start)
        {
            let sample = frame.iter().copied().sum::<f32>() / decoded_channels as f32;
            if !sample.is_finite() || sample.abs() > 1.0 {
                return Err(AudioDecodeError::Corrupt(
                    "decoded sample outside normalized PCM range".into(),
                ));
            }
            mono.push(sample);
        }
    }
    if mono.is_empty() {
        return Err(AudioDecodeError::Corrupt("no decoded samples".into()));
    }
    if expected_frames.is_some_and(|count| {
        let deficit = count.saturating_sub(mono.len() as u64);
        deficit
            > if codec == CODEC_TYPE_MP3 || codec == CODEC_TYPE_AAC {
                256
            } else {
                0
            }
    }) {
        return Err(AudioDecodeError::Corrupt(
            "decoded fewer frames than the source declares".into(),
        ));
    }
    resample(&mono, rate)
}

fn probe_error(error: SymphoniaError) -> AudioDecodeError {
    match error {
        SymphoniaError::Unsupported(_) => AudioDecodeError::UnsupportedCodec,
        other => AudioDecodeError::Corrupt(other.to_string()),
    }
}

fn resample(source: &[f32], rate: u32) -> Result<Vec<f32>, AudioDecodeError> {
    if rate == OUTPUT_RATE {
        return Ok(source.to_vec());
    }
    let output_len =
        (source.len() as u64 * u64::from(OUTPUT_RATE) + u64::from(rate / 2)) / u64::from(rate);
    if output_len > MAX_OUTPUT_SAMPLES as u64 {
        return Err(AudioDecodeError::Limit);
    }
    let cutoff = (f64::from(OUTPUT_RATE) / f64::from(rate)).min(1.0) * 0.95;
    let ratio = f64::from(rate) / f64::from(OUTPUT_RATE);
    let mut output = Vec::with_capacity(output_len as usize);
    for n in 0..output_len {
        let center = n as f64 * ratio;
        let nearest = center.floor() as isize;
        let mut weighted = 0.0;
        let mut normalizer = 0.0;
        for index in nearest - SINC_RADIUS + 1..=nearest + SINC_RADIUS {
            if index < 0 || index >= source.len() as isize {
                continue;
            }
            let distance = index as f64 - center;
            if distance.abs() >= SINC_RADIUS as f64 {
                continue;
            }
            let phase = std::f64::consts::PI * distance / SINC_RADIUS as f64;
            let blackman = 0.42 + 0.5 * phase.cos() + 0.08 * (2.0 * phase).cos();
            let argument = std::f64::consts::PI * cutoff * distance;
            let sinc = if argument.abs() < 1e-10 {
                1.0
            } else {
                argument.sin() / argument
            };
            let weight = cutoff * sinc * blackman;
            weighted += f64::from(source[index as usize]) * weight;
            normalizer += weight;
        }
        if normalizer.abs() < 1e-8 {
            return Err(AudioDecodeError::Corrupt(
                "invalid resampling kernel".into(),
            ));
        }
        output.push((weighted / normalizer) as f32);
    }
    Ok(output)
}
