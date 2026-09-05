//! Local IPC client, launcher, and isolated semantic worker runtime.
//!
//! Only opaque identifiers, metadata, and caller-provided bytes cross this
//! boundary. The worker has no file-provider or network-facing dependency.

pub mod advanced;
pub mod document_summary;
pub mod embedding;
pub mod ingestion;
pub mod rag_retrieval;
pub mod representative_selection;
pub mod semantic_search;
pub mod semantic_storage;
#[cfg(feature = "zvec")]
pub mod zvec_storage;

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fm_semantic_protocol::{
    DeadlineKind, FrameError, LimitError, NegotiatedLimitsError, NegotiationError, Negotiator,
    ProtocolLimits, ProtocolVersion, RequestValidationError, SessionToken, StreamBudget,
    VersionRange, read_frame, v1, validate_ingestion_with_limits, validate_query, write_frame,
};
use prost::Message;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{Mutex as AsyncMutex, Notify, OwnedSemaphorePermit, Semaphore, mpsc, watch};
use tokio_util::sync::CancellationToken;

/// Failure to construct an owner-only Windows named-pipe policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PipeSecurityError {
    /// The supplied current-user SID was not a canonical numeric SID.
    #[error("invalid current-user security identifier")]
    InvalidUserSid,
}

/// Builds a protected Windows security descriptor granting access only to the
/// supplied current-user SID.
///
/// # Errors
///
/// Returns [`PipeSecurityError::InvalidUserSid`] rather than allowing SDDL
/// syntax to be injected through an invalid SID.
pub fn owner_only_pipe_sddl(user_sid: &str) -> Result<String, PipeSecurityError> {
    let Some(components) = user_sid.strip_prefix("S-") else {
        return Err(PipeSecurityError::InvalidUserSid);
    };
    let mut components = components.split('-');
    if components.clone().count() < 2
        || components.any(|component| {
            component.is_empty() || !component.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        return Err(PipeSecurityError::InvalidUserSid);
    }
    Ok(format!("O:{user_sid}G:{user_sid}D:P(A;;GA;;;{user_sid})"))
}

/// Reports whether a Windows named-pipe path is explicitly local.
#[must_use]
pub fn is_local_named_pipe_endpoint(name: &str) -> bool {
    const LOCAL_PIPE_PREFIX: &str = r"\\.\pipe\";
    name.get(..LOCAL_PIPE_PREFIX.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(LOCAL_PIPE_PREFIX))
        && name.len() > LOCAL_PIPE_PREFIX.len()
}

/// The platform-local endpoint used by a worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    /// A Unix-domain socket path.
    #[cfg(unix)]
    Unix(PathBuf),
    /// A Windows named-pipe name.
    #[cfg(windows)]
    Windows(String),
}

impl Endpoint {
    /// Selects the platform endpoint inside a per-user runtime directory.
    #[must_use]
    pub fn for_runtime_directory(directory: &Path) -> Self {
        #[cfg(unix)]
        {
            Self::Unix(directory.join("semantic-worker.sock"))
        }
        #[cfg(windows)]
        {
            let key = directory
                .to_string_lossy()
                .bytes()
                .fold(0_u64, |hash, byte| {
                    hash.wrapping_mul(1099511628211) ^ u64::from(byte)
                });
            Self::Windows(format!(r"\\.\pipe\procyon-semantic-{key:016x}"))
        }
    }

    /// Reports whether the endpoint currently exists.
    #[must_use]
    pub fn exists(&self) -> bool {
        match self {
            #[cfg(unix)]
            Self::Unix(path) => path.exists(),
            #[cfg(windows)]
            Self::Windows(_) => false,
        }
    }
}

/// A cryptographically random launch-time authentication secret.
#[derive(Clone, PartialEq, Eq)]
pub struct LaunchSecret([u8; 32]);

impl LaunchSecret {
    /// Generates a new ephemeral secret.
    #[must_use]
    pub fn generate() -> Self {
        Self(rand::random())
    }

    /// Constructs a provisioned secret from exactly 32 bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the bytes needed to provision the other endpoint.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for LaunchSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("LaunchSecret([REDACTED])")
    }
}

/// Runtime policy for one worker.
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    endpoint: Endpoint,
    secret: LaunchSecret,
    versions: VersionRange,
    limits: ProtocolLimits,
    idle_timeout: Duration,
    session_lifetime: Duration,
    ingestion_timeout: Option<Duration>,
    atomic_ingestion_delay: Duration,
    test_query_scan_delay: Duration,
    test_query_write_delay: Duration,
    test_event_scan_delay: Duration,
    test_event_phase_bytes: usize,
    test_idle_wait_delay: Duration,
}

impl WorkerConfig {
    /// Creates a worker configuration with bounded defaults.
    #[must_use]
    pub fn new(endpoint: Endpoint, secret: LaunchSecret) -> Self {
        Self {
            endpoint,
            secret,
            versions: VersionRange::exact(ProtocolVersion::new(1)),
            limits: ProtocolLimits::default(),
            idle_timeout: Duration::from_secs(30),
            session_lifetime: Duration::from_secs(60 * 60),
            ingestion_timeout: None,
            atomic_ingestion_delay: Duration::ZERO,
            test_query_scan_delay: Duration::ZERO,
            test_query_write_delay: Duration::ZERO,
            test_event_scan_delay: Duration::ZERO,
            test_event_phase_bytes: 0,
            test_idle_wait_delay: Duration::ZERO,
        }
    }

    /// Overrides the protocol versions served by this worker.
    #[must_use]
    pub fn with_versions(mut self, versions: VersionRange) -> Self {
        self.versions = versions;
        self
    }

    /// Overrides the idle timeout used after the last client disconnects.
    #[must_use]
    pub fn with_idle_timeout(mut self, timeout: Duration) -> Self {
        self.idle_timeout = timeout;
        self
    }

    /// Overrides the lifetime of newly authenticated sessions.
    #[must_use]
    pub fn with_session_lifetime(mut self, lifetime: Duration) -> Self {
        self.session_lifetime = lifetime;
        self
    }

    /// Overrides the maximum time allowed between ingestion start and its
    /// final content chunk.
    #[must_use]
    pub fn with_ingestion_timeout(mut self, timeout: Duration) -> Self {
        self.ingestion_timeout = Some(
            timeout.min(
                self.limits
                    .deadline(fm_semantic_protocol::DeadlineKind::Stream),
            ),
        );
        self
    }

    /// Overrides protocol resource limits.
    #[must_use]
    pub fn with_limits(mut self, limits: ProtocolLimits) -> Self {
        self.limits = limits;
        self.ingestion_timeout = self.ingestion_timeout.map(|timeout| {
            timeout.min(
                self.limits
                    .deadline(fm_semantic_protocol::DeadlineKind::Stream),
            )
        });
        self
    }

    /// Returns the configured ingestion timeout after applying the stream
    /// deadline ceiling.
    #[must_use]
    pub fn effective_ingestion_timeout(&self) -> Duration {
        self.ingestion_timeout.unwrap_or_else(|| {
            self.limits
                .deadline(fm_semantic_protocol::DeadlineKind::Stream)
        })
    }

    /// Adds a deterministic delay to the fake engine's atomic commit.
    #[must_use]
    pub fn with_atomic_ingestion_delay(mut self, delay: Duration) -> Self {
        self.atomic_ingestion_delay = delay;
        self
    }

    /// Adds deterministic fake-engine work to each scanned document. This is
    /// intended for query scheduling and cancellation tests.
    #[doc(hidden)]
    #[must_use]
    pub fn with_test_query_scan_delay(mut self, delay: Duration) -> Self {
        self.test_query_scan_delay = delay;
        self
    }

    /// Adds a deterministic delay while the fake engine writes each query
    /// result. This is intended for transport deadline tests.
    #[doc(hidden)]
    #[must_use]
    pub fn with_test_query_write_delay(mut self, delay: Duration) -> Self {
        self.test_query_write_delay = delay;
        self
    }

    /// Adds deterministic fake-engine work to each prepared event. This is
    /// intended for event scheduling and deadline tests.
    #[doc(hidden)]
    #[must_use]
    pub fn with_test_event_scan_delay(mut self, delay: Duration) -> Self {
        self.test_event_scan_delay = delay;
        self
    }

    /// Overrides the fake event phase size for response-limit tests.
    #[doc(hidden)]
    #[must_use]
    pub fn with_test_event_phase_bytes(mut self, bytes: usize) -> Self {
        self.test_event_phase_bytes = bytes;
        self
    }

    /// Widens idle-state transitions for deterministic lifecycle race tests.
    #[doc(hidden)]
    #[must_use]
    pub fn with_test_idle_wait_delay(mut self, delay: Duration) -> Self {
        self.test_idle_wait_delay = delay;
        self
    }
}

