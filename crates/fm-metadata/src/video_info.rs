//! Container-level properties of an ISO-BMFF (MP4/MOV) video: dimensions, length, frame rate.
//!
//! Only the container header is parsed - no frame is decoded - so any codec is described, not
//! just the H.264 the thumbnailer can decode. Unparsable input yields `None`.

use std::io::{Read, Seek};
use std::time::Duration;

/// What a caption or tooltip shows about one video.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct VideoInfo {
    /// Pixel width of the first video track.
    pub width: Option<u32>,
    /// Pixel height of the first video track.
    pub height: Option<u32>,
    /// Playing time of the container.
    pub duration: Option<Duration>,
    /// Frames per second of the first video track.
    pub frame_rate: Option<f64>,
    /// Codec of the first video track, for example `"H264"`.
    pub codec: Option<String>,
}

/// Reads the properties of the MP4/MOV container behind `reader`, `size` bytes long. `None` when
/// the data is not a readable container or its header is not within `reader`.
#[must_use]
pub fn read_video_info<R: Read + Seek>(reader: R, size: u64) -> Option<VideoInfo> {
    let reader = mp4::Mp4Reader::read_header(reader, size).ok()?;
    let track = reader
        .tracks()
        .values()
        .filter(|track| matches!(track.track_type(), Ok(mp4::TrackType::Video)))
        .min_by_key(|track| track.track_id());
    let positive = |value: u16| (value > 0).then_some(u32::from(value));
    Some(VideoInfo {
        width: track.and_then(|track| positive(track.width())),
        height: track.and_then(|track| positive(track.height())),
        duration: Some(reader.duration()).filter(|duration| !duration.is_zero()),
        frame_rate: track
            .map(mp4::Mp4Track::frame_rate)
            .filter(|rate| rate.is_finite() && *rate > 0.0),
        codec: track
            .and_then(|track| track.media_type().ok())
            .map(|media| format!("{media:?}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn bytes_that_are_not_a_container_yield_nothing() {
        let bytes = b"definitely not an mp4 file".to_vec();
        assert_eq!(
            read_video_info(Cursor::new(&bytes), bytes.len() as u64),
            None
        );
    }
}
