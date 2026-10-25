//! Full image metadata for the viewer (task 0273).
//!
//! Reads only the leading bytes through the VFS provider, so local, archive and remote images are
//! all handled the same way and a huge file is never loaded whole.

use crate::rejection::{Rejection, reason};
use fm_domain::{EntryId, Location};
use fm_metadata::{ImageTags, read_image_tags};
use fm_transport_dto::{
    GpsPositionDto, ImageGpsEntryDto, ImageMetadataDto, ImageTagDto, ImageTagGroupDto,
    ReadImageGpsRequestDto, ReadImageGpsResponseDto, ReadImageMetadataRequestDto,
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
        editable_fields: fm_metadata::editable_fields(tags.format)
            .iter()
            .map(|field| field.id().to_owned())
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

/// Bytes read per image for the bulk GPS scan: EXIF normally sits in the first few KiB.
const GPS_HEAD_BYTES: u64 = 512 * 1024;
const MAX_GPS_LOCATIONS: usize = 100;

async fn read_head(
    providers: &ProviderRegistry,
    location: &Location,
    limit: u64,
) -> Result<Vec<u8>, ApplicationError> {
    let provider = providers.resolve(location)?;
    let entry = EntryRef {
        id: EntryId::new(),
        location: location.clone(),
    };
    let reader = provider.open_read(&entry, CancellationToken::new()).await?;
    let mut bytes = Vec::new();
    reader
        .take(limit)
        .read_to_end(&mut bytes)
        .await
        .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;
    Ok(bytes)
}

pub(crate) async fn read_image_gps(
    providers: &ProviderRegistry,
    request: ReadImageGpsRequestDto,
) -> Result<ReadImageGpsResponseDto, ApplicationError> {
    if request.locations.len() > MAX_GPS_LOCATIONS {
        return Err(ApplicationError::from(
            Rejection::new(
                reason::SELECTION_SIZE,
                format!("at most {MAX_GPS_LOCATIONS} images can be read at once"),
            )
            .with("max", MAX_GPS_LOCATIONS),
        ));
    }
    let mut entries = Vec::with_capacity(request.locations.len());
    for dto in request.locations {
        let location: Location = dto.clone().into();
        let entry = match read_head(providers, &location, GPS_HEAD_BYTES).await {
            Ok(bytes) => {
                let tags = fm_metadata::read_image_tags(&bytes);
                let taken_at = fm_metadata::read_photo_info(&bytes).date_taken.map(|time| {
                    format!(
                        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
                        time.year, time.month, time.day, time.hour, time.minute, time.second
                    )
                });
                ImageGpsEntryDto {
                    location: dto,
                    gps: tags.gps.map(|gps| GpsPositionDto {
                        latitude: gps.latitude,
                        longitude: gps.longitude,
                    }),
                    taken_at,
                    unreadable: false,
                }
            }
            Err(_) => ImageGpsEntryDto {
                location: dto,
                gps: None,
                taken_at: None,
                unreadable: true,
            },
        };
        entries.push(entry);
    }
    Ok(ReadImageGpsResponseDto { entries })
}
