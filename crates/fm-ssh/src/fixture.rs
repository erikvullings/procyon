//! An in-process SSH + SFTP server for tests (task 0104, spec §18 "SFTP").
//!
//! ## Fixture choice
//!
//! Two hermetic options were viable: (a) an in-process `russh`/`russh-sftp`
//! server, or (b) spawning the system `sshd`/`sftp-server` against a
//! generated throwaway config. This module implements (a) because:
//!
//! - it needs no external process, privileged config file, or `sshd`
//!   available on the host, so it works identically in a sandboxed CI
//!   environment or a contributor's machine without local SSH tooling setup;
//! - `russh`/`russh-sftp` are already this crate's real client dependencies
//!   for production code, so the fixture exercises the *actual* wire
//!   protocol end to end (not a mock or stub) - it is an independent server
//!   implementation from the client code under test, so a round trip through
//!   it genuinely verifies wire compatibility, just as connecting to a real
//!   `sshd` would.
//!
//! Exposed unconditionally (not behind `#[cfg(test)]`) so this module is
//! usable as a `dev-dependency` fixture from other crates' tests too (task
//! 0104's `fm-vfs-sftp`, and future SSH-terminal consumers), matching the
//! task's requirement that the fixture be reusable rather than private to
//! one test file.
//!
//! ## Filesystem model
//!
//! The fixture serves real files under a real temporary directory
//! ([`SshFixture::root`]), translating client-presented paths the way a
//! real SFTP server does: the wire protocol is always Unix-style
//! (forward-slash-separated, rooted at `/`), so every incoming path is
//! resolved relative to `root` regardless of the host OS's native path
//! syntax (this matters on Windows, where `root`'s real path has a drive
//! letter and backslashes that can't appear in an SFTP wire path). Tests
//! address files with root-relative Unix-style segments (see
//! `SshFixture::path`'s callers), not `root`'s native path.

use std::collections::HashMap;
use std::io::SeekFrom;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use russh::keys::{Algorithm, PrivateKey, PublicKey};
use russh::server::{
    Auth, ChannelOpenHandle, Config as ServerConfig, Handler as ServerHandler, Msg,
    Server as ServerTrait,
};
use russh::{Channel, ChannelId, Pty};
use russh_sftp::protocol::{
    Attrs, Data, File as SftpFile, FileAttributes, Handle as SftpHandle, Name, OpenFlags, Status,
    StatusCode,
};
use tokio::fs::File as TokioFile;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex as AsyncMutex;
use tokio::task::JoinHandle;

/// Username the fixture accepts.
pub const FIXTURE_USERNAME: &str = "fixture-user";
/// Password the fixture accepts when password authentication is enabled.
pub const FIXTURE_PASSWORD: &str = "fixture-password";

/// A running in-process SSH+SFTP fixture server.
pub struct SshFixture {
    /// The loopback address the fixture is listening on.
    pub addr: SocketAddr,
    /// The real temporary directory files are served from/into.
    pub root: tempfile::TempDir,
    /// The fixture's host key.
    pub host_key: PrivateKey,
    /// The fixture host key's SHA-256 fingerprint.
    pub host_key_fingerprint: String,
    /// A client private key the fixture accepts for public-key
    /// authentication.
    pub authorized_client_key: PrivateKey,
    /// The most recent `exec` request's command bytes, if any (task 0105's
    /// shell-channel tests use this to verify a `cd <dir>`-prefixed command
    /// was sent, and that it was quoted correctly).
    pub last_exec_command: Arc<std::sync::Mutex<Option<Vec<u8>>>>,
    accept_task: JoinHandle<()>,
}

impl Drop for SshFixture {
    fn drop(&mut self) {
        self.accept_task.abort();
    }
}

impl SshFixture {
    /// Starts a fixture bound to an ephemeral localhost port, with a freshly
    /// generated host key and one authorized client key (for public-key
    /// authentication tests).
    pub async fn start() -> Self {
        Self::start_with_read_delay(std::time::Duration::ZERO).await
    }

