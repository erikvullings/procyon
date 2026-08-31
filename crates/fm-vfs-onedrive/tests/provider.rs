//! Contract tests for [`fm_vfs_onedrive::OneDriveFileSystemProvider`], run
//! against the in-process Microsoft Graph fixture in
//! [`fm_vfs_onedrive::fixture`] (task 0110's acceptance criteria: "Tests
//! mock or safely fixture provider API behavior" - no real Microsoft call is
//! ever made).

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use fm_domain::{EntryId, Location, ProviderId};
use fm_vfs::{
    ChangeTracking, CopyCommitOptions, FileSystemProvider, ListOptions, ProviderCapabilities,
    ProviderChange, RemoveOptions, VfsError, WriteOptions,
};
use fm_vfs_onedrive::fixture::GraphFixture;
use fm_vfs_onedrive::{
    GraphConfig, OneDriveAccessToken, OneDriveConnectionResolver, OneDriveFileSystemProvider,
    RetryPolicy, SIMPLE_UPLOAD_THRESHOLD, UPLOAD_FRAGMENT_SIZE,
};
use futures::StreamExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

const CONNECTION_A: &str = "11111111-1111-4111-8111-111111111111";
const CONNECTION_B: &str = "22222222-2222-4222-8222-222222222222";

/// Resolves every connection id to a fixed token, or a distinct token per
/// connection id when seeded with more than one - either way, always
/// `Ok`, never touching a real credential store.
struct FixedResolver {
    tokens: std::collections::HashMap<String, String>,
    default_token: String,
}

impl FixedResolver {
    fn single(token: &str) -> Self {
        Self {
            tokens: std::collections::HashMap::new(),
            default_token: token.to_owned(),
        }
    }

    fn per_connection(pairs: &[(&str, &str)]) -> Self {
        Self {
            tokens: pairs
                .iter()
                .map(|(id, token)| ((*id).to_owned(), (*token).to_owned()))
                .collect(),
            default_token: "unused-default-token".to_owned(),
        }
    }
}

#[async_trait]
impl OneDriveConnectionResolver for FixedResolver {
    async fn resolve(&self, connection_id: &str) -> Result<OneDriveAccessToken, VfsError> {
        let token = self
            .tokens
            .get(connection_id)
            .cloned()
            .unwrap_or_else(|| self.default_token.clone());
        Ok(OneDriveAccessToken::new(token))
    }
}

fn test_config(fixture: &GraphFixture) -> GraphConfig {
    GraphConfig::new(
        url::Url::parse(&fixture.graph_base_url()).expect("valid fixture base URL"),
        RetryPolicy::for_tests(Duration::from_millis(5)),
        Duration::from_millis(15),
    )
}

fn provider_with(
    fixture: &GraphFixture,
    resolver: Arc<dyn OneDriveConnectionResolver>,
) -> OneDriveFileSystemProvider {
    OneDriveFileSystemProvider::with_config(resolver, test_config(fixture))
}

async fn provider(fixture: &GraphFixture) -> OneDriveFileSystemProvider {
    provider_with(
        fixture,
        Arc::new(FixedResolver::single("fixture-bearer-token")),
    )
}

fn root(connection_id: &str) -> Location {
    Location::parse(&format!("onedrive://{connection_id}/")).expect("valid onedrive root")
}

fn entry(location: Location) -> fm_vfs::EntryRef {
    fm_vfs::EntryRef {
        id: EntryId::new(),
        location,
    }
}

fn cancel() -> CancellationToken {
    CancellationToken::new()
}

#[tokio::test]
async fn provider_id_capabilities_and_change_tracking_are_reported_honestly() {
    let fixture = GraphFixture::start().await;
    let provider = provider(&fixture).await;

    assert_eq!(provider.id(), ProviderId::new("onedrive"));
    let capabilities = provider.capabilities();
    assert!(capabilities.contains(ProviderCapabilities::LIST));
    assert!(capabilities.contains(ProviderCapabilities::READ));
    assert!(capabilities.contains(ProviderCapabilities::WRITE));
    assert!(capabilities.contains(ProviderCapabilities::CREATE_DIRECTORY));
    assert!(capabilities.contains(ProviderCapabilities::RENAME));
    assert!(capabilities.contains(ProviderCapabilities::MOVE));
    assert!(capabilities.contains(ProviderCapabilities::TRASH));
    assert!(capabilities.contains(ProviderCapabilities::WATCH));
    assert!(capabilities.contains(ProviderCapabilities::RANDOM_ACCESS));
    // Never over-advertise: no true permanent delete, no native
    // server-side copy in this slice, no POSIX-style permissions/checksum.
    assert!(!capabilities.contains(ProviderCapabilities::DELETE));
    assert!(!capabilities.contains(ProviderCapabilities::SERVER_SIDE_COPY));
    assert!(!capabilities.contains(ProviderCapabilities::SET_PERMISSIONS));
    assert!(!capabilities.contains(ProviderCapabilities::CHECKSUM));

    assert_eq!(provider.change_tracking(), ChangeTracking::DeltaApi);
}

#[tokio::test]
async fn transfer_capabilities_use_the_exact_endpoint_format_and_distinguish_connections() {
    let fixture = GraphFixture::start().await;
    let provider = provider(&fixture).await;

    let a = provider.transfer_capabilities(&root(CONNECTION_A)).unwrap();
    let b = provider.transfer_capabilities(&root(CONNECTION_B)).unwrap();

    assert_eq!(a.endpoint.as_str(), format!("onedrive:{CONNECTION_A}"));
    assert_eq!(b.endpoint.as_str(), format!("onedrive:{CONNECTION_B}"));
    assert!(!a.shares_endpoint_with(&b));
    assert!(
        a.resumable_upload,
        "resumable upload is genuinely implemented"
    );
    assert!(
        a.resumable_download,
        "ranged download is genuinely implemented"
    );
    assert!(a.random_read);
    assert!(
        !a.server_side_copy,
        "no over-advertised same-drive async copy in this slice"
    );
    assert!(a.server_side_move, "rename/move is a real same-drive PATCH");
    assert!(!a.random_write);
}

