//! Pre-flight conflict check for a planned copy or move (task 0062).
//!
//! Lets the confirmation dialog ask the conflict question up front, so the user picks a policy
//! once instead of confirming the transfer and then answering a conflict prompt mid-job. It uses
//! the engine's own top-level rule — `destination.join(source name)` that the destination provider
//! can inspect — but is advisory only: the job re-checks every entry while it runs, so anything
//! created after this check is still caught by the operation's conflict policy.

use fm_domain::{EntryId, Location};
use fm_transport_dto::{TransferConflictsDto, TransferConflictsRequestDto};
use fm_vfs::{EntryRef, ProviderRegistry};
use futures::StreamExt as _;
use tokio_util::sync::CancellationToken;

use crate::error::ApplicationError;

/// Sources checked at most; a larger selection reports `complete: false`.
const MAX_CHECKED_SOURCES: usize = 1_000;
/// Concurrent destination lookups, so remote providers don't serialise one round trip per entry.
const CONCURRENT_LOOKUPS: usize = 16;

/// Returns the sources' names that already exist in the destination, in source order.
pub(crate) async fn transfer_conflicts(
    providers: &ProviderRegistry,
    request: TransferConflictsRequestDto,
) -> Result<TransferConflictsDto, ApplicationError> {
    let destination: Location = request.destination.into();
    let provider = providers.resolve(&destination)?;
    let complete = request.sources.len() <= MAX_CHECKED_SOURCES;
    let destination = &destination;
    let provider = &provider;
    let names = futures::stream::iter(request.sources.into_iter().take(MAX_CHECKED_SOURCES))
        .map(|source| async move {
            let name = Location::from(source).name().ok()?;
            let target = destination.join(&name).ok()?;
            let entry = EntryRef {
                id: EntryId::new(),
                location: target,
            };
            // Like the engine, anything the provider cannot inspect counts as absent.
            provider
                .inspect(&entry, CancellationToken::new())
                .await
                .ok()
                .map(|_| name)
        })
        .buffered(CONCURRENT_LOOKUPS)
        .filter_map(std::future::ready)
        .collect()
        .await;
    Ok(TransferConflictsDto { names, complete })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use fm_transport_dto::LocationDto;
    use fm_vfs_local::LocalFileSystemProvider;

    fn location(path: &std::path::Path) -> LocationDto {
        Location::from_native_path(path)
            .expect("native path location")
            .into()
    }

    #[tokio::test]
    async fn reports_only_names_that_already_exist_in_the_destination() {
        let root = tempfile::tempdir().expect("temp dir");
        let source_dir = root.path().join("source");
        let destination_dir = root.path().join("destination");
        std::fs::create_dir_all(&source_dir).expect("source dir");
        std::fs::create_dir_all(destination_dir.join("folder")).expect("destination dir");
        for name in ["clash.txt", "fresh.txt"] {
            std::fs::write(source_dir.join(name), b"new").expect("source file");
        }
        std::fs::create_dir(source_dir.join("folder")).expect("source folder");
        std::fs::write(destination_dir.join("clash.txt"), b"old").expect("existing file");

        let mut providers = ProviderRegistry::new();
        providers.register(Arc::new(LocalFileSystemProvider::new()));
        let result = transfer_conflicts(
            &providers,
            TransferConflictsRequestDto {
                sources: vec![
                    location(&source_dir.join("clash.txt")),
                    location(&source_dir.join("fresh.txt")),
                    location(&source_dir.join("folder")),
                ],
                destination: location(&destination_dir),
            },
        )
        .await
        .expect("conflict check");

        assert_eq!(result.names, vec!["clash.txt", "folder"]);
        assert!(result.complete);
    }
}
