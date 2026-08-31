//! Persistent, location-keyed PTY sessions for the embedded terminal drawer
//! (task 0126), extended to locations backed by an SSH connection (task
//! 0105): a `file:` location spawns a local shell via `portable-pty`, while
//! a `sftp:` location drives a real remote PTY over `fm-application`'s
//! [`FileManagerService::open_remote_shell`] instead.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use fm_application::{FileManagerService, RemoteShellChannel, RemoteShellEvent, RemoteShellWriter};
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;
use tauri::ipc::Channel;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase", tag = "type", content = "data")]
pub(crate) enum TerminalEvent {
    Output(Vec<u8>),
    Exited,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum TerminalError {
    #[error("embedded terminals are not available for this location")]
    UnsupportedLocation,
    #[error("terminal session `{0}` does not exist")]
    UnknownSession(String),
    #[error("terminal backend failed: {0}")]
    Backend(String),
}

impl Serialize for TerminalError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

/// One embedded-terminal location: either a local filesystem directory, or a
/// directory reachable over an already-configured SSH connection (task
/// 0105).
enum TerminalLocation {
    Local(PathBuf),
    Remote {
        connection_id: Uuid,
        remote_path: String,
    },
}

impl TerminalLocation {
    fn parse(uri: &str) -> Result<Self, TerminalError> {
        if let Some(path) = uri.strip_prefix("file://") {
            return Ok(Self::Local(PathBuf::from(path)));
        }
        if let Some(remainder) = uri.strip_prefix("sftp://") {
            let (connection_id, path) = remainder
                .split_once('/')
                .ok_or(TerminalError::UnsupportedLocation)?;
            let connection_id =
                Uuid::parse_str(connection_id).map_err(|_| TerminalError::UnsupportedLocation)?;
            let remote_path = decode_percent_path(path)?;
            return Ok(Self::Remote {
                connection_id,
                remote_path,
            });
        }
        Err(TerminalError::UnsupportedLocation)
    }

    /// The host-agnostic key one persistent terminal session is stored
    /// under - `local:`/`ssh:`-prefixed so the two schemes' keys can never
    /// collide.
    fn key(&self) -> String {
        match self {
            Self::Local(path) => format!("local:{}", path.display()),
            Self::Remote {
                connection_id,
                remote_path,
            } => format!("ssh:{connection_id}:{remote_path}"),
        }
    }
}

/// Percent-decodes a `sftp://<connection-id>/<path>` URI's path segments
/// back into real text (e.g. `%20` -> a space), mirroring `fm-vfs-sftp`'s own
/// `ParsedSftpLocation` (private to that crate, so not reusable directly
/// here) - each consumer of this scheme decodes it straight off the URI text
/// rather than reaching into another crate's internals, matching that
/// module's own documented convention.
fn decode_percent_path(path: &str) -> Result<String, TerminalError> {
    if path.is_empty() {
        return Ok("/".to_owned());
    }
    let mut segments = Vec::new();
    for segment in path.split('/') {
        let bytes = segment.as_bytes();
        let mut decoded = Vec::with_capacity(bytes.len());
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] == b'%'
                && index + 2 < bytes.len()
                && let (Some(high), Some(low)) =
                    (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
            {
                decoded.push(high * 16 + low);
                index += 3;
                continue;
            }
            decoded.push(bytes[index]);
            index += 1;
        }
        segments.push(String::from_utf8(decoded).map_err(|_| TerminalError::UnsupportedLocation)?);
    }
    Ok(format!("/{}", segments.join("/")))
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

enum SessionBackend {
    Local {
        master: Box<dyn MasterPty + Send>,
        writer: Box<dyn Write + Send>,
        _child: Box<dyn portable_pty::Child + Send + Sync>,
    },
    Remote {
        writer: RemoteShellWriter,
    },
}

struct Session {
    backend: SessionBackend,
    subscribers: Arc<Mutex<Vec<Channel<TerminalEvent>>>>,
    history: Arc<Mutex<Vec<u8>>>,
}

/// What happened when [`TerminalRegistry::reuse_existing`] looked up a key.
enum ReuseOutcome {
    NotFound,
    /// A local session was found, subscribed and resized synchronously.
    ReusedLocal,
    /// A remote session was found and subscribed; its resize still needs an
    /// async round trip, left to the caller.
    ReusedRemote(RemoteShellWriter),
}

/// Owns one live terminal session per backing location, independent of UI
/// panes.
///
/// `sessions` is `Arc`-wrapped so the background reader (a thread for a
/// local PTY, a tokio task for a remote SSH channel) can remove its own
/// entry once the process/channel ends - without this, a location whose
/// remote connection dropped would keep matching [`Self::reuse_existing`]
/// forever, reusing a permanently dead writer instead of genuinely
/// reconnecting on the next open (task 0105's "surfaces a clear
/// disconnected state" acceptance criterion needs a real redial path, not
/// just a UI message).
#[derive(Default)]
pub(crate) struct TerminalRegistry {
    sessions: Arc<Mutex<HashMap<String, Session>>>,
}

impl TerminalRegistry {
    /// Opens (or reuses) the terminal session for `location_uri`, dispatching
    /// to a local PTY (`native_path` must be `Some`) or a remote SSH shell
    /// channel (opened through `service`, task 0105) depending on the
    /// location's scheme.
    pub(crate) async fn open(
        &self,
        service: &FileManagerService,
        location_uri: &str,
        native_path: Option<&Path>,
        size: PtySize,
        channel: Channel<TerminalEvent>,
    ) -> Result<String, TerminalError> {
        let location = TerminalLocation::parse(location_uri)?;
        let key = location.key();

        match self.reuse_existing(&key, size, channel.clone())? {
            ReuseOutcome::ReusedLocal => return Ok(key),
            ReuseOutcome::ReusedRemote(writer) => {
                writer
                    .resize(u32::from(size.cols), u32::from(size.rows))
                    .await
                    .map_err(backend)?;
                return Ok(key);
            }
            ReuseOutcome::NotFound => {}
        }

        match location {
            TerminalLocation::Local(_) => {
                let cwd = native_path.ok_or(TerminalError::UnsupportedLocation)?;
                self.open_local(key, cwd, size, channel)
            }
            TerminalLocation::Remote {
                connection_id,
                remote_path,
            } => {
                let remote_channel = service
                    .open_remote_shell(
                        connection_id,
                        Some(&remote_path),
                        "xterm-256color",
                        size.cols,
                        size.rows,
                    )
                    .await
                    .map_err(|error| TerminalError::Backend(error.to_string()))?;
                self.open_remote(key, remote_channel, channel)
            }
        }
    }