#[tokio::test]
async fn lists_a_single_page_of_children() {
    let fixture = GraphFixture::start().await;
    fixture.create_folder("", "Documents").await;
    fixture.create_file("", "report.pdf", b"hello world").await;
    let provider = provider(&fixture).await;

    let page = provider
        .list(&root(CONNECTION_A), ListOptions::default(), cancel())
        .await
        .expect("list must succeed");

    assert_eq!(page.entries.len(), 2);
    assert!(!page.has_more);
    assert!(page.continuation_token.is_none());
    let names: Vec<_> = page
        .entries
        .iter()
        .map(|entry| entry.name.clone())
        .collect();
    assert!(names.contains(&"Documents".to_owned()));
    assert!(names.contains(&"report.pdf".to_owned()));
    let file = page
        .entries
        .iter()
        .find(|entry| entry.name == "report.pdf")
        .unwrap();
    assert_eq!(file.size, Some(11));
    assert_eq!(file.kind, fm_domain::EntryKind::File);
    let folder = page
        .entries
        .iter()
        .find(|entry| entry.name == "Documents")
        .unwrap();
    assert_eq!(folder.kind, fm_domain::EntryKind::Directory);
    assert_eq!(folder.size, None);
}

#[tokio::test]
async fn lists_nested_folders() {
    let fixture = GraphFixture::start().await;
    fixture.create_folder("", "Documents").await;
    fixture.create_file("Documents", "notes.txt", b"abc").await;
    let provider = provider(&fixture).await;

    let documents = root(CONNECTION_A).join("Documents").unwrap();
    let page = provider
        .list(&documents, ListOptions::default(), cancel())
        .await
        .unwrap();

    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].name, "notes.txt");
    assert_eq!(
        page.entries[0].location.uri,
        format!("onedrive://{CONNECTION_A}/Documents/notes.txt")
    );
}

/// Task 0110: "first request honors page_size; continuation_token must be
/// the full opaque `@odata.nextLink`, called verbatim." Also exercises
/// "multi-page ... ordering with interleaved unsorted items, not only one
/// sorted page" by deliberately ordering children non-alphabetically.
#[tokio::test]
async fn pages_through_multiple_unsorted_interleaved_pages_by_following_the_opaque_next_link_verbatim()
 {
    let fixture = GraphFixture::start().await;
    for name in ["zeta", "alpha", "mu", "beta", "omega"] {
        fixture.create_file("", name, name.as_bytes()).await;
    }
    // Deliberately not alphabetical - a provider that silently re-sorts
    // would fail the ordering assertions below.
    fixture
        .set_children_order("", vec!["zeta", "alpha", "mu", "beta", "omega"])
        .await;
    let provider = provider(&fixture).await;

    let first = provider
        .list(
            &root(CONNECTION_A),
            ListOptions {
                page_size: 2,
                continuation_token: None,
            },
            cancel(),
        )
        .await
        .expect("first page must succeed");
    assert_eq!(names_of(&first.entries), vec!["zeta", "alpha"]);
    assert!(first.has_more);
    let token_after_first = first
        .continuation_token
        .clone()
        .expect("a next link must be present");
    assert!(
        token_after_first.contains("cursor_2"),
        "the fixture's opaque token: {token_after_first}"
    );

    let second = provider
        .list(
            &root(CONNECTION_A),
            ListOptions {
                page_size: 2,
                continuation_token: first.continuation_token,
            },
            cancel(),
        )
        .await
        .expect("second page must succeed");
    assert_eq!(names_of(&second.entries), vec!["mu", "beta"]);
    assert!(second.has_more);

    let third = provider
        .list(
            &root(CONNECTION_A),
            ListOptions {
                page_size: 2,
                continuation_token: second.continuation_token,
            },
            cancel(),
        )
        .await
        .expect("third page must succeed");
    assert_eq!(names_of(&third.entries), vec!["omega"]);
    assert!(!third.has_more);
    assert!(third.continuation_token.is_none());

    // The first request's $top must reflect the requested page_size.
    let requests = fixture.requests().await;
    let first_list_request = requests
        .iter()
        .find(|request| {
            request.path.contains("/children")
                && request.path.contains("$top=2")
                && !request.path.contains("skiptoken")
        })
        .expect("the first page request must carry $top=2");
    assert!(first_list_request.path.contains("$top=2"));
}

fn names_of(entries: &[fm_domain::EntrySummary]) -> Vec<String> {
    entries.iter().map(|entry| entry.name.clone()).collect()
}

#[tokio::test]
async fn rejects_a_continuation_token_that_is_not_same_origin_with_the_configured_graph_endpoint() {
    let fixture = GraphFixture::start().await;
    let provider = provider(&fixture).await;

    let error = provider
        .list(
            &root(CONNECTION_A),
            ListOptions {
                page_size: 10,
                continuation_token: Some(
                    "http://evil.example.test/v1.0/me/drive/root/children".to_owned(),
                ),
            },
            cancel(),
        )
        .await
        .expect_err("a foreign continuation link must be rejected");
    assert!(matches!(error, VfsError::Io { .. }));
}

#[tokio::test]
async fn inspect_and_file_size_read_a_single_item_directly() {
    let fixture = GraphFixture::start().await;
    fixture.create_file("", "report.pdf", b"hello world").await;
    let provider = provider(&fixture).await;

    let file_location = root(CONNECTION_A).join("report.pdf").unwrap();
    let summary = provider
        .inspect(&entry(file_location.clone()), cancel())
        .await
        .unwrap();
    assert_eq!(summary.name, "report.pdf");
    assert_eq!(summary.size, Some(11));

    let size = provider
        .file_size(&entry(file_location), cancel())
        .await
        .unwrap();
    assert_eq!(size, 11);
}

#[tokio::test]
async fn inspect_reports_not_found_for_a_missing_item() {
    let fixture = GraphFixture::start().await;
    let provider = provider(&fixture).await;

    let missing = root(CONNECTION_A).join("missing.txt").unwrap();
    let error = provider
        .inspect(&entry(missing), cancel())
        .await
        .expect_err("must fail");
    assert!(matches!(error, VfsError::NotFound { .. }));
}

#[tokio::test]
async fn file_size_on_a_directory_reports_is_a_directory() {
    let fixture = GraphFixture::start().await;
    fixture.create_folder("", "Documents").await;
    let provider = provider(&fixture).await;

    let documents = root(CONNECTION_A).join("Documents").unwrap();
    let error = provider
        .file_size(&entry(documents), cancel())
        .await
        .expect_err("must fail");
    assert!(matches!(error, VfsError::IsADirectory { .. }));
}