/// Worker-server startup or transport failure.
#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    /// Local IPC failed.
    #[error("worker local IPC failed: {0}")]
    Io(#[from] io::Error),
    /// Framing failed.
    #[error(transparent)]
    Frame(#[from] FrameError),
    /// Runtime directory ownership or permissions are unsafe.
    #[error("unsafe worker runtime directory: {0}")]
    UnsafeRuntimeDirectory(String),
    /// Another worker already owns the per-user lifetime lock.
    #[error("another semantic worker is already running")]
    AlreadyRunning,
}

/// Typed host-side worker failure.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// Local IPC failed.
    #[error("worker local IPC failed: {0}")]
    Io(#[from] io::Error),
    /// Framing failed.
    #[error(transparent)]
    Frame(#[from] FrameError),
    /// Authentication was rejected without exposing either secret.
    #[error("worker authentication was rejected")]
    Unauthenticated,
    /// Protocol negotiation requires a component update.
    #[error("worker protocol is incompatible: {code:?}: {message}")]
    Incompatible {
        /// Stable compatibility category.
        code: v1::ErrorCode,
        /// Actionable diagnostic.
        message: String,
    },
    /// A typed remote operation failure.
    #[error("worker request failed: {code:?}: {message}")]
    Remote {
        /// Stable protocol category.
        code: v1::ErrorCode,
        /// Actionable diagnostic.
        message: String,
    },
    /// The worker disconnected before completing the request.
    #[error("worker disconnected before completing the request")]
    Disconnected,
    /// The worker returned an unexpected response shape.
    #[error("worker returned an unexpected response")]
    UnexpectedResponse,
    /// Other clients are still using the shared worker.
    #[error("worker shutdown requires {remaining_clients} other connected client(s) to disconnect")]
    ShutdownBlocked {
        /// Number of other connections that must close before retrying.
        remaining_clients: u32,
    },
    /// The local endpoint is not protected for the current user.
    #[error("worker endpoint is not protected for the current user")]
    InsecureEndpoint,
    /// The worker advertised invalid or unsafe resource limits.
    #[error("worker advertised invalid resource limits: {0}")]
    InvalidNegotiatedLimits(#[from] NegotiatedLimitsError),
    /// The worker selected a protocol version outside the caller's range.
    #[error("worker selected a protocol version outside the requested range")]
    InvalidNegotiatedVersion,
    /// The configured secret file was malformed or unsafe.
    #[error("invalid worker secret file")]
    InvalidSecretFile,
}

enum ConnectorSource {
    Provisioned(LaunchSecret),
    Desktop {
        runtime_directory: PathBuf,
        executable: PathBuf,
        idle_timeout: Duration,
    },
}

/// Discovers or starts the one per-user worker on demand.
pub struct WorkerConnector {
    endpoint: Endpoint,
    source: ConnectorSource,
}

impl WorkerConnector {
    /// Uses an administrator-provisioned endpoint and secret without spawning.
    #[must_use]
    pub fn provisioned(endpoint: Endpoint, secret: LaunchSecret) -> Self {
        Self {
            endpoint,
            source: ConnectorSource::Provisioned(secret),
        }
    }

    /// Creates an on-demand desktop launcher.
    #[must_use]
    pub fn desktop(runtime_directory: &Path, executable: &Path) -> Self {
        Self {
            endpoint: Endpoint::for_runtime_directory(runtime_directory),
            source: ConnectorSource::Desktop {
                runtime_directory: runtime_directory.to_owned(),
                executable: executable.to_owned(),
                idle_timeout: Duration::from_secs(30),
            },
        }
    }

    /// Overrides the launched worker's last-client idle timeout.
    #[must_use]
    pub fn with_idle_timeout(mut self, timeout: Duration) -> Self {
        if let ConnectorSource::Desktop { idle_timeout, .. } = &mut self.source {
            *idle_timeout = timeout;
        }
        self
    }

    /// Connects to a provisioned worker or discovers/starts the desktop worker.
    ///
    /// Concurrent callers serialize launch through an owner-only filesystem
    /// lock and re-check discovery after acquiring it.
    ///
    /// # Errors
    ///
    /// Returns a typed launch, transport, authentication, or protocol error.
    pub async fn connect(&self) -> Result<WorkerClient, ClientError> {
        match &self.source {
            ConnectorSource::Provisioned(secret) => {
                WorkerClient::connect(&self.endpoint, secret.clone()).await
            }
            ConnectorSource::Desktop {
                runtime_directory,
                executable,
                idle_timeout,
            } => {
                ensure_runtime_directory(runtime_directory)?;
                let secret_path = runtime_directory.join("launch.secret");
                if let Some(client) = discover_desktop_worker(&self.endpoint, &secret_path).await? {
                    return Ok(client);
                }

                let lock_path = runtime_directory.join("launch.lock");
                let lock = std::fs::OpenOptions::new()
                    .create(true)
                    .read(true)
                    .write(true)
                    .truncate(false)
                    .open(lock_path)?;
                secure_file(&lock)?;
                let lock = tokio::task::spawn_blocking(move || {
                    fs2::FileExt::lock_exclusive(&lock)?;
                    Ok::<_, io::Error>(lock)
                })
                .await
                .map_err(|error| io::Error::other(error.to_string()))??;

                if let Some(client) = discover_desktop_worker(&self.endpoint, &secret_path).await? {
                    drop(lock);
                    return Ok(client);
                }

                let secret = LaunchSecret::generate();
                write_secret_file(&secret_path, &secret)?;
                let child = std::process::Command::new(executable)
                    .arg("--runtime-dir")
                    .arg(runtime_directory)
                    .arg("--idle-timeout-ms")
                    .arg(idle_timeout.as_millis().to_string())
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()?;
                spawn_child_reaper(child);

                let mut last_error = None;
                for _ in 0..200 {
                    match WorkerClient::connect(&self.endpoint, secret.clone()).await {
                        Ok(client) => {
                            drop(lock);
                            return Ok(client);
                        }
                        Err(error) if endpoint_is_absent_or_stale(&error) => {
                            last_error = Some(error);
                        }
                        Err(error) => {
                            drop(lock);
                            return Err(error);
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                drop(lock);
                Err(last_error.unwrap_or(ClientError::Disconnected))
            }
        }
    }
}

fn spawn_child_reaper(mut child: std::process::Child) {
    let task = tokio::task::spawn_blocking(move || {
        let _wait_result = child.wait();
    });
    drop(task);
}

async fn discover_desktop_worker(
    endpoint: &Endpoint,
    secret_path: &Path,
) -> Result<Option<WorkerClient>, ClientError> {
    match read_secret_file(secret_path) {
        Ok(secret) => match WorkerClient::connect(endpoint, secret).await {
            Ok(client) => Ok(Some(client)),
            Err(error) if endpoint_is_absent_or_stale(&error) => Ok(None),
            Err(error) => Err(error),
        },
        Err(ClientError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
            match connect_local(endpoint).await {
                Ok(_) => Err(ClientError::InvalidSecretFile),
                Err(error) if endpoint_is_absent_or_stale(&error) => Ok(None),
                Err(error) => Err(error),
            }
        }
        Err(ClientError::InvalidSecretFile) => match connect_local(endpoint).await {
            Ok(_) => Err(ClientError::InvalidSecretFile),
            Err(error) if endpoint_is_absent_or_stale(&error) => Ok(None),
            Err(error) => Err(error),
        },
        Err(error) => Err(error),
    }
}

fn endpoint_is_absent_or_stale(error: &ClientError) -> bool {
    match error {
        ClientError::Io(error) | ClientError::Frame(FrameError::Io(error)) => {
            io_error_is_absent_or_stale(error)
        }
        _ => false,
    }
}

fn io_error_is_absent_or_stale(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound
            | io::ErrorKind::ConnectionRefused
            | io::ErrorKind::AddrNotAvailable
    )
}

type BoxedIo = Box<dyn LocalIo>;

trait LocalIo: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T> LocalIo for T where T: AsyncRead + AsyncWrite + Unpin + Send {}

struct ConnectionWriter {
    transport: AsyncMutex<tokio::io::WriteHalf<BoxedIo>>,
    disconnected: CancellationToken,
}

struct ClientInner {
    writer: AsyncMutex<tokio::io::WriteHalf<BoxedIo>>,
    pending: Mutex<HashMap<u64, PendingResponse>>,
    next_id: AtomicU64,
    limits: RwLock<ProtocolLimits>,
    handles: AtomicUsize,
    close: CancellationToken,
}

#[derive(Clone)]
struct PendingResponse {
    sender: mpsc::Sender<v1::ServerFrame>,
    completed: Arc<AtomicBool>,
}

struct PendingRequest {
    inner: Arc<ClientInner>,
    correlation_id: u64,
    cancellation: Option<PendingCancellation>,
    completed: Arc<AtomicBool>,
}

struct PendingCancellation {
    session: v1::SessionContext,
    request_id: String,
}

impl PendingRequest {
    fn completed(&mut self) {
        self.completed.store(true, Ordering::Release);
        self.cancellation = None;
    }
}

impl Drop for PendingRequest {
    fn drop(&mut self) {
        self.inner
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.correlation_id);
        let Some(cancellation) = self.cancellation.take() else {
            return;
        };
        if self.completed.load(Ordering::Acquire) {
            return;
        }
        spawn_best_effort_cancellation(Arc::clone(&self.inner), cancellation);
    }
}

impl ClientInner {
    fn limits(&self) -> ProtocolLimits {
        *self
            .limits
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Reusable multiplexed client for one authenticated worker session.
pub struct WorkerClient {
    inner: Arc<ClientInner>,
    session: v1::SessionContext,
    protocol_version: u32,
}

/// A host-facing semantic result detached from protobuf DTOs.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchResult {
    /// Opaque document identifier.
    pub document_id: String,
    /// Deterministic fake relevance score.
    pub score: f64,
    /// Structured document metadata.
    pub metadata: BTreeMap<String, String>,
    /// Bounded matching excerpt.
    pub excerpt: String,
}

/// Stable concept-folder query carried over the authenticated worker channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConceptFolderQuery {
    /// Vocabulary owning the selected concepts.
    pub vocabulary_id: String,
    /// Pre-expanded stable concept URIs.
    pub concept_uris: Vec<String>,
    /// Optional enrolled-root scope.
    pub root_id: Option<String>,
    /// Optional workspace scope.
    pub workspace_id: Option<String>,
    /// Whether currently unavailable occurrences remain visible.
    pub include_unavailable: bool,
    /// Stable paging offset.
    pub offset: u64,
}

/// Tenant and library ownership for a host-provided ingestion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestionScope {
    tenant_id: String,
    library_id: String,
}

impl IngestionScope {
    /// Creates an opaque tenant/library scope.
    pub fn new(tenant_id: impl Into<String>, library_id: impl Into<String>) -> Self {
        Self {
            tenant_id: tenant_id.into(),
            library_id: library_id.into(),
        }
    }
}

/// Host-facing ingestion status detached from protobuf DTOs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestionJobStatus {
    /// Opaque job identifier.
    pub job_id: String,
    /// Opaque document identifier.
    pub document_id: String,
    /// Current lifecycle state.
    pub state: IngestionState,
}

/// One validated, path-free ingestion request handed to a worker backend.
#[derive(Debug, Clone)]
pub struct WorkerIngestionInput {
    /// Stable request/job identity.
    pub job_id: String,
    /// Tenant boundary.
    pub tenant_id: String,
    /// Enrolled library.
    pub library_id: String,
    /// Opaque document identity.
    pub document_id: String,
    /// Trusted media type.
    pub media_type: String,
    /// Structured host metadata.
    pub metadata: BTreeMap<String, String>,
    /// Bounded source bytes.
    pub content: Vec<u8>,
}

/// Durable worker-job snapshot returned through the protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerIngestionJob {
    /// Opaque document identity.
    pub document_id: String,
    /// Current lifecycle state.
    pub state: IngestionState,
    /// Stable detailed phase.
    pub phase: String,
    /// Completed work units.
    pub completed: u64,
    /// Total known work units.
    pub total: u64,
    /// Sanitized failure, if any.
    pub error: Option<String>,
}

/// Injectable durable ingestion implementation used by the local IPC server.
pub trait WorkerIngestionBackend: Send + Sync {
    /// Durably queues path-free content and returns its stable job identity.
    fn enqueue(
        &self,
        input: WorkerIngestionInput,
        cancellation: CancellationToken,
    ) -> Result<String, String>;

    /// Reads a job only within its authenticated tenant/library scope.
    fn job(
        &self,
        tenant_id: &str,
        library_id: &str,
        job_id: &str,
    ) -> Result<Option<WorkerIngestionJob>, String>;
}

/// One path-free semantic query handed to the worker retrieval backend.
#[derive(Debug, Clone)]
pub struct WorkerQueryInput {
    /// Tenant boundary.
    pub tenant_id: String,
    /// Enrolled library.
    pub library_id: String,
    /// User query text.
    pub query: String,
    /// Stable concept-folder selection, when this is not a dense text query.
    pub concept_query: Option<ConceptFolderQuery>,
    /// Maximum file-primary results.
    pub maximum_results: u32,
}

/// Injectable dense retrieval implementation used by the local IPC server.
pub trait WorkerQueryBackend: Send + Sync {
    /// Executes one bounded, tenant-scoped query.
    fn query(
        &self,
        input: WorkerQueryInput,
        cancellation: &CancellationToken,
    ) -> Result<Vec<SearchResult>, String>;
}

/// Host-facing ingestion lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestionState {
    /// Waiting for execution.
    Pending,
    /// Currently executing.
    Running,
    /// Finished successfully.
    Completed,
    /// Finished with an error.
    Failed,
    /// Cancelled before completion.
    Cancelled,
}

/// Host-facing worker health.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerHealth {
    /// Ready for work.
    Serving,
    /// Alive but temporarily degraded.
    Degraded,
    /// Finishing active atomic work before exit.
    Draining,
}

/// Host-facing scoped progress event detached from protobuf DTOs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerProgressEvent {
    /// Opaque job identifier.
    pub operation_id: String,
    /// Stable phase name.
    pub phase: String,
}

impl Clone for WorkerClient {
    fn clone(&self) -> Self {
        self.inner.handles.fetch_add(1, Ordering::Relaxed);
        Self {
            inner: Arc::clone(&self.inner),
            session: self.session.clone(),
            protocol_version: self.protocol_version,
        }
    }
}

impl Drop for WorkerClient {
    fn drop(&mut self) {
        if self.inner.handles.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.inner.close.cancel();
        }
    }
}

impl fmt::Debug for WorkerClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WorkerClient")
            .field("protocol_version", &self.protocol_version)
            .field("session", &"[REDACTED]")
            .finish()
    }
}

impl WorkerClient {
    /// Connects, negotiates version one, and authenticates a reusable session.
    ///
    /// # Errors
    ///
    /// Returns typed local transport, compatibility, or authentication errors.
    pub async fn connect(endpoint: &Endpoint, secret: LaunchSecret) -> Result<Self, ClientError> {
        Self::connect_with_versions(endpoint, secret, 1, 1).await
    }

    /// Connects with an explicit inclusive caller protocol range.
    ///
    /// # Errors
    ///
    /// Returns typed local transport, compatibility, or authentication errors.
    pub async fn connect_with_versions(
        endpoint: &Endpoint,
        secret: LaunchSecret,
        minimum_version: u32,
        maximum_version: u32,
    ) -> Result<Self, ClientError> {
        let stream = connect_local(endpoint).await?;
        let (mut reader, writer) = tokio::io::split(stream);
        let inner = Arc::new(ClientInner {
            writer: AsyncMutex::new(writer),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            limits: RwLock::new(ProtocolLimits::default()),
            handles: AtomicUsize::new(1),
            close: CancellationToken::new(),
        });
        let reader_inner = Arc::clone(&inner);
        tokio::spawn(async move {
            loop {
                let frame = tokio::select! {
                    () = reader_inner.close.cancelled() => break,
                    result = read_frame::<_, v1::ServerFrame>(
                        &mut reader,
                        reader_inner.limits().max_message_bytes(),
                    ) => match result {
                        Ok(frame) => frame,
                        Err(_) => break,
                    }
                };
                let pending = reader_inner
                    .pending
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .get(&frame.correlation_id)
                    .cloned();
                if let Some(pending) = pending {
                    if matches!(
                        &frame.payload,
                        Some(
                            v1::server_frame::Payload::StreamEnd(_)
                                | v1::server_frame::Payload::Error(_)
                        )
                    ) {
                        pending.completed.store(true, Ordering::Release);
                    }
                    let correlation_id = frame.correlation_id;
                    if pending.sender.send(frame).await.is_err() {
                        reader_inner
                            .pending
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .remove(&correlation_id);
                    }
                }
            }
            reader_inner
                .pending
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clear();
        });
        let mut client = Self {
            inner,
            session: v1::SessionContext::default(),
            protocol_version: 0,
        };
        let negotiated = client
            .unary(v1::client_frame::Payload::Negotiate(v1::NegotiateRequest {
                minimum_version,
                maximum_version,
                requested_capabilities: all_capabilities(),
            }))
            .await?;
        let v1::server_frame::Payload::Negotiated(response) = negotiated else {
            return Err(ClientError::UnexpectedResponse);
        };
        if response.selected_version < minimum_version
            || response.selected_version > maximum_version
        {
            return Err(ClientError::InvalidNegotiatedVersion);
        }
        client.protocol_version = response.selected_version;
        let limits = response
            .limits
            .as_ref()
            .ok_or(ClientError::UnexpectedResponse)
            .and_then(|limits| ProtocolLimits::from_negotiated(limits).map_err(Into::into))?;
        *client
            .inner
            .limits
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = limits;
        let opened = client
            .unary(v1::client_frame::Payload::OpenSession(
                v1::OpenSessionRequest {
                    authentication_token: secret.as_bytes().to_vec(),
                },
            ))
            .await?;
        let v1::server_frame::Payload::SessionOpened(response) = opened else {
            return Err(ClientError::UnexpectedResponse);
        };
        client.session = v1::SessionContext {
            session_id: response.session_id,
            session_token: response.session_token,
        };
        Ok(client)
    }

    /// Returns the negotiated protocol version.
    #[must_use]
    pub const fn protocol_version(&self) -> u32 {
        self.protocol_version
    }

    /// Reads authenticated worker health.
    ///
    /// # Errors
    ///
    /// Returns a typed transport or remote protocol failure.
    pub async fn health(&self) -> Result<WorkerHealth, ClientError> {
        let payload = self
            .unary(v1::client_frame::Payload::Health(v1::HealthRequest {
                session: Some(self.session.clone()),
            }))
            .await?;
        match payload {
            v1::server_frame::Payload::Health(response) => {
                match v1::HealthStatus::try_from(response.status)
                    .unwrap_or(v1::HealthStatus::Unspecified)
                {
                    v1::HealthStatus::Serving => Ok(WorkerHealth::Serving),
                    v1::HealthStatus::Degraded => Ok(WorkerHealth::Degraded),
                    v1::HealthStatus::Draining => Ok(WorkerHealth::Draining),
                    v1::HealthStatus::Unspecified => Err(ClientError::UnexpectedResponse),
                }
            }
            _ => Err(ClientError::UnexpectedResponse),
        }
    }

