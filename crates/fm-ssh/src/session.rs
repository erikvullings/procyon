//! One authenticated SSH session and lazily-opened SFTP subsystem (task
//! 0104).

use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use russh::client::{self, AuthResult};
#[cfg(unix)]
use russh::keys::agent::AgentIdentity;
#[cfg(unix)]
use russh::keys::agent::client::{AgentClient, AgentStream};
use russh::keys::{PrivateKeyWithHashAlg, PublicKey};
use russh_sftp::client::SftpSession;
use tokio::sync::Mutex as AsyncMutex;

use crate::error::SshError;
use crate::fingerprint::fingerprint_of;
use crate::known_hosts::{HostKeyVerification, KnownHostsStore, verify_host_key};
use crate::shell::{RemoteShellChannel, shell_quote};
use crate::types::{SshConnectTarget, SshConnectionParameters, SshCredential};

/// How long a connection attempt (transport + host-key exchange) may take
/// before it is treated as failed (spec §6.8 "timeouts").
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// An authenticated SSH session, with a lazily-opened, cached SFTP
/// subsystem.
pub struct SshSession {
    handle: client::Handle<ClientHandler>,
    sftp: AsyncMutex<Option<Arc<SftpSession>>>,
}

impl std::fmt::Debug for SshSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SshSession")
            .field("is_closed", &self.is_closed())
            .finish_non_exhaustive()
    }
}

impl SshSession {
    /// Establishes a new authenticated SSH session, verifying the host key
    /// through `known_hosts` under `known_hosts_key` (spec §6.4).
    ///
    /// Never auto-accepts an unverified or changed host key: both are
    /// reported as a distinct [`SshError`] variant before any credential is
    /// even sent, and no entry is written to `known_hosts` by this call -
    /// only [`crate::KnownHostsStore::accept`] does that, and only a caller
    /// explicitly invokes it.
    pub async fn connect(
        params: &SshConnectionParameters,
        known_hosts: Arc<dyn KnownHostsStore>,
        known_hosts_key: &str,
    ) -> Result<Self, SshError> {
        let mut handle = establish_transport(
            &params.target,
            params.keepalive,
            known_hosts,
            known_hosts_key,
        )
        .await?
        .handle;

        authenticate(&mut handle, &params.target.username, &params.credential).await?;

        Ok(Self {
            handle,
            sftp: AsyncMutex::new(None),
        })
    }

    /// Connects and verifies the host key only, without authenticating, then
    /// immediately disconnects (spec §6.4's explicit host-key confirmation
    /// flow, used before a caller necessarily has a working credential).
    ///
    /// Unlike [`Self::connect`], an unverified or changed host key is not an
    /// error here - it *is* the answer to "what is this host presenting
    /// right now", returned as [`HostKeyVerification::Unverified`]/
    /// [`HostKeyVerification::Mismatch`]. `Err` is reserved for a genuine
    /// transport failure (unreachable host, timeout).
    pub async fn probe_host_key(
        target: &SshConnectTarget,
        known_hosts: Arc<dyn KnownHostsStore>,
        known_hosts_key: &str,
    ) -> Result<HostKeyVerification, SshError> {
        match establish_transport(target, None, known_hosts, known_hosts_key).await {
            Ok(established) => {
                let _ = established
                    .handle
                    .disconnect(
                        russh::Disconnect::ByApplication,
                        "host-key probe complete",
                        "en",
                    )
                    .await;
                Ok(established.verification)
            }
            Err(SshError::HostKeyUnverified { fingerprint }) => {
                Ok(HostKeyVerification::Unverified { fingerprint })
            }
            Err(SshError::HostKeyMismatch {
                fingerprint,
                expected_fingerprint,
            }) => Ok(HostKeyVerification::Mismatch {
                fingerprint,
                expected_fingerprint,
            }),
            Err(other) => Err(other),
        }
    }

