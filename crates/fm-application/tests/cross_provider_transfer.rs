//! Cross-provider transfer planning end to end (task 0108).
//!
//! `ssh_sftp_operations.rs` already covers `local -> SFTP`, `SFTP -> local`
//! and same-connection `SFTP -> SFTP`, and `copy_file_operation.rs` covers
//! `local -> local`; this file covers every remaining direction pair among
//! local, SFTP and FTP, and the two same-connection-versus-cross-connection
//! distinctions that the planner's endpoint model exists to make:
//!
//! | source | destination | covered here |
//! |--------|-------------|--------------|
//! | local  | FTP         | yes |
//! | FTP    | local       | yes |
//! | FTP    | FTP (same connection)      | yes |
//! | FTP    | FTP (different connections)| yes |
//! | SFTP   | SFTP (different connections)| yes |
//! | SFTP   | FTP         | yes |
//! | FTP    | SFTP        | yes |
//!
//! Every remote side is a real in-process protocol fixture on loopback
//! (`fm_ssh::fixture::SshFixture`, `fm_vfs_ftp::fixture::FtpFixture`); nothing
//! here reaches an external server.

use std::fs;
use std::time::Duration;

use fm_application::FileManagerService;
use fm_domain::Location;
use fm_ssh::fixture::{FIXTURE_PASSWORD, FIXTURE_USERNAME, SshFixture};
use fm_transport_dto::{
    ConnectionConfigurationDto, ConnectionKindDto, ConnectionSecretInputDto,
    CreateConnectionRequestDto, FtpConnectionConfigurationDto, HostKeyPolicyDto,
    OperationConflictPolicyDto, OperationKindDto, OperationStateDto, RuntimeKindDto,
    SshAuthenticationMethodDto, SshConnectionConfigurationDto, StartOperationRequestDto,
};
use fm_vfs_ftp::fixture::{
    FIXTURE_PASSWORD as FTP_PASSWORD, FIXTURE_USERNAME as FTP_USERNAME, FtpFixture,
};

fn service(root: &tempfile::TempDir) -> FileManagerService {
    FileManagerService::new(
        RuntimeKindDto::BrowserServer,
        root.path().join("workspaces"),
        root.path().join("settings"),
    )
}

/// Registers a saved FTP profile pointed at `fixture` and returns its id.
async fn register_ftp(
    service: &FileManagerService,
    name: &str,
    fixture: &FtpFixture,
) -> uuid::Uuid {
    service
        .create_connection(CreateConnectionRequestDto {
            name: name.to_owned(),
            kind: ConnectionKindDto::Ftp,
            configuration: ConnectionConfigurationDto::Ftp(FtpConnectionConfigurationDto {
                host: fixture.addr.ip().to_string(),
                port: fixture.addr.port(),
                username: FTP_USERNAME.to_owned(),
                start_path: Some("/".to_owned()),
            }),
            secret: Some(ConnectionSecretInputDto::Password {
                password: FTP_PASSWORD.to_owned(),
            }),
        })
        .await
        .expect("create_connection must succeed")
        .id
}

/// Registers a saved SSH profile pointed at `fixture` and trusts its host key,
/// which is a precondition for any `sftp://` location to resolve.
async fn register_sftp(
    service: &FileManagerService,
    name: &str,
    fixture: &SshFixture,
) -> uuid::Uuid {
    let created = service
        .create_connection(CreateConnectionRequestDto {
            name: name.to_owned(),
            kind: ConnectionKindDto::Ssh,
            configuration: ConnectionConfigurationDto::Ssh(SshConnectionConfigurationDto {
                host: fixture.addr.ip().to_string(),
                port: fixture.addr.port(),
                username: FIXTURE_USERNAME.to_owned(),
                start_path: Some(format!("/home/{FIXTURE_USERNAME}")),
                authentication: SshAuthenticationMethodDto::Password,
                host_key_policy: HostKeyPolicyDto::PromptOnFirstUse,
                keepalive_seconds: None,
            }),
            secret: Some(ConnectionSecretInputDto::Password {
                password: FIXTURE_PASSWORD.to_owned(),
            }),
        })
        .await
        .expect("create_connection must succeed");
    service
        .accept_ssh_host_key(created.id, fixture.host_key_fingerprint.clone())
        .await
        .expect("accepting the fixture host key must succeed");
    created.id
}