    /// If a session already exists for `key`, replays its history into
    /// `channel`, subscribes it, and reports which backend it is (a local
    /// session is resized synchronously here; a remote one still needs an
    /// async resize the caller performs).
    fn reuse_existing(
        &self,
        key: &str,
        size: PtySize,
        channel: Channel<TerminalEvent>,
    ) -> Result<ReuseOutcome, TerminalError> {
        let mut sessions = self.sessions.lock().expect("terminal registry poisoned");
        let Some(session) = sessions.get_mut(key) else {
            return Ok(ReuseOutcome::NotFound);
        };
        let history = session
            .history
            .lock()
            .expect("terminal history poisoned")
            .clone();
        if !history.is_empty() {
            let _ = channel.send(TerminalEvent::Output(history));
        }
        session
            .subscribers
            .lock()
            .expect("terminal subscribers poisoned")
            .push(channel);
        match &session.backend {
            SessionBackend::Local { master, .. } => {
                master.resize(size).map_err(backend)?;
                Ok(ReuseOutcome::ReusedLocal)
            }
            SessionBackend::Remote { writer } => Ok(ReuseOutcome::ReusedRemote(writer.clone())),
        }
    }

    fn open_local(
        &self,
        key: String,
        cwd: &Path,
        size: PtySize,
        channel: Channel<TerminalEvent>,
    ) -> Result<String, TerminalError> {
        let pair = native_pty_system().openpty(size).map_err(backend)?;
        let shell =
            std::env::var(if cfg!(windows) { "COMSPEC" } else { "SHELL" }).unwrap_or_else(|_| {
                if cfg!(windows) {
                    "cmd.exe".into()
                } else {
                    "/bin/sh".into()
                }
            });
        let mut command = CommandBuilder::new(shell);
        command.cwd(cwd);
        let child = pair.slave.spawn_command(command).map_err(backend)?;
        let writer = pair.master.take_writer().map_err(backend)?;
        let mut reader = pair.master.try_clone_reader().map_err(backend)?;
        let subscribers = Arc::new(Mutex::new(vec![channel]));
        let history = Arc::new(Mutex::new(Vec::new()));
        let thread_subscribers = Arc::clone(&subscribers);
        let thread_history = Arc::clone(&history);
        let cleanup_sessions = Arc::clone(&self.sessions);
        let cleanup_key = key.clone();
        std::thread::spawn(move || {
            let mut buffer = [0_u8; 8192];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => {
                        record_and_broadcast(&thread_history, &thread_subscribers, &buffer[..read]);
                    }
                }
            }
            broadcast_exit(&thread_subscribers);
            cleanup_sessions
                .lock()
                .expect("terminal registry poisoned")
                .remove(&cleanup_key);
        });
        self.sessions
            .lock()
            .expect("terminal registry poisoned")
            .insert(
                key.clone(),
                Session {
                    backend: SessionBackend::Local {
                        master: pair.master,
                        writer,
                        _child: child,
                    },
                    subscribers,
                    history,
                },
            );
        Ok(key)
    }

    fn open_remote(
        &self,
        key: String,
        remote_channel: RemoteShellChannel,
        channel: Channel<TerminalEvent>,
    ) -> Result<String, TerminalError> {
        let RemoteShellChannel { mut reader, writer } = remote_channel;
        let subscribers = Arc::new(Mutex::new(vec![channel]));
        let history = Arc::new(Mutex::new(Vec::new()));
        let thread_subscribers = Arc::clone(&subscribers);
        let thread_history = Arc::clone(&history);
        let cleanup_sessions = Arc::clone(&self.sessions);
        let cleanup_key = key.clone();
        tokio::spawn(async move {
            while let Some(RemoteShellEvent::Data(bytes)) = reader.next().await {
                record_and_broadcast(&thread_history, &thread_subscribers, &bytes);
            }
            broadcast_exit(&thread_subscribers);
            cleanup_sessions
                .lock()
                .expect("terminal registry poisoned")
                .remove(&cleanup_key);
        });
        self.sessions
            .lock()
            .expect("terminal registry poisoned")
            .insert(
                key.clone(),
                Session {
                    backend: SessionBackend::Remote { writer },
                    subscribers,
                    history,
                },
            );
        Ok(key)
    }

    pub(crate) async fn write(&self, id: &str, data: &[u8]) -> Result<(), TerminalError> {
        let remote_writer = {
            let mut sessions = self.sessions.lock().expect("terminal registry poisoned");
            let session = sessions
                .get_mut(id)
                .ok_or_else(|| TerminalError::UnknownSession(id.into()))?;
            match &mut session.backend {
                SessionBackend::Local { writer, .. } => {
                    writer
                        .write_all(data)
                        .and_then(|_| writer.flush())
                        .map_err(backend)?;
                    None
                }
                SessionBackend::Remote { writer } => Some(writer.clone()),
            }
        };
        if let Some(writer) = remote_writer {
            writer.write(data).await.map_err(backend)?;
        }
        Ok(())
    }

    pub(crate) async fn resize(&self, id: &str, size: PtySize) -> Result<(), TerminalError> {
        let remote_writer = {
            let sessions = self.sessions.lock().expect("terminal registry poisoned");
            let session = sessions
                .get(id)
                .ok_or_else(|| TerminalError::UnknownSession(id.into()))?;
            match &session.backend {
                SessionBackend::Local { master, .. } => {
                    master.resize(size).map_err(backend)?;
                    None
                }
                SessionBackend::Remote { writer } => Some(writer.clone()),
            }
        };
        if let Some(writer) = remote_writer {
            writer
                .resize(u32::from(size.cols), u32::from(size.rows))
                .await
                .map_err(backend)?;
        }
        Ok(())
    }
}