    /// Streams one provider-neutral document into the fake ingestion engine.
    ///
    /// # Errors
    ///
    /// Returns a typed transport, authentication, cancellation, or limit error.
    pub async fn ingest(
        &self,
        request_id: &str,
        scope: IngestionScope,
        document_id: &str,
        metadata: BTreeMap<String, String>,
        media_type: &str,
        content: Vec<u8>,
    ) -> Result<String, ClientError> {
        if u64::try_from(content.len()).unwrap_or(u64::MAX) > self.inner.limits().max_stream_bytes()
        {
            return Err(ClientError::Remote {
                code: v1::ErrorCode::LimitExceeded,
                message: "ingestion stream exceeds its negotiated limit".to_owned(),
            });
        }
        let deadline = deadline_after(self.inner.limits().deadline(DeadlineKind::Stream));
        let correlation_id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let request_id = request_id.to_owned();
        let (mut receiver, mut pending_request) = self.register(
            correlation_id,
            Some(PendingCancellation {
                session: self.session.clone(),
                request_id: request_id.clone(),
            }),
        );
        let scope = Some(v1::ResourceScope {
            tenant_id: scope.tenant_id,
            library_id: scope.library_id,
        });
        self.send(
            correlation_id,
            v1::client_frame::Payload::Ingestion(v1::IngestionRequest {
                session: Some(self.session.clone()),
                scope: scope.clone(),
                request_id: request_id.clone(),
                payload: Some(v1::ingestion_request::Payload::Start(v1::IngestionStart {
                    document_id: document_id.to_owned(),
                    metadata: metadata
                        .into_iter()
                        .map(|(key, value)| v1::MetadataEntry { key, value })
                        .collect(),
                    media_type: media_type.to_owned(),
                    expected_content_bytes: u64::try_from(content.len()).unwrap_or(u64::MAX),
                })),
            }),
            deadline,
        )
        .await?;
        let chunk_size = self
            .inner
            .limits()
            .max_message_bytes()
            .saturating_sub(1024)
            .max(1);
        if content.is_empty() {
            self.send_ingestion_chunk(
                correlation_id,
                scope,
                request_id,
                v1::FileContentChunk {
                    sequence: 0,
                    content: Vec::new(),
                    end_of_stream: true,
                },
                deadline,
            )
            .await?;
        } else {
            let chunk_count = content.len().div_ceil(chunk_size);
            for (sequence, chunk) in content.chunks(chunk_size).enumerate() {
                self.send_ingestion_chunk(
                    correlation_id,
                    scope.clone(),
                    request_id.clone(),
                    v1::FileContentChunk {
                        sequence: u64::try_from(sequence).unwrap_or(u64::MAX),
                        content: chunk.to_vec(),
                        end_of_stream: sequence + 1 == chunk_count,
                    },
                    deadline,
                )
                .await?;
            }
        }
        let responses = self
            .collect(
                correlation_id,
                &mut receiver,
                deadline,
                &mut pending_request,
            )
            .await?;
        match responses.as_slice() {
            [v1::server_frame::Payload::IngestionAccepted(response)] => Ok(response.job_id.clone()),
            _ => Err(ClientError::UnexpectedResponse),
        }
    }

    /// Executes a scoped query and collects its bounded result stream.
    ///
    /// # Errors
    ///
    /// Returns a typed transport, authentication, cancellation, or limit error.
    pub async fn query(
        &self,
        tenant_id: &str,
        library_id: &str,
        query: &str,
        maximum_results: u32,
    ) -> Result<Vec<SearchResult>, ClientError> {
        let request_id = format!(
            "query-{}",
            self.inner.next_id.fetch_add(1, Ordering::Relaxed)
        );
        self.query_with_request_id(&request_id, tenant_id, library_id, query, maximum_results)
            .await
    }

    /// Executes a scoped query with a caller-selected cancellation identifier.
    ///
    /// # Errors
    ///
    /// Returns a typed transport, authentication, cancellation, or limit error.
    pub async fn query_with_request_id(
        &self,
        request_id: &str,
        tenant_id: &str,
        library_id: &str,
        query: &str,
        maximum_results: u32,
    ) -> Result<Vec<SearchResult>, ClientError> {
        let payloads = self
            .request(
                v1::client_frame::Payload::Query(v1::QueryRequest {
                    session: Some(self.session.clone()),
                    scope: Some(v1::ResourceScope {
                        tenant_id: tenant_id.to_owned(),
                        library_id: library_id.to_owned(),
                    }),
                    request_id: request_id.to_owned(),
                    query: query.to_owned(),
                    maximum_results,
                    concept_query: None,
                }),
                DeadlineKind::Stream,
            )
            .await?;
        Ok(payloads
            .into_iter()
            .filter_map(|payload| match payload {
                v1::server_frame::Payload::QueryEvent(v1::QueryEvent {
                    payload: Some(v1::query_event::Payload::Result(result)),
                }) => Some(SearchResult {
                    document_id: result.document_id,
                    score: result.score,
                    metadata: result
                        .metadata
                        .into_iter()
                        .map(|entry| (entry.key, entry.value))
                        .collect(),
                    excerpt: result.excerpt,
                }),
                _ => None,
            })
            .collect())
    }

    /// Executes a stable concept-folder query without embedding query text.
    pub async fn query_concepts_with_request_id(
        &self,
        request_id: &str,
        tenant_id: &str,
        library_id: &str,
        query: ConceptFolderQuery,
        maximum_results: u32,
    ) -> Result<Vec<SearchResult>, ClientError> {
        let payloads = self
            .request(
                v1::client_frame::Payload::Query(v1::QueryRequest {
                    session: Some(self.session.clone()),
                    scope: Some(v1::ResourceScope {
                        tenant_id: tenant_id.to_owned(),
                        library_id: library_id.to_owned(),
                    }),
                    request_id: request_id.to_owned(),
                    query: String::new(),
                    maximum_results,
                    concept_query: Some(v1::ConceptQuery {
                        vocabulary_id: query.vocabulary_id,
                        concept_uris: query.concept_uris,
                        root_id: query.root_id,
                        workspace_id: query.workspace_id,
                        include_unavailable: query.include_unavailable,
                        offset: query.offset,
                    }),
                }),
                DeadlineKind::Stream,
            )
            .await?;
        Ok(payloads
            .into_iter()
            .filter_map(|payload| match payload {
                v1::server_frame::Payload::QueryEvent(v1::QueryEvent {
                    payload: Some(v1::query_event::Payload::Result(result)),
                }) => Some(SearchResult {
                    document_id: result.document_id,
                    score: result.score,
                    metadata: result
                        .metadata
                        .into_iter()
                        .map(|entry| (entry.key, entry.value))
                        .collect(),
                    excerpt: result.excerpt,
                }),
                _ => None,
            })
            .collect())
    }

    /// Requests prompt cancellation of an operation owned by this session.
    ///
    /// # Errors
    ///
    /// Returns a typed transport or remote protocol failure.
    pub async fn cancel(&self, request_id: &str) -> Result<bool, ClientError> {
        let payload = self
            .unary(v1::client_frame::Payload::Cancel(v1::CancelRequest {
                session: Some(self.session.clone()),
                request_id: request_id.to_owned(),
            }))
            .await?;
        match payload {
            v1::server_frame::Payload::Cancelled(response) => Ok(response.accepted),
            _ => Err(ClientError::UnexpectedResponse),
        }
    }

    /// Reads one scoped ingestion job.
    ///
    /// # Errors
    ///
    /// Returns a typed transport, authentication, or scope failure.
    pub async fn ingestion_job(
        &self,
        tenant_id: &str,
        library_id: &str,
        job_id: &str,
    ) -> Result<IngestionJobStatus, ClientError> {
        let payload = self
            .unary(v1::client_frame::Payload::GetIngestionJob(
                v1::IngestionJobRequest {
                    session: Some(self.session.clone()),
                    scope: Some(v1::ResourceScope {
                        tenant_id: tenant_id.to_owned(),
                        library_id: library_id.to_owned(),
                    }),
                    job_id: job_id.to_owned(),
                },
            ))
            .await?;
        match payload {
            v1::server_frame::Payload::IngestionJob(job) => Ok(IngestionJobStatus {
                job_id: job.job_id,
                document_id: job.document_id,
                state: match v1::JobState::try_from(job.state).unwrap_or(v1::JobState::Unspecified)
                {
                    v1::JobState::Pending => IngestionState::Pending,
                    v1::JobState::Running => IngestionState::Running,
                    v1::JobState::Completed => IngestionState::Completed,
                    v1::JobState::Failed => IngestionState::Failed,
                    v1::JobState::Cancelled => IngestionState::Cancelled,
                    v1::JobState::Unspecified => return Err(ClientError::UnexpectedResponse),
                },
            }),
            _ => Err(ClientError::UnexpectedResponse),
        }
    }

    /// Reads a finite, scoped snapshot of current worker events.
    ///
    /// # Errors
    ///
    /// Returns a typed transport, authentication, or scope failure.
    pub async fn events_snapshot(
        &self,
        tenant_id: &str,
        library_id: &str,
    ) -> Result<Vec<WorkerProgressEvent>, ClientError> {
        let payloads = self
            .request(
                v1::client_frame::Payload::Events(v1::EventSubscription {
                    session: Some(self.session.clone()),
                    scope: Some(v1::ResourceScope {
                        tenant_id: tenant_id.to_owned(),
                        library_id: library_id.to_owned(),
                    }),
                }),
                DeadlineKind::Stream,
            )
            .await?;
        Ok(payloads
            .into_iter()
            .filter_map(|payload| match payload {
                v1::server_frame::Payload::WorkerEvent(v1::WorkerEvent {
                    payload: Some(v1::worker_event::Payload::Progress(progress)),
                }) => Some(WorkerProgressEvent {
                    operation_id: progress.operation_id,
                    phase: progress.phase,
                }),
                _ => None,
            })
            .collect())
    }

    /// Requests graceful worker shutdown.
    ///
    /// # Errors
    ///
    /// Returns a typed transport or remote protocol failure.
    pub async fn shutdown(&self, grace: Duration) -> Result<(), ClientError> {
        let payload = self
            .unary(v1::client_frame::Payload::Shutdown(v1::ShutdownRequest {
                session: Some(self.session.clone()),
                grace_period_ms: u64::try_from(grace.as_millis()).unwrap_or(u64::MAX),
            }))
            .await?;
        if matches!(
            &payload,
            v1::server_frame::Payload::ShutdownStarted(v1::ShutdownResponse { draining: true, .. })
        ) {
            return Ok(());
        }
        if let v1::server_frame::Payload::ShutdownStarted(v1::ShutdownResponse {
            draining: false,
            remaining_clients,
        }) = payload
        {
            return Err(ClientError::ShutdownBlocked { remaining_clients });
        }
        Err(ClientError::UnexpectedResponse)
    }

    async fn unary(
        &self,
        payload: v1::client_frame::Payload,
    ) -> Result<v1::server_frame::Payload, ClientError> {
        let mut responses = self.request(payload, DeadlineKind::Request).await?;
        if responses.len() == 1 {
            return Ok(responses.remove(0));
        }
        Err(ClientError::UnexpectedResponse)
    }

    async fn request(
        &self,
        payload: v1::client_frame::Payload,
        deadline_kind: DeadlineKind,
    ) -> Result<Vec<v1::server_frame::Payload>, ClientError> {
        let deadline = deadline_after(self.inner.limits().deadline(deadline_kind));
        let correlation_id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let cancellation = match &payload {
            v1::client_frame::Payload::Query(request) => Some(PendingCancellation {
                session: self.session.clone(),
                request_id: request.request_id.clone(),
            }),
            _ => None,
        };
        let (mut receiver, mut pending_request) = self.register(correlation_id, cancellation);
        self.send(correlation_id, payload, deadline).await?;
        self.collect(
            correlation_id,
            &mut receiver,
            deadline,
            &mut pending_request,
        )
        .await
    }

    fn register(
        &self,
        correlation_id: u64,
        cancellation: Option<PendingCancellation>,
    ) -> (mpsc::Receiver<v1::ServerFrame>, PendingRequest) {
        const RESPONSE_BUFFER_FRAMES: usize = 16;
        let (sender, receiver) = mpsc::channel(RESPONSE_BUFFER_FRAMES);
        let completed = Arc::new(AtomicBool::new(false));
        self.inner
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                correlation_id,
                PendingResponse {
                    sender,
                    completed: Arc::clone(&completed),
                },
            );
        (
            receiver,
            PendingRequest {
                inner: Arc::clone(&self.inner),
                correlation_id,
                cancellation,
                completed,
            },
        )
    }

    async fn send(
        &self,
        correlation_id: u64,
        payload: v1::client_frame::Payload,
        deadline: tokio::time::Instant,
    ) -> Result<(), ClientError> {
        let frame = v1::ClientFrame {
            correlation_id,
            payload: Some(payload),
        };
        let write = async {
            let mut writer = self.inner.writer.lock().await;
            write_frame(
                &mut *writer,
                &frame,
                self.inner.limits().max_message_bytes(),
            )
            .await
        };
        let result = tokio::select! {
            () = self.inner.close.cancelled() => Err(ClientError::Disconnected),
            result = tokio::time::timeout_at(deadline, write) => match result {
                Ok(result) => result.map_err(ClientError::from),
                Err(_) => Err(ClientError::Remote {
                    code: v1::ErrorCode::DeadlineExceeded,
                    message: "worker request deadline exceeded while writing".to_owned(),
                }),
            },
        };
        if result.is_err() {
            self.remove_pending(correlation_id);
            self.inner.close.cancel();
        }
        result
    }

    async fn send_ingestion_chunk(
        &self,
        correlation_id: u64,
        scope: Option<v1::ResourceScope>,
        request_id: String,
        chunk: v1::FileContentChunk,
        deadline: tokio::time::Instant,
    ) -> Result<(), ClientError> {
        self.send(
            correlation_id,
            v1::client_frame::Payload::Ingestion(v1::IngestionRequest {
                session: Some(self.session.clone()),
                scope,
                request_id,
                payload: Some(v1::ingestion_request::Payload::Chunk(chunk)),
            }),
            deadline,
        )
        .await
    }

    async fn collect(
        &self,
        correlation_id: u64,
        receiver: &mut mpsc::Receiver<v1::ServerFrame>,
        deadline: tokio::time::Instant,
        pending_request: &mut PendingRequest,
    ) -> Result<Vec<v1::server_frame::Payload>, ClientError> {
        let mut payloads = Vec::new();
        let mut budget = StreamBudget::new(self.inner.limits().max_stream_bytes());
        loop {
            let frame = tokio::select! {
                () = self.inner.close.cancelled() => {
                    self.remove_pending(correlation_id);
                    return Err(ClientError::Disconnected);
                }
                result = tokio::time::timeout_at(deadline, receiver.recv()) => match result {
                    Ok(Some(frame)) => frame,
                    Ok(None) => break,
                    Err(_) => {
                        self.remove_pending(correlation_id);
                        return Err(ClientError::Remote {
                            code: v1::ErrorCode::DeadlineExceeded,
                            message: "worker response deadline exceeded".to_owned(),
                        });
                    }
                },
            };
            if !matches!(
                &frame.payload,
                Some(v1::server_frame::Payload::StreamEnd(_) | v1::server_frame::Payload::Error(_))
            ) && budget
                .consume(u64::try_from(frame.encoded_len()).unwrap_or(u64::MAX))
                .is_err()
            {
                self.remove_pending(correlation_id);
                self.inner.close.cancel();
                return Err(ClientError::Remote {
                    code: v1::ErrorCode::LimitExceeded,
                    message: "worker response stream exceeded its negotiated limit".to_owned(),
                });
            }
            match frame.payload {
                Some(v1::server_frame::Payload::StreamEnd(_)) => {
                    self.remove_pending(correlation_id);
                    pending_request.completed();
                    return Ok(payloads);
                }
                Some(v1::server_frame::Payload::Error(error)) => {
                    self.remove_pending(correlation_id);
                    pending_request.completed();
                    return Err(map_remote_error(error));
                }
                Some(payload) => payloads.push(payload),
                None => {}
            }
        }

        self.remove_pending(correlation_id);
        Err(ClientError::Disconnected)
    }

    fn remove_pending(&self, correlation_id: u64) {
        self.inner
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&correlation_id);
    }
}

