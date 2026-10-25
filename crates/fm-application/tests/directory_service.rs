//! Integration coverage for authoritative directory listing behavior.

use std::sync::Arc;

use fm_application::DirectoryService;
use fm_domain::{Location, PaneId};
use fm_events::{
    BackendEventPayload, DirectoryDeltaPayload, EventBus, SessionId, SubscriptionEvent,
};
use fm_transport_dto::{ListDirectoryRequest, LocationDto, SortDescriptorDto, SortDirectionDto};
use fm_vfs::ProviderRegistry;
use fm_vfs_local::LocalFileSystemProvider;
use uuid::Uuid;

fn service() -> DirectoryService {
    let mut providers = ProviderRegistry::new();
    providers.register(Arc::new(LocalFileSystemProvider));
    DirectoryService::new(providers)
}

#[tokio::test]
async fn open_downloads_like_directory_publishes_batched_create_rename_delete_deltas() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    let downloads = root.path().join("Downloads");
    std::fs::create_dir(&downloads).expect("must create Downloads fixture");
    let location = Location::from_native_path(&downloads).expect("temp path must be representable");
    let workspace_id = fm_domain::WorkspaceId::new();
    let pane_id = PaneId::new();
    let events = EventBus::new(32);
    let mut providers = ProviderRegistry::new();
    providers.register(Arc::new(LocalFileSystemProvider));
    let service = DirectoryService::with_event_bus(providers, events.clone());
    let mut initial = request(pane_id, &location);
    initial.workspace_id = workspace_id.into();
    service.list(initial).await.expect("initial listing");
    let mut subscription = events.subscribe(SessionId::new("test"), [workspace_id], None);
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;

    std::fs::write(downloads.join("before.txt"), b"file").expect("create fixture");
    let added = tokio::time::timeout(std::time::Duration::from_secs(3), subscription.recv())
        .await
        .expect("delta timeout")
        .expect("event bus open");
    let added_id = match added {
        SubscriptionEvent::Event(event) => match event.payload {
            BackendEventPayload::DirectoryDelta {
                pane_id: event_pane,
                delta:
                    DirectoryDeltaPayload::EntriesAdded {
                        revision: 2,
                        entries,
                    },
            } => {
                assert_eq!(event_pane, pane_id);
                entries[0].id
            }
            payload => panic!("unexpected payload: {payload:?}"),
        },
        event => panic!("unexpected subscription item: {event:?}"),
    };

    std::fs::rename(downloads.join("before.txt"), downloads.join("after.txt"))
        .expect("rename fixture");
    let renamed = tokio::time::timeout(std::time::Duration::from_secs(3), subscription.recv())
        .await
        .expect("delta timeout")
        .expect("event bus open");
    match renamed {
        SubscriptionEvent::Event(event) => match event.payload {
            BackendEventPayload::DirectoryDelta {
                delta:
                    DirectoryDeltaPayload::EntriesUpdated {
                        revision: 3,
                        entries,
                    },
                ..
            } => assert_eq!(entries[0].id, added_id),
            payload => panic!("unexpected payload: {payload:?}"),
        },
        event => panic!("unexpected subscription item: {event:?}"),
    }

    std::fs::remove_file(downloads.join("after.txt")).expect("remove fixture");
    let removed = tokio::time::timeout(std::time::Duration::from_secs(3), subscription.recv())
        .await
        .expect("delta timeout")
        .expect("event bus open");
    match removed {
        SubscriptionEvent::Event(event) => match event.payload {
            BackendEventPayload::DirectoryDelta {
                delta:
                    DirectoryDeltaPayload::EntriesRemoved {
                        revision: 4,
                        entry_ids,
                    },
                ..
            } => assert_eq!(entry_ids, [added_id]),
            payload => panic!("unexpected payload: {payload:?}"),
        },
        event => panic!("unexpected subscription item: {event:?}"),
    }
}

fn request(pane_id: PaneId, location: &Location) -> ListDirectoryRequest {
    ListDirectoryRequest {
        workspace_id: Uuid::new_v4(),
        pane_id: pane_id.into(),
        request_id: Uuid::new_v4(),
        location: LocationDto::from(location.clone()),
        continuation_token: None,
        sort: Vec::new(),
        show_hidden: true,
        folders_first: false,
        show_git_status: false,
    }
}

