//! Detailed, on-demand entry metadata.
//!
//! This is the expensive metadata deliberately excluded from directory
//! listings (specification §5.2): permissions, ownership, extended attributes,
//! checksums, image dimensions and media properties.

mod audio;
mod cache;
mod image_tags;
mod pdf;
mod photo;
mod thumbnail;
mod video;

pub use audio::{
    AudioInfo, AudioReadError, SUPPORTED_AUDIO_EXTENSIONS, is_supported_audio_extension,
    read_audio_info, read_audio_info_from_bytes,
};
pub use cache::{ThumbnailCache, ThumbnailCacheError};
pub use image_tags::{
    GpsPosition, ImageTag, ImageTagGroup, ImageTags, detect_format, read_image_tags,
};
pub use pdf::{SUPPORTED_PDF_EXTENSIONS, generate_pdf_thumbnail, is_supported_pdf_extension};
pub use photo::{CaptureTime, PhotoInfo, read_photo_info};
pub use thumbnail::{
    GeneratedThumbnail, MAX_SOURCE_BYTES, SUPPORTED_IMAGE_EXTENSIONS, ThumbnailError,
    ThumbnailSize, generate_image_thumbnail, is_supported_image_extension,
};
pub use video::{
    SUPPORTED_VIDEO_EXTENSIONS, SampledVideoFrame, VideoSamplingError, generate_video_thumbnail,
    is_supported_video_extension, sample_video_frames, sample_video_frames_cancellable,
};
