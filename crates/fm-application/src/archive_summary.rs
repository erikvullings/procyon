use fm_archive::ArchiveFileSystemProvider;
use fm_domain::Location;
use fm_transport_dto::ArchiveSummaryResponseDto;
use fm_vfs::{ProviderCapabilities, ProviderRegistry};
use tokio_util::sync::CancellationToken;

use crate::error::ApplicationError;
use crate::folder_size::calculate_directory_stats;

pub(crate) async fn calculate_archive_summary(
    providers: &ProviderRegistry,
    archive_provider: &ArchiveFileSystemProvider,
    archive_file: Location,
) -> Result<ArchiveSummaryResponseDto, ApplicationError> {
    if archive_file.provider_id.as_str() != "local" || !archive_file.uri.starts_with("file://") {
        return Err(ApplicationError::InvalidRequest(
            "archive summary requires a local archive file".to_owned(),
        ));
    }
    let location = Location::parse(&format!(
        "archive://{}!/",
        &archive_file.uri["file://".len()..]
    ))
    .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;
    let provider = providers
        .resolve(&location)
        .map_err(ApplicationError::from)?;
    provider
        .capabilities_for(&location)
        .map_err(ApplicationError::from)?
        .require(ProviderCapabilities::LIST)
        .map_err(ApplicationError::from)?;

    let metadata = archive_provider.summary_metadata(&location).await?;
    let stats =
        calculate_directory_stats(provider.as_ref(), location, CancellationToken::new()).await?;
    Ok(ArchiveSummaryResponseDto {
        format: metadata.format.to_owned(),
        file_count: stats.file_count,
        directory_count: stats.directory_count,
        uncompressed_size: stats.total_bytes,
        compressed_size: metadata.compressed_size,
    })
}
