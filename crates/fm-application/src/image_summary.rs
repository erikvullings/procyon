//! Short display summaries of many images for grid captions and tooltips (task 0347).
//!
//! Reads only the leading bytes of each file through the VFS provider, so local, archive and
//! remote images are handled alike and a huge file is never loaded whole. Failures are reported
//! per image, never for the whole batch.

use crate::error::ApplicationError;
use crate::rejection::{Rejection, reason};
use fm_domain::{EntryId, Location};
use fm_metadata::ImageSummary;
use fm_transport_dto::{
    ImageSummaryEntryDto, LocationDto, ReadImageSummariesRequestDto, ReadImageSummariesResponseDto,
};
use fm_vfs::{EntryRef, ProviderRegistry};
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

/// Bytes read per image: headers and EXIF normally sit in the first few KiB.
const SUMMARY_HEAD_BYTES: u64 = 512 * 1024;
const MAX_SUMMARY_LOCATIONS: usize = 64;

async fn read_head(
    providers: &ProviderRegistry,
    location: &Location,
) -> Result<Vec<u8>, ApplicationError> {
    let provider = providers.resolve(location)?;
    let entry = EntryRef {
        id: EntryId::new(),
        location: location.clone(),
    };
    let reader = provider.open_read(&entry, CancellationToken::new()).await?;
    let mut bytes = Vec::new();
    reader
        .take(SUMMARY_HEAD_BYTES)
        .read_to_end(&mut bytes)
        .await
        .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;
    Ok(bytes)
}

pub(crate) async fn read_image_summaries(
    providers: &ProviderRegistry,
    request: ReadImageSummariesRequestDto,
) -> Result<ReadImageSummariesResponseDto, ApplicationError> {
    if request.locations.len() > MAX_SUMMARY_LOCATIONS {
        return Err(ApplicationError::from(
            Rejection::new(
                reason::SELECTION_SIZE,
                format!("at most {MAX_SUMMARY_LOCATIONS} images can be read at once"),
            )
            .with("max", MAX_SUMMARY_LOCATIONS),
        ));
    }
    let mut entries = Vec::with_capacity(request.locations.len());
    for dto in request.locations {
        let location: Location = dto.clone().into();
        let entry = match read_head(providers, &location).await {
            Ok(bytes) => entry_from(dto, &fm_metadata::read_image_summary(&bytes)),
            Err(_) => ImageSummaryEntryDto {
                unreadable: true,
                ..entry_from(dto, &ImageSummary::default())
            },
        };
        entries.push(entry);
    }
    Ok(ReadImageSummariesResponseDto { entries })
}

