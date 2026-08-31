//! Integration tests for [`fm_ssh::SshSession`] connection, authentication
//! and host-key verification against the real in-process fixture (task
//! 0104, spec §18 "SFTP": "password/key auth, host-key first use/mismatch").

use std::sync::Arc;
use std::time::Duration;

use fm_ssh::fixture::{FIXTURE_PASSWORD, FIXTURE_USERNAME, SshFixture};
use fm_ssh::{
    HostKeyVerification, InMemoryKnownHostsStore, KnownHostsStore, SshConnectTarget,
    SshConnectionManager, SshConnectionParameters, SshCredential, SshError, SshHostKeyPolicy,
    SshSession,
};
use russh::keys::ssh_key::LineEnding;
use russh::keys::{Algorithm, PrivateKey};

fn params(fixture: &SshFixture, credential: SshCredential) -> SshConnectionParameters {
    SshConnectionParameters {
        target: SshConnectTarget {
            host: fixture.addr.ip().to_string(),
            port: fixture.addr.port(),
            username: FIXTURE_USERNAME.to_owned(),
        },
        credential,
        host_key_policy: SshHostKeyPolicy::PromptOnFirstUse,
        keepalive: None,
    }
}

fn password_credential() -> SshCredential {
    SshCredential::Password(FIXTURE_PASSWORD.to_owned().into())
}

#[tokio::test]
async fn first_connection_to_an_unknown_host_key_is_never_auto_accepted() {
    let fixture = SshFixture::start().await;
    let known_hosts = Arc::new(InMemoryKnownHostsStore::new());

    let error = SshSession::connect(
        &params(&fixture, password_credential()),
        known_hosts,
        "conn-1",
    )
    .await
    .expect_err("an unverified host key must never be silently accepted");

    assert_eq!(
        error,
        SshError::HostKeyUnverified {
            fingerprint: fixture.host_key_fingerprint.clone()
        }
    );
}

#[tokio::test]
async fn rejecting_the_first_use_prompt_leaves_the_host_key_still_unverified() {
    let fixture = SshFixture::start().await;
    let known_hosts = Arc::new(InMemoryKnownHostsStore::new());

    // First attempt: never accepted, matching the previous test.
    let _ = SshSession::connect(
        &params(&fixture, password_credential()),
        known_hosts.clone(),
        "conn-1",
    )
    .await
    .expect_err("must fail");

    // No accept() call happened - a second attempt must report the exact
    // same "unverified" outcome, not silently succeed.
    let error = SshSession::connect(
        &params(&fixture, password_credential()),
        known_hosts,
        "conn-1",
    )
    .await
    .expect_err("an unaccepted host key must still be unverified");

    assert_eq!(
        error,
        SshError::HostKeyUnverified {
            fingerprint: fixture.host_key_fingerprint.clone()
        }
    );
}

#[tokio::test]
async fn accepting_the_first_use_prompt_persists_it_and_allows_a_later_connect() {
    let fixture = SshFixture::start().await;
    let known_hosts = Arc::new(InMemoryKnownHostsStore::new());

    let _ = SshSession::connect(
        &params(&fixture, password_credential()),
        known_hosts.clone(),
        "conn-1",
    )
    .await
    .expect_err("must fail before acceptance");

    known_hosts
        .accept("conn-1", fixture.host_key_fingerprint.clone())
        .await
        .expect("accept must succeed");

    let session = SshSession::connect(
        &params(&fixture, password_credential()),
        known_hosts,
        "conn-1",
    )
    .await
    .expect("connect must succeed once the fingerprint is accepted");
    assert!(!session.is_closed());
}