#[tokio::test]
async fn listing_a_directory_produces_a_loaded_snapshot_for_the_request() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    std::fs::write(root.path().join("report.txt"), b"contents").expect("must create fixture");
    let location =
        Location::from_native_path(root.path()).expect("temp path must be representable");
    let pane_id = PaneId::new();
    let request = request(pane_id, &location);
    let request_id = request.request_id;

    let snapshot = service().list(request).await.expect("listing must succeed");

    assert_eq!(snapshot.pane_id, pane_id);
    assert_eq!(snapshot.request_id, request_id);
    assert_eq!(snapshot.revision, 1);
    assert_eq!(snapshot.location, location);
    assert_eq!(snapshot.entries.len(), 1);
    assert_eq!(snapshot.entries[0].name, "report.txt");
    assert!(!snapshot.has_more);
    assert!(snapshot.continuation_token.is_none());
    assert_eq!(snapshot.loading_state, fm_domain::LoadingState::Loaded);
}

#[tokio::test]
async fn successful_snapshots_have_monotonic_revisions_per_pane() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    let location =
        Location::from_native_path(root.path()).expect("temp path must be representable");
    let pane_id = PaneId::new();
    let service = service();

    let first = service
        .list(request(pane_id, &location))
        .await
        .expect("first listing must succeed");
    let second = service
        .list(request(pane_id, &location))
        .await
        .expect("second listing must succeed");

    assert_eq!(first.revision, 1);
    assert_eq!(second.revision, 2);
}

#[tokio::test]
async fn listing_filters_hidden_entries_and_sorts_folders_first_then_by_name() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    std::fs::write(root.path().join(".hidden"), b"hidden").expect("must create fixture");
    std::fs::write(root.path().join("zeta.txt"), b"zeta").expect("must create fixture");
    std::fs::create_dir(root.path().join("beta")).expect("must create fixture directory");
    std::fs::create_dir(root.path().join("alpha")).expect("must create fixture directory");
    let location =
        Location::from_native_path(root.path()).expect("temp path must be representable");
    let mut request = request(PaneId::new(), &location);
    request.show_hidden = false;
    request.folders_first = true;
    request.sort = vec![SortDescriptorDto {
        column_id: "core.name".to_owned(),
        direction: SortDirectionDto::Ascending,
    }];

    let snapshot = service().list(request).await.expect("listing must succeed");
    let names: Vec<&str> = snapshot
        .entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();

    assert_eq!(names, ["alpha", "beta", "zeta.txt"]);
}

#[tokio::test]
async fn paging_a_large_directory_returns_every_entry_in_global_sort_order() {
    // Regression test: providers enumerate directories in arbitrary (e.g. filesystem/inode)
    // order, which does not match name order. `list()` must fully enumerate and globally sort
    // the directory once, then bound each page to a slice of that already-sorted list — never
    // sort a provider-sized page independently, since that cannot produce a correct order across
    // page boundaries (previously surfaced as later pages containing names that sort earlier
    // than names already shown on prior pages).
    let root = tempfile::tempdir().expect("must create a temp directory");
    for index in 0..600 {
        std::fs::write(root.path().join(format!("entry-{index:04}")), b"x")
            .expect("must create fixture");
    }
    let location =
        Location::from_native_path(root.path()).expect("temp path must be representable");
    let pane_id = PaneId::new();
    let service = service();
    let mut token = None;
    let mut names = Vec::new();

    loop {
        let mut request = request(pane_id, &location);
        request.continuation_token = token;
        let snapshot = service.list(request).await.expect("page must load");
        assert_eq!(snapshot.total_known_entries, Some(600));
        names.extend(snapshot.entries.into_iter().map(|entry| entry.name));
        if !snapshot.has_more {
            assert_eq!(snapshot.continuation_token, None);
            break;
        }
        token = snapshot.continuation_token;
    }

    assert_eq!(names.len(), 600);
    let mut sorted_names = names.clone();
    sorted_names.sort();
    assert_eq!(
        names, sorted_names,
        "pages must be contiguous slices of one sorted list"
    );
}

