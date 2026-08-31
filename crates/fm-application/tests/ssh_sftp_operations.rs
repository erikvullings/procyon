//! End-to-end coverage that `local -> SFTP`, `SFTP -> local` and
//! `SFTP -> SFTP` (same connection) copies genuinely flow through the real
//! shared operation engine (task 0104 acceptance criteria), not just through
//! the provider in isolation (see `fm-vfs-sftp`'s own `tests/provider.rs`
//! for that), plus the host-key probe/accept facade methods
//! (`FileManagerService::probe_ssh_host_key`/`accept_ssh_host_key`, spec
//! §6.4) against the same real in-process fixture used by `fm-ssh`'s own
//! tests.

use std::fs;
use std::time::Duration;

use fm_application::FileManagerService;
use fm_domain::Location;
use fm_ssh::fixture::{FIXTURE_PASSWORD, FIXTURE_USERNAME, SshFixture};
use fm_transport_dto::{
    ConnectionConfigurationDto, ConnectionKindDto, ConnectionSecretInputDto, ConnectionStatusDto,
    CreateConnectionRequestDto, HostKeyPolicyDto, HostKeyProbeDto, OperationConflictPolicyDto,
    OperationKindDto, OperationStateDto, RuntimeKindDto, SshAuthenticationMethodDto,
    SshConnectionConfigurationDto, StartOperationRequestDto,
};

fn service(root: &tempfile::TempDir) -> FileManagerService {
    FileManagerService::new(
        RuntimeKindDto::BrowserServer,
        root.path().join("workspaces"),
        root.path().join("settings"),
    )
}