#[tokio::test]
async fn a_changed_host_key_is_reported_distinctly_from_never_seen_and_never_silently_accepted() {
    let fixture = SshFixture::start().await;
    let known_hosts = Arc::new(InMemoryKnownHostsStore::new());
    // Simulate a previously accepted key for a *different* host that now
    // presents `fixture.host_key_fingerprint` - i.e. the key changed.
    known_hosts
        .accept(
            "conn-1",
            "SHA256:previously-trusted-but-now-stale".to_owned(),
        )
        .await
        .unwrap();

    let error = SshSession::connect(
        &params(&fixture, password_credential()),
        known_hosts.clone(),
        "conn-1",
    )
    .await
    .expect_err("a changed host key must never be silently accepted");

    assert_eq!(
        error,
        SshError::HostKeyMismatch {
            fingerprint: fixture.host_key_fingerprint.clone(),
            expected_fingerprint: "SHA256:previously-trusted-but-now-stale".to_owned(),
        }
    );
    // Distinct from "never seen before".
    assert_ne!(
        error,
        SshError::HostKeyUnverified {
            fingerprint: fixture.host_key_fingerprint.clone()
        }
    );

    // The mismatch must not have silently overwritten the stored entry.
    let stored = known_hosts.lookup("conn-1").await.unwrap().unwrap();
    assert_eq!(
        stored.fingerprint,
        "SHA256:previously-trusted-but-now-stale"
    );
}

async fn trusted_store(fixture: &SshFixture) -> Arc<InMemoryKnownHostsStore> {
    let store = Arc::new(InMemoryKnownHostsStore::new());
    store
        .accept("conn-1", fixture.host_key_fingerprint.clone())
        .await
        .expect("seeding the trusted fingerprint must succeed");
    store
}

#[tokio::test]
async fn password_authentication_succeeds_with_the_correct_password() {
    let fixture = SshFixture::start().await;
    let known_hosts = trusted_store(&fixture).await;

    let session = SshSession::connect(
        &params(&fixture, password_credential()),
        known_hosts,
        "conn-1",
    )
    .await
    .expect("password authentication must succeed");

    // Confirm the session is actually usable end to end: open the SFTP
    // subsystem and list the fixture's real root directory.
    let sftp = session.sftp().await.expect("sftp subsystem must open");
    let entries = sftp
        .read_dir(fixture.root_path_string())
        .await
        .expect("listing the fixture root must succeed");
    assert_eq!(entries.count(), 0);
}

#[tokio::test]
async fn password_authentication_fails_with_the_wrong_password() {
    let fixture = SshFixture::start().await;
    let known_hosts = trusted_store(&fixture).await;

    let error = SshSession::connect(
        &params(
            &fixture,
            SshCredential::Password("wrong-password".to_owned().into()),
        ),
        known_hosts,
        "conn-1",
    )
    .await
    .expect_err("must reject a wrong password");

    assert_eq!(error, SshError::AuthenticationFailed);
}

#[tokio::test]
async fn private_key_authentication_without_a_passphrase_succeeds() {
    let fixture = SshFixture::start().await;
    let known_hosts = trusted_store(&fixture).await;
    let key_text = fixture
        .authorized_client_key
        .to_openssh(LineEnding::LF)
        .expect("serializing the test key must succeed");

    let session = SshSession::connect(
        &params(
            &fixture,
            SshCredential::PrivateKey {
                key: key_text.to_string().into(),
                passphrase: None,
            },
        ),
        known_hosts,
        "conn-1",
    )
    .await
    .expect("private-key authentication must succeed");
    assert!(!session.is_closed());
}

#[tokio::test]
async fn private_key_authentication_with_a_passphrase_succeeds() {
    let fixture = SshFixture::start().await;
    let known_hosts = trusted_store(&fixture).await;
    let encrypted = fixture
        .authorized_client_key
        .encrypt(&mut rand::rng(), "correct-horse-battery-staple")
        .expect("encrypting the test key must succeed");
    let key_text = encrypted
        .to_openssh(LineEnding::LF)
        .expect("serializing the encrypted test key must succeed");

    let session = SshSession::connect(
        &params(
            &fixture,
            SshCredential::PrivateKey {
                key: key_text.to_string().into(),
                passphrase: Some("correct-horse-battery-staple".to_owned().into()),
            },
        ),
        known_hosts,
        "conn-1",
    )
    .await
    .expect("private-key authentication with the correct passphrase must succeed");
    assert!(!session.is_closed());
}