    /// Whether the underlying transport has been closed (locally or by the
    /// peer). A dead session should be discarded and reconnected rather than
    /// reused.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.handle.is_closed()
    }

    /// Returns the SFTP subsystem for this session, opening it on first use
    /// and reusing it for subsequent calls.
    pub async fn sftp(&self) -> Result<Arc<SftpSession>, SshError> {
        let mut guard = self.sftp.lock().await;
        if let Some(existing) = guard.as_ref() {
            return Ok(existing.clone());
        }
        let channel = self
            .handle
            .channel_open_session()
            .await
            .map_err(|error| SshError::Session(error.to_string()))?;
        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(|error| SshError::Session(error.to_string()))?;
        let sftp = SftpSession::new(channel.into_stream())
            .await
            .map_err(|error| SshError::Sftp(error.to_string()))?;
        let sftp = Arc::new(sftp);
        *guard = Some(sftp.clone());
        Ok(sftp)
    }

    /// Opens a new interactive remote shell channel (task 0105) with a PTY
    /// of the given `term`/`cols`/`rows`, optionally starting inside
    /// `remote_cwd`.
    ///
    /// A fresh channel every call - unlike [`Self::sftp`], a shell is not
    /// cached/reused across calls; the caller (the embedded terminal
    /// registry) is what tracks one persistent channel per location.
    ///
    /// SSH's `exec` request takes a single opaque command string rather than
    /// a `cwd` field, so `remote_cwd` is implemented as `cd <dir> && exec
    /// $SHELL -l`, with `dir` quoted by [`shell_quote`] so an awkward or
    /// attacker-influenced path cannot break out of the command. Without a
    /// `remote_cwd`, a plain `RequestShell` starts the account's configured
    /// login shell in its default directory.
    pub async fn open_shell(
        &self,
        term: &str,
        cols: u32,
        rows: u32,
        remote_cwd: Option<&str>,
    ) -> Result<RemoteShellChannel, SshError> {
        let channel = self
            .handle
            .channel_open_session()
            .await
            .map_err(|error| SshError::Session(error.to_string()))?;
        channel
            .request_pty(true, term, cols, rows, 0, 0, &[])
            .await
            .map_err(|error| SshError::Session(error.to_string()))?;
        match remote_cwd {
            Some(cwd) => {
                let command = format!(
                    "cd {} 2>/dev/null; exec \"${{SHELL:-/bin/sh}}\" -l",
                    shell_quote(cwd)
                );
                channel
                    .exec(true, command.into_bytes())
                    .await
                    .map_err(|error| SshError::Session(error.to_string()))?;
            }
            None => {
                channel
                    .request_shell(true)
                    .await
                    .map_err(|error| SshError::Session(error.to_string()))?;
            }
        }
        let (read, write) = channel.split();
        Ok(RemoteShellChannel::new(read, write))
    }
}

struct EstablishedTransport {
    handle: client::Handle<ClientHandler>,
    /// Always [`HostKeyVerification::Trusted`] - `establish_transport` never
    /// returns `Ok` otherwise.
    verification: HostKeyVerification,
}

/// Connects and verifies the host key, stopping short of authentication.
///
/// Shared by [`SshSession::connect`] (which authenticates immediately after)
/// and [`SshSession::probe_host_key`] (which disconnects immediately after).
/// Only ever returns `Ok` when the key was trusted; an unverified or changed
/// key surfaces as the matching [`SshError`] variant, never a bare
/// `HostKeyVerification` the caller could accidentally treat as success.
async fn establish_transport(
    target: &SshConnectTarget,
    keepalive: Option<Duration>,
    known_hosts: Arc<dyn KnownHostsStore>,
    known_hosts_key: &str,
) -> Result<EstablishedTransport, SshError> {
    let outcome: Arc<StdMutex<Option<HostKeyVerification>>> = Arc::new(StdMutex::new(None));
    let handler = ClientHandler {
        known_hosts,
        known_hosts_key: known_hosts_key.to_owned(),
        outcome: outcome.clone(),
    };
    let config = Arc::new(client::Config {
        keepalive_interval: keepalive,
        ..Default::default()
    });
    let address = (target.host.as_str(), target.port);

    let connect_result =
        tokio::time::timeout(CONNECT_TIMEOUT, client::connect(config, address, handler)).await;
    match connect_result {
        Err(_) => Err(SshError::Timeout {
            host: target.host.clone(),
            port: target.port,
        }),
        Ok(Err(_)) => Err(host_key_or_connect_error(
            &outcome,
            &target.host,
            target.port,
        )),
        Ok(Ok(handle)) => {
            let verification = outcome
                .lock()
                .expect("host-key outcome lock poisoned")
                .take()
                .unwrap_or(HostKeyVerification::Unverified {
                    fingerprint: String::new(),
                });
            Ok(EstablishedTransport {
                handle,
                verification,
            })
        }
    }
}

