//! Recursive directory-size computation for the Total Commander-style "press a key on a folder
//! to see how much space it consumes" behaviour (task 0071's directory-size acceptance criterion).
//!
//! A plain, stateless async function - not spawned as a background task - so cancellation is the
//! same as every other one-shot request in this app (`search_in_file`, `read_file_range`): the
//! caller's `CancellationToken` is never externally triggered here, but dropping the awaiting
//! future (an aborted HTTP request, or the frontend simply discarding a late Tauri result once the
//! cursor has moved on) stops the walk from being *applied*, which is what the user actually
//! observes. Only [`fm_domain::EntryKind::Directory`] entries are recursed into - symlinks are
//! counted as their own (unfollowed) size, matching the copy/delete planners' cycle-safety
//! convention (see `operation_planner.rs`'s `DeleteExecutor::plan`).

use fm_domain::EntryKind;
use fm_domain::Location;
use fm_transport_dto::CalculateFolderSizeResponseDto;
use fm_vfs::{ListOptions, ProviderCapabilities, ProviderRegistry};
use tokio_util::sync::CancellationToken;

use crate::error::ApplicationError;

/// Recursively sums the size of every file/symlink under `location`, paginating each directory
/// level via the provider's `list()` the same way `DeleteExecutor::plan` does.
pub(crate) async fn calculate_folder_size(
    providers: &ProviderRegistry,
    location: Location,
) -> Result<CalculateFolderSizeResponseDto, ApplicationError> {
    let provider = providers
        .resolve(&location)
        .map_err(ApplicationError::from)?;
    provider
        .capabilities_for(&location)
        .map_err(ApplicationError::from)?
        .require(ProviderCapabilities::LIST)
        .map_err(ApplicationError::from)?;

    let cancellation = CancellationToken::new();
    let mut total_bytes: u64 = 0;
    let mut file_count: u64 = 0;
    let mut stack = vec![location];
    while let Some(current) = stack.pop() {
        if cancellation.is_cancelled() {
            return Err(fm_vfs::VfsError::Cancelled.into());
        }
        let mut continuation_token = None;
        loop {
            let page = provider
                .list(
                    &current,
                    ListOptions {
                        page_size: 512,
                        continuation_token,
                    },
                    cancellation.clone(),
                )
                .await
                .map_err(ApplicationError::from)?;
            for entry in page.entries {
                match entry.kind {
                    EntryKind::Directory => stack.push(entry.location),
                    EntryKind::File | EntryKind::Symlink => {
                        total_bytes += entry.size.unwrap_or(0);
                        file_count += 1;
                    }
                }
            }
            if !page.has_more {
                break;
            }
            continuation_token = page.continuation_token;
        }
    }
    Ok(CalculateFolderSizeResponseDto {
        total_bytes,
        file_count,
    })
}