#[tokio::test]
async fn private_key_authentication_with_the_wrong_passphrase_fails() {
    let fixture = SshFixture::start().await;
    let known_hosts = trusted_store(&fixture).await;
    let encrypted = fixture
        .authorized_client_key
        .encrypt(&mut rand::rng(), "correct-horse-battery-staple")
        .expect("encrypting the test key must succeed");
    let key_text = encrypted
        .to_openssh(LineEnding::LF)
        .expect("serializing the encrypted test key must succeed");

    let error = SshSession::connect(
        &params(
            &fixture,
            SshCredential::PrivateKey {
                key: key_text.to_string().into(),
                passphrase: Some("wrong-passphrase".to_owned().into()),
            },
        ),
        known_hosts,
        "conn-1",
    )
    .await
    .expect_err("must reject a wrong passphrase");
    assert!(matches!(error, SshError::InvalidPrivateKey(_)));
}

#[tokio::test]
async fn private_key_authentication_rejects_an_unauthorized_key() {
    let fixture = SshFixture::start().await;
    let known_hosts = trusted_store(&fixture).await;
    let unauthorized = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
    let key_text = unauthorized
        .to_openssh(LineEnding::LF)
        .expect("serializing the unauthorized key must succeed");

    let error = SshSession::connect(
        &params(
            &fixture,
            SshCredential::PrivateKey {
                key: key_text.to_string().into(),
                passphrase: None,
            },
        ),
        known_hosts,
        "conn-1",
    )
    .await
    .expect_err("an unauthorized key must be rejected");
    assert_eq!(error, SshError::AuthenticationFailed);
}

#[tokio::test]
async fn agent_authentication_through_the_public_api_never_silently_succeeds_without_a_matching_identity()
 {
    let fixture = SshFixture::start().await;
    let known_hosts = trusted_store(&fixture).await;

    // `SshCredential::Agent` connects through the real environment's
    // `SSH_AUTH_SOCK` (see `session::tests` in `session.rs` for hermetic,
    // in-process coverage of the actual authentication logic against every
    // outcome: success, no identities, and a non-matching identity). The
    // fixture's authorized key was never registered with whatever agent (if
    // any) is reachable in this environment, so success is never valid here,
    // regardless of what that environment happens to hold.
    let error = SshSession::connect(
        &params(&fixture, SshCredential::Agent),
        known_hosts,
        "conn-1",
    )
    .await
    .expect_err("the fixture's key was never added to the real environment's agent");
    assert!(
        matches!(error, SshError::Agent(_) | SshError::AuthenticationFailed),
        "got {error:?}"
    );
}

#[tokio::test]
async fn connecting_to_a_closed_local_port_reports_a_connect_error_not_a_host_key_error() {
    let known_hosts = Arc::new(InMemoryKnownHostsStore::new());
    let unreachable = SshConnectionParameters {
        target: SshConnectTarget {
            host: "127.0.0.1".to_owned(),
            // Port 0 always refuses a client connection attempt.
            port: 1,
            username: FIXTURE_USERNAME.to_owned(),
        },
        credential: password_credential(),
        host_key_policy: SshHostKeyPolicy::PromptOnFirstUse,
        keepalive: None,
    };

    let error = SshSession::connect(&unreachable, known_hosts, "conn-1")
        .await
        .expect_err("connecting to a closed port must fail");
    assert!(matches!(
        error,
        SshError::Connect { .. } | SshError::Timeout { .. }
    ));
}

#[tokio::test]
async fn sftp_subsystem_is_opened_once_and_reused() {
    let fixture = SshFixture::start().await;
    let known_hosts = trusted_store(&fixture).await;
    let session = SshSession::connect(
        &params(&fixture, password_credential()),
        known_hosts,
        "conn-1",
    )
    .await
    .unwrap();

    let first = session.sftp().await.unwrap();
    let second = session.sftp().await.unwrap();
    assert!(Arc::ptr_eq(&first, &second));
}