fn record_and_broadcast(
    history: &Mutex<Vec<u8>>,
    subscribers: &Mutex<Vec<Channel<TerminalEvent>>>,
    bytes: &[u8],
) {
    history
        .lock()
        .expect("terminal history poisoned")
        .extend_from_slice(bytes);
    let mut listeners = subscribers.lock().expect("terminal subscribers poisoned");
    listeners.retain(|listener| listener.send(TerminalEvent::Output(bytes.to_vec())).is_ok());
}

fn broadcast_exit(subscribers: &Mutex<Vec<Channel<TerminalEvent>>>) {
    let mut listeners = subscribers.lock().expect("terminal subscribers poisoned");
    listeners.retain(|listener| listener.send(TerminalEvent::Exited).is_ok());
}

fn backend(error: impl std::fmt::Display) -> TerminalError {
    TerminalError::Backend(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_local_filesystem_location_is_a_host_agnostic_terminal_key() {
        let location = TerminalLocation::parse("file:///projects/foo").unwrap();
        assert_eq!(location.key(), "local:/projects/foo");
    }

    #[test]
    fn an_sftp_location_keys_by_connection_id_and_decoded_remote_path() {
        let connection_id = Uuid::nil();
        let uri = format!("sftp://{connection_id}/home/erik/projects");
        let location = TerminalLocation::parse(&uri).unwrap();
        assert_eq!(
            location.key(),
            format!("ssh:{connection_id}:/home/erik/projects")
        );
    }

    #[test]
    fn an_sftp_location_percent_decodes_awkward_path_segments() {
        let connection_id = Uuid::nil();
        let uri = format!("sftp://{connection_id}/tmp/a%20space");
        let location = TerminalLocation::parse(&uri).unwrap();
        assert_eq!(location.key(), format!("ssh:{connection_id}:/tmp/a space"));
    }

    #[test]
    fn an_sftp_location_with_an_invalid_connection_id_is_unsupported() {
        assert!(matches!(
            TerminalLocation::parse("sftp://not-a-uuid/tmp"),
            Err(TerminalError::UnsupportedLocation)
        ));
    }

    #[test]
    fn an_unknown_scheme_is_unsupported() {
        assert!(matches!(
            TerminalLocation::parse("ftp://host/projects/foo"),
            Err(TerminalError::UnsupportedLocation)
        ));
    }
}