    /// Like [`Self::start`], but with an artificial delay inserted before
    /// every SFTP `read` response.
    ///
    /// The fixture otherwise streams over loopback with no real disk I/O
    /// wait, so a client reading a large file can complete a transfer within
    /// a single scheduling turn - too fast for a test racing another task
    /// against "is this still in progress" (e.g. requesting cancellation
    /// mid-transfer) to reliably win under CI scheduling contention. A small
    /// per-read delay forces a real, timer-driven yield on every chunk,
    /// giving the rest of the runtime a guaranteed opportunity to run
    /// between chunks regardless of system load.
    pub async fn start_with_read_delay(read_delay: std::time::Duration) -> Self {
        let host_key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)
            .expect("generating a fixture host key must succeed");
        let host_key_fingerprint = crate::fingerprint::fingerprint_of(host_key.public_key());
        let authorized_client_key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)
            .expect("generating a fixture client key must succeed");

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("binding an ephemeral fixture port must succeed");
        let addr = listener
            .local_addr()
            .expect("a bound listener must have a local address");

        let server_config = Arc::new(ServerConfig {
            keys: vec![host_key.clone()],
            ..Default::default()
        });

        let root = tempfile::tempdir().expect("creating a fixture root directory must succeed");
        let authorized_public_key = authorized_client_key.public_key().clone();
        let last_exec_command = Arc::new(std::sync::Mutex::new(None));
        let mut server = FixtureServer {
            root: root.path().to_path_buf(),
            authorized_public_key,
            last_exec_command: last_exec_command.clone(),
            read_delay,
        };

        let accept_task = tokio::spawn(async move {
            let _ = server.run_on_socket(server_config, &listener).await;
        });

        Self {
            addr,
            root,
            host_key,
            host_key_fingerprint,
            authorized_client_key,
            last_exec_command,
            accept_task,
        }
    }

    /// A path under the fixture's real root directory.
    #[must_use]
    pub fn path(&self, relative: &str) -> PathBuf {
        self.root.path().join(relative)
    }

    /// The virtual SFTP root path (`/`), for building remote-path text -
    /// see the module doc's "Filesystem model" section for why this is
    /// `/` rather than `root`'s real, host-native path.
    #[must_use]
    pub fn root_path_string(&self) -> String {
        "/".to_owned()
    }
}

#[derive(Clone)]
struct FixtureServer {
    root: PathBuf,
    authorized_public_key: PublicKey,
    last_exec_command: Arc<std::sync::Mutex<Option<Vec<u8>>>>,
    read_delay: std::time::Duration,
}

impl ServerTrait for FixtureServer {
    type Handler = FixtureSshHandler;

    fn new_client(&mut self, _peer_addr: Option<SocketAddr>) -> Self::Handler {
        FixtureSshHandler {
            root: self.root.clone(),
            authorized_public_key: self.authorized_public_key.clone(),
            channels: Arc::new(AsyncMutex::new(HashMap::new())),
            last_exec_command: self.last_exec_command.clone(),
            read_delay: self.read_delay,
        }
    }
}

struct FixtureSshHandler {
    root: PathBuf,
    authorized_public_key: PublicKey,
    channels: Arc<AsyncMutex<HashMap<ChannelId, Channel<Msg>>>>,
    last_exec_command: Arc<std::sync::Mutex<Option<Vec<u8>>>>,
    read_delay: std::time::Duration,
}

impl ServerHandler for FixtureSshHandler {
    type Error = russh::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        if user == FIXTURE_USERNAME && password == FIXTURE_PASSWORD {
            Ok(Auth::Accept)
        } else {
            Ok(Auth::reject())
        }
    }

    async fn auth_publickey(
        &mut self,
        user: &str,
        public_key: &PublicKey,
    ) -> Result<Auth, Self::Error> {
        if user == FIXTURE_USERNAME && *public_key == self.authorized_public_key {
            Ok(Auth::Accept)
        } else {
            Ok(Auth::reject())
        }
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        reply: ChannelOpenHandle,
        _session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        self.channels.lock().await.insert(channel.id(), channel);
        reply.accept().await;
        Ok(())
    }

    async fn subsystem_request(
        &mut self,
        channel_id: ChannelId,
        name: &str,
        session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        if name != "sftp" {
            session.channel_failure(channel_id)?;
            return Ok(());
        }
        let Some(channel) = self.channels.lock().await.remove(&channel_id) else {
            session.channel_failure(channel_id)?;
            return Ok(());
        };
        session.channel_success(channel_id)?;
        russh_sftp::server::run(
            channel.into_stream(),
            FixtureSftpHandler::new(self.root.clone(), self.read_delay),
        )
        .await;
        Ok(())
    }

    /// Accepts any PTY request unconditionally (task 0105's shell-channel
    /// tests only care that the request round-trips, not real terminal
    /// semantics).
    async fn pty_request(
        &mut self,
        channel: ChannelId,
        _term: &str,
        _col_width: u32,
        _row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        _modes: &[(Pty, u32)],
        session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        Ok(())
    }

    /// Accepts a shell request; the fixture never actually spawns a real
    /// login shell process (see [`Self::data`]'s echo behavior below).
    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        Ok(())
    }

    /// Records the `exec` command text (for `cd <dir> && exec $SHELL`
    /// quoting assertions) and accepts the request.
    async fn exec_request(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        *self
            .last_exec_command
            .lock()
            .expect("fixture exec-command lock poisoned") = Some(data.to_vec());
        session.channel_success(channel)?;
        Ok(())
    }

    async fn window_change_request(
        &mut self,
        channel: ChannelId,
        _col_width: u32,
        _row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        Ok(())
    }

    /// Echoes every byte back on the same channel, standing in for a real
    /// remote shell so tests can verify the client's read/write plumbing
    /// against a genuine `russh` server without needing an actual login
    /// shell subprocess.
    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        session.data(channel, data.to_vec())?;
        Ok(())
    }
}