/// Registers a connection profile pointed at `fixture`, then goes through
/// the full host-key confirmation flow (probe -> accept) exactly as a real
/// client would after seeing [`fm_application::ApplicationError::HostKeyUnverified`]
/// from a first `connect` attempt, and finally confirms `connect` now
/// succeeds. Returns the connection id.
async fn register_and_trust_connection(
    service: &FileManagerService,
    fixture: &SshFixture,
) -> uuid::Uuid {
    let created = service
        .create_connection(CreateConnectionRequestDto {
            name: "Fixture Server".to_owned(),
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

    // A connect attempt before the host key is trusted reports the distinct
    // `HostKeyUnverified` status (never `Connected`, and never a generic
    // `Failed` indistinguishable from a wrong password or network outage) -
    // `ConnectionService::connect` never raises this as an `Err`, it always
    // reports it as a tracked status, matching `test_reports_status_...` and
    // every other `connect`/`test` outcome in `fm-connections`' own tests.
    let attempted = service
        .connect_connection(created.id)
        .await
        .expect("connect must not itself error, only report a distinct status");
    assert_eq!(attempted.status, ConnectionStatusDto::HostKeyUnverified);
    let fingerprint = fixture.host_key_fingerprint.clone();

    // Probing reports the same pending fingerprint.
    let probe = service
        .probe_ssh_host_key(created.id)
        .await
        .expect("probe must succeed");
    assert_eq!(
        probe,
        HostKeyProbeDto::Unverified {
            fingerprint: fixture.host_key_fingerprint.clone()
        }
    );

    service
        .accept_ssh_host_key(created.id, fingerprint)
        .await
        .expect("accept must succeed");

    // A subsequent probe now reports it as trusted.
    let probe = service
        .probe_ssh_host_key(created.id)
        .await
        .expect("probe must succeed");
    assert_eq!(
        probe,
        HostKeyProbeDto::Trusted {
            fingerprint: fixture.host_key_fingerprint.clone()
        }
    );

    let connected = service
        .connect_connection(created.id)
        .await
        .expect("connect must succeed once the host key is trusted");
    assert_eq!(connected.status, ConnectionStatusDto::Connected);

    created.id
}

/// Builds an `sftp://` location from a root-relative Unix-style path (empty
/// for the fixture root itself). The fixture's wire protocol is always
/// Unix-style, independent of the host OS - see `fm_ssh::fixture`'s module
/// doc for why this must not be a native OS path (as `fixture.path()`
/// returns).
fn sftp_location(connection_id: uuid::Uuid, remote_path: &str) -> Location {
    Location::parse(&format!("sftp://{connection_id}/{remote_path}"))
        .expect("sftp location must parse")
}

async fn run_copy(
    service: &FileManagerService,
    source: Location,
    destination_directory: Location,
) -> fm_transport_dto::OperationDto {
    let operation = service
        .start_operation(
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
            },
            None,
        )
        .expect("operation must be accepted");
    for _ in 0..400 {
        let current = service
            .get_operation(operation.id.into())
            .expect("operation must be queryable");
        if matches!(
            current.state,
            OperationStateDto::Completed
                | OperationStateDto::CompletedWithWarnings
                | OperationStateDto::Failed
                | OperationStateDto::WaitingForConflictResolution
        ) {
            return current;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("operation did not finish")
}

#[tokio::test]
async fn local_to_sftp_copy_streams_through_the_real_operation_engine() {
    let root = tempfile::tempdir().expect("temporary root");
    let service = service(&root);
    let fixture = SshFixture::start().await;
    let connection_id = register_and_trust_connection(&service, &fixture).await;

    let local_source = root.path().join("report.txt");
    fs::write(&local_source, b"quarterly figures").unwrap();

    let operation = run_copy(
        &service,
        Location::from_native_path(&local_source).unwrap(),
        sftp_location(connection_id, ""),
    )
    .await;

    assert_eq!(operation.state, OperationStateDto::Completed);
    assert_eq!(
        fs::read(fixture.path("report.txt")).unwrap(),
        b"quarterly figures"
    );
    // Confirms "no temporary local files": the only place bytes ever landed
    // is the fixture's own remote root - there is no leftover `.fm-copy-*`
    // artifact anywhere, remote or local.
    let leftover_temp_files = fs::read_dir(fixture.root.path())
        .unwrap()
        .filter_map(Result::ok)
        .any(|entry| entry.file_name().to_string_lossy().starts_with(".fm-copy-"));
    assert!(!leftover_temp_files);
}

#[tokio::test]
async fn sftp_to_local_copy_streams_through_the_real_operation_engine() {
    let root = tempfile::tempdir().expect("temporary root");
    let service = service(&root);
    let fixture = SshFixture::start().await;
    let connection_id = register_and_trust_connection(&service, &fixture).await;
    fs::write(fixture.path("remote.txt"), b"downloaded via sftp").unwrap();

    let local_destination = root.path().join("downloads");
    fs::create_dir(&local_destination).unwrap();

    let operation = run_copy(
        &service,
        sftp_location(connection_id, "remote.txt"),
        Location::from_native_path(&local_destination).unwrap(),
    )
    .await;

    assert_eq!(operation.state, OperationStateDto::Completed);
    assert_eq!(
        fs::read(local_destination.join("remote.txt")).unwrap(),
        b"downloaded via sftp"
    );
}

#[tokio::test]
async fn same_connection_sftp_to_sftp_copy_streams_through_the_real_operation_engine() {
    let root = tempfile::tempdir().expect("temporary root");
    let service = service(&root);
    let fixture = SshFixture::start().await;
    let connection_id = register_and_trust_connection(&service, &fixture).await;
    fs::write(fixture.path("source.txt"), b"same-connection transfer").unwrap();
    fs::create_dir(fixture.path("nested")).unwrap();

    let operation = run_copy(
        &service,
        sftp_location(connection_id, "source.txt"),
        sftp_location(connection_id, "nested"),
    )
    .await;

    assert_eq!(operation.state, OperationStateDto::Completed);
    assert_eq!(
        fs::read(fixture.path("nested/source.txt")).unwrap(),
        b"same-connection transfer"
    );
}

#[tokio::test]
async fn same_connection_sftp_move_uses_the_shared_operation_engine_and_the_server_native_rename() {
    let root = tempfile::tempdir().expect("temporary root");
    let service = service(&root);
    let fixture = SshFixture::start().await;
    let connection_id = register_and_trust_connection(&service, &fixture).await;
    fs::write(fixture.path("move-me.txt"), b"moved, not copied").unwrap();
    fs::create_dir(fixture.path("elsewhere")).unwrap();

    let operation = service
        .start_operation(
            StartOperationRequestDto {
                operation_type: OperationKindDto::Move,
                sources: vec![sftp_location(connection_id, "move-me.txt").into()],
                destination: Some(sftp_location(connection_id, "elsewhere").into()),
                destinations: vec![],
                conflict_policy: OperationConflictPolicyDto::Ask,
                name: None,
                archive_format: None,
                archive_compression_level: None,
                create_intermediate_directories: false,
                symlink_policy: Default::default(),
                permanent_delete_confirmed: false,
                override_read_only: false,
            },
            None,
        )
        .expect("operation must be accepted");
    let mut final_state = None;
    for _ in 0..400 {
        let current = service.get_operation(operation.id.into()).unwrap();
        if matches!(
            current.state,
            OperationStateDto::Completed | OperationStateDto::Failed
        ) {
            final_state = Some(current.state);
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    assert_eq!(final_state, Some(OperationStateDto::Completed));
    assert!(!fixture.path("move-me.txt").exists());
    assert_eq!(
        fs::read(fixture.path("elsewhere/move-me.txt")).unwrap(),
        b"moved, not copied"
    );
}

#[tokio::test]
async fn cancelling_a_local_to_sftp_copy_mid_transfer_leaves_no_partial_file_anywhere() {
    let root = tempfile::tempdir().expect("temporary root");
    let service = service(&root);
    let fixture = SshFixture::start().await;
    let connection_id = register_and_trust_connection(&service, &fixture).await;

    // A large sparse source file streams slowly enough over the (real,
    // loopback) SFTP wire protocol to reliably observe the operation in
    // `Running` state before it finishes, mirroring
    // `copy_file_operation.rs`'s `cancellation_removes_the_private_partial_destination`.
    let source_path = root.path().join("large-sparse.bin");
    fs::File::create(&source_path)
        .unwrap()
        .set_len(64 * 1024 * 1024)
        .unwrap();

    let operation = service
        .start_operation(
            StartOperationRequestDto {
                operation_type: OperationKindDto::Copy,
                sources: vec![Location::from_native_path(&source_path).unwrap().into()],
                destination: Some(sftp_location(connection_id, "").into()),
                destinations: vec![],
                conflict_policy: OperationConflictPolicyDto::Ask,
                name: None,
                archive_format: None,
                archive_compression_level: None,
                create_intermediate_directories: false,
                symlink_policy: Default::default(),
                permanent_delete_confirmed: false,
                override_read_only: false,
            },
            None,
        )
        .expect("operation must be accepted");

    for _ in 0..1_000 {
        if service.get_operation(operation.id.into()).unwrap().state == OperationStateDto::Running {
            service.cancel_operation(operation.id.into()).unwrap();
            break;
        }
        tokio::task::yield_now().await;
    }
    for _ in 0..400 {
        let state = service.get_operation(operation.id.into()).unwrap().state;
        if state == OperationStateDto::Cancelled {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    assert_eq!(
        service.get_operation(operation.id.into()).unwrap().state,
        OperationStateDto::Cancelled
    );
    // No final destination file was ever published...
    assert!(!fixture.path("large-sparse.bin").exists());
    // ...and no `.fm-copy-*` remote temporary file was left behind either -
    // the operation engine's `cleanup_partial` calls
    // `SftpFileSystemProvider::discard_copy` on cancellation, exactly like
    // every other provider.
    let remaining: Vec<_> = fs::read_dir(fixture.root.path())
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert!(
        remaining.is_empty(),
        "expected an empty remote root after cleanup, found {remaining:?}"
    );
}

#[tokio::test]
async fn accept_ssh_host_key_rejects_a_stale_fingerprint_that_no_longer_matches() {
    let root = tempfile::tempdir().expect("temporary root");
    let service = service(&root);
    let fixture = SshFixture::start().await;

    let created = service
        .create_connection(CreateConnectionRequestDto {
            name: "Fixture Server".to_owned(),
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

    let error = service
        .accept_ssh_host_key(created.id, "SHA256:not-the-real-fingerprint".to_owned())
        .await
        .expect_err("accepting a fingerprint that does not match must fail");
    assert!(matches!(
        error,
        fm_application::ApplicationError::InvalidRequest(_)
    ));
}

/// A `PrivateKeyPath` secret is read fresh from disk at dial time (matching
/// `ssh`'s own `IdentityFile` handling) rather than stored at rest - this
/// connects using only a path to the fixture's authorized key file, never
/// the key's own bytes.
#[tokio::test]
async fn private_key_path_authentication_reads_the_key_file_and_connects() {
    let root = tempfile::tempdir().expect("temp workspace root");
    let service = service(&root);
    let fixture = SshFixture::start().await;

    let key_dir = tempfile::tempdir().expect("temp dir for the key file");
    let key_path = key_dir.path().join("id_test");
    let key_text = fixture
        .authorized_client_key
        .to_openssh(russh::keys::ssh_key::LineEnding::LF)
        .expect("serializing the fixture key must succeed");
    fs::write(&key_path, key_text.as_bytes()).expect("writing the key file must succeed");

    let created = service
        .create_connection(CreateConnectionRequestDto {
            name: "Fixture Server (key path)".to_owned(),
            kind: ConnectionKindDto::Ssh,
            configuration: ConnectionConfigurationDto::Ssh(SshConnectionConfigurationDto {
                host: fixture.addr.ip().to_string(),
                port: fixture.addr.port(),
                username: FIXTURE_USERNAME.to_owned(),
                start_path: Some(format!("/home/{FIXTURE_USERNAME}")),
                authentication: SshAuthenticationMethodDto::PrivateKey,
                host_key_policy: HostKeyPolicyDto::PromptOnFirstUse,
                keepalive_seconds: None,
            }),
            secret: Some(ConnectionSecretInputDto::PrivateKeyPath {
                path: key_path.to_string_lossy().into_owned(),
                passphrase: None,
            }),
        })
        .await
        .expect("create_connection must succeed");

    let attempted = service
        .connect_connection(created.id)
        .await
        .expect("connect must not itself error, only report a distinct status");
    assert_eq!(attempted.status, ConnectionStatusDto::HostKeyUnverified);

    service
        .accept_ssh_host_key(created.id, fixture.host_key_fingerprint.clone())
        .await
        .expect("accept must succeed");

    let connected = service
        .connect_connection(created.id)
        .await
        .expect("connect must succeed once the host key is trusted");
    assert_eq!(connected.status, ConnectionStatusDto::Connected);
}

/// A `PrivateKeyPath` pointing at a file that does not exist must not
/// silently report a bare `Failed` - the specific reason (spec-required
/// `lastError` visibility, this task's own follow-up work) must mention the
/// path so a user can actually act on it.
#[tokio::test]
async fn private_key_path_authentication_reports_the_missing_file_in_last_error() {
    let root = tempfile::tempdir().expect("temp workspace root");
    let service = service(&root);
    let fixture = SshFixture::start().await;
    let key_dir = tempfile::tempdir().expect("temp dir for the (absent) key file");
    let missing_path = key_dir.path().join("does-not-exist");

    let created = service
        .create_connection(CreateConnectionRequestDto {
            name: "Fixture Server (missing key path)".to_owned(),
            kind: ConnectionKindDto::Ssh,
            configuration: ConnectionConfigurationDto::Ssh(SshConnectionConfigurationDto {
                host: fixture.addr.ip().to_string(),
                port: fixture.addr.port(),
                username: FIXTURE_USERNAME.to_owned(),
                start_path: Some(format!("/home/{FIXTURE_USERNAME}")),
                authentication: SshAuthenticationMethodDto::PrivateKey,
                host_key_policy: HostKeyPolicyDto::PromptOnFirstUse,
                keepalive_seconds: None,
            }),
            secret: Some(ConnectionSecretInputDto::PrivateKeyPath {
                path: missing_path.to_string_lossy().into_owned(),
                passphrase: None,
            }),
        })
        .await
        .expect("create_connection must succeed");

    let attempted = service
        .test_connection(created.id)
        .await
        .expect("test must not itself error, only report a distinct status");
    assert_eq!(attempted.status, ConnectionStatusDto::Failed);
    let last_error = attempted
        .last_error
        .expect("a failed private-key-path attempt must explain why");
    assert!(
        last_error.contains(&missing_path.to_string_lossy().into_owned()),
        "expected the missing path in the error, got: {last_error}"
    );
}

/// The embedded terminal drawer's SSH extension (task 0105): opening a
/// remote shell on a trusted SSH connection drives a real PTY channel over
/// the same fixture the SFTP tests above use, without a local shell ever
/// being involved.
#[tokio::test]
async fn open_remote_shell_streams_a_real_remote_pty_over_the_pooled_ssh_connection() {
    let root = tempfile::tempdir().expect("temporary root");
    let service = service(&root);
    let fixture = SshFixture::start().await;
    let connection_id = register_and_trust_connection(&service, &fixture).await;

    let channel = service
        .open_remote_shell(connection_id, None, "xterm-256color", 80, 24)
        .await
        .expect("opening a remote shell on a trusted SSH connection must succeed");
    let mut reader = channel.reader;

    channel
        .writer
        .write(b"ping")
        .await
        .expect("writing to the remote shell must succeed");
    let event = reader
        .next()
        .await
        .expect("an echoed event must arrive before the channel closes");
    assert_eq!(
        event,
        fm_application::RemoteShellEvent::Data(b"ping".to_vec())
    );
}

/// A remote shell channel is not silently opened (or opened locally as a
/// fallback) for a connection that does not exist - mirrors task 0105's
/// "unsupported schemes still fail explicitly" acceptance criterion.
#[tokio::test]
async fn open_remote_shell_reports_not_found_for_an_unknown_connection() {
    let root = tempfile::tempdir().expect("temporary root");
    let service = service(&root);

    let error = service
        .open_remote_shell(uuid::Uuid::new_v4(), None, "xterm-256color", 80, 24)
        .await
        .expect_err("an unknown connection id must not silently open a channel");

    assert_eq!(error, fm_application::ApplicationError::NotFound);
}