fn host_key_or_connect_error(
    outcome: &StdMutex<Option<HostKeyVerification>>,
    host: &str,
    port: u16,
) -> SshError {
    match outcome
        .lock()
        .expect("host-key outcome lock poisoned")
        .take()
    {
        Some(HostKeyVerification::Unverified { fingerprint }) => {
            SshError::HostKeyUnverified { fingerprint }
        }
        Some(HostKeyVerification::Mismatch {
            fingerprint,
            expected_fingerprint,
        }) => SshError::HostKeyMismatch {
            fingerprint,
            expected_fingerprint,
        },
        _ => SshError::Connect {
            host: host.to_owned(),
            port,
            message: "connection rejected during transport or key exchange".to_owned(),
        },
    }
}

async fn authenticate(
    handle: &mut client::Handle<ClientHandler>,
    username: &str,
    credential: &SshCredential,
) -> Result<(), SshError> {
    let result = match credential {
        SshCredential::None => handle
            .authenticate_none(username)
            .await
            .map_err(|error| SshError::Session(error.to_string()))?,
        SshCredential::Password(password) => handle
            .authenticate_password(username, password.as_str())
            .await
            .map_err(|error| SshError::Session(error.to_string()))?,
        SshCredential::PrivateKey { key, passphrase } => {
            let private_key = russh::keys::decode_secret_key(
                key.as_str(),
                passphrase.as_ref().map(|value| value.as_str()),
            )
            .map_err(|error| SshError::InvalidPrivateKey(error.to_string()))?;
            // OpenSSH uses rsa-sha2-* signatures for RSA keys by default on
            // modern servers. Keep parity by preferring SHA-2 here instead of
            // leaving hash selection implicit.
            let key_with_hash = PrivateKeyWithHashAlg::new(
                Arc::new(private_key),
                Some(russh::keys::HashAlg::Sha512),
            );
            handle
                .authenticate_publickey(username, key_with_hash)
                .await
                .map_err(|error| SshError::Session(error.to_string()))?
        }
        #[cfg(unix)]
        SshCredential::Agent => {
            let auth_sock = std::env::var("SSH_AUTH_SOCK").unwrap_or_else(|_| "<unset>".to_owned());
            tracing::info!(ssh_auth_sock = %auth_sock, "connecting to the local SSH agent");
            let mut agent = AgentClient::connect_env().await.map_err(|error| {
                tracing::warn!(ssh_auth_sock = %auth_sock, %error, "could not reach the local SSH agent");
                SshError::Agent(format!(
                    "could not reach the local SSH agent (is SSH_AUTH_SOCK set and ssh-add run?): {error}"
                ))
            })?;
            return authenticate_with_agent(handle, username, &mut agent).await;
        }
        #[cfg(not(unix))]
        SshCredential::Agent => {
            return Err(SshError::Agent(
                "SSH agent authentication is not yet supported on this platform".into(),
            ));
        }
    };

    if matches!(result, AuthResult::Success) {
        Ok(())
    } else {
        Err(SshError::AuthenticationFailed)
    }
}

