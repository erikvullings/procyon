//! Full image metadata for the viewer (task 0273).
//!
//! Reads only the leading bytes through the VFS provider, so local, archive and remote images are
//! all handled the same way and a huge file is never loaded whole.

use fm_domain::{EntryId, Location};
use fm_metadata::{ImageTags, read_image_tags};
use fm_transport_dto::{
    GpsPositionDto, ImageMetadataDto, ImageTagDto, ImageTagGroupDto, ReadImageMetadataRequestDto,
};
use fm_vfs::{EntryRef, ProviderRegistry};
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

use crate::error::ApplicationError;

/// Bytes read from the start of the file; headers, EXIF, IPTC, ICC and XMP normally sit here.
pub(crate) const IMAGE_METADATA_HEAD_BYTES: u64 = 4 * 1024 * 1024;

fn to_dto(tags: ImageTags) -> ImageMetadataDto {
    ImageMetadataDto {
        supported: tags.format.is_some(),
        format: tags.format.map(str::to_owned),
        groups: tags
            .groups
            .into_iter()
            .map(|group| ImageTagGroupDto {
                namespace: group.namespace,
                tags: group
                    .tags
                    .into_iter()
                    .map(|tag| ImageTagDto {
                        name: tag.name,
                        value: tag.value,
                        raw: tag.raw,
                    })
                    .collect(),
            })
            .collect(),
        gps: tags.gps.map(|gps| GpsPositionDto {
            latitude: gps.latitude,
            longitude: gps.longitude,
        }),
    }
}

pub(crate) async fn read_image_metadata(
    providers: &ProviderRegistry,
    request: ReadImageMetadataRequestDto,
) -> Result<ImageMetadataDto, ApplicationError> {
    let location: Location = request.location.into();
    let provider = providers.resolve(&location)?;
    let entry = EntryRef {
        id: EntryId::new(),
        location,
    };
    let reader = provider.open_read(&entry, CancellationToken::new()).await?;
    let mut bytes = Vec::new();
    reader
        .take(IMAGE_METADATA_HEAD_BYTES)
        .read_to_end(&mut bytes)
        .await
        .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;
    Ok(to_dto(read_image_tags(&bytes)))
}