#[tokio::test]
async fn watched_paged_directory_resets_to_a_consistent_first_page_after_an_external_addition() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    let downloads = root.path().join("Downloads");
    std::fs::create_dir(&downloads).expect("must create Downloads fixture");
    for index in 1..=300 {
        std::fs::write(downloads.join(format!("entry-{index:04}.txt")), b"x")
            .expect("must create fixture");
    }
    let location =
        Location::from_native_path(&downloads).expect("Downloads path must be representable");
    let workspace_id = fm_domain::WorkspaceId::new();
    let pane_id = PaneId::new();
    let events = EventBus::new(32);
    let mut providers = ProviderRegistry::new();
    providers.register(Arc::new(LocalFileSystemProvider));
    let service = DirectoryService::with_event_bus(providers, events.clone());
    let mut first_request = request(pane_id, &location);
    first_request.workspace_id = workspace_id.into();
    first_request.sort = vec![SortDescriptorDto {
        column_id: "core.name".to_owned(),
        direction: SortDirectionDto::Ascending,
    }];
    let first = service
        .list(first_request)
        .await
        .expect("first page must load");
    assert_eq!(first.entries.len(), 256);
    assert_eq!(first.total_known_entries, Some(300));
    assert!(first.has_more);
    assert_eq!(first.continuation_token.as_deref(), Some("256"));
    let mut subscription = events.subscribe(SessionId::new("test"), [workspace_id], None);
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;

    std::fs::write(downloads.join("entry-0000-new.txt"), b"new")
        .expect("must create external fixture");
    let changed = tokio::time::timeout(std::time::Duration::from_secs(3), subscription.recv())
        .await
        .expect("delta timeout")
        .expect("event bus open");
    let reset = match changed {
        SubscriptionEvent::Event(event) => match event.payload {
            BackendEventPayload::DirectoryDelta {
                pane_id: event_pane,
                delta: DirectoryDeltaPayload::Reset { snapshot },
            } => {
                assert_eq!(event_pane, pane_id);
                snapshot
            }
            payload => panic!("expected a paged reset, got {payload:?}"),
        },
        event => panic!("unexpected subscription item: {event:?}"),
    };
    assert_eq!(reset.entries.len(), 256);
    assert_eq!(reset.entries[0].name, "entry-0000-new.txt");
    assert_eq!(reset.total_known_entries, Some(301));
    assert!(reset.has_more);
    assert_eq!(reset.continuation_token.as_deref(), Some("256"));

    let mut next_request = request(pane_id, &location);
    next_request.workspace_id = workspace_id.into();
    next_request.sort = vec![SortDescriptorDto {
        column_id: "core.name".to_owned(),
        direction: SortDirectionDto::Ascending,
    }];
    next_request.continuation_token = reset.continuation_token.clone();
    let next = service
        .list(next_request)
        .await
        .expect("continuation page must load");
    assert_eq!(next.entries.len(), 45);
    assert_eq!(next.total_known_entries, Some(301));
    assert!(!next.has_more);
    assert_eq!(next.continuation_token, None);

    let names: Vec<_> = reset
        .entries
        .iter()
        .map(|entry| entry.name.clone())
        .chain(next.entries.iter().map(|entry| entry.name.clone()))
        .collect();
    let unique: std::collections::HashSet<_> = names.iter().collect();
    assert_eq!(names.len(), 301);
    assert_eq!(unique.len(), 301);
    assert_eq!(
        names
            .iter()
            .filter(|name| name.as_str() == "entry-0000-new.txt")
            .count(),
        1
    );
}

#[tokio::test]
async fn a_non_existent_directory_maps_to_not_found_without_exposing_an_os_error() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    let missing = root.path().join("missing");
    let location = Location::from_native_path(&missing).expect("temp path must be representable");

    let error = service()
        .list(request(PaneId::new(), &location))
        .await
        .expect_err("missing directory must fail");

    assert_eq!(error, fm_application::ApplicationError::NotFound);
    let dto = error.into_dto(Uuid::new_v4());
    assert_eq!(dto.message, "resource not found");
    assert!(!dto.message.contains(missing.to_string_lossy().as_ref()));
}

#[cfg(unix)]
#[tokio::test]
async fn an_unreadable_directory_maps_to_permission_denied_where_enforced() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().expect("must create a temp directory");
    let blocked = root.path().join("blocked");
    std::fs::create_dir(&blocked).expect("must create fixture directory");
    std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o000))
        .expect("must remove permissions");
    let location = Location::from_native_path(&blocked).expect("temp path must be representable");
    let result = service().list(request(PaneId::new(), &location)).await;
    std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o700))
        .expect("must restore permissions");

    match result {
        Err(fm_application::ApplicationError::PermissionDenied) => {}
        Ok(_) => eprintln!("filesystem/user does not enforce mode-based read denial"),
        Err(error) => panic!("unexpected unreadable-directory result: {error}"),
    }
}