/// Tries every plain public-key identity the connected agent offers, in the
/// order the agent returns them, until one authenticates - matching how
/// OpenSSH's own client tries agent identities. Agent-held certificates are
/// skipped (a documented gap, not silently ignored: OpenSSH certificate
/// identities are rarer than plain keys and need a distinct
/// `authenticate_certificate_with` call this crate does not yet make).
///
/// Generic over the transport so tests can exercise this against an
/// in-process agent server via [`AgentClient::connect_uds`] rather than the
/// real environment's `SSH_AUTH_SOCK`.
#[cfg(unix)]
async fn authenticate_with_agent<S: AgentStream + Send + Unpin>(
    handle: &mut client::Handle<ClientHandler>,
    username: &str,
    agent: &mut AgentClient<S>,
) -> Result<(), SshError> {
    let identities = agent
        .request_identities()
        .await
        .map_err(|error| SshError::Agent(format!("could not list agent identities: {error}")))?;
    tracing::info!(
        identity_count = identities.len(),
        identities = ?identities
            .iter()
            .map(|identity| match identity {
                AgentIdentity::PublicKey { key, comment } => {
                    format!("{} {} ({comment})", key.algorithm(), fingerprint_of(key))
                }
                AgentIdentity::Certificate { certificate, comment } => {
                    format!(
                        "certificate {} ({comment})",
                        certificate.public_key().fingerprint(russh::keys::HashAlg::Sha256)
                    )
                }
            })
            .collect::<Vec<_>>(),
        "SSH agent reported identities",
    );
    let public_keys: Vec<PublicKey> = identities
        .into_iter()
        .filter_map(|identity| match identity {
            AgentIdentity::PublicKey { key, .. } => Some(key),
            AgentIdentity::Certificate { .. } => None,
        })
        .collect();
    if public_keys.is_empty() {
        return Err(SshError::Agent(
            "the SSH agent has no usable public-key identities (try `ssh-add -l`)".to_owned(),
        ));
    }

    for key in &public_keys {
        let fingerprint = fingerprint_of(key);
        let result = handle
            .authenticate_publickey_with(username, key.clone(), None, agent)
            .await
            .map_err(|error| SshError::Session(error.to_string()))?;
        tracing::info!(%fingerprint, success = matches!(result, AuthResult::Success), "tried agent identity");
        if matches!(result, AuthResult::Success) {
            return Ok(());
        }
    }
    Err(SshError::AuthenticationFailed)
}

struct ClientHandler {
    known_hosts: Arc<dyn KnownHostsStore>,
    known_hosts_key: String,
    outcome: Arc<StdMutex<Option<HostKeyVerification>>>,
}

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKey,
    ) -> Result<bool, Self::Error> {
        let fingerprint = fingerprint_of(server_public_key);
        // A known-hosts store failure must never be treated as trust: fail
        // closed as "unverified" rather than silently accepting.
        let verification = verify_host_key(
            self.known_hosts.as_ref(),
            &self.known_hosts_key,
            &fingerprint,
        )
        .await
        .unwrap_or(HostKeyVerification::Unverified {
            fingerprint: fingerprint.clone(),
        });
        let trusted = matches!(verification, HostKeyVerification::Trusted { .. });
        *self.outcome.lock().expect("host-key outcome lock poisoned") = Some(verification);
        Ok(trusted)
    }
}

#[cfg(test)]
mod tests {
    //! Exercises real `ssh-agent` wire-protocol authentication (spec §6.3)
    //! against an in-process agent server, matching this crate's fixture
    //! philosophy (`fixture.rs`'s doc comment): no external `ssh-agent`
    //! process, works identically in a sandboxed CI environment.

    use std::sync::Arc;

    #[cfg(unix)]
    use russh::keys::agent::client::AgentClient;
    #[cfg(unix)]
    use russh::keys::agent::server;
    #[cfg(unix)]
    use tokio::net::{UnixListener, UnixStream};
    use zeroize::Zeroizing;

    use super::*;
    use crate::fixture::{FIXTURE_PASSWORD, FIXTURE_USERNAME, SshFixture};
    use crate::known_hosts::InMemoryKnownHostsStore;
    use crate::shell::RemoteShellEvent;
    use crate::types::SshHostKeyPolicy;