fn deadline_after(duration: Duration) -> tokio::time::Instant {
    let now = tokio::time::Instant::now();
    now.checked_add(duration).unwrap_or(now)
}

fn spawn_best_effort_cancellation(inner: Arc<ClientInner>, cancellation: PendingCancellation) {
    if inner.close.is_cancelled() {
        return;
    }
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        return;
    };
    drop(runtime.spawn(async move {
        let correlation_id = inner.next_id.fetch_add(1, Ordering::Relaxed);
        let frame = v1::ClientFrame {
            correlation_id,
            payload: Some(v1::client_frame::Payload::Cancel(v1::CancelRequest {
                session: Some(cancellation.session),
                request_id: cancellation.request_id,
            })),
        };
        let deadline = deadline_after(inner.limits().deadline(DeadlineKind::Request));
        let write = async {
            let mut writer = inner.writer.lock().await;
            write_frame(&mut *writer, &frame, inner.limits().max_message_bytes()).await
        };
        tokio::select! {
            () = inner.close.cancelled() => {}
            _ = tokio::time::timeout_at(deadline, write) => {}
        }
    }));
}

fn map_remote_error(error: v1::ProtocolError) -> ClientError {
    let code = v1::ErrorCode::try_from(error.code).unwrap_or(v1::ErrorCode::Internal);
    match code {
        v1::ErrorCode::Unauthenticated => ClientError::Unauthenticated,
        v1::ErrorCode::ClientUpdateRequired | v1::ErrorCode::WorkerUpdateRequired => {
            ClientError::Incompatible {
                code,
                message: error.message,
            }
        }
        _ => ClientError::Remote {
            code,
            message: error.message,
        },
    }
}

fn all_capabilities() -> Vec<i32> {
    [
        v1::Capability::Ingestion,
        v1::Capability::Query,
        v1::Capability::Events,
        v1::Capability::Cancellation,
        v1::Capability::GracefulShutdown,
    ]
    .into_iter()
    .map(i32::from)
    .collect()
}

struct RuntimeState {
    config: WorkerConfig,
    draining: AtomicBool,
    shutdown: CancellationToken,
    next_session: AtomicU64,
    next_job: AtomicU64,
    ingestion_backend: Option<Arc<dyn WorkerIngestionBackend>>,
    query_backend: Option<Arc<dyn WorkerQueryBackend>>,
    documents: Mutex<Vec<Arc<Document>>>,
    jobs: Mutex<Arc<HashMap<String, Arc<Job>>>>,
    connections: watch::Sender<usize>,
    authenticated_connections: AtomicUsize,
    session_gate: Mutex<()>,
    active_atomic: AtomicUsize,
    atomic_finished: Notify,
    concurrency: Arc<Semaphore>,
}

#[derive(Clone)]
struct Document {
    tenant_id: String,
    library_id: String,
    document_id: String,
    metadata: BTreeMap<String, String>,
    content: Vec<u8>,
}

struct Job {
    tenant_id: String,
    library_id: String,
    document_id: String,
}

struct PendingIngestion {
    request_id: String,
    scope: v1::ResourceScope,
    start: v1::IngestionStart,
    content: Vec<u8>,
    budget: StreamBudget,
    next_sequence: u64,
    deadline: tokio::time::Instant,
    cancellation: CancellationToken,
    stream_finished: CancellationToken,
    _permit: OwnedSemaphorePermit,
}

struct ConnectionState {
    ingestions: AsyncMutex<HashMap<u64, PendingIngestion>>,
    cancellations: Mutex<HashMap<String, CancellationToken>>,
    session: Mutex<Option<AuthenticatedSession>>,
    negotiated: AtomicBool,
    authenticated: AtomicBool,
    disconnected: CancellationToken,
}

struct AuthenticatedSession {
    session_id: String,
    token: Vec<u8>,
    expires_at: Option<tokio::time::Instant>,
}

struct PendingIngestionExpiration {
    correlation_id: u64,
    request_id: String,
    deadline: tokio::time::Instant,
    cancellation: CancellationToken,
    stream_finished: CancellationToken,
    state: Arc<RuntimeState>,
    connection: Arc<ConnectionState>,
    writer: Arc<ConnectionWriter>,
}

enum StreamPreparationError {
    Cancelled,
    Deadline,
    Limit(LimitError),
    Backend,
}

enum QueryRegistration {
    Unregistered,
    Registered(CancellationToken),
    Duplicate,
}

struct QueryRegistrationGuard {
    connection: Arc<ConnectionState>,
    request_id: String,
}

impl Drop for QueryRegistrationGuard {
    fn drop(&mut self) {
        self.connection
            .cancellations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.request_id);
    }
}

/// Isolated worker serving only platform-local IPC.
pub struct WorkerServer {
    state: Arc<RuntimeState>,
}

impl WorkerServer {
    /// Creates a worker using the deterministic in-memory fake engine.
    #[must_use]
    pub fn new(config: WorkerConfig) -> Self {
        Self::with_optional_backends(config, None, None)
    }

    /// Creates a worker backed by the durable ingestion pipeline.
    #[must_use]
    pub fn with_ingestion_backend(
        config: WorkerConfig,
        ingestion_backend: Arc<dyn WorkerIngestionBackend>,
    ) -> Self {
        Self::with_optional_backends(config, Some(ingestion_backend), None)
    }

    /// Creates a worker backed by dense retrieval while retaining fake
    /// ingestion, primarily for independently testing query deployments.
    #[must_use]
    pub fn with_query_backend(
        config: WorkerConfig,
        query_backend: Arc<dyn WorkerQueryBackend>,
    ) -> Self {
        Self::with_optional_backends(config, None, Some(query_backend))
    }

    /// Creates a worker backed by durable ingestion and dense retrieval.
    #[must_use]
    pub fn with_backends(
        config: WorkerConfig,
        ingestion_backend: Arc<dyn WorkerIngestionBackend>,
        query_backend: Arc<dyn WorkerQueryBackend>,
    ) -> Self {
        Self::with_optional_backends(config, Some(ingestion_backend), Some(query_backend))
    }

    fn with_optional_backends(
        config: WorkerConfig,
        ingestion_backend: Option<Arc<dyn WorkerIngestionBackend>>,
        query_backend: Option<Arc<dyn WorkerQueryBackend>>,
    ) -> Self {
        let concurrency = Arc::new(Semaphore::new(config.limits.max_concurrent_requests()));
        let (connections, _) = watch::channel(0);
        Self {
            state: Arc::new(RuntimeState {
                config,
                draining: AtomicBool::new(false),
                shutdown: CancellationToken::new(),
                next_session: AtomicU64::new(1),
                next_job: AtomicU64::new(1),
                ingestion_backend,
                query_backend,
                documents: Mutex::new(Vec::new()),
                jobs: Mutex::new(Arc::new(HashMap::new())),
                connections,
                authenticated_connections: AtomicUsize::new(0),
                session_gate: Mutex::new(()),
                active_atomic: AtomicUsize::new(0),
                atomic_finished: Notify::new(),
                concurrency,
            }),
        }
    }

    /// Binds the local endpoint and serves until graceful or idle shutdown.
    ///
    /// # Errors
    ///
    /// Returns a typed endpoint, permission, or transport failure.
    pub async fn run(self) -> Result<(), ServerError> {
        run_local(self.state).await
    }
}

async fn serve_connection(stream: BoxedIo, state: Arc<RuntimeState>) -> Result<(), ServerError> {
    struct ConnectionGuard {
        state: Arc<RuntimeState>,
        connection: Arc<ConnectionState>,
    }
    impl Drop for ConnectionGuard {
        fn drop(&mut self) {
            let _session_gate = self
                .state
                .session_gate
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            cancel_connection_work(&self.connection);
            if self
                .connection
                .session
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
                .is_some()
            {
                self.state
                    .authenticated_connections
                    .fetch_sub(1, Ordering::AcqRel);
            }
            self.state
                .connections
                .send_modify(|count| *count = count.saturating_sub(1));
        }
    }

    let (mut reader, writer) = tokio::io::split(stream);
    let connection = Arc::new(ConnectionState {
        ingestions: AsyncMutex::new(HashMap::new()),
        cancellations: Mutex::new(HashMap::new()),
        session: Mutex::new(None),
        negotiated: AtomicBool::new(false),
        authenticated: AtomicBool::new(false),
        disconnected: CancellationToken::new(),
    });
    let writer = Arc::new(ConnectionWriter {
        transport: AsyncMutex::new(writer),
        disconnected: connection.disconnected.clone(),
    });
    let _guard = ConnectionGuard {
        state: Arc::clone(&state),
        connection: Arc::clone(&connection),
    };
    let handshake_deadline = deadline_after(
        state
            .config
            .limits
            .deadline(fm_semantic_protocol::DeadlineKind::Request),
    );
    loop {
        let read =
            read_frame::<_, v1::ClientFrame>(&mut reader, state.config.limits.max_message_bytes());
        let result = tokio::select! {
            () = state.shutdown.cancelled() => return Ok(()),
            () = connection.disconnected.cancelled() => return Ok(()),
            result = async {
                if connection.authenticated.load(Ordering::Acquire) {
                    tokio::time::timeout(
                        state
                            .config
                            .limits
                            .deadline(fm_semantic_protocol::DeadlineKind::Stream),
                        read,
                    )
                    .await
                } else {
                    tokio::time::timeout_at(handshake_deadline, read).await
                }
            } => result,
        };
        let frame = match result {
            Err(_) => {
                cancel_connection_work(&connection);
                return Ok(());
            }
            Ok(Ok(frame)) => frame,
            Ok(Err(FrameError::Io(error)))
                if matches!(
                    error.kind(),
                    io::ErrorKind::UnexpectedEof
                        | io::ErrorKind::ConnectionReset
                        | io::ErrorKind::BrokenPipe
                ) =>
            {
                cancel_connection_work(&connection);
                return Ok(());
            }
            Ok(Err(error)) => return Err(error.into()),
        };
        let connection_state = Arc::clone(&state);
        let request_connection = Arc::clone(&connection);
        let connection_writer = Arc::clone(&writer);
        if matches!(
            &frame.payload,
            Some(
                v1::client_frame::Payload::Ingestion(_)
                    | v1::client_frame::Payload::Cancel(_)
                    | v1::client_frame::Payload::Shutdown(_)
            )
        ) || matches!(
            &frame.payload,
            Some(v1::client_frame::Payload::Query(_))
                if state.draining.load(Ordering::Acquire)
        ) {
            handle_frame(
                frame,
                connection_state,
                request_connection,
                connection_writer,
                None,
                QueryRegistration::Unregistered,
            )
            .await;
        } else {
            let Ok(permit) = Arc::clone(&state.concurrency).try_acquire_owned() else {
                send_error(
                    &writer,
                    frame.correlation_id,
                    protocol_error(
                        v1::ErrorCode::LimitExceeded,
                        "concurrent request limit reached",
                    ),
                    state.config.limits,
                )
                .await;
                continue;
            };
            let query_registration = match &frame.payload {
                Some(v1::client_frame::Payload::Query(request))
                    if session_matches(&connection, request.session.as_ref())
                        && !state.draining.load(Ordering::Acquire)
                        && !connection.disconnected.is_cancelled()
                        && validate_query(request).is_ok() =>
                {
                    match register_cancellation(&connection, &request.request_id) {
                        Some(cancellation) => QueryRegistration::Registered(cancellation),
                        None => QueryRegistration::Duplicate,
                    }
                }
                _ => QueryRegistration::Unregistered,
            };
            tokio::spawn(async move {
                handle_frame(
                    frame,
                    connection_state,
                    request_connection,
                    connection_writer,
                    Some(permit),
                    query_registration,
                )
                .await;
            });
        }
    }
}

