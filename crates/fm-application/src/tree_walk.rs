//! Shared recursive listing of provider trees for scan-style jobs (broken-link doctor, folder
//! snapshots). Symbolic links are leaves, so a walk can never leave its roots through a link.

use std::collections::HashSet;

use fm_domain::{EntryKind, EntrySummary, Location};
use fm_vfs::{ListOptions, ProviderCapabilities, ProviderRegistry};
use tokio_util::sync::CancellationToken;

/// Lists every entry under `roots`: files, symlinks and directories. Symlinks are leaves.
pub(crate) async fn walk(
    providers: &ProviderRegistry,
    roots: &[Location],
    show_hidden: bool,
    cancellation: &CancellationToken,
    limit: usize,
) -> (Vec<EntrySummary>, bool) {
    let mut found = Vec::new();
    let mut visited: HashSet<String> = HashSet::new();
    let mut pending: Vec<Location> = roots.to_vec();
    while let Some(location) = pending.pop() {
        if cancellation.is_cancelled() {
            break;
        }
        if !visited.insert(location.uri.clone()) {
            continue;
        }
        let Ok(provider) = providers.resolve(&location) else {
            continue;
        };
        let Ok(capabilities) = provider.capabilities_for(&location) else {
            continue;
        };
        if !capabilities.contains(ProviderCapabilities::LIST) {
            continue;
        }
        let mut continuation_token = None;
        loop {
            if cancellation.is_cancelled() {
                break;
            }
            let Ok(page) = provider
                .list(
                    &location,
                    ListOptions {
                        page_size: 500,
                        continuation_token: continuation_token.take(),
                    },
                    cancellation.clone(),
                )
                .await
            else {
                break;
            };
            for entry in page.entries {
                if !show_hidden && entry.hidden {
                    continue;
                }
                if entry.kind == EntryKind::Directory {
                    pending.push(entry.location.clone());
                }
                found.push(entry);
                if found.len() >= limit {
                    return (found, true);
                }
            }
            continuation_token = page.continuation_token;
            if continuation_token.is_none() {
                break;
            }
        }
    }
    (found, false)
}