#[tokio::test]
async fn refresh_affected_publishes_nothing_when_the_listing_is_unchanged() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    std::fs::write(root.path().join("one.txt"), b"x").expect("must create fixture");
    let location =
        Location::from_native_path(root.path()).expect("temp path must be representable");
    let workspace_id = fm_domain::WorkspaceId::new();
    let pane_id = PaneId::new();
    let events = EventBus::new(32);
    let mut providers = ProviderRegistry::new();
    providers.register(Arc::new(LocalFileSystemProvider));
    let service = DirectoryService::with_event_bus(providers, events.clone());
    let mut first_request = request(pane_id, &location);
    first_request.workspace_id = workspace_id.into();
    service
        .list(first_request)
        .await
        .expect("first page must load");
    let mut subscription = events.subscribe(SessionId::new("test"), [workspace_id], None);

    let affected = std::collections::HashSet::from([location.clone()]);
    service.refresh_affected(&affected).await;
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(300), subscription.recv())
            .await
            .is_err(),
        "an unchanged listing must not publish a delta"
    );

    std::fs::write(root.path().join("two.txt"), b"y").expect("must create fixture");
    service.refresh_affected(&affected).await;
    let event = tokio::time::timeout(std::time::Duration::from_secs(3), subscription.recv())
        .await
        .expect("delta timeout")
        .expect("event bus open");
    match event {
        SubscriptionEvent::Event(event) => match event.payload {
            BackendEventPayload::DirectoryDelta {
                delta: DirectoryDeltaPayload::EntriesAdded { entries, .. },
                ..
            } => assert_eq!(entries.len(), 1),
            payload => panic!("expected an added delta, got {payload:?}"),
        },
        event => panic!("unexpected subscription item: {event:?}"),
    }
}

/// Drains deltas until one satisfies `matches` or the timeout elapses.
async fn wait_for_delta<T>(
    subscription: &mut fm_events::EventSubscription,
    timeout: std::time::Duration,
    mut matches: impl FnMut(&DirectoryDeltaPayload) -> Option<T>,
) -> Option<T> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.checked_duration_since(tokio::time::Instant::now())?;
        let event = tokio::time::timeout(remaining, subscription.recv())
            .await
            .ok()?
            .ok()?;
        if let SubscriptionEvent::Event(event) = event
            && let BackendEventPayload::DirectoryDelta { delta, .. } = event.payload
            && let Some(found) = matches(&delta)
        {
            return Some(found);
        }
    }
}

fn names_in(delta: &DirectoryDeltaPayload) -> Vec<String> {
    match delta {
        DirectoryDeltaPayload::EntriesAdded { entries, .. }
        | DirectoryDeltaPayload::EntriesUpdated { entries, .. } => {
            entries.iter().map(|e| e.name.clone()).collect()
        }
        DirectoryDeltaPayload::Reset { snapshot } => {
            snapshot.entries.iter().map(|e| e.name.clone()).collect()
        }
        DirectoryDeltaPayload::EntriesRemoved { .. } => Vec::new(),
    }
}

struct Fixture {
    _root: tempfile::TempDir,
    dir: std::path::PathBuf,
    service: DirectoryService,
    events: EventBus,
    workspace_id: fm_domain::WorkspaceId,
    pane_id: PaneId,
    location: Location,
}

impl Fixture {
    fn new(files: usize) -> Self {
        let root = tempfile::tempdir().expect("temp dir");
        let dir = root.path().join("Downloads");
        std::fs::create_dir(&dir).expect("fixture dir");
        for index in 0..files {
            std::fs::write(dir.join(format!("file-{index:04}.dat")), b"x").expect("fixture file");
        }
        let location = Location::from_native_path(&dir).expect("location");
        let events = EventBus::new(256);
        let mut providers = ProviderRegistry::new();
        providers.register(Arc::new(LocalFileSystemProvider));
        Self {
            _root: root,
            dir,
            service: DirectoryService::with_event_bus(providers, events.clone()),
            events,
            workspace_id: fm_domain::WorkspaceId::new(),
            pane_id: PaneId::new(),
            location,
        }
    }

    fn request(&self, location: &Location) -> ListDirectoryRequest {
        let mut request = request(self.pane_id, location);
        request.workspace_id = self.workspace_id.into();
        request.sort = vec![SortDescriptorDto {
            column_id: "core.modified".to_owned(),
            direction: SortDirectionDto::Descending,
        }];
        request
    }