async fn handle_frame(
    frame: v1::ClientFrame,
    state: Arc<RuntimeState>,
    connection: Arc<ConnectionState>,
    writer: Arc<ConnectionWriter>,
    request_permit: Option<OwnedSemaphorePermit>,
    query_registration: QueryRegistration,
) {
    let _request_permit = request_permit;
    let correlation_id = frame.correlation_id;
    let payload = match frame.payload {
        Some(v1::client_frame::Payload::Negotiate(request)) => {
            match Negotiator::new(
                state.config.versions,
                [
                    v1::Capability::Ingestion,
                    v1::Capability::Query,
                    v1::Capability::Events,
                    v1::Capability::Cancellation,
                    v1::Capability::GracefulShutdown,
                ],
                state.config.limits,
            )
            .negotiate(&request)
            {
                Ok(response) => {
                    connection.negotiated.store(true, Ordering::Release);
                    v1::server_frame::Payload::Negotiated(response)
                }
                Err(error) => {
                    send_error(
                        &writer,
                        correlation_id,
                        negotiation_error(error),
                        state.config.limits,
                    )
                    .await;
                    return;
                }
            }
        }
        Some(v1::client_frame::Payload::OpenSession(request)) => {
            if !connection.negotiated.load(Ordering::Acquire) {
                send_error(
                    &writer,
                    correlation_id,
                    protocol_error(
                        v1::ErrorCode::InvalidRequest,
                        "protocol negotiation is required",
                    ),
                    state.config.limits,
                )
                .await;
                return;
            }
            if !secret_matches(&state.config.secret, &request.authentication_token) {
                send_error(
                    &writer,
                    correlation_id,
                    protocol_error(v1::ErrorCode::Unauthenticated, "authentication rejected"),
                    state.config.limits,
                )
                .await;
                return;
            }
            let session_gate = state
                .session_gate
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if state.draining.load(Ordering::Acquire) || connection.disconnected.is_cancelled() {
                drop(session_gate);
                send_error(
                    &writer,
                    correlation_id,
                    protocol_error(v1::ErrorCode::Unavailable, "worker is draining"),
                    state.config.limits,
                )
                .await;
                return;
            }
            let id = state.next_session.fetch_add(1, Ordering::Relaxed);
            let session_id = format!("session-{id}");
            let token = LaunchSecret::generate().as_bytes().to_vec();
            let expires_at = tokio::time::Instant::now().checked_add(state.config.session_lifetime);
            let mut session = connection
                .session
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if session.is_none() {
                state
                    .authenticated_connections
                    .fetch_add(1, Ordering::AcqRel);
            }
            *session = Some(AuthenticatedSession {
                session_id: session_id.clone(),
                token: token.clone(),
                expires_at,
            });
            drop(session);
            connection.authenticated.store(true, Ordering::Release);
            drop(session_gate);
            v1::server_frame::Payload::SessionOpened(v1::OpenSessionResponse {
                session_id,
                session_token: token,
                expires_at_unix_ms: now_millis().saturating_add(
                    u64::try_from(state.config.session_lifetime.as_millis()).unwrap_or(u64::MAX),
                ),
            })
        }
        Some(v1::client_frame::Payload::Health(request)) => {
            if !session_matches(&connection, request.session.as_ref()) {
                send_error(
                    &writer,
                    correlation_id,
                    protocol_error(v1::ErrorCode::Unauthenticated, "session rejected"),
                    state.config.limits,
                )
                .await;
                return;
            }
            v1::server_frame::Payload::Health(v1::HealthResponse {
                status: if state.draining.load(Ordering::Acquire) {
                    v1::HealthStatus::Draining.into()
                } else {
                    v1::HealthStatus::Serving.into()
                },
                detail: String::new(),
                protocol_version: 1,
                active_requests: u32::try_from(
                    state.config.limits.max_concurrent_requests()
                        - state.concurrency.available_permits(),
                )
                .unwrap_or(u32::MAX),
            })
        }
        Some(v1::client_frame::Payload::Shutdown(request)) => {
            if !session_matches(&connection, request.session.as_ref()) {
                send_error(
                    &writer,
                    correlation_id,
                    protocol_error(v1::ErrorCode::Unauthenticated, "session rejected"),
                    state.config.limits,
                )
                .await;
                return;
            }
            let session_gate = state
                .session_gate
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let connected_clients = state.authenticated_connections.load(Ordering::Acquire);
            if connected_clients > 1 {
                drop(session_gate);
                send_payload(
                    &writer,
                    correlation_id,
                    v1::server_frame::Payload::ShutdownStarted(v1::ShutdownResponse {
                        draining: false,
                        remaining_clients: u32::try_from(connected_clients - 1).unwrap_or(u32::MAX),
                    }),
                    state.config.limits,
                )
                .await;
                return;
            }
            state.draining.store(true, Ordering::Release);
            drop(session_gate);
            let payload = v1::server_frame::Payload::ShutdownStarted(v1::ShutdownResponse {
                draining: true,
                remaining_clients: 0,
            });
            send_payload(&writer, correlation_id, payload, state.config.limits).await;
            let requested = Duration::from_millis(request.grace_period_ms);
            let grace = requested.min(
                state
                    .config
                    .limits
                    .deadline(fm_semantic_protocol::DeadlineKind::Shutdown),
            );
            tokio::spawn(async move {
                let wait_for_atomic = async {
                    loop {
                        let finished = state.atomic_finished.notified();
                        if state.active_atomic.load(Ordering::Acquire) == 0 {
                            break;
                        }
                        finished.await;
                    }
                };
                let _ = tokio::time::timeout(grace, wait_for_atomic).await;
                state.shutdown.cancel();
            });
            return;
        }
        Some(v1::client_frame::Payload::Ingestion(request)) => {
            if !session_matches(&connection, request.session.as_ref()) {
                send_error(
                    &writer,
                    correlation_id,
                    protocol_error(v1::ErrorCode::Unauthenticated, "session rejected"),
                    state.config.limits,
                )
                .await;
                return;
            }
            if state.draining.load(Ordering::Acquire) {
                send_error(
                    &writer,
                    correlation_id,
                    protocol_error(v1::ErrorCode::Unavailable, "worker is draining"),
                    state.config.limits,
                )
                .await;
                return;
            }
            if let Err(error) = validate_ingestion_with_limits(&request, state.config.limits) {
                if matches!(
                    &request.payload,
                    Some(v1::ingestion_request::Payload::Chunk(_))
                ) {
                    let pending = connection.ingestions.lock().await.remove(&correlation_id);
                    discard_pending_ingestion(&connection, pending.as_ref());
                }
                send_error(
                    &writer,
                    correlation_id,
                    request_validation_error(error),
                    state.config.limits,
                )
                .await;
                return;
            }
            match request.payload {
                Some(v1::ingestion_request::Payload::Start(start)) => {
                    if connection
                        .ingestions
                        .lock()
                        .await
                        .contains_key(&correlation_id)
                    {
                        send_error(
                            &writer,
                            correlation_id,
                            protocol_error(
                                v1::ErrorCode::InvalidRequest,
                                "ingestion already started",
                            ),
                            state.config.limits,
                        )
                        .await;
                        return;
                    }
                    let Some(scope) = request.scope else {
                        send_error(
                            &writer,
                            correlation_id,
                            protocol_error(v1::ErrorCode::InvalidRequest, "scope is required"),
                            state.config.limits,
                        )
                        .await;
                        return;
                    };
                    if start.expected_content_bytes > state.config.limits.max_stream_bytes() {
                        send_error(
                            &writer,
                            correlation_id,
                            protocol_error(
                                v1::ErrorCode::LimitExceeded,
                                "declared content exceeds stream limit",
                            ),
                            state.config.limits,
                        )
                        .await;
                        return;
                    }
                    let Ok(permit) = Arc::clone(&state.concurrency).try_acquire_owned() else {
                        send_error(
                            &writer,
                            correlation_id,
                            protocol_error(
                                v1::ErrorCode::LimitExceeded,
                                "concurrent request limit reached",
                            ),
                            state.config.limits,
                        )
                        .await;
                        return;
                    };
                    let Some(cancellation) =
                        register_cancellation(&connection, &request.request_id)
                    else {
                        send_error(
                            &writer,
                            correlation_id,
                            protocol_error(
                                v1::ErrorCode::InvalidRequest,
                                "request identifier is already active",
                            ),
                            state.config.limits,
                        )
                        .await;
                        return;
                    };
                    let stream_finished = CancellationToken::new();
                    let request_id = request.request_id;
                    let ingestion_timeout = state.config.effective_ingestion_timeout();
                    let deadline = deadline_after(ingestion_timeout);
                    connection.ingestions.lock().await.insert(
                        correlation_id,
                        PendingIngestion {
                            request_id: request_id.clone(),
                            scope,
                            start,
                            content: Vec::new(),
                            budget: StreamBudget::new(state.config.limits.max_stream_bytes()),
                            next_sequence: 0,
                            deadline,
                            cancellation: cancellation.clone(),
                            stream_finished: stream_finished.clone(),
                            _permit: permit,
                        },
                    );
                    tokio::spawn(expire_pending_ingestion(PendingIngestionExpiration {
                        correlation_id,
                        request_id,
                        deadline,
                        cancellation,
                        stream_finished,
                        state: Arc::clone(&state),
                        connection: Arc::clone(&connection),
                        writer: Arc::clone(&writer),
                    }));
                    return;
                }
                Some(v1::ingestion_request::Payload::Chunk(chunk)) => {
                    let mut ingestions = connection.ingestions.lock().await;
                    let Some(pending) = ingestions.get_mut(&correlation_id) else {
                        drop(ingestions);
                        send_error(
                            &writer,
                            correlation_id,
                            protocol_error(v1::ErrorCode::InvalidRequest, "ingestion has no start"),
                            state.config.limits,
                        )
                        .await;
                        return;
                    };
                    if request.request_id != pending.request_id {
                        let pending = ingestions.remove(&correlation_id);
                        drop(ingestions);
                        discard_pending_ingestion(&connection, pending.as_ref());
                        send_error(
                            &writer,
                            correlation_id,
                            protocol_error(
                                v1::ErrorCode::InvalidRequest,
                                "ingestion request identifier changed",
                            ),
                            state.config.limits,
                        )
                        .await;
                        return;
                    }
                    let scope_matches = request.scope.as_ref().is_some_and(|scope| {
                        scope.tenant_id == pending.scope.tenant_id
                            && scope.library_id == pending.scope.library_id
                    });
                    let next_sequence = pending.next_sequence.checked_add(1);
                    if !scope_matches
                        || chunk.sequence != pending.next_sequence
                        || next_sequence.is_none()
                    {
                        let pending = ingestions.remove(&correlation_id);
                        drop(ingestions);
                        discard_pending_ingestion(&connection, pending.as_ref());
                        send_error(
                            &writer,
                            correlation_id,
                            protocol_error(
                                v1::ErrorCode::InvalidRequest,
                                "ingestion chunk sequence or scope changed",
                            ),
                            state.config.limits,
                        )
                        .await;
                        return;
                    }
                    if pending
                        .budget
                        .consume(u64::try_from(chunk.content.len()).unwrap_or(u64::MAX))
                        .is_err()
                    {
                        let pending = ingestions.remove(&correlation_id);
                        drop(ingestions);
                        discard_pending_ingestion(&connection, pending.as_ref());
                        send_error(
                            &writer,
                            correlation_id,
                            protocol_error(
                                v1::ErrorCode::LimitExceeded,
                                "ingestion stream limit exceeded",
                            ),
                            state.config.limits,
                        )
                        .await;
                        return;
                    }
                    pending.next_sequence = next_sequence.unwrap_or(pending.next_sequence);
                    pending.content.extend_from_slice(&chunk.content);
                    if !chunk.end_of_stream {
                        return;
                    }
                    let Some(pending) = ingestions.remove(&correlation_id) else {
                        return;
                    };
                    pending.stream_finished.cancel();
                    drop(ingestions);
                    state.active_atomic.fetch_add(1, Ordering::AcqRel);
                    tokio::spawn(complete_ingestion(
                        pending,
                        correlation_id,
                        Arc::clone(&state),
                        Arc::clone(&connection),
                        Arc::clone(&writer),
                    ));
                    return;
                }
                None => {
                    send_error(
                        &writer,
                        correlation_id,
                        protocol_error(
                            v1::ErrorCode::InvalidRequest,
                            "ingestion payload is required",
                        ),
                        state.config.limits,
                    )
                    .await;
                    return;
                }
            }
        }
        Some(v1::client_frame::Payload::Query(request)) => {
            let mut registration_guard =
                matches!(&query_registration, QueryRegistration::Registered(_)).then(|| {
                    QueryRegistrationGuard {
                        connection: Arc::clone(&connection),
                        request_id: request.request_id.clone(),
                    }
                });
            let deadline = deadline_after(
                state
                    .config
                    .limits
                    .deadline(fm_semantic_protocol::DeadlineKind::Stream),
            );
            if !session_matches(&connection, request.session.as_ref()) {
                send_error(
                    &writer,
                    correlation_id,
                    protocol_error(v1::ErrorCode::Unauthenticated, "session rejected"),
                    state.config.limits,
                )
                .await;
                return;
            }
            if state.draining.load(Ordering::Acquire) {
                send_error(
                    &writer,
                    correlation_id,
                    protocol_error(v1::ErrorCode::Unavailable, "worker is draining"),
                    state.config.limits,
                )
                .await;
                return;
            }
            if let Err(error) = validate_query(&request) {
                send_error(
                    &writer,
                    correlation_id,
                    request_validation_error(error),
                    state.config.limits,
                )
                .await;
                return;
            }
            let Some(scope) = request.scope else {
                send_error(
                    &writer,
                    correlation_id,
                    protocol_error(v1::ErrorCode::InvalidRequest, "scope is required"),
                    state.config.limits,
                )
                .await;
                return;
            };
            if connection.disconnected.is_cancelled() {
                return;
            }
            let cancellation = match query_registration {
                QueryRegistration::Registered(cancellation) => cancellation,
                QueryRegistration::Duplicate => {
                    send_error(
                        &writer,
                        correlation_id,
                        protocol_error(
                            v1::ErrorCode::InvalidRequest,
                            "request identifier is already active",
                        ),
                        state.config.limits,
                    )
                    .await;
                    return;
                }
                QueryRegistration::Unregistered => {
                    let Some(cancellation) =
                        register_cancellation(&connection, &request.request_id)
                    else {
                        send_error(
                            &writer,
                            correlation_id,
                            protocol_error(
                                v1::ErrorCode::InvalidRequest,
                                "request identifier is already active",
                            ),
                            state.config.limits,
                        )
                        .await;
                        return;
                    };
                    registration_guard = Some(QueryRegistrationGuard {
                        connection: Arc::clone(&connection),
                        request_id: request.request_id.clone(),
                    });
                    cancellation
                }
            };
            let documents = state
                .documents
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            let query_backend = state.query_backend.clone();
            let query_text = request.query.clone();
            let concept_query = request.concept_query.map(|query| ConceptFolderQuery {
                vocabulary_id: query.vocabulary_id,
                concept_uris: query.concept_uris,
                root_id: query.root_id,
                workspace_id: query.workspace_id,
                include_unavailable: query.include_unavailable,
                offset: query.offset,
            });
            let maximum_results = request.maximum_results;
            let backend_scope = scope.clone();
            let scan_cancellation = cancellation.clone();
            let scan_disconnected = connection.disconnected.clone();
            let limits = state.config.limits;
            let test_scan_delay = state.config.test_query_scan_delay;
            let prepared = tokio::task::spawn_blocking(move || {
                if let Some(backend) = query_backend {
                    let results = backend
                        .query(
                            WorkerQueryInput {
                                tenant_id: backend_scope.tenant_id,
                                library_id: backend_scope.library_id,
                                query: query_text,
                                concept_query,
                                maximum_results,
                            },
                            &scan_cancellation,
                        )
                        .map_err(|_| StreamPreparationError::Backend)?;
                    return bounded_result_frames(
                        correlation_id,
                        results,
                        &scan_cancellation,
                        &scan_disconnected,
                        limits,
                        deadline,
                    );
                }
                bounded_query_frames(QueryScan {
                    documents,
                    correlation_id,
                    scope,
                    query: query_text,
                    maximum_results,
                    cancellation: scan_cancellation,
                    disconnected: scan_disconnected,
                    limits,
                    deadline,
                    test_scan_delay,
                })
            })
            .await;
            let preparation = match prepared {
                Ok(preparation) => preparation,
                Err(_) => {
                    send_error(
                        &writer,
                        correlation_id,
                        protocol_error(v1::ErrorCode::Internal, "query execution failed"),
                        state.config.limits,
                    )
                    .await;
                    return;
                }
            };
            let frames = match preparation {
                Ok(frames) => frames,
                Err(StreamPreparationError::Cancelled) => {
                    send_error(
                        &writer,
                        correlation_id,
                        protocol_error(v1::ErrorCode::Cancelled, "query cancelled"),
                        state.config.limits,
                    )
                    .await;
                    return;
                }
                Err(StreamPreparationError::Deadline) => {
                    send_error(
                        &writer,
                        correlation_id,
                        protocol_error(
                            v1::ErrorCode::DeadlineExceeded,
                            "query stream deadline exceeded",
                        ),
                        state.config.limits,
                    )
                    .await;
                    return;
                }
                Err(StreamPreparationError::Limit(error)) => {
                    send_error(
                        &writer,
                        correlation_id,
                        protocol_error(
                            v1::ErrorCode::LimitExceeded,
                            format!("query result {error}"),
                        ),
                        state.config.limits,
                    )
                    .await;
                    return;
                }
                Err(StreamPreparationError::Backend) => {
                    send_error(
                        &writer,
                        correlation_id,
                        protocol_error(v1::ErrorCode::Internal, "semantic query failed"),
                        state.config.limits,
                    )
                    .await;
                    return;
                }
            };
            send_query_results(
                &writer,
                correlation_id,
                frames,
                cancellation,
                state.config.limits,
                deadline,
                state.config.test_query_write_delay,
            )
            .await;
            drop(registration_guard);
            return;
        }
        Some(v1::client_frame::Payload::GetIngestionJob(request)) => {
            if !session_matches(&connection, request.session.as_ref()) {
                send_error(
                    &writer,
                    correlation_id,
                    protocol_error(v1::ErrorCode::Unauthenticated, "session rejected"),
                    state.config.limits,
                )
                .await;
                return;
            }
            let Some(scope) = request.scope else {
                send_error(
                    &writer,
                    correlation_id,
                    protocol_error(v1::ErrorCode::InvalidRequest, "scope is required"),
                    state.config.limits,
                )
                .await;
                return;
            };
            if let Some(backend) = &state.ingestion_backend {
                let job = match backend.job(&scope.tenant_id, &scope.library_id, &request.job_id) {
                    Ok(Some(job)) => job,
                    Ok(None) => {
                        send_error(
                            &writer,
                            correlation_id,
                            protocol_error(
                                v1::ErrorCode::InvalidRequest,
                                "ingestion job was not found",
                            ),
                            state.config.limits,
                        )
                        .await;
                        return;
                    }
                    Err(error) => {
                        send_error(
                            &writer,
                            correlation_id,
                            protocol_error(v1::ErrorCode::Internal, error),
                            state.config.limits,
                        )
                        .await;
                        return;
                    }
                };
                let state_value = match job.state {
                    IngestionState::Pending => v1::JobState::Pending,
                    IngestionState::Running => v1::JobState::Running,
                    IngestionState::Completed => v1::JobState::Completed,
                    IngestionState::Failed => v1::JobState::Failed,
                    IngestionState::Cancelled => v1::JobState::Cancelled,
                };
                let payload = v1::server_frame::Payload::IngestionJob(v1::IngestionJob {
                    job_id: request.job_id.clone(),
                    document_id: job.document_id,
                    state: state_value.into(),
                    progress: Some(v1::Progress {
                        operation_id: request.job_id,
                        phase: job.phase,
                        completed_units: job.completed,
                        total_units: job.total,
                    }),
                    error: job
                        .error
                        .map(|error| protocol_error(v1::ErrorCode::Internal, error)),
                });
                send_payload_until(
                    &writer,
                    correlation_id,
                    payload,
                    state.config.limits,
                    deadline_after(
                        state
                            .config
                            .limits
                            .deadline(fm_semantic_protocol::DeadlineKind::Request),
                    ),
                    None,
                )
                .await;
                return;
            }
            let job = state
                .jobs
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get(&request.job_id)
                .filter(|job| {
                    job.tenant_id == scope.tenant_id && job.library_id == scope.library_id
                })
                .map(|job| (job.document_id.clone(), request.job_id.clone()));
            let Some((document_id, job_id)) = job else {
                send_error(
                    &writer,
                    correlation_id,
                    protocol_error(v1::ErrorCode::InvalidRequest, "ingestion job was not found"),
                    state.config.limits,
                )
                .await;
                return;
            };
            v1::server_frame::Payload::IngestionJob(v1::IngestionJob {
                job_id: job_id.clone(),
                document_id,
                state: v1::JobState::Completed.into(),
                progress: Some(v1::Progress {
                    operation_id: job_id,
                    phase: "completed".to_owned(),
                    completed_units: 1,
                    total_units: 1,
                }),
                error: None,
            })
        }
        Some(v1::client_frame::Payload::Events(request)) => {
            let deadline = deadline_after(
                state
                    .config
                    .limits
                    .deadline(fm_semantic_protocol::DeadlineKind::Stream),
            );
            if !session_matches(&connection, request.session.as_ref()) {
                send_error(
                    &writer,
                    correlation_id,
                    protocol_error(v1::ErrorCode::Unauthenticated, "session rejected"),
                    state.config.limits,
                )
                .await;
                return;
            }
            let Some(scope) = request.scope else {
                send_error(
                    &writer,
                    correlation_id,
                    protocol_error(v1::ErrorCode::InvalidRequest, "scope is required"),
                    state.config.limits,
                )
                .await;
                return;
            };
            let jobs = Arc::clone(
                &state
                    .jobs
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            );
            let event_scan = EventScan {
                jobs,
                correlation_id,
                scope,
                disconnected: connection.disconnected.clone(),
                limits: state.config.limits,
                deadline,
                test_scan_delay: state.config.test_event_scan_delay,
                test_phase_bytes: state.config.test_event_phase_bytes,
            };
            let preparation = tokio::time::timeout_at(
                deadline,
                tokio::task::spawn_blocking(move || bounded_event_frames(event_scan)),
            )
            .await;
            let frames = match preparation {
                Ok(Ok(Ok(frames))) => frames,
                Ok(Ok(Err(StreamPreparationError::Cancelled))) => return,
                Ok(Ok(Err(StreamPreparationError::Deadline))) | Err(_) => {
                    send_error(
                        &writer,
                        correlation_id,
                        protocol_error(
                            v1::ErrorCode::DeadlineExceeded,
                            "event stream deadline exceeded",
                        ),
                        state.config.limits,
                    )
                    .await;
                    return;
                }
                Ok(Ok(Err(StreamPreparationError::Limit(error)))) => {
                    send_error(
                        &writer,
                        correlation_id,
                        protocol_error(v1::ErrorCode::LimitExceeded, format!("event {error}")),
                        state.config.limits,
                    )
                    .await;
                    return;
                }
                Ok(Ok(Err(StreamPreparationError::Backend))) => {
                    send_error(
                        &writer,
                        correlation_id,
                        protocol_error(v1::ErrorCode::Internal, "event snapshot failed"),
                        state.config.limits,
                    )
                    .await;
                    return;
                }
                Ok(Err(_)) => {
                    send_error(
                        &writer,
                        correlation_id,
                        protocol_error(v1::ErrorCode::Internal, "event snapshot failed"),
                        state.config.limits,
                    )
                    .await;
                    return;
                }
            };
            send_event_snapshot(
                &writer,
                correlation_id,
                frames,
                state.config.limits,
                deadline,
            )
            .await;
            return;
        }
        Some(v1::client_frame::Payload::Cancel(request)) => {
            if !session_matches(&connection, request.session.as_ref()) {
                send_error(
                    &writer,
                    correlation_id,
                    protocol_error(v1::ErrorCode::Unauthenticated, "session rejected"),
                    state.config.limits,
                )
                .await;
                return;
            }
            let cancellation = connection
                .cancellations
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get(&request.request_id)
                .cloned();
            let accepted = cancellation.is_some();
            if let Some(cancellation) = cancellation {
                cancellation.cancel();
            }
            send_payload(
                &writer,
                correlation_id,
                v1::server_frame::Payload::Cancelled(v1::CancelResponse { accepted }),
                state.config.limits,
            )
            .await;
            return;
        }
        None => {
            send_error(
                &writer,
                correlation_id,
                protocol_error(v1::ErrorCode::InvalidRequest, "unsupported request"),
                state.config.limits,
            )
            .await;
            return;
        }
    };
    send_payload(&writer, correlation_id, payload, state.config.limits).await;
}