#[tokio::test]
async fn create_directory_succeeds_and_reports_already_exists_on_conflict() {
    let fixture = GraphFixture::start().await;
    let provider = provider(&fixture).await;

    let created = provider
        .create_directory(&root(CONNECTION_A), "Documents", cancel())
        .await
        .unwrap();
    assert_eq!(
        created.location.uri,
        format!("onedrive://{CONNECTION_A}/Documents")
    );
    assert!(fixture.exists("Documents").await);

    let error = provider
        .create_directory(&root(CONNECTION_A), "Documents", cancel())
        .await
        .expect_err("a duplicate name must be rejected");
    assert!(matches!(error, VfsError::AlreadyExists { .. }));
}

#[tokio::test]
async fn rename_changes_the_name_within_the_same_directory() {
    let fixture = GraphFixture::start().await;
    fixture.create_file("", "old.txt", b"content").await;
    let provider = provider(&fixture).await;

    let source = entry(root(CONNECTION_A).join("old.txt").unwrap());
    let destination = root(CONNECTION_A).join("new.txt").unwrap();
    let renamed = provider
        .rename(&source, &destination, cancel())
        .await
        .unwrap();

    assert_eq!(
        renamed.location.uri,
        format!("onedrive://{CONNECTION_A}/new.txt")
    );
    assert!(!fixture.exists("old.txt").await);
    assert_eq!(
        fixture.file_content("new.txt").await,
        Some(b"content".to_vec())
    );
}

#[tokio::test]
async fn rename_moves_across_directories_using_the_destination_parents_real_graph_id() {
    let fixture = GraphFixture::start().await;
    fixture.create_folder("", "Source").await;
    fixture.create_folder("", "Target").await;
    fixture
        .create_file("Source", "report.pdf", b"content")
        .await;
    let provider = provider(&fixture).await;

    let source = entry(
        root(CONNECTION_A)
            .join("Source")
            .unwrap()
            .join("report.pdf")
            .unwrap(),
    );
    let destination = root(CONNECTION_A)
        .join("Target")
        .unwrap()
        .join("report.pdf")
        .unwrap();
    let moved = provider
        .rename(&source, &destination, cancel())
        .await
        .unwrap();

    assert_eq!(
        moved.location.uri,
        format!("onedrive://{CONNECTION_A}/Target/report.pdf")
    );
    assert!(!fixture.exists("Source/report.pdf").await);
    assert_eq!(
        fixture.file_content("Target/report.pdf").await,
        Some(b"content".to_vec())
    );

    // The PATCH body must reference the destination parent's real Graph
    // item id, not a bare name or path (task 0110).
    let requests = fixture.requests().await;
    let patch = requests
        .iter()
        .find(|request| request.method == "PATCH")
        .expect("a PATCH request must have been sent");
    let body: serde_json::Value = serde_json::from_slice(&patch.body).unwrap();
    assert_eq!(body["name"], serde_json::json!("report.pdf"));
    assert!(
        body["parentReference"]["id"]
            .as_str()
            .unwrap()
            .starts_with("01FIXTUREITEM")
    );
}

#[tokio::test]
async fn rename_across_two_different_connections_is_rejected() {
    let fixture = GraphFixture::start().await;
    fixture.create_file("", "report.pdf", b"content").await;
    let provider = provider(&fixture).await;

    let source = entry(root(CONNECTION_A).join("report.pdf").unwrap());
    let destination = root(CONNECTION_B).join("report.pdf").unwrap();
    let error = provider
        .rename(&source, &destination, cancel())
        .await
        .expect_err("must fail");
    assert!(matches!(error, VfsError::InvalidLocation { .. }));
}

#[tokio::test]
async fn remove_without_trash_is_honestly_unsupported() {
    let fixture = GraphFixture::start().await;
    fixture.create_file("", "report.pdf", b"content").await;
    let provider = provider(&fixture).await;

    let error = provider
        .remove(
            &entry(root(CONNECTION_A).join("report.pdf").unwrap()),
            RemoveOptions {
                recursive: false,
                use_trash: false,
            },
            cancel(),
        )
        .await
        .expect_err("permanent delete is not available");
    assert!(matches!(
        error,
        VfsError::UnsupportedCapability { capability } if capability == ProviderCapabilities::DELETE
    ));
    // Nothing was actually deleted.
    assert!(fixture.exists("report.pdf").await);
}

#[tokio::test]
async fn remove_with_trash_recycles_the_item_and_is_idempotent() {
    let fixture = GraphFixture::start().await;
    fixture.create_file("", "report.pdf", b"content").await;
    let provider = provider(&fixture).await;

    let target = entry(root(CONNECTION_A).join("report.pdf").unwrap());
    provider
        .remove(
            &target,
            RemoveOptions {
                recursive: false,
                use_trash: true,
            },
            cancel(),
        )
        .await
        .expect("recycle-bin delete must succeed");
    assert!(!fixture.exists("report.pdf").await);

    // Removing an already-removed item is not an error (idempotent).
    provider
        .remove(
            &target,
            RemoveOptions {
                recursive: false,
                use_trash: true,
            },
            cancel(),
        )
        .await
        .expect("removing an already-removed item must succeed");
}