    fn subscribe(&self) -> fm_events::EventSubscription {
        self.events
            .subscribe(SessionId::new("test"), [self.workspace_id], None)
    }
}

const SEEN: std::time::Duration = std::time::Duration::from_secs(5);

#[tokio::test]
async fn temp_file_then_rename_into_a_large_directory_is_reported() {
    let fx = Fixture::new(649);
    let first = fx
        .service
        .list(fx.request(&fx.location))
        .await
        .expect("list");
    assert!(first.has_more);
    let mut sub = fx.subscribe();
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    std::fs::write(fx.dir.join(".out.tmp"), b"{}\n").expect("tmp");
    std::fs::rename(fx.dir.join(".out.tmp"), fx.dir.join("converted.jsonl")).expect("rename");
    let seen = wait_for_delta(&mut sub, SEEN, |d| {
        names_in(d)
            .contains(&"converted.jsonl".to_owned())
            .then_some(())
    })
    .await;
    assert!(seen.is_some(), "rename-into-place was never reported");
}

#[tokio::test]
async fn truncate_and_rewrite_of_a_large_file_is_reported_with_final_size() {
    let fx = Fixture::new(649);
    let _ = fx
        .service
        .list(fx.request(&fx.location))
        .await
        .expect("list");
    let mut sub = fx.subscribe();
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    // `jq ... > out.jsonl`: create (truncate) then stream-write for a while.
    let path = fx.dir.join("out.jsonl");
    let mut file = std::fs::File::create(&path).expect("create");
    use std::io::Write;
    for _ in 0..10 {
        file.write_all(&[b'a'; 1_000_000]).expect("write");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    drop(file);
    let size = wait_for_delta(&mut sub, SEEN, |d| match d {
        DirectoryDeltaPayload::Reset { snapshot } => snapshot
            .entries
            .iter()
            .find(|e| e.name == "out.jsonl")
            .and_then(|e| e.size),
        DirectoryDeltaPayload::EntriesAdded { entries, .. }
        | DirectoryDeltaPayload::EntriesUpdated { entries, .. } => entries
            .iter()
            .find(|e| e.name == "out.jsonl")
            .and_then(|e| e.size),
        DirectoryDeltaPayload::EntriesRemoved { .. } => None,
    })
    .await;
    assert!(size.is_some(), "file written in place never reported");
    // Final state must eventually be consistent.
    let mut last = size;
    while let Some(next) =
        wait_for_delta(&mut sub, std::time::Duration::from_secs(2), |d| match d {
            DirectoryDeltaPayload::Reset { snapshot } => snapshot
                .entries
                .iter()
                .find(|e| e.name == "out.jsonl")
                .and_then(|e| e.size),
            DirectoryDeltaPayload::EntriesAdded { entries, .. }
            | DirectoryDeltaPayload::EntriesUpdated { entries, .. } => entries
                .iter()
                .find(|e| e.name == "out.jsonl")
                .and_then(|e| e.size),
            DirectoryDeltaPayload::EntriesRemoved { .. } => None,
        })
        .await
    {
        last = Some(next);
    }
    assert_eq!(last, Some(Some(10_000_000)).flatten());
}

#[tokio::test]
async fn navigating_away_and_back_lists_files_created_while_away_and_keeps_watching() {
    let fx = Fixture::new(649);
    let other = tempfile::tempdir().expect("other");
    let other_location = Location::from_native_path(other.path()).expect("other loc");
    let _ = fx
        .service
        .list(fx.request(&fx.location))
        .await
        .expect("list");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let _ = fx
        .service
        .list(fx.request(&other_location))
        .await
        .expect("away");
    std::fs::write(fx.dir.join("while-away.txt"), b"x").expect("create");
    let back = fx
        .service
        .list(fx.request(&fx.location))
        .await
        .expect("back");
    assert_eq!(back.total_known_entries, Some(650));
    let mut sub = fx.subscribe();
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    std::fs::write(fx.dir.join("after-back.txt"), b"x").expect("create");
    let seen = wait_for_delta(&mut sub, SEEN, |d| {
        names_in(d)
            .contains(&"after-back.txt".to_owned())
            .then_some(())
    })
    .await;
    assert!(seen.is_some(), "no delta after navigating back");
}