fn cancel_connection_work(connection: &ConnectionState) {
    connection.disconnected.cancel();
    connection
        .cancellations
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .drain()
        .for_each(|(_, token)| token.cancel());
}

fn register_cancellation(
    connection: &ConnectionState,
    request_id: &str,
) -> Option<CancellationToken> {
    let mut cancellations = connection
        .cancellations
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if cancellations.contains_key(request_id) {
        return None;
    }
    let cancellation = CancellationToken::new();
    cancellations.insert(request_id.to_owned(), cancellation.clone());
    Some(cancellation)
}

fn discard_pending_ingestion(connection: &ConnectionState, pending: Option<&PendingIngestion>) {
    let Some(pending) = pending else {
        return;
    };
    pending.stream_finished.cancel();
    connection
        .cancellations
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&pending.request_id);
}

async fn expire_pending_ingestion(expiration: PendingIngestionExpiration) {
    let PendingIngestionExpiration {
        correlation_id,
        request_id,
        deadline,
        cancellation,
        stream_finished,
        state,
        connection,
        writer,
    } = expiration;
    enum Expiration {
        Cancelled,
        Deadline,
    }

    let expiration = tokio::select! {
        biased;
        () = connection.disconnected.cancelled() => return,
        () = stream_finished.cancelled() => return,
        () = cancellation.cancelled() => Expiration::Cancelled,
        () = tokio::time::sleep_until(deadline) => Expiration::Deadline,
    };
    let removed = {
        let mut ingestions = connection.ingestions.lock().await;
        if ingestions
            .get(&correlation_id)
            .is_some_and(|pending| pending.request_id == request_id)
        {
            ingestions.remove(&correlation_id)
        } else {
            None
        }
    };
    if removed.is_none() {
        return;
    }
    connection
        .cancellations
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&request_id);
    let (code, message) = match expiration {
        Expiration::Cancelled => (v1::ErrorCode::Cancelled, "ingestion cancelled"),
        Expiration::Deadline => (
            v1::ErrorCode::DeadlineExceeded,
            "ingestion stream deadline exceeded",
        ),
    };
    send_error(
        &writer,
        correlation_id,
        protocol_error(code, message),
        state.config.limits,
    )
    .await;
}

async fn complete_ingestion(
    pending: PendingIngestion,
    correlation_id: u64,
    state: Arc<RuntimeState>,
    connection: Arc<ConnectionState>,
    writer: Arc<ConnectionWriter>,
) {
    struct AtomicGuard(Arc<RuntimeState>);
    impl Drop for AtomicGuard {
        fn drop(&mut self) {
            self.0.active_atomic.fetch_sub(1, Ordering::AcqRel);
            self.0.atomic_finished.notify_waiters();
        }
    }
    let _guard = AtomicGuard(Arc::clone(&state));
    struct CancellationGuard {
        connection: Arc<ConnectionState>,
        request_id: String,
    }
    impl Drop for CancellationGuard {
        fn drop(&mut self) {
            self.connection
                .cancellations
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&self.request_id);
        }
    }
    let _cancellation_guard = CancellationGuard {
        connection: Arc::clone(&connection),
        request_id: pending.request_id.clone(),
    };
    let response_deadline = pending.deadline;
    let response_cancellation = pending.cancellation.clone();

    if pending.content.len() as u64 != pending.start.expected_content_bytes {
        send_error(
            &writer,
            correlation_id,
            protocol_error(
                v1::ErrorCode::InvalidRequest,
                "content length does not match declaration",
            ),
            state.config.limits,
        )
        .await;
        return;
    }
    tokio::select! {
        () = connection.disconnected.cancelled() => return,
        () = tokio::time::sleep_until(pending.deadline) => {
            send_error(
                &writer,
                correlation_id,
                protocol_error(
                    v1::ErrorCode::DeadlineExceeded,
                    "ingestion stream deadline exceeded",
                ),
                state.config.limits,
            ).await;
            return;
        }
        () = pending.cancellation.cancelled() => {
            send_error(
                &writer,
                correlation_id,
                protocol_error(v1::ErrorCode::Cancelled, "ingestion cancelled"),
                state.config.limits,
            ).await;
            return;
        }
        () = tokio::time::sleep(state.config.atomic_ingestion_delay) => {}
    }
    let document_id = pending.start.document_id;
    let metadata = pending
        .start
        .metadata
        .into_iter()
        .map(|entry| (entry.key, entry.value))
        .collect::<BTreeMap<_, _>>();
    if let Some(backend) = &state.ingestion_backend {
        let input = WorkerIngestionInput {
            job_id: pending.request_id,
            tenant_id: pending.scope.tenant_id,
            library_id: pending.scope.library_id,
            document_id,
            media_type: pending.start.media_type,
            metadata,
            content: pending.content,
        };
        let job_id = match backend.enqueue(input, pending.cancellation) {
            Ok(job_id) => job_id,
            Err(error) => {
                send_error(
                    &writer,
                    correlation_id,
                    protocol_error(v1::ErrorCode::Internal, &error),
                    state.config.limits,
                )
                .await;
                return;
            }
        };
        send_payload_until(
            &writer,
            correlation_id,
            v1::server_frame::Payload::IngestionAccepted(v1::IngestionAccepted { job_id }),
            state.config.limits,
            response_deadline,
            Some(&response_cancellation),
        )
        .await;
        return;
    }
    state
        .documents
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(Arc::new(Document {
            tenant_id: pending.scope.tenant_id.clone(),
            library_id: pending.scope.library_id.clone(),
            document_id: document_id.clone(),
            metadata,
            content: pending.content,
        }));
    let job_id = format!("job-{}", state.next_job.fetch_add(1, Ordering::Relaxed));
    {
        let mut jobs = state
            .jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Arc::make_mut(&mut *jobs).insert(
            job_id.clone(),
            Arc::new(Job {
                tenant_id: pending.scope.tenant_id,
                library_id: pending.scope.library_id,
                document_id,
            }),
        );
    }
    send_payload_until(
        &writer,
        correlation_id,
        v1::server_frame::Payload::IngestionAccepted(v1::IngestionAccepted { job_id }),
        state.config.limits,
        response_deadline,
        Some(&response_cancellation),
    )
    .await;
}