fn ftp_location(connection_id: uuid::Uuid, remote_path: &str) -> Location {
    Location::parse(&format!("ftp://{connection_id}/{remote_path}"))
        .expect("ftp location must parse")
}

fn sftp_location(connection_id: uuid::Uuid, remote_path: &str) -> Location {
    Location::parse(&format!("sftp://{connection_id}/{remote_path}"))
        .expect("sftp location must parse")
}

fn copy_request(source: Location, destination_directory: Location) -> StartOperationRequestDto {
    StartOperationRequestDto {
        operation_type: OperationKindDto::Copy,
        sources: vec![source.into()],
        destination: Some(destination_directory.into()),
        destinations: vec![],
        conflict_policy: OperationConflictPolicyDto::Ask,
        name: None,
        archive_format: None,
        archive_compression_level: None,
        create_intermediate_directories: false,
        symlink_policy: Default::default(),
        permanent_delete_confirmed: false,
        override_read_only: false,
    }
}

async fn await_terminal(
    service: &FileManagerService,
    id: uuid::Uuid,
) -> fm_transport_dto::OperationDto {
    // 30s budget: generous headroom for a slow/contended CI machine rather than
    // a tight bound on expected duration - these fixtures run entirely over
    // loopback, so a real hang always shows up long before this expires.
    for _ in 0..3_000 {
        let current = service
            .get_operation(id.into())
            .expect("operation must be queryable");
        if matches!(
            current.state,
            OperationStateDto::Completed
                | OperationStateDto::CompletedWithWarnings
                | OperationStateDto::Failed
                | OperationStateDto::Cancelled
                | OperationStateDto::WaitingForConflictResolution
        ) {
            return current;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("operation did not finish")
}

async fn run_copy(
    service: &FileManagerService,
    source: Location,
    destination_directory: Location,
) -> fm_transport_dto::OperationDto {
    let operation = service
        .start_operation(copy_request(source, destination_directory), None)
        .expect("operation must be accepted");
    await_terminal(service, operation.id).await
}

/// Every byte the operation engine ever writes locally lives under the
/// service's own root, so a genuinely temporary-file-free remote-to-remote
/// transfer leaves no `.fm-copy-*` artifact anywhere beneath it.
fn local_copy_temporaries(root: &std::path::Path) -> Vec<String> {
    fn walk(directory: &std::path::Path, found: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(directory) else {
            return;
        };
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(".fm-copy-") {
                found.push(name);
            }
            if entry.path().is_dir() {
                walk(&entry.path(), found);
            }
        }
    }
    let mut found = Vec::new();
    walk(root, &mut found);
    found
}

/* -------------------------------------------------------------------------- */
/*  local <-> FTP                                                             */
/* -------------------------------------------------------------------------- */

#[tokio::test]
async fn local_to_ftp_copy_streams_through_the_real_operation_engine() {
    let root = tempfile::tempdir().expect("temporary root");
    let service = service(&root);
    let fixture = FtpFixture::start().await;
    let connection_id = register_ftp(&service, "Fixture FTP", &fixture).await;

    let source = root.path().join("report.txt");
    fs::write(&source, b"quarterly figures").expect("write local source");

    let operation = run_copy(
        &service,
        Location::from_native_path(&source).expect("local location"),
        ftp_location(connection_id, ""),
    )
    .await;

    assert_eq!(operation.state, OperationStateDto::Completed);
    assert_eq!(
        fixture.get("/report.txt").await.as_deref(),
        Some(b"quarterly figures".as_slice())
    );
    assert_eq!(fixture.paths().await, vec!["/report.txt".to_owned()]);
}

#[tokio::test]
async fn ftp_to_local_copy_streams_through_the_real_operation_engine() {
    let root = tempfile::tempdir().expect("temporary root");
    let service = service(&root);
    let fixture = FtpFixture::start().await;
    let connection_id = register_ftp(&service, "Fixture FTP", &fixture).await;
    fixture.put("/remote.txt", b"downloaded via ftp").await;

    let destination = root.path().join("downloads");
    fs::create_dir(&destination).expect("create local destination");

    let operation = run_copy(
        &service,
        ftp_location(connection_id, "remote.txt"),
        Location::from_native_path(&destination).expect("local location"),
    )
    .await;

    assert_eq!(operation.state, OperationStateDto::Completed);
    assert_eq!(
        fs::read(destination.join("remote.txt")).expect("downloaded file"),
        b"downloaded via ftp"
    );
}

/* -------------------------------------------------------------------------- */
/*  Same-connection optimization, tested separately from cross-provider        */
/*  streaming (task 0108 implementation notes)                                */
/* -------------------------------------------------------------------------- */

#[tokio::test]
async fn same_connection_ftp_to_ftp_copy_completes_on_one_server() {
    let root = tempfile::tempdir().expect("temporary root");
    let service = service(&root);
    let fixture = FtpFixture::start().await;
    let connection_id = register_ftp(&service, "Fixture FTP", &fixture).await;
    fixture
        .put("/source.txt", b"same-connection transfer")
        .await;
    fixture.create_directory("/nested").await;

    let operation = run_copy(
        &service,
        ftp_location(connection_id, "source.txt"),
        ftp_location(connection_id, "nested"),
    )
    .await;

    assert_eq!(operation.state, OperationStateDto::Completed);
    assert_eq!(
        fixture.get("/nested/source.txt").await.as_deref(),
        Some(b"same-connection transfer".as_slice())
    );
}

/// Same-connection FTP moves must take the server-native `RNFR`/`RNTO` path:
/// the bytes never leave the server, and the source disappears.
#[tokio::test]
async fn same_connection_ftp_move_uses_the_server_native_rename() {
    let root = tempfile::tempdir().expect("temporary root");
    let service = service(&root);
    let fixture = FtpFixture::start().await;
    let connection_id = register_ftp(&service, "Fixture FTP", &fixture).await;
    fixture.put("/move-me.txt", b"moved, not copied").await;
    fixture.create_directory("/elsewhere").await;

    let mut request = copy_request(
        ftp_location(connection_id, "move-me.txt"),
        ftp_location(connection_id, "elsewhere"),
    );
    request.operation_type = OperationKindDto::Move;
    let operation = service
        .start_operation(request, None)
        .expect("operation must be accepted");
    let operation = await_terminal(&service, operation.id).await;

    assert_eq!(operation.state, OperationStateDto::Completed);
    assert_eq!(
        fixture.paths().await,
        vec!["/elsewhere/move-me.txt".to_owned()]
    );
}

/// Two FTP connections are two different servers even though both locations
/// carry the same `ftp` provider id — the transfer must stream rather than
/// attempt any same-server fast path, and a move must fall back to
/// copy-then-delete rather than a rename that could never work.
#[tokio::test]
async fn two_different_ftp_connections_stream_rather_than_taking_a_same_server_path() {
    let root = tempfile::tempdir().expect("temporary root");
    let service = service(&root);
    let source_fixture = FtpFixture::start().await;
    let destination_fixture = FtpFixture::start().await;
    let source_id = register_ftp(&service, "Source FTP", &source_fixture).await;
    let destination_id = register_ftp(&service, "Destination FTP", &destination_fixture).await;
    source_fixture
        .put("/across.txt", b"across two ftp hosts")
        .await;

    let operation = run_copy(
        &service,
        ftp_location(source_id, "across.txt"),
        ftp_location(destination_id, ""),
    )
    .await;

    assert_eq!(operation.state, OperationStateDto::Completed);
    assert_eq!(
        destination_fixture.get("/across.txt").await.as_deref(),
        Some(b"across two ftp hosts".as_slice())
    );
    // The source server is untouched: nothing was renamed away from it, and
    // no temporary was left on it either.
    assert_eq!(source_fixture.paths().await, vec!["/across.txt".to_owned()]);
    assert!(local_copy_temporaries(root.path()).is_empty());
}

#[tokio::test]
async fn cross_provider_move_undo_copies_back_then_removes_the_unchanged_output() {
    let root = tempfile::tempdir().expect("temporary root");
    let service = service(&root);
    let source_fixture = FtpFixture::start().await;
    let destination_fixture = FtpFixture::start().await;
    let source_id = register_ftp(&service, "Source FTP", &source_fixture).await;
    let destination_id = register_ftp(&service, "Destination FTP", &destination_fixture).await;
    source_fixture
        .put("/undo-move.txt", b"cross-provider undo")
        .await;
    let mut request = copy_request(
        ftp_location(source_id, "undo-move.txt"),
        ftp_location(destination_id, ""),
    );
    request.operation_type = OperationKindDto::Move;

    let operation = service
        .start_operation(request, None)
        .expect("move must be accepted");
    let completed = await_terminal(&service, operation.id).await;
    assert_eq!(completed.state, OperationStateDto::Completed);
    assert!(completed.undo.available);
    assert!(source_fixture.get("/undo-move.txt").await.is_none());

    let undo = service
        .undo_operation(operation.id.into())
        .expect("undo must be accepted");
    let undone = await_terminal(&service, undo.id).await;

    assert_eq!(undone.state, OperationStateDto::Completed);
    assert_eq!(
        source_fixture.get("/undo-move.txt").await.as_deref(),
        Some(b"cross-provider undo".as_slice())
    );
    assert!(destination_fixture.get("/undo-move.txt").await.is_none());
}

#[tokio::test]
async fn two_different_sftp_connections_stream_rather_than_taking_a_same_server_path() {
    let root = tempfile::tempdir().expect("temporary root");
    let service = service(&root);
    let source_fixture = SshFixture::start().await;
    let destination_fixture = SshFixture::start().await;
    let source_id = register_sftp(&service, "Source SFTP", &source_fixture).await;
    let destination_id = register_sftp(&service, "Destination SFTP", &destination_fixture).await;
    fs::write(source_fixture.path("across.txt"), b"across two ssh hosts")
        .expect("seed the source fixture");

    let operation = run_copy(
        &service,
        sftp_location(source_id, "across.txt"),
        sftp_location(destination_id, ""),
    )
    .await;

    assert_eq!(operation.state, OperationStateDto::Completed);
    assert_eq!(
        fs::read(destination_fixture.path("across.txt")).expect("transferred file"),
        b"across two ssh hosts"
    );
    // The source is still there — this was a copy, not a same-server rename.
    assert!(source_fixture.path("across.txt").exists());
    assert!(local_copy_temporaries(root.path()).is_empty());
}

/* -------------------------------------------------------------------------- */
/*  Remote to remote, across two different protocols                          */
/* -------------------------------------------------------------------------- */

#[tokio::test]
async fn sftp_to_ftp_copy_requires_no_temporary_local_file() {
    let root = tempfile::tempdir().expect("temporary root");
    let service = service(&root);
    let ssh = SshFixture::start().await;
    let ftp = FtpFixture::start().await;
    let sftp_id = register_sftp(&service, "Fixture SFTP", &ssh).await;
    let ftp_id = register_ftp(&service, "Fixture FTP", &ftp).await;
    fs::write(ssh.path("handover.txt"), b"sftp to ftp, streamed").expect("seed the ssh fixture");

    let operation = run_copy(
        &service,
        sftp_location(sftp_id, "handover.txt"),
        ftp_location(ftp_id, ""),
    )
    .await;

    assert_eq!(operation.state, OperationStateDto::Completed);
    assert_eq!(
        ftp.get("/handover.txt").await.as_deref(),
        Some(b"sftp to ftp, streamed".as_slice())
    );
    // The bytes went straight from one server to the other: no `.fm-copy-*`
    // staging file was created on local disk, and the temporary the FTP side
    // did use has already been published into its final name.
    assert!(local_copy_temporaries(root.path()).is_empty());
    assert_eq!(ftp.paths().await, vec!["/handover.txt".to_owned()]);
}

#[tokio::test]
async fn ftp_to_sftp_copy_requires_no_temporary_local_file() {
    let root = tempfile::tempdir().expect("temporary root");
    let service = service(&root);
    let ssh = SshFixture::start().await;
    let ftp = FtpFixture::start().await;
    let sftp_id = register_sftp(&service, "Fixture SFTP", &ssh).await;
    let ftp_id = register_ftp(&service, "Fixture FTP", &ftp).await;
    ftp.put("/handover.txt", b"ftp to sftp, streamed").await;

    let operation = run_copy(
        &service,
        ftp_location(ftp_id, "handover.txt"),
        sftp_location(sftp_id, ""),
    )
    .await;

    assert_eq!(operation.state, OperationStateDto::Completed);
    assert_eq!(
        fs::read(ssh.path("handover.txt")).expect("transferred file"),
        b"ftp to sftp, streamed"
    );
    assert!(local_copy_temporaries(root.path()).is_empty());
    // No leftover remote temporary on the SFTP side either.
    let remaining: Vec<_> = fs::read_dir(ssh.root.path())
        .expect("read the remote root")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(remaining, vec!["handover.txt".to_owned()]);
}

/* -------------------------------------------------------------------------- */
/*  Provider-neutral progress and conflict handling                           */
/* -------------------------------------------------------------------------- */

/// Progress is reported in plain item/byte counts regardless of which strategy
/// the planner picked, so a cross-provider `SFTP -> FTP` transfer produces
/// exactly the same shape of progress as a local copy: no provider-specific
/// units and no missing totals.
#[tokio::test]
async fn progress_is_reported_provider_neutrally_for_a_cross_provider_transfer() {
    let root = tempfile::tempdir().expect("temporary root");
    let service = service(&root);
    let ssh = SshFixture::start().await;
    let ftp = FtpFixture::start().await;
    let sftp_id = register_sftp(&service, "Fixture SFTP", &ssh).await;
    let ftp_id = register_ftp(&service, "Fixture FTP", &ftp).await;
    let body = vec![7_u8; 4096];
    fs::write(ssh.path("measured.bin"), &body).expect("seed the ssh fixture");

    let remote = run_copy(
        &service,
        sftp_location(sftp_id, "measured.bin"),
        ftp_location(ftp_id, ""),
    )
    .await;

    // The same copy performed entirely locally, as the neutrality baseline.
    let local_source = root.path().join("measured.bin");
    let local_destination = root.path().join("out");
    fs::write(&local_source, &body).expect("seed the local source");
    fs::create_dir(&local_destination).expect("create the local destination");
    let local = run_copy(
        &service,
        Location::from_native_path(&local_source).expect("local location"),
        Location::from_native_path(&local_destination).expect("local location"),
    )
    .await;

    assert_eq!(remote.state, OperationStateDto::Completed);
    assert_eq!(local.state, OperationStateDto::Completed);
    assert_eq!(remote.progress.total_items, Some(1));
    assert_eq!(remote.progress.completed_items, 1);
    assert_eq!(remote.progress.total_bytes, Some(body.len() as u64));
    assert_eq!(remote.progress.completed_bytes, body.len() as u64);
    // Identical progress despite completely different transfer strategies
    // (direct streaming across two protocols versus a local server-side clone).
    assert_eq!(remote.progress.total_items, local.progress.total_items);
    assert_eq!(remote.progress.total_bytes, local.progress.total_bytes);
    assert_eq!(
        remote.progress.completed_bytes,
        local.progress.completed_bytes
    );
}

/// A conflicting destination behaves the same across providers as it does
/// locally: the operation stops for a decision instead of overwriting, and it
/// leaves neither a clobbered destination nor a stray temporary behind.
#[tokio::test]
async fn a_cross_provider_conflict_waits_for_a_decision_without_touching_the_destination() {
    let root = tempfile::tempdir().expect("temporary root");
    let service = service(&root);
    let ssh = SshFixture::start().await;
    let ftp = FtpFixture::start().await;
    let sftp_id = register_sftp(&service, "Fixture SFTP", &ssh).await;
    let ftp_id = register_ftp(&service, "Fixture FTP", &ftp).await;
    fs::write(ssh.path("shared.txt"), b"the incoming version").expect("seed the ssh fixture");
    ftp.put("/shared.txt", b"the existing version").await;

    let operation = run_copy(
        &service,
        sftp_location(sftp_id, "shared.txt"),
        ftp_location(ftp_id, ""),
    )
    .await;

    assert_eq!(
        operation.state,
        OperationStateDto::WaitingForConflictResolution
    );
    // The existing destination is untouched...
    assert_eq!(
        ftp.get("/shared.txt").await.as_deref(),
        Some(b"the existing version".as_slice())
    );
    // ...and no partially written temporary was left on the destination server.
    assert_eq!(ftp.paths().await, vec!["/shared.txt".to_owned()]);
    assert!(local_copy_temporaries(root.path()).is_empty());
}

/* -------------------------------------------------------------------------- */
/*  Cancellation and partial-destination cleanup                              */
/* -------------------------------------------------------------------------- */

/// Cancelling a remote-to-remote transfer must reach both providers and leave
/// no partially written destination — neither the final name nor the
/// destination-owned `.fm-copy-*` temporary.
#[tokio::test]
async fn cancelling_an_sftp_to_ftp_copy_leaves_no_partial_destination() {
    let root = tempfile::tempdir().expect("temporary root");
    let service = service(&root);
    // This test races another task (the poll loop below) against a transfer
    // that streams over loopback with no real disk I/O, so a large enough
    // file was not by itself a reliable way to keep the transfer "still
    // running" when cancellation lands - under CI scheduling contention the
    // whole thing can complete within a single scheduling turn regardless of
    // size. A small per-read delay on the SFTP source forces a real,
    // timer-driven yield on every chunk instead, giving the runtime a
    // guaranteed opportunity to run the poll loop between chunks.
    let ssh = SshFixture::start_with_read_delay(Duration::from_millis(1)).await;
    let ftp = FtpFixture::start().await;
    let sftp_id = register_sftp(&service, "Fixture SFTP", &ssh).await;
    let ftp_id = register_ftp(&service, "Fixture FTP", &ftp).await;
    // Large enough (combined with the read delay above) that the transfer is
    // reliably still running when the cancellation lands, mirroring
    // `ssh_sftp_operations.rs`.
    fs::File::create(ssh.path("large.bin"))
        .expect("create the remote source")
        .set_len(64 * 1024 * 1024)
        .expect("size the remote source");

    let operation = service
        .start_operation(
            copy_request(
                sftp_location(sftp_id, "large.bin"),
                ftp_location(ftp_id, ""),
            ),
            None,
        )
        .expect("operation must be accepted");
    // Poll with a real sleep (not just `yield_now`) between checks: on a
    // slow/contended CI runner, connection setup and auth can take real wall
    // time, and a purely cooperative yield loop can burn through its budget
    // before the operation ever reaches `Running`, silently letting it run
    // to completion instead of cancelling it. Same 30s budget as
    // `await_terminal`, for the same reason.
    let mut cancelled = false;
    for _ in 0..3_000 {
        if service
            .get_operation(operation.id.into())
            .expect("operation must be queryable")
            .state
            == OperationStateDto::Running
        {
            service
                .cancel_operation(operation.id.into())
                .expect("cancellation must be accepted");
            cancelled = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(cancelled, "operation never reached Running to cancel");
    let final_state = await_terminal(&service, operation.id).await.state;

    assert_eq!(final_state, OperationStateDto::Cancelled);
    assert!(ftp.get("/large.bin").await.is_none());
    assert!(
        ftp.paths().await.is_empty(),
        "expected an empty FTP root after cleanup, found {:?}",
        ftp.paths().await
    );
    assert!(local_copy_temporaries(root.path()).is_empty());
}