#[tokio::test]
async fn same_filesystem_is_true_only_for_the_same_connection() {
    let fixture = GraphFixture::start().await;
    let provider = provider(&fixture).await;

    let source = entry(root(CONNECTION_A).join("a.txt").unwrap());
    assert!(
        provider
            .same_filesystem(&source, &root(CONNECTION_A), cancel())
            .await
            .unwrap()
    );
    assert!(
        !provider
            .same_filesystem(&source, &root(CONNECTION_B), cancel())
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn open_read_downloads_full_content_without_ever_sending_the_bearer_to_the_transfer_host() {
    let fixture = GraphFixture::start().await;
    fixture.create_file("", "report.pdf", b"hello world").await;
    let provider = provider(&fixture).await;

    let mut reader = provider
        .open_read(
            &entry(root(CONNECTION_A).join("report.pdf").unwrap()),
            cancel(),
        )
        .await
        .expect("open_read must succeed");
    let mut buffer = Vec::new();
    reader
        .read_to_end(&mut buffer)
        .await
        .expect("read must succeed");
    assert_eq!(buffer, b"hello world");

    // The metadata request (Graph host) is authenticated; the transfer
    // host download request must **never** carry a bearer token at all
    // (task 0110's explicit, must-assert requirement).
    let transfer_requests = fixture.transfer_requests().await;
    let download = transfer_requests
        .iter()
        .find(|request| request.path.starts_with("/download/"))
        .expect("a download request must have been captured");
    assert!(
        !download.has_authorization_header(),
        "headers were: {:?}",
        download.headers
    );

    let graph_requests = fixture.requests().await;
    assert!(
        graph_requests
            .iter()
            .all(fm_vfs_onedrive::fixture::CapturedRequest::has_authorization_header)
    );
}

#[tokio::test]
async fn read_range_requests_a_byte_range_and_accepts_a_partial_response() {
    let fixture = GraphFixture::start().await;
    fixture.create_file("", "report.pdf", b"0123456789").await;
    let provider = provider(&fixture).await;

    let mut reader = provider
        .read_range(
            &entry(root(CONNECTION_A).join("report.pdf").unwrap()),
            2,
            Some(3),
            cancel(),
        )
        .await
        .expect("read_range must succeed");
    let mut buffer = Vec::new();
    reader
        .read_to_end(&mut buffer)
        .await
        .expect("read must succeed");
    assert_eq!(buffer, b"234");

    let transfer_requests = fixture.transfer_requests().await;
    let download = transfer_requests
        .iter()
        .rev()
        .find(|request| request.path.starts_with("/download/"))
        .expect("a download request must have been captured");
    assert_eq!(download.headers.get("range"), Some(&"bytes=2-4".to_owned()));
    assert!(!download.has_authorization_header());
}

#[tokio::test]
async fn open_read_on_the_root_is_rejected_as_a_directory() {
    let fixture = GraphFixture::start().await;
    let provider = provider(&fixture).await;

    let result = provider
        .open_read(&entry(root(CONNECTION_A)), cancel())
        .await;
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("reading the root as a file must fail"),
    };
    assert!(matches!(error, VfsError::IsADirectory { .. }));
}

async fn write_all_and_shutdown(
    provider: &OneDriveFileSystemProvider,
    location: &Location,
    contents: &[u8],
    cancellation: CancellationToken,
) -> std::io::Result<()> {
    let mut writer = provider
        .open_write(location, WriteOptions::default(), cancellation)
        .await
        .expect("open_write must succeed");
    writer.write_all(contents).await?;
    writer.shutdown().await
}

#[tokio::test]
async fn open_write_uploads_a_small_payload_of_unknown_size() {
    let fixture = GraphFixture::start().await;
    let provider = provider(&fixture).await;
    let destination = root(CONNECTION_A).join("small.txt").unwrap();

    write_all_and_shutdown(&provider, &destination, b"hello onedrive", cancel())
        .await
        .expect("a small unknown-size upload must succeed");

    assert_eq!(
        fixture.file_content("small.txt").await,
        Some(b"hello onedrive".to_vec())
    );
    // A small write must go through the plain `/content` endpoint, never a
    // session, and must carry the bearer (it targets the Graph host).
    let requests = fixture.requests().await;
    let content_put = requests
        .iter()
        .find(|request| request.method == "PUT" && request.path.contains("/content"))
        .expect("a simple PUT must have been sent");
    assert!(content_put.has_authorization_header());
    assert!(
        !requests
            .iter()
            .any(|request| request.path.contains("createUploadSession"))
    );
}

#[tokio::test]
async fn open_write_refuses_a_payload_larger_than_the_unknown_size_bound_without_publishing_anything()
 {
    let fixture = GraphFixture::start().await;
    let provider = provider(&fixture).await;
    let destination = root(CONNECTION_A).join("too-big.bin").unwrap();
    let payload = vec![0x42_u8; (SIMPLE_UPLOAD_THRESHOLD + 1) as usize];

    let result = write_all_and_shutdown(&provider, &destination, &payload, cancel()).await;

    assert!(
        result.is_err(),
        "an unknown-size write over the bound must fail, not silently truncate or succeed"
    );
    assert!(!fixture.exists("too-big.bin").await);
}

#[tokio::test]
async fn open_write_sized_below_the_threshold_uses_a_single_simple_upload() {
    let fixture = GraphFixture::start().await;
    let provider = provider(&fixture).await;
    let destination = root(CONNECTION_A).join("sized-small.txt").unwrap();
    let payload = b"a small, size-known payload";

    let mut writer = provider
        .open_write_sized(
            &destination,
            WriteOptions::default(),
            payload.len() as u64,
            cancel(),
        )
        .await
        .unwrap();
    writer.write_all(payload).await.unwrap();
    writer.shutdown().await.expect("shutdown must succeed");

    assert_eq!(
        fixture.file_content("sized-small.txt").await,
        Some(payload.to_vec())
    );
    let requests = fixture.requests().await;
    assert!(
        !requests
            .iter()
            .any(|request| request.path.contains("createUploadSession"))
    );
}

#[tokio::test]
async fn open_write_sized_above_the_threshold_drives_a_real_sequential_chunked_upload_session() {
    let fixture = GraphFixture::start().await;
    let provider = provider(&fixture).await;
    let destination = root(CONNECTION_A).join("large.bin").unwrap();

    let total_size = UPLOAD_FRAGMENT_SIZE * 2 + 12_345;
    let payload: Vec<u8> = (0..total_size).map(|index| (index % 251) as u8).collect();

    let mut writer = provider
        .open_write_sized(&destination, WriteOptions::default(), total_size, cancel())
        .await
        .unwrap();
    writer.write_all(&payload).await.unwrap();
    writer.shutdown().await.expect("shutdown must succeed");

    assert_eq!(fixture.file_content("large.bin").await, Some(payload));

    // One authenticated `createUploadSession`, then exactly 3 sequential,
    // bearer-free chunk PUTs (two full 320-KiB-multiple fragments, one
    // final short one) to the transfer host.
    let requests = fixture.requests().await;
    let session_creates: Vec<_> = requests
        .iter()
        .filter(|r| r.path.contains("createUploadSession"))
        .collect();
    assert_eq!(session_creates.len(), 1);
    assert!(session_creates[0].has_authorization_header());

    let transfer_requests = fixture.transfer_requests().await;
    let chunk_puts: Vec<_> = transfer_requests
        .iter()
        .filter(|request| request.method == "PUT" && request.path.starts_with("/upload-sessions/"))
        .collect();
    assert_eq!(
        chunk_puts.len(),
        3,
        "expected 2 full fragments + 1 short final fragment"
    );
    for chunk in &chunk_puts {
        assert!(
            !chunk.has_authorization_header(),
            "chunk PUTs must never carry the bearer token"
        );
        let content_range = chunk
            .headers
            .get("content-range")
            .expect("Content-Range must be present");
        assert!(
            content_range.ends_with(&format!("/{total_size}")),
            "declared total must be stable: {content_range}"
        );
    }
    let first_range = chunk_puts[0].headers.get("content-range").unwrap();
    assert_eq!(
        *first_range,
        format!("bytes 0-{}/{total_size}", UPLOAD_FRAGMENT_SIZE - 1)
    );
    let second_range = chunk_puts[1].headers.get("content-range").unwrap();
    assert_eq!(
        *second_range,
        format!(
            "bytes {}-{}/{total_size}",
            UPLOAD_FRAGMENT_SIZE,
            UPLOAD_FRAGMENT_SIZE * 2 - 1
        )
    );
    let third_range = chunk_puts[2].headers.get("content-range").unwrap();
    assert_eq!(
        *third_range,
        format!(
            "bytes {}-{}/{total_size}",
            UPLOAD_FRAGMENT_SIZE * 2,
            total_size - 1
        )
    );
}

#[tokio::test]
async fn open_write_sized_rejects_a_short_write_and_publishes_nothing() {
    let fixture = GraphFixture::start().await;
    let provider = provider(&fixture).await;
    let destination = root(CONNECTION_A).join("short.txt").unwrap();

    let mut writer = provider
        .open_write_sized(&destination, WriteOptions::default(), 1000, cancel())
        .await
        .unwrap();
    writer.write_all(b"only ten!!").await.unwrap();
    let result = writer.shutdown().await;

    assert!(
        result.is_err(),
        "shutdown must surface the size-contract violation, not a success-shaped failure"
    );
    assert!(!fixture.exists("short.txt").await);
}

#[tokio::test]
async fn open_write_sized_rejects_an_oversized_write_and_publishes_nothing() {
    let fixture = GraphFixture::start().await;
    let provider = provider(&fixture).await;
    let destination = root(CONNECTION_A).join("oversized.txt").unwrap();

    let mut writer = provider
        .open_write_sized(&destination, WriteOptions::default(), 4, cancel())
        .await
        .unwrap();
    let write_result = writer.write_all(b"way too many bytes").await;
    let final_result = match write_result {
        Ok(()) => writer.shutdown().await,
        Err(error) => Err(error),
    };

    assert!(
        final_result.is_err(),
        "writing past the declared size must surface an error somewhere"
    );
    assert!(!fixture.exists("oversized.txt").await);
}

#[tokio::test]
async fn cancelling_a_chunked_upload_mid_flight_deletes_the_session_and_publishes_nothing() {
    let fixture = GraphFixture::start().await;
    let provider = provider(&fixture).await;
    let destination = root(CONNECTION_A).join("cancelled.bin").unwrap();
    let total_size = UPLOAD_FRAGMENT_SIZE * 2;
    let cancellation = cancel();

    let mut writer = provider
        .open_write_sized(
            &destination,
            WriteOptions::default(),
            total_size,
            cancellation.clone(),
        )
        .await
        .unwrap();
    // Write less than one full fragment, so the background task is
    // guaranteed to still be waiting for more bytes when we cancel.
    writer.write_all(&vec![0x11_u8; 1024]).await.unwrap();

    // Wait for the upload session to actually exist before cancelling, so
    // this genuinely exercises the DELETE cleanup path rather than racing
    // cancellation against session creation (in which case there would be
    // nothing to delete yet - also correct, but not what this test covers).
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if fixture
                .requests()
                .await
                .iter()
                .any(|request| request.path.contains("createUploadSession"))
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("a createUploadSession request must happen");

    cancellation.cancel();
    let result = writer.shutdown().await;

    assert!(
        result.is_err(),
        "a cancelled upload must surface as a failed shutdown"
    );
    assert!(!fixture.exists("cancelled.bin").await);
    let transfer_requests = fixture.transfer_requests().await;
    assert!(
        transfer_requests
            .iter()
            .any(|request| request.method == "DELETE"
                && request.path.starts_with("/upload-sessions/")),
        "cancellation must best-effort DELETE the upload session; captured: {transfer_requests:?}"
    );
}

/// Regression test for a real truncation bug: `drive_chunks`' final-chunk
/// branch used to return success immediately after the last expected byte
/// was accepted, without ever checking whether the caller had written
/// *more* than the declared `expected_size`. Any trailing bytes already
/// sitting in the duplex pipe were then silently dropped - a truncated
/// upload reported as a success. The fix must detect the excess **before**
/// the final chunk is ever sent, so the file is never completed/published
/// on the server at all once the size contract is violated.
#[tokio::test]
async fn writing_more_than_the_declared_size_in_a_chunked_session_fails_and_publishes_nothing() {
    let fixture = GraphFixture::start().await;
    let provider = provider(&fixture).await;
    let destination = root(CONNECTION_A).join("oversized-session.bin").unwrap();
    // The smallest size that still forces the session (chunked) path:
    // `UPLOAD_FRAGMENT_SIZE` is smaller than `SIMPLE_UPLOAD_THRESHOLD`, so
    // this always needs at least two fragments.
    let expected_size = SIMPLE_UPLOAD_THRESHOLD + 1;

    let mut writer = provider
        .open_write_sized(
            &destination,
            WriteOptions::default(),
            expected_size,
            cancel(),
        )
        .await
        .unwrap();
    let mut payload = vec![0x37_u8; expected_size as usize];
    payload.extend_from_slice(b"unexpected-trailing-bytes-beyond-the-declared-size");
    let write_result = writer.write_all(&payload).await;
    let final_result = match write_result {
        Ok(()) => writer.shutdown().await,
        Err(error) => Err(error),
    };

    assert!(
        final_result.is_err(),
        "writing past the declared size in the chunked/session path must surface an error, not silently truncate"
    );
    assert!(
        !fixture.exists("oversized-session.bin").await,
        "no destination must ever be published when the size contract was violated"
    );

    let transfer_requests = fixture.transfer_requests().await;
    let chunk_puts = transfer_requests
        .iter()
        .filter(|request| request.method == "PUT" && request.path.starts_with("/upload-sessions/"))
        .count();
    assert_eq!(
        chunk_puts, 1,
        "only the non-final chunk may have been sent; the final chunk must never be uploaded once \
         excess data is detected, or the file would already be complete on the server despite the \
         reported error - captured: {transfer_requests:?}"
    );
    assert!(
        transfer_requests
            .iter()
            .any(|request| request.method == "DELETE"
                && request.path.starts_with("/upload-sessions/")),
        "the incomplete session must be best-effort deleted; captured: {transfer_requests:?}"
    );
}

#[tokio::test]
async fn commit_copy_publishes_a_temporary_by_renaming_it_into_place() {
    let fixture = GraphFixture::start().await;
    let provider = provider(&fixture).await;
    let temporary = root(CONNECTION_A).join(".fm-copy-test").unwrap();
    write_all_and_shutdown(&provider, &temporary, b"copied content", cancel())
        .await
        .unwrap();

    let destination = root(CONNECTION_A).join("final.txt").unwrap();
    let published = provider
        .commit_copy(
            &entry(temporary.clone()),
            &temporary,
            &destination,
            CopyCommitOptions {
                overwrite: false,
                preserve_metadata: false,
            },
            cancel(),
        )
        .await
        .expect("commit_copy must succeed");

    assert_eq!(published.location.uri, destination.uri);
    assert_eq!(
        fixture.file_content("final.txt").await,
        Some(b"copied content".to_vec())
    );
    assert!(!fixture.exists(".fm-copy-test").await);
}

#[tokio::test]
async fn commit_copy_without_overwrite_reports_already_exists_and_leaves_both_files() {
    let fixture = GraphFixture::start().await;
    let provider = provider(&fixture).await;
    fixture.create_file("", "final.txt", b"existing").await;
    let temporary = root(CONNECTION_A).join(".fm-copy-test").unwrap();
    write_all_and_shutdown(&provider, &temporary, b"copied content", cancel())
        .await
        .unwrap();

    let destination = root(CONNECTION_A).join("final.txt").unwrap();
    let error = provider
        .commit_copy(
            &entry(temporary.clone()),
            &temporary,
            &destination,
            CopyCommitOptions {
                overwrite: false,
                preserve_metadata: false,
            },
            cancel(),
        )
        .await
        .expect_err("must refuse to overwrite");

    assert!(matches!(error, VfsError::AlreadyExists { .. }));
    assert_eq!(
        fixture.file_content("final.txt").await,
        Some(b"existing".to_vec())
    );
    assert!(fixture.exists(".fm-copy-test").await);
}

#[tokio::test]
async fn commit_copy_with_overwrite_replaces_the_existing_destination() {
    let fixture = GraphFixture::start().await;
    let provider = provider(&fixture).await;
    fixture.create_file("", "final.txt", b"stale").await;
    let temporary = root(CONNECTION_A).join(".fm-copy-test").unwrap();
    write_all_and_shutdown(&provider, &temporary, b"fresh content", cancel())
        .await
        .unwrap();

    let destination = root(CONNECTION_A).join("final.txt").unwrap();
    provider
        .commit_copy(
            &entry(temporary.clone()),
            &temporary,
            &destination,
            CopyCommitOptions {
                overwrite: true,
                preserve_metadata: false,
            },
            cancel(),
        )
        .await
        .expect("commit_copy with overwrite must succeed");

    assert_eq!(
        fixture.file_content("final.txt").await,
        Some(b"fresh content".to_vec())
    );
    assert!(!fixture.exists(".fm-copy-test").await);
}

#[tokio::test]
async fn discard_copy_removes_a_temporary_and_is_idempotent_when_already_gone() {
    let fixture = GraphFixture::start().await;
    let provider = provider(&fixture).await;
    let temporary = root(CONNECTION_A).join(".fm-copy-test").unwrap();
    write_all_and_shutdown(&provider, &temporary, b"scratch", cancel())
        .await
        .unwrap();

    provider
        .discard_copy(&temporary, cancel())
        .await
        .expect("discard_copy must succeed");
    assert!(!fixture.exists(".fm-copy-test").await);

    provider
        .discard_copy(&temporary, cancel())
        .await
        .expect("discarding an already-gone temporary must still succeed");
}

#[tokio::test]
async fn throttling_honors_retry_after_before_succeeding() {
    let fixture = GraphFixture::start().await;
    fixture.create_file("", "report.pdf", b"hello").await;
    fixture.queue_throttle(429, Some(1)).await;
    let provider = provider(&fixture).await;

    let start = std::time::Instant::now();
    let page = provider
        .list(&root(CONNECTION_A), ListOptions::default(), cancel())
        .await
        .expect("the request must eventually succeed once Retry-After has elapsed");
    let elapsed = start.elapsed();

    assert_eq!(page.entries.len(), 1);
    assert!(
        elapsed >= Duration::from_millis(900),
        "elapsed: {elapsed:?}"
    );
    assert!(elapsed < Duration::from_secs(5), "elapsed: {elapsed:?}");
}

#[tokio::test]
async fn throttling_without_retry_after_falls_back_to_bounded_backoff() {
    let fixture = GraphFixture::start().await;
    fixture.create_file("", "report.pdf", b"hello").await;
    fixture.queue_throttle(503, None).await;
    fixture.queue_throttle(503, None).await;
    let provider = provider(&fixture).await;

    let start = std::time::Instant::now();
    let page = provider
        .list(&root(CONNECTION_A), ListOptions::default(), cancel())
        .await
        .expect("the request must eventually succeed after the fallback backoff");
    let elapsed = start.elapsed();

    assert_eq!(page.entries.len(), 1);
    // `test_config`'s `RetryPolicy::for_tests(5ms)` backs off 5ms then
    // 10ms - two orders of magnitude under a second, proving the short
    // configured backoff was used rather than a production-scale one.
    assert!(elapsed < Duration::from_millis(500), "elapsed: {elapsed:?}");
}

#[tokio::test]
async fn throttling_exhausting_all_retries_reports_an_honest_failure() {
    let fixture = GraphFixture::start().await;
    for _ in 0..10 {
        fixture.queue_throttle(503, None).await;
    }
    let provider = provider(&fixture).await;

    let error = provider
        .list(&root(CONNECTION_A), ListOptions::default(), cancel())
        .await
        .expect_err("must eventually give up");
    assert!(matches!(error, VfsError::Io { .. }));
}

#[tokio::test]
async fn watch_does_not_emit_on_the_seed_tick_and_reports_a_coalesced_change_afterwards() {
    let fixture = GraphFixture::start().await;
    let provider = provider(&fixture).await;
    let cancellation = cancel();
    let mut stream = provider
        .watch(&root(CONNECTION_A), cancellation.clone())
        .await
        .expect("watch must succeed");

    let nothing_yet = tokio::time::timeout(Duration::from_millis(70), stream.next()).await;
    assert!(nothing_yet.is_err(), "the seed tick alone must never emit");

    fixture.create_file("", "new.txt", b"x").await;

    let first = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("a real change must eventually be observed")
        .expect("the stream must not end here");
    assert!(matches!(first, Ok(ProviderChange::Changed)));

    cancellation.cancel();
}

/// Task 0110: "Tests must cover multi-page initial/next/delta ordering with
/// interleaved unsorted items, not only one sorted page" - applied to the
/// delta feed specifically (the earlier listing test already covers
/// `/children`).
#[tokio::test]
async fn watch_follows_multiple_interleaved_delta_pages_within_one_round() {
    let fixture = GraphFixture::start().await;
    let config = test_config(&fixture).with_delta_page_size(2);
    let provider =
        OneDriveFileSystemProvider::with_config(Arc::new(FixedResolver::single("token")), config);
    let cancellation = cancel();
    let mut stream = provider
        .watch(&root(CONNECTION_A), cancellation.clone())
        .await
        .expect("watch must succeed");

    let nothing_yet = tokio::time::timeout(Duration::from_millis(70), stream.next()).await;
    assert!(nothing_yet.is_err());

    // Five changes, deliberately interleaved across two different
    // folders (not one flat, alphabetically-sortable sequence), forced
    // into 3 pages by the `delta_page_size: 2` override above.
    fixture.create_folder("", "alpha").await;
    fixture.create_folder("", "zulu").await;
    fixture.create_file("zulu", "one.txt", b"1").await;
    fixture.create_file("alpha", "two.txt", b"2").await;
    fixture.create_file("zulu", "three.txt", b"3").await;

    let changed = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("a coalesced change must be observed")
        .expect("the stream must not end here");
    assert!(matches!(changed, Ok(ProviderChange::Changed)));

    let requests = fixture.requests().await;
    let delta_requests: Vec<_> = requests
        .iter()
        .filter(|request| request.path.contains("/delta"))
        .collect();
    // The seed (`token=latest`) plus at least 3 pages (5 items at 2 per
    // page) proves the multi-page path was genuinely exercised, not just
    // a single lucky page.
    assert!(
        delta_requests.len() >= 4,
        "expected a seed + >=3 pages, got {}",
        delta_requests.len()
    );

    cancellation.cancel();
}

/// Task 0110 review: the opaque delta-link contract requires `$top` to be
/// established once, on the initial `token=latest` seed request, and every
/// subsequent `@odata.nextLink`/`@odata.deltaLink` to be followed exactly
/// as returned - never re-appended, reconstructed, or duplicated.
#[tokio::test]
async fn watch_puts_top_on_the_seed_request_and_never_appends_or_duplicates_it_afterwards() {
    let fixture = GraphFixture::start().await;
    let config = test_config(&fixture).with_delta_page_size(2);
    let provider =
        OneDriveFileSystemProvider::with_config(Arc::new(FixedResolver::single("token")), config);
    let cancellation = cancel();
    let mut stream = provider
        .watch(&root(CONNECTION_A), cancellation.clone())
        .await
        .expect("watch must succeed");

    // Let the seed tick actually happen before creating anything - files
    // created before the seed would already be part of its own baseline
    // snapshot and would never show up as a subsequent change.
    let nothing_yet = tokio::time::timeout(Duration::from_millis(70), stream.next()).await;
    assert!(nothing_yet.is_err());

    // Five changes, forced into multiple pages by `delta_page_size: 2`, so
    // both the opening `@odata.nextLink` pagination *and* the final
    // `@odata.deltaLink` handed back for the *next* round are exercised.
    fixture.create_folder("", "alpha").await;
    fixture.create_folder("", "zulu").await;
    fixture.create_file("zulu", "one.txt", b"1").await;
    fixture.create_file("alpha", "two.txt", b"2").await;
    fixture.create_file("zulu", "three.txt", b"3").await;

    let first_round = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("a coalesced change must be observed")
        .expect("the stream must not end here");
    assert!(matches!(first_round, Ok(ProviderChange::Changed)));

    // Force a *second* round with more changes than fit on one page so the
    // deltaLink handed back at the end of the first round is itself
    // exercised as a followed-verbatim starting point, not just the seed.
    fixture.create_file("alpha", "four.txt", b"4").await;
    fixture.create_file("zulu", "five.txt", b"5").await;
    fixture.create_file("alpha", "six.txt", b"6").await;
    let second_round = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("a second coalesced change must be observed")
        .expect("the stream must not end here");
    assert!(matches!(second_round, Ok(ProviderChange::Changed)));

    let requests = fixture.requests().await;
    let seed_requests: Vec<_> = requests
        .iter()
        .filter(|request| request.path.contains("token=latest"))
        .collect();
    assert_eq!(
        seed_requests.len(),
        1,
        "exactly one seed request: {requests:#?}"
    );
    assert_eq!(
        top_occurrences(&seed_requests[0].path),
        1,
        "the seed request must carry exactly one $top: {}",
        seed_requests[0].path
    );
    assert!(
        seed_requests[0].path.contains("$top=2"),
        "the seed request must carry the configured page size: {}",
        seed_requests[0].path
    );

    let poll_requests: Vec<_> = requests
        .iter()
        .filter(|request| request.path.contains("/delta?cursor="))
        .collect();
    assert!(
        poll_requests.len() >= 5,
        "expected multiple pages across two rounds, got {}: {requests:#?}",
        poll_requests.len()
    );
    for request in &poll_requests {
        assert_eq!(
            top_occurrences(&request.path),
            1,
            "no duplicate or appended $top on a followed opaque link: {}",
            request.path
        );
        assert!(
            request.path.contains("$top=2"),
            "the page size must still flow through unchanged: {}",
            request.path
        );
    }

    cancellation.cancel();
}

fn top_occurrences(path: &str) -> usize {
    path.matches("$top=").count()
}

#[tokio::test]
async fn watch_treats_410_as_reset_required_and_recovers_with_a_fresh_seed() {
    let fixture = GraphFixture::start().await;
    let provider = provider(&fixture).await;
    let cancellation = cancel();
    let stream = provider
        .watch(&root(CONNECTION_A), cancellation.clone())
        .await
        .expect("watch must succeed");

    // Drive the stream from an independent background task: a `.next()`
    // call wrapped in a short test-side timeout would otherwise only
    // advance the underlying poll loop while actively awaited, starving it
    // (and racing it against `fixture.create_file` below) the moment this
    // test does anything else (like inspecting `fixture.requests()`).
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let driver = tokio::spawn(async move {
        let mut stream = stream;
        while let Some(item) = stream.next().await {
            if tx.send(item).is_err() {
                break;
            }
        }
    });

    // Race-free: forces the very first real (cursor-bearing) poll after
    // seeding to receive `410 Gone`, regardless of the opaque cursor's
    // actual text.
    fixture.expire_next_delta_poll().await;

    let reset = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("a reset must eventually be observed")
        .expect("the driver task must not have ended here");
    assert!(matches!(reset, Ok(ProviderChange::ResetRequired)));

    // The watch must keep going afterwards (a fresh reseed), not end. Wait
    // for the reseed's `token=latest` request to actually happen before
    // creating the file, so the reseed's baseline snapshot cannot possibly
    // already include it - otherwise no further change would ever be
    // observed for it. The background driver task above keeps the stream
    // progressing independently while this loop only observes it.
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let seed_requests = fixture
                .requests()
                .await
                .iter()
                .filter(|request| request.path.contains("token=latest"))
                .count();
            if seed_requests >= 2 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("a reseed request must happen after the reset");

    fixture.create_file("", "after-reset.txt", b"x").await;
    let changed = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("a change after recovery must eventually be observed")
        .expect("the driver task must not have ended here");
    assert!(matches!(changed, Ok(ProviderChange::Changed)));

    cancellation.cancel();
    let _ = driver.await;
    cancellation.cancel();
}

#[tokio::test]
async fn watch_stops_polling_once_cancelled() {
    let fixture = GraphFixture::start().await;
    let provider = provider(&fixture).await;
    let cancellation = cancel();
    let mut stream = provider
        .watch(&root(CONNECTION_A), cancellation.clone())
        .await
        .expect("watch must succeed");

    // Prime one poll so the stream is definitely past the seed tick.
    let _ = tokio::time::timeout(Duration::from_millis(60), stream.next()).await;
    cancellation.cancel();

    let ended = tokio::time::timeout(Duration::from_secs(1), stream.next()).await;
    assert!(
        matches!(ended, Ok(None)),
        "a cancelled watch stream must end, not keep polling"
    );

    let count_at_cancel = fixture.requests().await.len();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let count_after_wait = fixture.requests().await.len();
    assert_eq!(
        count_at_cancel, count_after_wait,
        "no further requests must be made after cancellation"
    );
}

/// Task 0110 review: a permanent authorization failure (the resolver never
/// hands back a usable token, e.g. a revoked grant) must surface promptly
/// as an `Err` from the watch stream rather than being silently retried
/// forever with only a backoff sleep and no observable signal.
#[tokio::test]
async fn watch_surfaces_a_permanent_credential_failure_instead_of_retrying_silently_forever() {
    struct AlwaysFailingResolver;

    #[async_trait]
    impl OneDriveConnectionResolver for AlwaysFailingResolver {
        async fn resolve(&self, _connection_id: &str) -> Result<OneDriveAccessToken, VfsError> {
            Err(VfsError::CredentialRequired)
        }
    }

    let fixture = GraphFixture::start().await;
    let provider = OneDriveFileSystemProvider::with_config(
        Arc::new(AlwaysFailingResolver),
        test_config(&fixture),
    );
    let cancellation = cancel();
    let mut stream = provider
        .watch(&root(CONNECTION_A), cancellation.clone())
        .await
        .expect("watch must succeed");

    let first = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .expect(
            "a permanent credential failure must surface promptly, not be silently retried forever",
        )
        .expect("the stream must not end here");
    assert!(
        matches!(first, Err(VfsError::CredentialRequired)),
        "expected a surfaced CredentialRequired, got {first:?}"
    );

    // The resolver fails before any network call, so nothing was ever sent.
    assert!(fixture.requests().await.is_empty());

    cancellation.cancel();
}

/// A permanent failure encountered *mid-poll* (after a working cursor was
/// already established) must also surface as an `Err`, and the stream must
/// stay alive afterwards - a later, real change can still be observed once
/// whatever caused the permanent failure is resolved.
#[tokio::test]
async fn watch_surfaces_a_permanent_permission_error_mid_poll_without_ending_the_stream() {
    let fixture = GraphFixture::start().await;
    let provider = provider(&fixture).await;
    let cancellation = cancel();
    let mut stream = provider
        .watch(&root(CONNECTION_A), cancellation.clone())
        .await
        .expect("watch must succeed");

    // Let the stream seed and settle into normal polling first.
    let _ = tokio::time::timeout(Duration::from_millis(60), stream.next()).await;

    // Simulate a revoked grant/conditional-access rejection on the very
    // next poll.
    fixture.queue_throttle(403, None).await;

    let permanent = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("a permanent permission failure must surface promptly")
        .expect("the stream must not end here");
    assert!(
        matches!(permanent, Err(VfsError::PermissionDenied { .. })),
        "expected a surfaced PermissionDenied, got {permanent:?}"
    );

    // The watch must still be alive: a later, real change is still
    // observable once polling succeeds again.
    fixture
        .create_file("", "after-permission-error.txt", b"x")
        .await;
    let changed = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("a change after recovery must eventually be observed")
        .expect("the stream must not end here");
    assert!(matches!(changed, Ok(ProviderChange::Changed)));

    cancellation.cancel();
}

#[tokio::test]
async fn two_saved_connections_use_distinct_tokens_and_never_cross_contaminate() {
    let fixture = GraphFixture::start().await;
    let resolver = Arc::new(FixedResolver::per_connection(&[
        (CONNECTION_A, "token-for-a"),
        (CONNECTION_B, "token-for-b"),
    ]));
    let provider = provider_with(&fixture, resolver);

    provider
        .create_directory(&root(CONNECTION_A), "FromA", cancel())
        .await
        .unwrap();
    provider
        .create_directory(&root(CONNECTION_B), "FromB", cancel())
        .await
        .unwrap();

    assert!(fixture.exists("FromA").await);
    assert!(fixture.exists("FromB").await);
    assert_ne!(
        provider
            .transfer_capabilities(&root(CONNECTION_A))
            .unwrap()
            .endpoint,
        provider
            .transfer_capabilities(&root(CONNECTION_B))
            .unwrap()
            .endpoint,
    );
}