async fn send_query_results(
    writer: &ConnectionWriter,
    correlation_id: u64,
    frames: Vec<v1::ServerFrame>,
    cancellation: CancellationToken,
    limits: ProtocolLimits,
    deadline: tokio::time::Instant,
    test_write_delay: Duration,
) {
    let count = u32::try_from(frames.len()).unwrap_or(u32::MAX);
    let mut budget = StreamBudget::new(limits.max_stream_bytes());
    for frame in frames {
        if tokio::time::Instant::now() >= deadline {
            send_error(
                writer,
                correlation_id,
                protocol_error(
                    v1::ErrorCode::DeadlineExceeded,
                    "query stream deadline exceeded",
                ),
                limits,
            )
            .await;
            return;
        }

        tokio::select! {
            () = writer.disconnected.cancelled() => return,
            () = tokio::time::sleep_until(deadline) => {
                send_error(
                    writer,
                    correlation_id,
                    protocol_error(
                        v1::ErrorCode::DeadlineExceeded,
                        "query stream deadline exceeded",
                    ),
                    limits,
                ).await;
                return;
            }
            () = cancellation.cancelled() => {
                send_error(
                    writer,
                    correlation_id,
                    protocol_error(v1::ErrorCode::Cancelled, "query cancelled"),
                    limits,
                ).await;
                return;
            }
            () = tokio::time::sleep(Duration::from_millis(2)) => {}
        }
        if !send_bounded_stream_frame(
            writer,
            &frame,
            &mut budget,
            limits,
            deadline,
            Some(&cancellation),
            test_write_delay,
        )
        .await
        {
            if cancellation.is_cancelled() {
                send_error(
                    writer,
                    correlation_id,
                    protocol_error(v1::ErrorCode::Cancelled, "query cancelled"),
                    limits,
                )
                .await;
            } else if tokio::time::Instant::now() >= deadline {
                send_error(
                    writer,
                    correlation_id,
                    protocol_error(
                        v1::ErrorCode::DeadlineExceeded,
                        "query stream deadline exceeded",
                    ),
                    limits,
                )
                .await;
            }
            return;
        }
    }

    let completed = v1::ServerFrame {
        correlation_id,
        payload: Some(v1::server_frame::Payload::QueryEvent(v1::QueryEvent {
            payload: Some(v1::query_event::Payload::Completed(v1::QueryCompleted {
                result_count: count,
            })),
        })),
    };
    if !send_bounded_stream_frame(
        writer,
        &completed,
        &mut budget,
        limits,
        deadline,
        Some(&cancellation),
        Duration::ZERO,
    )
    .await
    {
        if cancellation.is_cancelled() {
            send_error(
                writer,
                correlation_id,
                protocol_error(v1::ErrorCode::Cancelled, "query cancelled"),
                limits,
            )
            .await;
        } else if tokio::time::Instant::now() >= deadline {
            send_error(
                writer,
                correlation_id,
                protocol_error(
                    v1::ErrorCode::DeadlineExceeded,
                    "query stream deadline exceeded",
                ),
                limits,
            )
            .await;
        }
        return;
    }
    let end = v1::ServerFrame {
        correlation_id,
        payload: Some(v1::server_frame::Payload::StreamEnd(v1::StreamEnd {})),
    };
    let _ = send_frame_until(writer, &end, limits, deadline, None, Duration::ZERO).await;
}

struct QueryScan {
    documents: Vec<Arc<Document>>,
    correlation_id: u64,
    scope: v1::ResourceScope,
    query: String,
    maximum_results: u32,
    cancellation: CancellationToken,
    disconnected: CancellationToken,
    limits: ProtocolLimits,
    deadline: tokio::time::Instant,
    test_scan_delay: Duration,
}

fn bounded_result_frames(
    correlation_id: u64,
    results: Vec<SearchResult>,
    cancellation: &CancellationToken,
    disconnected: &CancellationToken,
    limits: ProtocolLimits,
    deadline: tokio::time::Instant,
) -> Result<Vec<v1::ServerFrame>, StreamPreparationError> {
    let mut budget = StreamBudget::new(limits.max_stream_bytes());
    let mut frames = Vec::with_capacity(results.len());
    for result in results {
        check_query_scan_state(cancellation, disconnected, deadline)?;
        let frame = v1::ServerFrame {
            correlation_id,
            payload: Some(v1::server_frame::Payload::QueryEvent(v1::QueryEvent {
                payload: Some(v1::query_event::Payload::Result(v1::QueryResult {
                    document_id: result.document_id,
                    score: result.score,
                    metadata: result
                        .metadata
                        .into_iter()
                        .map(|(key, value)| v1::MetadataEntry { key, value })
                        .collect(),
                    excerpt: result.excerpt,
                })),
            })),
        };
        limits
            .check_message_bytes(frame.encoded_len())
            .map_err(StreamPreparationError::Limit)?;
        budget
            .consume(u64::try_from(frame.encoded_len()).unwrap_or(u64::MAX))
            .map_err(StreamPreparationError::Limit)?;
        frames.push(frame);
    }
    Ok(frames)
}

fn bounded_query_frames(scan: QueryScan) -> Result<Vec<v1::ServerFrame>, StreamPreparationError> {
    let QueryScan {
        documents,
        correlation_id,
        scope,
        query,
        maximum_results,
        cancellation,
        disconnected,
        limits,
        deadline,
        test_scan_delay,
    } = scan;
    let query = query.to_lowercase();
    let mut budget = StreamBudget::new(limits.max_stream_bytes());
    let mut frames = Vec::new();
    let maximum_results = usize::try_from(maximum_results).unwrap_or(usize::MAX);
    for document in documents {
        check_query_scan_state(&cancellation, &disconnected, deadline)?;
        if frames.len() >= maximum_results {
            break;
        }
        if document.tenant_id != scope.tenant_id || document.library_id != scope.library_id {
            continue;
        }
        delay_query_scan(test_scan_delay, &cancellation, &disconnected, deadline)?;
        let content = String::from_utf8_lossy(&document.content);
        let matches_query = content.to_lowercase().contains(&query);
        check_query_scan_state(&cancellation, &disconnected, deadline)?;
        if !matches_query {
            continue;
        }
        let frame = v1::ServerFrame {
            correlation_id,
            payload: Some(v1::server_frame::Payload::QueryEvent(v1::QueryEvent {
                payload: Some(v1::query_event::Payload::Result(v1::QueryResult {
                    document_id: document.document_id.clone(),
                    score: 1.0,
                    metadata: document
                        .metadata
                        .iter()
                        .map(|(key, value)| v1::MetadataEntry {
                            key: key.clone(),
                            value: value.clone(),
                        })
                        .collect(),
                    excerpt: content.chars().take(256).collect(),
                })),
            })),
        };
        limits
            .check_message_bytes(frame.encoded_len())
            .map_err(StreamPreparationError::Limit)?;
        budget
            .consume(u64::try_from(frame.encoded_len()).unwrap_or(u64::MAX))
            .map_err(StreamPreparationError::Limit)?;
        frames.push(frame);
    }
    Ok(frames)
}

fn check_query_scan_state(
    cancellation: &CancellationToken,
    disconnected: &CancellationToken,
    deadline: tokio::time::Instant,
) -> Result<(), StreamPreparationError> {
    if cancellation.is_cancelled() || disconnected.is_cancelled() {
        return Err(StreamPreparationError::Cancelled);
    }
    if tokio::time::Instant::now() >= deadline {
        return Err(StreamPreparationError::Deadline);
    }
    Ok(())
}

fn delay_query_scan(
    delay: Duration,
    cancellation: &CancellationToken,
    disconnected: &CancellationToken,
    deadline: tokio::time::Instant,
) -> Result<(), StreamPreparationError> {
    let delay_deadline = std::time::Instant::now()
        .checked_add(delay)
        .unwrap_or_else(std::time::Instant::now);
    loop {
        check_query_scan_state(cancellation, disconnected, deadline)?;
        let remaining = delay_deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Ok(());
        }
        std::thread::sleep(remaining.min(Duration::from_millis(5)));
    }
}

async fn send_bounded_stream_frame(
    writer: &ConnectionWriter,
    frame: &v1::ServerFrame,
    budget: &mut StreamBudget,
    limits: ProtocolLimits,
    deadline: tokio::time::Instant,
    cancellation: Option<&CancellationToken>,
    test_write_delay: Duration,
) -> bool {
    if limits.check_message_bytes(frame.encoded_len()).is_err() {
        send_error(
            writer,
            frame.correlation_id,
            protocol_error(
                v1::ErrorCode::LimitExceeded,
                "response message limit exceeded",
            ),
            limits,
        )
        .await;
        return false;
    }
    if budget
        .consume(u64::try_from(frame.encoded_len()).unwrap_or(u64::MAX))
        .is_err()
    {
        send_error(
            writer,
            frame.correlation_id,
            protocol_error(
                v1::ErrorCode::LimitExceeded,
                "response stream limit exceeded",
            ),
            limits,
        )
        .await;
        return false;
    }
    send_frame_until(
        writer,
        frame,
        limits,
        deadline,
        cancellation,
        test_write_delay,
    )
    .await
}

async fn send_event_snapshot(
    writer: &ConnectionWriter,
    correlation_id: u64,
    frames: Vec<v1::ServerFrame>,
    limits: ProtocolLimits,
    deadline: tokio::time::Instant,
) {
    let mut budget = StreamBudget::new(limits.max_stream_bytes());
    for frame in frames {
        if !send_bounded_stream_frame(
            writer,
            &frame,
            &mut budget,
            limits,
            deadline,
            None,
            Duration::ZERO,
        )
        .await
        {
            return;
        }
    }
    let end = v1::ServerFrame {
        correlation_id,
        payload: Some(v1::server_frame::Payload::StreamEnd(v1::StreamEnd {})),
    };
    let _ = send_frame_until(writer, &end, limits, deadline, None, Duration::ZERO).await;
}

struct EventScan {
    jobs: Arc<HashMap<String, Arc<Job>>>,
    correlation_id: u64,
    scope: v1::ResourceScope,
    disconnected: CancellationToken,
    limits: ProtocolLimits,
    deadline: tokio::time::Instant,
    test_scan_delay: Duration,
    test_phase_bytes: usize,
}

fn bounded_event_frames(scan: EventScan) -> Result<Vec<v1::ServerFrame>, StreamPreparationError> {
    let EventScan {
        jobs,
        correlation_id,
        scope,
        disconnected,
        limits,
        deadline,
        test_scan_delay,
        test_phase_bytes,
    } = scan;
    let mut budget = StreamBudget::new(limits.max_stream_bytes());
    let mut frames = Vec::new();
    for (job_id, job) in jobs.iter() {
        check_event_scan_state(&disconnected, deadline)?;
        if job.tenant_id != scope.tenant_id || job.library_id != scope.library_id {
            continue;
        }
        delay_event_scan(test_scan_delay, &disconnected, deadline)?;
        let frame = v1::ServerFrame {
            correlation_id,
            payload: Some(v1::server_frame::Payload::WorkerEvent(v1::WorkerEvent {
                payload: Some(v1::worker_event::Payload::Progress(v1::Progress {
                    operation_id: job_id.clone(),
                    phase: if test_phase_bytes == 0 {
                        "completed".to_owned()
                    } else {
                        "x".repeat(test_phase_bytes)
                    },
                    completed_units: 1,
                    total_units: 1,
                })),
            })),
        };
        limits
            .check_message_bytes(frame.encoded_len())
            .map_err(StreamPreparationError::Limit)?;
        budget
            .consume(u64::try_from(frame.encoded_len()).unwrap_or(u64::MAX))
            .map_err(StreamPreparationError::Limit)?;
        frames.push(frame);
    }
    Ok(frames)
}

fn check_event_scan_state(
    disconnected: &CancellationToken,
    deadline: tokio::time::Instant,
) -> Result<(), StreamPreparationError> {
    if disconnected.is_cancelled() {
        return Err(StreamPreparationError::Cancelled);
    }
    if tokio::time::Instant::now() >= deadline {
        return Err(StreamPreparationError::Deadline);
    }
    Ok(())
}

fn delay_event_scan(
    delay: Duration,
    disconnected: &CancellationToken,
    deadline: tokio::time::Instant,
) -> Result<(), StreamPreparationError> {
    let delay_deadline = std::time::Instant::now()
        .checked_add(delay)
        .unwrap_or_else(std::time::Instant::now);
    loop {
        check_event_scan_state(disconnected, deadline)?;
        let remaining = delay_deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Ok(());
        }
        std::thread::sleep(remaining.min(Duration::from_millis(5)));
    }
}

async fn send_frame_until(
    writer: &ConnectionWriter,
    frame: &v1::ServerFrame,
    limits: ProtocolLimits,
    deadline: tokio::time::Instant,
    cancellation: Option<&CancellationToken>,
    test_write_delay: Duration,
) -> bool {
    enum SendOutcome {
        Written,
        Stopped,
        TransportFailed,
    }

    let cancelled = async {
        if let Some(cancellation) = cancellation {
            cancellation.cancelled().await;
        } else {
            std::future::pending::<()>().await;
        }
    };
    let write = async {
        let mut transport = writer.transport.lock().await;
        if !test_write_delay.is_zero() {
            tokio::time::sleep(test_write_delay).await;
        }
        write_frame(&mut *transport, frame, limits.max_message_bytes()).await
    };
    let outcome = tokio::select! {
        biased;
        () = writer.disconnected.cancelled() => SendOutcome::Stopped,
        () = cancelled => SendOutcome::Stopped,
        () = tokio::time::sleep_until(deadline) => SendOutcome::Stopped,
        result = write => if result.is_ok() {
            SendOutcome::Written
        } else {
            SendOutcome::TransportFailed
        },
    };
    if matches!(outcome, SendOutcome::TransportFailed) {
        writer.disconnected.cancel();
    }
    matches!(outcome, SendOutcome::Written)
}

