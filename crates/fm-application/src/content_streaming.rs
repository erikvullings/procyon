//! Raw byte-range reads and in-file content search for the Lister-style large-file viewer
//! (task 0088). Both operations only ever need a [`ProviderRegistry`] — no other facade state —
//! so they take one by reference rather than living as `&self` methods on the facade.
//!
//! Split out of the `FileManagerService` facade (task 0119).

use fm_domain::{EntryId, Location};
use fm_transport_dto::{
    ReadFileRangeRequestDto, ReadFileRangeResponseDto, SearchInFileMatchDto,
    SearchInFileRequestDto, SearchInFileResponseDto,
};
use fm_vfs::{EntryRef, ProviderCapabilities, ProviderReadStream, ProviderRegistry};
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

use crate::error::ApplicationError;
use crate::file_editor::read_stream_error;

/// Maximum bytes returned by a single [`read_file_range`] call (task 0088).
pub(crate) const MAX_RANGE_LENGTH: u64 = 1_048_576;

/// Maximum matches returned by a single [`search_in_file`] call (task 0088).
const MAX_SEARCH_MATCHES: usize = 5_000;

/// Discards `remaining` bytes from `reader`, for providers without random-access reads.
/// Stops early at EOF (a request offset past the end of the file yields an empty range).
async fn skip_bytes(reader: &mut ProviderReadStream, mut remaining: u64) -> std::io::Result<()> {
    let mut buffer = [0_u8; 64 * 1024];
    while remaining > 0 {
        let chunk = remaining.min(buffer.len() as u64) as usize;
        let read = reader.read(&mut buffer[..chunk]).await?;
        if read == 0 {
            break;
        }
        remaining -= read as u64;
    }
    Ok(())
}

/// Reads one bounded chunk of a file's raw bytes, for the in-app large
/// file viewer (task 0088).
///
/// Uses [`fm_vfs::VfsProvider::read_range`] directly when the provider
/// advertises [`ProviderCapabilities::RANDOM_ACCESS`]; otherwise falls
/// back to a sequential skip-read over [`fm_vfs::VfsProvider::open_read`]
/// (a documented reduced-capability path - only [`ProviderCapabilities::READ`]
/// is required in that case).
pub(crate) async fn read_file_range(
    providers: &ProviderRegistry,
    request: ReadFileRangeRequestDto,
) -> Result<ReadFileRangeResponseDto, ApplicationError> {
    if request.length == 0 || request.length > MAX_RANGE_LENGTH {
        return Err(ApplicationError::InvalidRequest(format!(
            "length must be between 1 and {MAX_RANGE_LENGTH} bytes"
        )));
    }
    let location: Location = request.location.into();
    let provider = providers
        .resolve(&location)
        .map_err(ApplicationError::from)?;
    provider
        .capabilities_for(&location)
        .map_err(ApplicationError::from)?
        .require(ProviderCapabilities::READ)
        .map_err(ApplicationError::from)?;
    let entry = EntryRef {
        id: EntryId::new(),
        location: location.clone(),
    };
    let cancellation = CancellationToken::new();
    let mut reader: ProviderReadStream = if provider
        .capabilities_for(&location)
        .map_err(ApplicationError::from)?
        .contains(ProviderCapabilities::RANDOM_ACCESS)
    {
        provider
            .read_range(
                &entry,
                request.offset,
                Some(request.length),
                cancellation.clone(),
            )
            .await
            .map_err(ApplicationError::from)?
    } else {
        let mut sequential = provider
            .open_read(&entry, cancellation.clone())
            .await
            .map_err(ApplicationError::from)?;
        skip_bytes(&mut sequential, request.offset)
            .await
            .map_err(read_stream_error)?;
        sequential
    };
    let mut data = vec![0_u8; request.length as usize];
    let mut filled = 0_usize;
    while filled < data.len() {
        let read = reader
            .read(&mut data[filled..])
            .await
            .map_err(read_stream_error)?;
        if read == 0 {
            break;
        }
        filled += read;
    }
    data.truncate(filled);
    let eof = (filled as u64) < request.length;
    let probably_binary = (request.offset == 0).then(|| fm_vfs::looks_like_binary(&data));
    Ok(ReadFileRangeResponseDto {
        offset: request.offset,
        length: data.len() as u64,
        data,
        eof,
        probably_binary,
    })
}

/// Searches a single file's content for a substring or regex, for the
/// in-app large file viewer (task 0088). Only requires
/// [`ProviderCapabilities::READ`], so it works for every provider.
pub(crate) async fn search_in_file(
    providers: &ProviderRegistry,
    request: SearchInFileRequestDto,
) -> Result<SearchInFileResponseDto, ApplicationError> {
    let location: Location = request.location.into();
    let provider = providers
        .resolve(&location)
        .map_err(ApplicationError::from)?;
    provider
        .capabilities_for(&location)
        .map_err(ApplicationError::from)?
        .require(ProviderCapabilities::READ)
        .map_err(ApplicationError::from)?;
    let entry = EntryRef {
        id: EntryId::new(),
        location,
    };
    let cancellation = CancellationToken::new();
    let reader = provider
        .open_read(&entry, cancellation.clone())
        .await
        .map_err(ApplicationError::from)?;
    let query = fm_vfs::ContentQuery::new(
        &request.query,
        request.regex,
        request.case_sensitive,
        request.whole_word,
    )
    .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;
    let outcome = fm_vfs::search_content(reader, &query, MAX_SEARCH_MATCHES, &cancellation)
        .await
        .map_err(ApplicationError::from)?;
    Ok(SearchInFileResponseDto {
        matches: outcome
            .matches
            .into_iter()
            .map(|found| SearchInFileMatchDto {
                line_number: found.line_number,
                offset: found.match_start,
                length: found.match_len,
            })
            .collect(),
        truncated: outcome.truncated,
    })
}