enum FixtureHandle {
    File(TokioFile),
    Dir {
        entries: Vec<(String, std::fs::Metadata)>,
        sent: bool,
    },
}

struct FixtureSftpHandler {
    root: PathBuf,
    handles: HashMap<String, FixtureHandle>,
    next_handle_id: u64,
    read_delay: std::time::Duration,
}

impl FixtureSftpHandler {
    fn new(root: PathBuf, read_delay: std::time::Duration) -> Self {
        Self {
            root,
            handles: HashMap::new(),
            next_handle_id: 0,
            read_delay,
        }
    }

    fn allocate_handle(&mut self, handle: FixtureHandle) -> String {
        self.next_handle_id += 1;
        let id = self.next_handle_id.to_string();
        self.handles.insert(id.clone(), handle);
        id
    }

    /// Resolves an SFTP wire path (always Unix-style, rooted at `/`) to a
    /// real path under [`Self::root`] - see the module doc's "Filesystem
    /// model" section.
    fn resolve(&self, path: &str) -> PathBuf {
        match path.trim_start_matches('/') {
            "" => self.root.clone(),
            relative => self.root.join(relative),
        }
    }
}

fn ok_status(id: u32) -> Status {
    Status {
        id,
        status_code: StatusCode::Ok,
        error_message: "Ok".to_owned(),
        language_tag: "en-US".to_owned(),
    }
}

fn map_io_error(error: &std::io::Error) -> StatusCode {
    match error.kind() {
        std::io::ErrorKind::NotFound => StatusCode::NoSuchFile,
        std::io::ErrorKind::PermissionDenied => StatusCode::PermissionDenied,
        _ => StatusCode::Failure,
    }
}

impl russh_sftp::server::Handler for FixtureSftpHandler {
    type Error = StatusCode;

    fn unimplemented(&self) -> Self::Error {
        StatusCode::OpUnsupported
    }

    async fn open(
        &mut self,
        id: u32,
        filename: String,
        pflags: OpenFlags,
        _attrs: FileAttributes,
    ) -> Result<SftpHandle, Self::Error> {
        let mut options = tokio::fs::OpenOptions::new();
        options.read(pflags.contains(OpenFlags::READ));
        options.write(pflags.contains(OpenFlags::WRITE) || pflags.contains(OpenFlags::APPEND));
        options.append(pflags.contains(OpenFlags::APPEND));
        if pflags.contains(OpenFlags::CREATE) {
            if pflags.contains(OpenFlags::EXCLUDE) {
                options.create_new(true);
            } else {
                options.create(true);
            }
        }
        options.truncate(pflags.contains(OpenFlags::TRUNCATE));

        let file = options
            .open(self.resolve(&filename))
            .await
            .map_err(|error| map_io_error(&error))?;
        let handle = self.allocate_handle(FixtureHandle::File(file));
        Ok(SftpHandle { id, handle })
    }

    async fn close(&mut self, id: u32, handle: String) -> Result<Status, Self::Error> {
        self.handles.remove(&handle);
        Ok(ok_status(id))
    }

    async fn read(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        len: u32,
    ) -> Result<Data, Self::Error> {
        if !self.read_delay.is_zero() {
            tokio::time::sleep(self.read_delay).await;
        }
        let Some(FixtureHandle::File(file)) = self.handles.get_mut(&handle) else {
            return Err(StatusCode::Failure);
        };
        file.seek(SeekFrom::Start(offset))
            .await
            .map_err(|error| map_io_error(&error))?;
        let mut buffer = vec![0_u8; len as usize];
        let read = file
            .read(&mut buffer)
            .await
            .map_err(|error| map_io_error(&error))?;
        if read == 0 {
            return Err(StatusCode::Eof);
        }
        buffer.truncate(read);
        Ok(Data { id, data: buffer })
    }