async fn send_payload(
    writer: &ConnectionWriter,
    correlation_id: u64,
    payload: v1::server_frame::Payload,
    limits: ProtocolLimits,
) {
    let deadline = deadline_after(limits.deadline(fm_semantic_protocol::DeadlineKind::Request));
    send_payload_until(writer, correlation_id, payload, limits, deadline, None).await;
}

async fn send_payload_until(
    writer: &ConnectionWriter,
    correlation_id: u64,
    payload: v1::server_frame::Payload,
    limits: ProtocolLimits,
    deadline: tokio::time::Instant,
    cancellation: Option<&CancellationToken>,
) {
    let response = v1::ServerFrame {
        correlation_id,
        payload: Some(payload),
    };
    if !send_frame_until(
        writer,
        &response,
        limits,
        deadline,
        cancellation,
        Duration::ZERO,
    )
    .await
    {
        return;
    }
    let end = v1::ServerFrame {
        correlation_id,
        payload: Some(v1::server_frame::Payload::StreamEnd(v1::StreamEnd {})),
    };
    let _ = send_frame_until(writer, &end, limits, deadline, cancellation, Duration::ZERO).await;
}

async fn send_error(
    writer: &ConnectionWriter,
    correlation_id: u64,
    error: v1::ProtocolError,
    limits: ProtocolLimits,
) {
    send_payload(
        writer,
        correlation_id,
        v1::server_frame::Payload::Error(error),
        limits,
    )
    .await;
}

fn protocol_error(code: v1::ErrorCode, message: impl Into<String>) -> v1::ProtocolError {
    v1::ProtocolError {
        code: code.into(),
        message: message.into(),
        retryable: matches!(code, v1::ErrorCode::Unavailable),
        required_protocol_version: 0,
    }
}

fn negotiation_error(error: NegotiationError) -> v1::ProtocolError {
    match error {
        NegotiationError::Compatibility(error) => error.to_wire(),
        NegotiationError::InvalidVersionRange(_) => {
            protocol_error(v1::ErrorCode::InvalidRequest, error.to_string())
        }
    }
}

fn request_validation_error(error: RequestValidationError) -> v1::ProtocolError {
    let code = match error {
        RequestValidationError::DeclaredContentTooLarge | RequestValidationError::ChunkTooLarge => {
            v1::ErrorCode::LimitExceeded
        }
        RequestValidationError::MissingSession
        | RequestValidationError::MissingScope
        | RequestValidationError::MissingRequestId
        | RequestValidationError::MissingPayload
        | RequestValidationError::MissingDocumentId
        | RequestValidationError::EmptyQuery
        | RequestValidationError::InvalidConceptQuery
        | RequestValidationError::InvalidMaximumResults => v1::ErrorCode::InvalidRequest,
    };
    protocol_error(code, error.to_string())
}

fn secret_matches(expected: &LaunchSecret, presented: &[u8]) -> bool {
    let Ok(token) = SessionToken::new(expected.as_bytes().to_vec()) else {
        return false;
    };
    let Ok(presented) = SessionToken::new(presented.to_vec()) else {
        return false;
    };
    fm_semantic_protocol::Authenticator::new(token)
        .authenticate(Some(&presented))
        .is_ok()
}

fn session_matches(connection: &ConnectionState, session: Option<&v1::SessionContext>) -> bool {
    let Some(session) = session else {
        return false;
    };
    connection
        .session
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .is_some_and(|expected| {
            expected
                .expires_at
                .is_none_or(|expires_at| tokio::time::Instant::now() < expires_at)
                && expected.session_id == session.session_id
                && constant_time_eq(&expected.token, &session.session_token)
        })
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    for index in 0..left.len().max(right.len()) {
        difference |= usize::from(
            left.get(index).copied().unwrap_or_default()
                ^ right.get(index).copied().unwrap_or_default(),
        );
    }
    difference == 0
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

fn ensure_runtime_directory(directory: &Path) -> Result<(), io::Error> {
    std::fs::create_dir_all(directory)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))?;
        let mode = std::fs::metadata(directory)?.permissions().mode() & 0o777;
        if mode != 0o700 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "worker runtime directory is not owner-only",
            ));
        }
    }
    Ok(())
}

fn secure_file(file: &std::fs::File) -> Result<(), io::Error> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(windows)]
    let _ = file;
    Ok(())
}

fn write_secret_file(path: &Path, secret: &LaunchSecret) -> Result<(), io::Error> {
    use std::io::Write;

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)?;
    secure_file(&file)?;
    file.write_all(secret.as_bytes())?;
    file.sync_all()
}

fn read_secret_file(path: &Path) -> Result<LaunchSecret, ClientError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::metadata(path)?.permissions().mode() & 0o077 != 0 {
            return Err(ClientError::InvalidSecretFile);
        }
    }
    let bytes = std::fs::read(path)?;
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| ClientError::InvalidSecretFile)?;
    Ok(LaunchSecret::from_bytes(bytes))
}

/// Runs the desktop worker from an owner-only per-user runtime directory.
///
/// A lifetime lock guarantees singleton ownership independently of launch
/// coordination.
///
/// # Errors
///
/// Returns a typed lock, secret, endpoint, permission, or transport failure.
pub async fn run_desktop_worker(
    runtime_directory: &Path,
    idle_timeout: Duration,
) -> Result<(), ServerError> {
    ensure_runtime_directory(runtime_directory)?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(runtime_directory.join("worker.lock"))?;
    secure_file(&lock)?;
    fs2::FileExt::try_lock_exclusive(&lock).map_err(|error| {
        if error.kind() == io::ErrorKind::WouldBlock {
            ServerError::AlreadyRunning
        } else {
            ServerError::Io(error)
        }
    })?;
    let pid_path = runtime_directory.join("worker.pid");
    let mut pid_file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&pid_path)?;
    secure_file(&pid_file)?;
    {
        use std::io::Write;
        writeln!(pid_file, "{}", std::process::id())?;
        pid_file.sync_all()?;
    }
    let secret = read_secret_file(&runtime_directory.join("launch.secret")).map_err(|error| {
        ServerError::Io(io::Error::new(
            io::ErrorKind::PermissionDenied,
            error.to_string(),
        ))
    })?;
    let endpoint = Endpoint::for_runtime_directory(runtime_directory);
    let result =
        WorkerServer::new(WorkerConfig::new(endpoint, secret).with_idle_timeout(idle_timeout))
            .run()
            .await;
    let _ = std::fs::remove_file(pid_path);
    drop(lock);
    result
}

#[cfg(unix)]
async fn connect_local(endpoint: &Endpoint) -> Result<BoxedIo, ClientError> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};

    let Endpoint::Unix(path) = endpoint;
    let verify_path = || {
        let metadata = std::fs::symlink_metadata(path).map_err(ClientError::Io)?;
        let current_uid = rustix::process::geteuid().as_raw();
        if !metadata.file_type().is_socket()
            || metadata.uid() != current_uid
            || metadata.mode() & 0o777 != 0o600
        {
            return Err(ClientError::InsecureEndpoint);
        }
        Ok(())
    };
    for attempt in 0..20 {
        match verify_path() {
            Ok(()) => break,
            Err(ClientError::InsecureEndpoint) if attempt < 19 => {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            Err(error) => return Err(error),
        }
    }
    let stream = tokio::net::UnixStream::connect(path)
        .await
        .map_err(ClientError::Io)?;
    verify_path()?;
    if stream
        .peer_cred()
        .map_err(|_| ClientError::InsecureEndpoint)?
        .uid()
        != rustix::process::geteuid().as_raw()
    {
        return Err(ClientError::InsecureEndpoint);
    }
    Ok(Box::new(stream))
}

#[cfg(windows)]
async fn connect_local(endpoint: &Endpoint) -> Result<BoxedIo, ClientError> {
    use interprocess::local_socket::{ConnectOptions, ToFsName};
    use interprocess::os::windows::local_socket::NamedPipe;

    let Endpoint::Windows(name) = endpoint;
    if !is_local_named_pipe_endpoint(name) {
        return Err(ClientError::InsecureEndpoint);
    }
    let name = name
        .as_str()
        .to_fs_name::<NamedPipe>()
        .map_err(ClientError::Io)?;
    let stream = ConnectOptions::new()
        .name(name)
        .connect_tokio()
        .await
        .map_err(ClientError::Io)?;
    verify_current_user_only_pipe(&stream)?;
    Ok(Box::new(stream))
}

#[cfg(windows)]
fn current_user_only_security_descriptor()
-> Result<interprocess::os::windows::security_descriptor::SecurityDescriptor, io::Error> {
    use interprocess::os::windows::security_descriptor::SecurityDescriptor;
    use widestring::U16CString;

    let current_user = windows_permissions::utilities::current_process_sid()?;
    let sddl = owner_only_pipe_sddl(&current_user.to_string())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let sddl = U16CString::from_str(sddl)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    SecurityDescriptor::deserialize(&sddl)
}

#[cfg(windows)]
fn verify_current_user_only_pipe(
    stream: &interprocess::local_socket::tokio::Stream,
) -> Result<(), ClientError> {
    use interprocess::local_socket::tokio::Stream as LocalSocketStream;
    use windows_permissions::constants::{
        AccessRights, AceType, SeObjectType, SecurityInformation,
    };

    let LocalSocketStream::NamedPipe(pipe) = stream;
    let current_user = windows_permissions::utilities::current_process_sid()
        .map_err(|_| ClientError::InsecureEndpoint)?;
    let descriptor = windows_permissions::wrappers::GetSecurityInfo(
        pipe.inner(),
        SeObjectType::SE_KERNEL_OBJECT,
        SecurityInformation::Owner | SecurityInformation::Dacl,
    )
    .map_err(|_| ClientError::InsecureEndpoint)?;
    let dacl = descriptor.dacl().ok_or(ClientError::InsecureEndpoint)?;
    let ace = dacl.get_ace(0).ok_or(ClientError::InsecureEndpoint)?;
    if descriptor.owner() != Some(current_user.as_ref())
        || dacl.len() != 1
        || ace.ace_type() != AceType::ACCESS_ALLOWED_ACE_TYPE
        || ace.sid() != Some(current_user.as_ref())
        || !ace.mask().contains(AccessRights::GenericAll)
    {
        return Err(ClientError::InsecureEndpoint);
    }
    Ok(())
}

#[cfg(unix)]
async fn run_local(state: Arc<RuntimeState>) -> Result<(), ServerError> {
    use std::os::unix::fs::PermissionsExt;

    let Endpoint::Unix(path) = &state.config.endpoint;
    let parent = path
        .parent()
        .ok_or_else(|| ServerError::UnsafeRuntimeDirectory("endpoint has no parent".to_owned()))?;
    std::fs::create_dir_all(parent)?;
    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
    if path.exists() {
        match tokio::net::UnixStream::connect(path).await {
            Ok(_) => {
                return Err(ServerError::Io(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    "worker endpoint is already active",
                )));
            }
            Err(_) => std::fs::remove_file(path)?,
        }
    }
    let listener = tokio::net::UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    let mut connections = state.connections.subscribe();
    loop {
        let connection_count = *connections.borrow_and_update();
        if connection_count == 0 {
            tokio::select! {
                () = state.shutdown.cancelled() => break,
                () = tokio::time::sleep(state.config.idle_timeout) => break,
                accepted = listener.accept() => {
                    let (stream, _) = accepted?;
                    state
                        .connections
                        .send_modify(|count| *count = count.saturating_add(1));
                    let connection_state = Arc::clone(&state);
                    tokio::spawn(async move {
                        let _ = serve_connection(Box::new(stream), connection_state).await;
                    });
                }
            }
        } else {
            tokio::time::sleep(state.config.test_idle_wait_delay).await;
            tokio::select! {
                () = state.shutdown.cancelled() => break,
                changed = connections.changed() => {
                    if changed.is_err() {
                        break;
                    }
                }
                accepted = listener.accept() => {
                    let (stream, _) = accepted?;
                    state
                        .connections
                        .send_modify(|count| *count = count.saturating_add(1));
                    let connection_state = Arc::clone(&state);
                    tokio::spawn(async move {
                        let _ = serve_connection(Box::new(stream), connection_state).await;
                    });
                }
            }
        }
    }
    drop(listener);
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(windows)]
async fn run_local(state: Arc<RuntimeState>) -> Result<(), ServerError> {
    use interprocess::local_socket::traits::tokio::Listener as _;
    use interprocess::local_socket::{ListenerOptions, ToFsName};
    use interprocess::os::windows::local_socket::{ListenerOptionsExt, NamedPipe};

    let Endpoint::Windows(name) = &state.config.endpoint;
    if !is_local_named_pipe_endpoint(name) {
        return Err(ServerError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "worker endpoint must be a local named pipe",
        )));
    }
    let name = name.as_str().to_fs_name::<NamedPipe>()?;
    let listener = ListenerOptions::new()
        .name(name)
        .security_descriptor(current_user_only_security_descriptor()?)
        .create_tokio()?;
    let mut connections = state.connections.subscribe();
    loop {
        let connection_count = *connections.borrow_and_update();
        let stream = if connection_count == 0 {
            tokio::select! {
                () = state.shutdown.cancelled() => break,
                () = tokio::time::sleep(state.config.idle_timeout) => break,
                accepted = listener.accept() => accepted?,
            }
        } else {
            tokio::time::sleep(state.config.test_idle_wait_delay).await;
            tokio::select! {
                () = state.shutdown.cancelled() => break,
                changed = connections.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    continue;
                },
                accepted = listener.accept() => accepted?,
            }
        };
        state
            .connections
            .send_modify(|count| *count = count.saturating_add(1));
        let connection_state = Arc::clone(&state);
        tokio::spawn(async move {
            let _ = serve_connection(Box::new(stream), connection_state).await;
        });
    }
    Ok(())
}