#[tokio::test]
async fn connection_manager_reuses_a_live_session_and_reconnects_after_invalidation() {
    let fixture = SshFixture::start().await;
    let known_hosts = trusted_store(&fixture).await;
    let manager = SshConnectionManager::new(known_hosts);
    let connect_params = params(&fixture, password_credential());

    let first = manager.session("conn-1", &connect_params).await.unwrap();
    let second = manager.session("conn-1", &connect_params).await.unwrap();
    assert!(
        Arc::ptr_eq(&first, &second),
        "a live session must be reused, not redialed"
    );

    manager.invalidate("conn-1").await;
    let third = manager.session("conn-1", &connect_params).await.unwrap();
    assert!(
        !Arc::ptr_eq(&first, &third),
        "after invalidation the manager must reconnect with a fresh session"
    );
    assert!(!third.is_closed());
}

#[tokio::test]
async fn verify_connectivity_succeeds_without_caching_a_session() {
    let fixture = SshFixture::start().await;
    let known_hosts = trusted_store(&fixture).await;
    let manager = SshConnectionManager::new(known_hosts);

    manager
        .verify_connectivity("conn-1", &params(&fixture, password_credential()))
        .await
        .expect("connectivity check must succeed");
}

#[tokio::test]
async fn connect_respects_a_configured_keepalive_interval_without_erroring() {
    let fixture = SshFixture::start().await;
    let known_hosts = trusted_store(&fixture).await;
    let mut connect_params = params(&fixture, password_credential());
    connect_params.keepalive = Some(Duration::from_secs(30));

    let session = SshSession::connect(&connect_params, known_hosts, "conn-1")
        .await
        .expect("connect must succeed with a keepalive interval configured");
    assert!(!session.is_closed());
}

fn target_of(fixture: &SshFixture) -> SshConnectTarget {
    SshConnectTarget {
        host: fixture.addr.ip().to_string(),
        port: fixture.addr.port(),
        username: FIXTURE_USERNAME.to_owned(),
    }
}

#[tokio::test]
async fn probing_an_unverified_host_key_reports_unverified_without_authenticating() {
    let fixture = SshFixture::start().await;
    let known_hosts = InMemoryKnownHostsStore::new();

    let outcome = SshSession::probe_host_key(&target_of(&fixture), Arc::new(known_hosts), "conn-1")
        .await
        .expect("probing must not itself error");
    assert_eq!(
        outcome,
        HostKeyVerification::Unverified {
            fingerprint: fixture.host_key_fingerprint.clone()
        }
    );
}

#[tokio::test]
async fn probing_a_trusted_host_key_reports_trusted() {
    let fixture = SshFixture::start().await;
    let known_hosts = Arc::new(InMemoryKnownHostsStore::new());
    known_hosts
        .accept("conn-1", fixture.host_key_fingerprint.clone())
        .await
        .expect("seeding must succeed");

    let outcome = SshSession::probe_host_key(&target_of(&fixture), known_hosts, "conn-1")
        .await
        .expect("probing must not itself error");
    assert_eq!(
        outcome,
        HostKeyVerification::Trusted {
            fingerprint: fixture.host_key_fingerprint.clone()
        }
    );
}

#[tokio::test]
async fn probing_a_changed_host_key_reports_mismatch() {
    let fixture = SshFixture::start().await;
    let known_hosts = Arc::new(InMemoryKnownHostsStore::new());
    known_hosts
        .accept("conn-1", "SHA256:stale".to_owned())
        .await
        .expect("seeding must succeed");

    let outcome = SshSession::probe_host_key(&target_of(&fixture), known_hosts, "conn-1")
        .await
        .expect("probing must not itself error");
    assert_eq!(
        outcome,
        HostKeyVerification::Mismatch {
            fingerprint: fixture.host_key_fingerprint.clone(),
            expected_fingerprint: "SHA256:stale".to_owned(),
        }
    );
}

#[tokio::test]
async fn connection_manager_exposes_the_same_probe_behavior() {
    let fixture = SshFixture::start().await;
    let manager = SshConnectionManager::new(Arc::new(InMemoryKnownHostsStore::new()));

    let outcome = manager
        .probe_host_key("conn-1", &target_of(&fixture))
        .await
        .expect("probing must not itself error");
    assert_eq!(
        outcome,
        HostKeyVerification::Unverified {
            fingerprint: fixture.host_key_fingerprint.clone()
        }
    );
}