    /// Starts an in-process agent server on an ephemeral Unix socket,
    /// pre-loaded with `identity`, and returns a client already connected to
    /// it. The server task is leaked for the test's lifetime (process exit
    /// cleans up the socket file along with the temp directory).
    #[cfg(unix)]
    async fn agent_with_identity(identity: &russh::keys::PrivateKey) -> AgentClient<UnixStream> {
        let socket_dir = tempfile::tempdir().expect("creating a temp dir for the agent socket");
        let socket_path = socket_dir.path().join("agent.sock");
        let listener =
            UnixListener::bind(&socket_path).expect("binding the in-process agent socket");

        tokio::spawn(async move {
            // Keep the temp dir alive for as long as the server runs.
            let _socket_dir = socket_dir;
            let stream = Box::pin(futures::stream::unfold(listener, |listener| async move {
                let accepted = listener.accept().await.map(|(stream, _)| stream);
                Some((accepted, listener))
            }));
            let _ = server::serve(stream, ()).await;
        });

        // Give the spawned server a moment to start listening before the
        // first connect attempt.
        tokio::task::yield_now().await;

        let mut client = AgentClient::connect_uds(&socket_path)
            .await
            .expect("connecting to the in-process agent must succeed");
        client
            .add_identity(identity, &[])
            .await
            .expect("adding the test identity to the in-process agent must succeed");
        client
    }