    async fn write(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        data: Vec<u8>,
    ) -> Result<Status, Self::Error> {
        let Some(FixtureHandle::File(file)) = self.handles.get_mut(&handle) else {
            return Err(StatusCode::Failure);
        };
        file.seek(SeekFrom::Start(offset))
            .await
            .map_err(|error| map_io_error(&error))?;
        file.write_all(&data)
            .await
            .map_err(|error| map_io_error(&error))?;
        Ok(ok_status(id))
    }

    async fn lstat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        let metadata = tokio::fs::symlink_metadata(self.resolve(&path))
            .await
            .map_err(|error| map_io_error(&error))?;
        Ok(Attrs {
            id,
            attrs: FileAttributes::from(&metadata),
        })
    }

    async fn stat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        let metadata = tokio::fs::metadata(self.resolve(&path))
            .await
            .map_err(|error| map_io_error(&error))?;
        Ok(Attrs {
            id,
            attrs: FileAttributes::from(&metadata),
        })
    }

    async fn fstat(&mut self, id: u32, handle: String) -> Result<Attrs, Self::Error> {
        let Some(FixtureHandle::File(file)) = self.handles.get(&handle) else {
            return Err(StatusCode::Failure);
        };
        let metadata = file
            .metadata()
            .await
            .map_err(|error| map_io_error(&error))?;
        Ok(Attrs {
            id,
            attrs: FileAttributes::from(&metadata),
        })
    }

    async fn opendir(&mut self, id: u32, path: String) -> Result<SftpHandle, Self::Error> {
        let mut reader = tokio::fs::read_dir(self.resolve(&path))
            .await
            .map_err(|error| map_io_error(&error))?;
        let mut entries = Vec::new();
        while let Some(entry) = reader
            .next_entry()
            .await
            .map_err(|error| map_io_error(&error))?
        {
            let metadata = entry
                .metadata()
                .await
                .map_err(|error| map_io_error(&error))?;
            entries.push((entry.file_name().to_string_lossy().into_owned(), metadata));
        }
        let handle = self.allocate_handle(FixtureHandle::Dir {
            entries,
            sent: false,
        });
        Ok(SftpHandle { id, handle })
    }

    async fn readdir(&mut self, id: u32, handle: String) -> Result<Name, Self::Error> {
        let Some(FixtureHandle::Dir { entries, sent }) = self.handles.get_mut(&handle) else {
            return Err(StatusCode::Failure);
        };
        if *sent {
            return Err(StatusCode::Eof);
        }
        *sent = true;
        let files = entries
            .iter()
            .map(|(name, metadata)| SftpFile::new(name.clone(), FileAttributes::from(metadata)))
            .collect();
        Ok(Name { id, files })
    }

    async fn remove(&mut self, id: u32, filename: String) -> Result<Status, Self::Error> {
        tokio::fs::remove_file(self.resolve(&filename))
            .await
            .map_err(|error| map_io_error(&error))?;
        Ok(ok_status(id))
    }

    async fn mkdir(
        &mut self,
        id: u32,
        path: String,
        _attrs: FileAttributes,
    ) -> Result<Status, Self::Error> {
        tokio::fs::create_dir(self.resolve(&path))
            .await
            .map_err(|error| map_io_error(&error))?;
        Ok(ok_status(id))
    }

    async fn rmdir(&mut self, id: u32, path: String) -> Result<Status, Self::Error> {
        tokio::fs::remove_dir(self.resolve(&path))
            .await
            .map_err(|error| map_io_error(&error))?;
        Ok(ok_status(id))
    }

    async fn rename(
        &mut self,
        id: u32,
        oldpath: String,
        newpath: String,
    ) -> Result<Status, Self::Error> {
        tokio::fs::rename(self.resolve(&oldpath), self.resolve(&newpath))
            .await
            .map_err(|error| map_io_error(&error))?;
        Ok(ok_status(id))
    }

    async fn realpath(&mut self, id: u32, path: String) -> Result<Name, Self::Error> {
        let resolved = self.resolve(&path);
        let canonical = match tokio::fs::canonicalize(&resolved).await {
            Ok(canonical) => {
                let relative = canonical.strip_prefix(&self.root).unwrap_or(&canonical);
                let mut virtual_path = String::from("/");
                virtual_path.push_str(&relative.to_string_lossy().replace('\\', "/"));
                virtual_path
            }
            Err(_) => path,
        };
        Ok(Name {
            id,
            files: vec![SftpFile::dummy(canonical)],
        })
    }
}