fn entry_from(location: LocationDto, summary: &ImageSummary) -> ImageSummaryEntryDto {
    ImageSummaryEntryDto {
        location,
        width: summary.width,
        height: summary.height,
        bit_depth: summary.bit_depth,
        taken_at: summary.date_taken.map(|time| {
            format!(
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
                time.year, time.month, time.day, time.hour, time.minute, time.second
            )
        }),
        camera: summary.camera.clone(),
        aperture: summary.aperture,
        shutter_seconds: summary.shutter_seconds,
        iso: summary.iso,
        focal_length_mm: summary.focal_length_mm,
        exposure_bias_ev: summary.exposure_bias_ev,
        flash_fired: summary.flash_fired,
        white_balance_auto: summary.white_balance_auto,
        rating: summary.rating,
        unreadable: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fm_vfs_local::LocalFileSystemProvider;
    use std::io::Cursor;
    use std::sync::Arc;

    fn providers() -> ProviderRegistry {
        let mut providers = ProviderRegistry::new();
        providers.register(Arc::new(LocalFileSystemProvider));
        providers
    }

    fn dto_for(path: &std::path::Path) -> LocationDto {
        Location::from_native_path(path)
            .expect("native path to location")
            .into()
    }

    fn rgb_png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        image::DynamicImage::new_rgb8(width, height)
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .expect("encode png");
        bytes
    }

    async fn summarise(path: &std::path::Path) -> ImageSummaryEntryDto {
        let response = read_image_summaries(
            &providers(),
            ReadImageSummariesRequestDto {
                locations: vec![dto_for(path)],
            },
        )
        .await
        .expect("summaries");
        response.entries.into_iter().next().expect("one entry")
    }

    #[tokio::test]
    async fn a_png_reports_its_dimensions_and_bit_depth() {
        let dir = tempfile::tempdir().expect("temp dir");
        let target = dir.path().join("photo.png");
        std::fs::write(&target, rgb_png(7, 3)).expect("write fixture");

        let entry = summarise(&target).await;

        assert_eq!((entry.width, entry.height), (Some(7), Some(3)));
        assert_eq!(entry.bit_depth, Some(24));
        assert!(!entry.unreadable);
    }

    /// A JPEG with an SOF0 header (640x480, 8-bit, 3 components) and an EXIF block holding
    /// camera, capture time and exposure values.
    fn exif_jpeg() -> Vec<u8> {
        let make = b"ACME\0";
        let model = b"Cam 9000\0";
        let date = b"2024:03:05 14:07:09\0";
        let entry = |t: &mut Vec<u8>, tag: u16, kind: u16, count: u32, value: u32| {
            t.extend_from_slice(&tag.to_le_bytes());
            t.extend_from_slice(&kind.to_le_bytes());
            t.extend_from_slice(&count.to_le_bytes());
            t.extend_from_slice(&value.to_le_bytes());
        };
        let rational = |t: &mut Vec<u8>, numerator: u32, denominator: u32| {
            t.extend_from_slice(&numerator.to_le_bytes());
            t.extend_from_slice(&denominator.to_le_bytes());
        };
        let mut tiff = Vec::new();
        tiff.extend_from_slice(b"II*\0");
        tiff.extend_from_slice(&8_u32.to_le_bytes());
        // IFD0 at 8 (3 entries, 42 bytes) -> make at 50, model at 55, Exif IFD at 64.
        tiff.extend_from_slice(&3_u16.to_le_bytes());
        entry(&mut tiff, 0x010F, 2, 5, 50);
        entry(&mut tiff, 0x0110, 2, 9, 55);
        entry(&mut tiff, 0x8769, 4, 1, 64);
        tiff.extend_from_slice(&0_u32.to_le_bytes());
        tiff.extend_from_slice(make);
        tiff.extend_from_slice(model);
        // Exif IFD at 64 (5 entries, 66 bytes) -> data from 130.
        tiff.extend_from_slice(&5_u16.to_le_bytes());
        entry(&mut tiff, 0x829A, 5, 1, 130); // ExposureTime 1/250
        entry(&mut tiff, 0x829D, 5, 1, 138); // FNumber 28/10
        entry(&mut tiff, 0x8827, 3, 1, 400); // ISO
        entry(&mut tiff, 0x9003, 2, 20, 146); // DateTimeOriginal
        entry(&mut tiff, 0x920A, 5, 1, 166); // FocalLength 35/1
        tiff.extend_from_slice(&0_u32.to_le_bytes());
        rational(&mut tiff, 1, 250);
        rational(&mut tiff, 28, 10);
        tiff.extend_from_slice(date);
        rational(&mut tiff, 35, 1);

        let segment = |marker: u8, body: &[u8]| {
            let mut out = vec![0xFF, marker];
            out.extend_from_slice(&((body.len() + 2) as u16).to_be_bytes());
            out.extend_from_slice(body);
            out
        };
        let mut app1 = b"Exif\0\0".to_vec();
        app1.extend_from_slice(&tiff);
        let mut jpeg = vec![0xFF, 0xD8];
        jpeg.extend(segment(0xE1, &app1));
        // SOF0: precision 8, height 480, width 640, 3 components.
        jpeg.extend(segment(
            0xC0,
            &[
                8, 0x01, 0xE0, 0x02, 0x80, 3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1,
            ],
        ));
        jpeg.extend_from_slice(&[0xFF, 0xD9]);
        jpeg
    }

    #[tokio::test]
    async fn a_jpeg_reports_camera_capture_time_and_exposure() {
        let dir = tempfile::tempdir().expect("temp dir");
        let target = dir.path().join("photo.jpg");
        std::fs::write(&target, exif_jpeg()).expect("write fixture");

        let entry = summarise(&target).await;

        assert_eq!((entry.width, entry.height), (Some(640), Some(480)));
        assert_eq!(entry.bit_depth, Some(24));
        assert_eq!(entry.camera.as_deref(), Some("ACME Cam 9000"));
        assert_eq!(entry.taken_at.as_deref(), Some("2024-03-05T14:07:09"));
        assert_eq!(entry.aperture, Some(2.8));
        assert_eq!(entry.shutter_seconds, Some(0.004));
        assert_eq!(entry.iso, Some(400));
        assert_eq!(entry.focal_length_mm, Some(35.0));
    }

    #[tokio::test]
    async fn an_xmp_rating_is_reported() {
        let dir = tempfile::tempdir().expect("temp dir");
        let target = dir.path().join("rated.jpg");
        let mut xmp = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
        xmp.extend_from_slice(
            br#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmp:Rating="4"/></rdf:RDF></x:xmpmeta>"#,
        );
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE1];
        jpeg.extend_from_slice(&((xmp.len() + 2) as u16).to_be_bytes());
        jpeg.extend_from_slice(&xmp);
        jpeg.extend_from_slice(&[0xFF, 0xD9]);
        std::fs::write(&target, jpeg).expect("write fixture");

        assert_eq!(summarise(&target).await.rating, Some(4));
    }

    #[tokio::test]
    async fn fields_missing_from_the_file_stay_absent() {
        let dir = tempfile::tempdir().expect("temp dir");
        let target = dir.path().join("plain.png");
        std::fs::write(&target, rgb_png(2, 2)).expect("write fixture");

        let entry = summarise(&target).await;

        assert_eq!(entry.camera, None);
        assert_eq!(entry.taken_at, None);
        assert_eq!(entry.aperture, None);
        assert_eq!(entry.iso, None);
        assert_eq!(entry.rating, None);
    }

    #[tokio::test]
    async fn an_unreadable_file_is_flagged_without_failing_the_batch() {
        let dir = tempfile::tempdir().expect("temp dir");
        let good = dir.path().join("good.png");
        std::fs::write(&good, rgb_png(2, 2)).expect("write fixture");
        let missing = dir.path().join("missing.png");

        let response = read_image_summaries(
            &providers(),
            ReadImageSummariesRequestDto {
                locations: vec![dto_for(&missing), dto_for(&good)],
            },
        )
        .await
        .expect("summaries");

        assert!(response.entries[0].unreadable);
        assert!(!response.entries[1].unreadable);
        assert_eq!(response.entries[1].width, Some(2));
    }

    #[tokio::test]
    async fn too_many_locations_are_rejected() {
        let dir = tempfile::tempdir().expect("temp dir");
        let location = dto_for(&dir.path().join("a.png"));

        let result = read_image_summaries(
            &providers(),
            ReadImageSummariesRequestDto {
                locations: vec![location; MAX_SUMMARY_LOCATIONS + 1],
            },
        )
        .await;

        assert!(result.is_err());
    }
}