    #[cfg(unix)]
    async fn trusted_handle(fixture: &SshFixture) -> client::Handle<ClientHandler> {
        let known_hosts = Arc::new(InMemoryKnownHostsStore::new());
        known_hosts
            .accept("conn-1", fixture.host_key_fingerprint.clone())
            .await
            .expect("seeding the trusted fingerprint must succeed");
        let target = crate::types::SshConnectTarget {
            host: fixture.addr.ip().to_string(),
            port: fixture.addr.port(),
            username: FIXTURE_USERNAME.to_owned(),
        };
        establish_transport(&target, None, known_hosts, "conn-1")
            .await
            .expect("establishing the transport against the trusted fixture must succeed")
            .handle
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn agent_authentication_succeeds_with_the_fixture_s_authorized_key() {
        let fixture = SshFixture::start().await;
        let mut agent = agent_with_identity(&fixture.authorized_client_key).await;
        let mut handle = trusted_handle(&fixture).await;

        authenticate_with_agent(&mut handle, FIXTURE_USERNAME, &mut agent)
            .await
            .expect("the agent's authorized identity must authenticate");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn agent_authentication_fails_when_the_agent_only_holds_an_unauthorized_key() {
        let fixture = SshFixture::start().await;
        let unauthorized_key =
            russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)
                .expect("generating an unauthorized test key must succeed");
        let mut agent = agent_with_identity(&unauthorized_key).await;
        let mut handle = trusted_handle(&fixture).await;

        let error = authenticate_with_agent(&mut handle, FIXTURE_USERNAME, &mut agent)
            .await
            .expect_err("an identity the server never authorized must be rejected");

        assert_eq!(error, SshError::AuthenticationFailed);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn agent_authentication_reports_a_typed_error_when_the_agent_has_no_identities() {
        let fixture = SshFixture::start().await;
        let socket_dir = tempfile::tempdir().expect("creating a temp dir for the agent socket");
        let socket_path = socket_dir.path().join("empty-agent.sock");
        let listener =
            UnixListener::bind(&socket_path).expect("binding the in-process agent socket");
        tokio::spawn(async move {
            let _socket_dir = socket_dir;
            let stream = Box::pin(futures::stream::unfold(listener, |listener| async move {
                let accepted = listener.accept().await.map(|(stream, _)| stream);
                Some((accepted, listener))
            }));
            let _ = server::serve(stream, ()).await;
        });
        tokio::task::yield_now().await;
        let mut agent = AgentClient::connect_uds(&socket_path)
            .await
            .expect("connecting to the empty in-process agent must succeed");
        let mut handle = trusted_handle(&fixture).await;

        let error = authenticate_with_agent(&mut handle, FIXTURE_USERNAME, &mut agent)
            .await
            .expect_err("an agent with no identities must not silently succeed");

        assert!(matches!(error, SshError::Agent(_)), "got {error:?}");
    }

    /// Connects and authenticates a full [`SshSession`] against `fixture`
    /// with password auth, for task 0105's remote-shell-channel tests below.
    async fn connected_session(fixture: &SshFixture) -> SshSession {
        let known_hosts = Arc::new(InMemoryKnownHostsStore::new());
        known_hosts
            .accept("conn-1", fixture.host_key_fingerprint.clone())
            .await
            .expect("seeding the trusted fingerprint must succeed");
        let params = SshConnectionParameters {
            target: SshConnectTarget {
                host: fixture.addr.ip().to_string(),
                port: fixture.addr.port(),
                username: FIXTURE_USERNAME.to_owned(),
            },
            credential: SshCredential::Password(Zeroizing::new(FIXTURE_PASSWORD.to_owned())),
            host_key_policy: SshHostKeyPolicy::RequireKnownHost,
            keepalive: None,
        };
        SshSession::connect(&params, known_hosts, "conn-1")
            .await
            .expect("connecting to the trusted fixture must succeed")
    }

    #[tokio::test]
    async fn open_shell_without_a_cwd_requests_a_plain_shell_and_echoes_written_data() {
        let fixture = SshFixture::start().await;
        let session = connected_session(&fixture).await;

        let channel = session
            .open_shell("xterm-256color", 80, 24, None)
            .await
            .expect("opening a shell channel must succeed");
        let mut reader = channel.reader;
        let writer = channel.writer;

        writer.write(b"hello").await.expect("writing must succeed");
        let event = reader
            .next()
            .await
            .expect("an echoed event must arrive before the channel closes");
        assert_eq!(event, RemoteShellEvent::Data(b"hello".to_vec()));

        assert!(
            fixture
                .last_exec_command
                .lock()
                .expect("fixture exec-command lock poisoned")
                .is_none(),
            "a plain shell request must never go through exec"
        );
    }

    #[tokio::test]
    async fn open_shell_with_a_cwd_execs_a_quoted_cd_prefixed_login_shell() {
        let fixture = SshFixture::start().await;
        let session = connected_session(&fixture).await;

        let channel = session
            .open_shell("xterm-256color", 80, 24, Some("/tmp/o'brien"))
            .await
            .expect("opening a shell channel with a cwd must succeed");
        // The client-side `.exec(...).await` only waits for the request to be
        // enqueued for sending, not for the server to have processed it (see
        // `ChannelWriteHalf::send_msg`). Round-tripping one write/echo first
        // forces a wait for a response that can only arrive after the
        // server's single-threaded, in-order request processing already
        // recorded the exec command.
        let mut reader = channel.reader;
        channel
            .writer
            .write(b"ping")
            .await
            .expect("writing must succeed");
        reader
            .next()
            .await
            .expect("an echoed event must arrive before the channel closes");

        let recorded = fixture
            .last_exec_command
            .lock()
            .expect("fixture exec-command lock poisoned")
            .clone()
            .expect("an exec command must have been recorded");
        let command = String::from_utf8(recorded).expect("exec command must be valid UTF-8");

        assert_eq!(
            command, "cd '/tmp/o'\\''brien' 2>/dev/null; exec \"${SHELL:-/bin/sh}\" -l",
            "the awkward path must be safely single-quoted, not interpolated raw"
        );
    }

    #[tokio::test]
    async fn resize_sends_a_window_change_request_without_error() {
        let fixture = SshFixture::start().await;
        let session = connected_session(&fixture).await;
        let channel = session
            .open_shell("xterm-256color", 80, 24, None)
            .await
            .expect("opening a shell channel must succeed");

        channel
            .writer
            .resize(100, 40)
            .await
            .expect("resizing an open shell channel must succeed");
    }
}
