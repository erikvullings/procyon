//! Optional semantic capability boundary.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use fm_semantic_protocol::{FrameError, v1::ErrorCode};
use fm_semantic_worker::{WorkerClient, WorkerConnector};
use tokio::sync::Mutex as AsyncMutex;

pub use fm_semantic_worker::{
    Endpoint as SemanticWorkerEndpoint, LaunchSecret as SemanticWorkerSecret,
};

/// Health of the optional semantic worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticHealth {
    /// Ready to accept work.
    Serving,
    /// Alive but temporarily degraded.
    Degraded,
    /// Finishing active work before shutdown.
    Draining,
}

/// Actionable failures from the optional semantic capability.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SemanticError {
    /// No semantic capability was configured, or its worker cannot be reached.
    #[error("semantic capability is unavailable")]
    Unavailable,
    /// The worker rejected the session secret.
    #[error("semantic worker authentication was rejected")]
    AuthenticationRejected,
    /// The configured worker authentication material is invalid.
    #[error("semantic worker authentication is misconfigured")]
    AuthenticationConfiguration,
    /// The host application must be updated for protocol compatibility.
    #[error("semantic client update required: {0}")]
    ClientUpdateRequired(String),
    /// The semantic worker must be updated for protocol compatibility.
    #[error("semantic worker update required: {0}")]
    WorkerUpdateRequired(String),
    /// A negotiated resource limit was exceeded.
    #[error("semantic resource limit exceeded: {0}")]
    LimitExceeded(String),
    /// A semantic request exceeded its deadline.
    #[error("semantic request deadline exceeded: {0}")]
    DeadlineExceeded(String),
    /// The semantic operation was cancelled.
    #[error("semantic operation was cancelled")]
    Cancelled,
    /// Other Procyon clients are still using the shared worker.
    #[error("semantic worker shutdown requires {remaining_clients} other client(s) to disconnect")]
    ShutdownBlocked {
        /// Number of other clients that must disconnect before retrying.
        remaining_clients: u32,
    },
    /// The caller supplied an invalid or unknown semantic request.
    #[error("invalid semantic request: {0}")]
    InvalidRequest(String),
    /// The worker reported an operation failure.
    #[error("semantic worker failed: {0}")]
    WorkerFailure(String),
    /// The worker violated the negotiated protocol.
    #[error("semantic worker returned an invalid protocol response")]
    ProtocolViolation,
}

impl From<fm_semantic_worker::ClientError> for SemanticError {
    fn from(error: fm_semantic_worker::ClientError) -> Self {
        match error {
            fm_semantic_worker::ClientError::Io(_) => Self::Unavailable,
            fm_semantic_worker::ClientError::Frame(frame) => match frame {
                FrameError::Io(_) => Self::Unavailable,
                FrameError::TooLarge { .. } => {
                    Self::LimitExceeded("semantic message exceeds size limit".to_owned())
                }
                FrameError::Decode(_) => Self::ProtocolViolation,
            },
            fm_semantic_worker::ClientError::Unauthenticated => Self::AuthenticationRejected,
            fm_semantic_worker::ClientError::Incompatible { code, message } => match code {
                ErrorCode::ClientUpdateRequired => Self::ClientUpdateRequired(message),
                ErrorCode::WorkerUpdateRequired => Self::WorkerUpdateRequired(message),
                _ => Self::ProtocolViolation,
            },
            fm_semantic_worker::ClientError::Remote { code, message } => {
                map_remote_error(code, message)
            }
            fm_semantic_worker::ClientError::Disconnected => Self::Unavailable,
            fm_semantic_worker::ClientError::UnexpectedResponse => Self::ProtocolViolation,
            fm_semantic_worker::ClientError::ShutdownBlocked { remaining_clients } => {
                Self::ShutdownBlocked { remaining_clients }
            }
            // A rolling-compatible worker may simply not implement an optional
            // capability. That is an availability fact, not a protocol fault.
            fm_semantic_worker::ClientError::CapabilityUnavailable { .. } => Self::Unavailable,
            fm_semantic_worker::ClientError::InsecureEndpoint => Self::AuthenticationConfiguration,
            fm_semantic_worker::ClientError::InvalidNegotiatedLimits(_) => Self::ProtocolViolation,
            fm_semantic_worker::ClientError::InvalidNegotiatedVersion => Self::ProtocolViolation,
            fm_semantic_worker::ClientError::InvalidSecretFile => Self::AuthenticationConfiguration,
            fm_semantic_worker::ClientError::InvalidDeveloperModelPack(message) => {
                Self::WorkerFailure(message)
            }
            fm_semantic_worker::ClientError::InvalidManagedComponents(message) => {
                Self::WorkerFailure(message)
            }
            fm_semantic_worker::ClientError::ShutdownTimedOut => {
                Self::WorkerFailure("worker did not stop within the shutdown deadline".to_owned())
            }
        }
    }
}

fn map_remote_error(code: ErrorCode, message: String) -> SemanticError {
    match code {
        ErrorCode::Unspecified => SemanticError::ProtocolViolation,
        ErrorCode::Unauthenticated => SemanticError::AuthenticationRejected,
        ErrorCode::LimitExceeded => SemanticError::LimitExceeded(message),
        ErrorCode::DeadlineExceeded => SemanticError::DeadlineExceeded(message),
        ErrorCode::InvalidRequest => SemanticError::InvalidRequest(message),
        ErrorCode::ClientUpdateRequired => SemanticError::ClientUpdateRequired(message),
        ErrorCode::WorkerUpdateRequired => SemanticError::WorkerUpdateRequired(message),
        ErrorCode::Cancelled => SemanticError::Cancelled,
        ErrorCode::Unavailable => SemanticError::Unavailable,
        ErrorCode::Internal => SemanticError::WorkerFailure(message),
    }
}

macro_rules! opaque_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            /// Creates an identifier without interpreting its contents.
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            /// Returns the opaque identifier value.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self::new(value)
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self::new(value)
            }
        }
    };
}

opaque_id!(
    /// Opaque tenant identifier.
    TenantId
);
opaque_id!(
    /// Opaque library identifier within a tenant.
    LibraryId
);
opaque_id!(
    /// Opaque document identifier within a library.
    DocumentId
);
opaque_id!(
    /// Opaque semantic request or operation identifier.
    SemanticOperationId
);
opaque_id!(
    /// Opaque ingestion job identifier.
    SemanticJobId
);

/// Tenant and library ownership of a semantic operation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SemanticScope {
    /// Tenant owning the operation.
    pub tenant_id: TenantId,
    /// Library within the tenant.
    pub library_id: LibraryId,
}

impl SemanticScope {
    /// Creates a tenant/library scope.
    #[must_use]
    pub const fn new(tenant_id: TenantId, library_id: LibraryId) -> Self {
        Self {
            tenant_id,
            library_id,
        }
    }
}

/// One provider-neutral document to ingest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentIngestion {
    /// Tenant and library owning the document.
    pub scope: SemanticScope,
    /// Opaque identifier used for correlation and cancellation.
    pub operation_id: SemanticOperationId,
    /// Opaque document identifier.
    pub document_id: DocumentId,
    /// Structured metadata stored with the document.
    pub metadata: BTreeMap<String, String>,
    /// IANA media type of the byte stream.
    pub media_type: String,
    /// Provider-supplied document bytes.
    pub content: Vec<u8>,
}

/// One scoped semantic query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticQuery {
    /// Tenant and library to search.
    pub scope: SemanticScope,
    /// Opaque identifier used for correlation and cancellation.
    pub request_id: SemanticOperationId,
    /// Query text interpreted by the capability.
    pub text: String,
    /// Optional stable concept-folder query over published annotations.
    pub concept: Option<SemanticConceptQuery>,
    /// Maximum number of results to return.
    pub maximum_results: u32,
}

/// Pre-authorized and hierarchy-expanded concept query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticConceptQuery {
    /// Attached vocabulary identity.
    pub vocabulary_id: String,
    /// Stable selected and expanded concept URIs.
    pub concept_uris: Vec<String>,
    /// Optional enrolled-root filter.
    pub root_id: Option<String>,
    /// Optional workspace filter.
    pub workspace_id: Option<String>,
    /// Include unavailable sources for honest stale browsing.
    pub include_unavailable: bool,
    /// Stable paging offset.
    pub offset: u64,
}

/// One provider-neutral semantic query result.
#[derive(Debug, Clone, PartialEq)]
pub struct SemanticSearchResult {
    /// Opaque document identifier.
    pub document_id: DocumentId,
    /// Capability-assigned relevance score.
    pub score: f64,
    /// Structured document metadata.
    pub metadata: BTreeMap<String, String>,
    /// Bounded matching excerpt.
    pub excerpt: String,
}

/// Lifecycle state of an ingestion job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticIngestionState {
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
    /// Intentionally excluded from the derived index.
    Skipped,
}

/// Current state of one scoped ingestion job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticIngestionJob {
    /// Opaque job identifier.
    pub job_id: SemanticJobId,
    /// Opaque document identifier.
    pub document_id: DocumentId,
    /// Current lifecycle state.
    pub state: SemanticIngestionState,
    /// Sanitized failure or exclusion detail, when present.
    pub detail: Option<String>,
}

/// One scoped semantic progress event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticProgressEvent {
    /// Opaque operation or job identifier.
    pub operation_id: SemanticOperationId,
    /// Stable machine-readable phase.
    pub phase: String,
}

/// Provider-neutral semantic operations available to application hosts.
#[async_trait]
pub trait SemanticCapability: Send + Sync {
    /// Reports current capability health.
    async fn health(&self) -> Result<SemanticHealth, SemanticError>;

    /// Streams one provider-neutral document into the semantic capability.
    async fn ingest(&self, ingestion: DocumentIngestion) -> Result<SemanticJobId, SemanticError>;

    /// Executes a tenant/library-scoped semantic query.
    async fn query(&self, query: SemanticQuery)
    -> Result<Vec<SemanticSearchResult>, SemanticError>;

    /// Reads one ingestion job inside its tenant/library scope.
    async fn ingestion_job(
        &self,
        scope: SemanticScope,
        job_id: SemanticJobId,
    ) -> Result<SemanticIngestionJob, SemanticError>;

    /// Reads a finite snapshot of events inside a tenant/library scope.
    async fn events(
        &self,
        scope: SemanticScope,
    ) -> Result<Vec<SemanticProgressEvent>, SemanticError>;

    /// Executes one bounded hybrid knowledge retrieval.
    ///
    /// Capabilities that have no knowledge route report themselves unavailable
    /// rather than silently degrading search into a dense-only query.
    ///
    /// # Errors
    ///
    /// Returns a typed transport, capability, or protocol failure.
    async fn knowledge_search(
        &self,
        request_id: SemanticOperationId,
        request: fm_semantic_worker::knowledge_retrieval::KnowledgeRetrievalRequest,
    ) -> Result<fm_semantic_worker::knowledge_retrieval::KnowledgeRetrieval, SemanticError> {
        let _ = (request_id, request);
        Err(SemanticError::Unavailable)
    }

    /// Reports full-text and query-embedding availability independently.
    ///
    /// # Errors
    ///
    /// Returns a typed transport, capability, or protocol failure.
    async fn knowledge_capabilities(
        &self,
    ) -> Result<fm_semantic_worker::knowledge_retrieval::KnowledgeCapabilities, SemanticError> {
        Err(SemanticError::Unavailable)
    }

    /// Requests cancellation of an opaque operation.
    async fn cancel(&self, operation_id: SemanticOperationId) -> Result<bool, SemanticError>;

    /// Requests a bounded graceful shutdown.
    async fn shutdown(&self, grace: Duration) -> Result<(), SemanticError>;

    /// Stops any worker that is currently serving and drops the cached
    /// connection so the next operation starts a freshly configured one.
    ///
    /// Hosts call this after the backend-authoritative active model changed:
    /// the running worker still holds the previous model and its index. A
    /// capability with no separate worker process is unaffected.
    ///
    /// # Errors
    ///
    /// Returns a typed transport or protocol failure. A capability that is
    /// simply not running reports success.
    async fn restart(&self, grace: Duration) -> Result<(), SemanticError> {
        let _ = grace;
        Ok(())
    }
}

struct UnavailableSemanticCapability;

#[async_trait]
impl SemanticCapability for UnavailableSemanticCapability {
    async fn health(&self) -> Result<SemanticHealth, SemanticError> {
        Err(SemanticError::Unavailable)
    }

    async fn ingest(&self, _ingestion: DocumentIngestion) -> Result<SemanticJobId, SemanticError> {
        Err(SemanticError::Unavailable)
    }

    async fn query(
        &self,
        _query: SemanticQuery,
    ) -> Result<Vec<SemanticSearchResult>, SemanticError> {
        Err(SemanticError::Unavailable)
    }

    async fn ingestion_job(
        &self,
        _scope: SemanticScope,
        _job_id: SemanticJobId,
    ) -> Result<SemanticIngestionJob, SemanticError> {
        Err(SemanticError::Unavailable)
    }

    async fn events(
        &self,
        _scope: SemanticScope,
    ) -> Result<Vec<SemanticProgressEvent>, SemanticError> {
        Err(SemanticError::Unavailable)
    }

    async fn cancel(&self, _operation_id: SemanticOperationId) -> Result<bool, SemanticError> {
        Err(SemanticError::Unavailable)
    }

    async fn shutdown(&self, _grace: Duration) -> Result<(), SemanticError> {
        Err(SemanticError::Unavailable)
    }
}

#[derive(Clone)]
struct FakeDocument {
    metadata: BTreeMap<String, String>,
    content: Vec<u8>,
}

/// Deterministic in-memory semantic capability for mock mode and tests.
#[derive(Default)]
pub struct FakeSemanticCapability {
    documents: Mutex<BTreeMap<(SemanticScope, DocumentId), FakeDocument>>,
    jobs: Mutex<BTreeMap<(SemanticScope, SemanticJobId), SemanticIngestionJob>>,
    next_job_id: AtomicU64,
    draining: AtomicBool,
}

impl FakeSemanticCapability {
    /// Creates an empty deterministic capability.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl SemanticCapability for FakeSemanticCapability {
    async fn health(&self) -> Result<SemanticHealth, SemanticError> {
        if self.draining.load(Ordering::Relaxed) {
            Ok(SemanticHealth::Draining)
        } else {
            Ok(SemanticHealth::Serving)
        }
    }

    async fn ingest(&self, ingestion: DocumentIngestion) -> Result<SemanticJobId, SemanticError> {
        if self.draining.load(Ordering::Relaxed) {
            return Err(SemanticError::Unavailable);
        }
        let id = self.next_job_id.fetch_add(1, Ordering::Relaxed) + 1;
        let job_id = SemanticJobId::new(format!("fake-job-{id}"));
        let scope = ingestion.scope;
        let document_id = ingestion.document_id;
        self.documents
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                (scope.clone(), document_id.clone()),
                FakeDocument {
                    metadata: ingestion.metadata,
                    content: ingestion.content,
                },
            );
        self.jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                (scope, job_id.clone()),
                SemanticIngestionJob {
                    job_id: job_id.clone(),
                    document_id,
                    state: SemanticIngestionState::Completed,
                    detail: None,
                },
            );
        Ok(job_id)
    }

    async fn query(
        &self,
        query: SemanticQuery,
    ) -> Result<Vec<SemanticSearchResult>, SemanticError> {
        if self.draining.load(Ordering::Relaxed) {
            return Err(SemanticError::Unavailable);
        }
        let needle = query.text.to_lowercase();
        let limit = usize::try_from(query.maximum_results).unwrap_or(usize::MAX);
        Ok(self
            .documents
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .filter(|((scope, _), _)| *scope == query.scope)
            .filter(|(_, document)| {
                query.concept.as_ref().is_none_or(|concept| {
                    document.metadata.get("concept_uri").is_some_and(|uri| {
                        concept
                            .concept_uris
                            .iter()
                            .any(|candidate| candidate == uri)
                    })
                })
            })
            .filter(|(_, document)| {
                String::from_utf8_lossy(&document.content)
                    .to_lowercase()
                    .contains(&needle)
            })
            .take(limit)
            .map(|((_, document_id), document)| SemanticSearchResult {
                document_id: document_id.clone(),
                score: 1.0,
                metadata: document.metadata.clone(),
                excerpt: String::from_utf8_lossy(&document.content)
                    .chars()
                    .take(256)
                    .collect(),
            })
            .collect())
    }

    async fn ingestion_job(
        &self,
        scope: SemanticScope,
        job_id: SemanticJobId,
    ) -> Result<SemanticIngestionJob, SemanticError> {
        self.jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&(scope, job_id))
            .cloned()
            .ok_or_else(|| SemanticError::InvalidRequest("ingestion job was not found".to_owned()))
    }

    async fn events(
        &self,
        scope: SemanticScope,
    ) -> Result<Vec<SemanticProgressEvent>, SemanticError> {
        Ok(self
            .jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .filter(|((job_scope, _), _)| *job_scope == scope)
            .map(|((_, job_id), _)| SemanticProgressEvent {
                operation_id: SemanticOperationId::new(job_id.as_str()),
                phase: "completed".to_owned(),
            })
            .collect())
    }

    async fn cancel(&self, _operation_id: SemanticOperationId) -> Result<bool, SemanticError> {
        Ok(false)
    }

    async fn shutdown(&self, _grace: Duration) -> Result<(), SemanticError> {
        self.draining.store(true, Ordering::Relaxed);
        Ok(())
    }
}

/// Lazy IPC-backed semantic capability.
///
/// Construction only records connection configuration. The first semantic
/// operation connects to, or for desktop mode starts, the worker.
pub struct IpcSemanticCapability {
    connector: WorkerConnector,
    client: AsyncMutex<Option<WorkerClient>>,
}

impl IpcSemanticCapability {
    /// Configures an administrator-provisioned endpoint and secret.
    #[must_use]
    pub fn administrator_provisioned(
        endpoint: SemanticWorkerEndpoint,
        secret: SemanticWorkerSecret,
    ) -> Self {
        Self {
            connector: WorkerConnector::provisioned(endpoint, secret),
            client: AsyncMutex::new(None),
        }
    }

    /// Configures a desktop worker that is discovered or launched on demand.
    #[must_use]
    pub fn desktop_on_demand(runtime_directory: &Path, executable: &Path) -> Self {
        Self {
            connector: WorkerConnector::desktop(runtime_directory, executable),
            client: AsyncMutex::new(None),
        }
    }

    /// Configures a production desktop worker resolved lazily from verified managed state.
    #[must_use]
    pub fn desktop_managed(
        runtime_directory: &Path,
        resolver: fm_semantic_worker::ManagedWorkerResolver,
    ) -> Self {
        Self {
            connector: WorkerConnector::desktop_managed_resolved(runtime_directory, resolver),
            client: AsyncMutex::new(None),
        }
    }

    /// Configures an explicitly non-production desktop worker bundle.
    ///
    /// All paths are resolved by the trusted desktop host from verified,
    /// catalog-installed artifacts; none originate in frontend requests.
    #[must_use]
    pub fn desktop_developer_bundle(
        runtime_directory: &Path,
        executable: &Path,
        data_directory: &Path,
        native_library_directory: &Path,
        model_pack: Option<fm_semantic_worker::DeveloperModelPackResolver>,
    ) -> Self {
        let connector = WorkerConnector::desktop_developer(
            runtime_directory,
            executable,
            data_directory,
            Some(native_library_directory),
        );
        Self {
            connector: match model_pack {
                Some(resolver) => connector.with_developer_model_pack_resolver(resolver),
                None => connector,
            },
            client: AsyncMutex::new(None),
        }
    }

    async fn worker_client(&self) -> Result<WorkerClient, SemanticError> {
        let mut client = self.client.lock().await;
        if let Some(client) = client.as_ref() {
            return Ok(client.clone());
        }
        let connected = self
            .connector
            .connect()
            .await
            .map_err(SemanticError::from)?;
        *client = Some(connected.clone());
        Ok(connected)
    }

    async fn adapt_result<T>(
        &self,
        result: Result<T, fm_semantic_worker::ClientError>,
    ) -> Result<T, SemanticError> {
        if result.as_ref().is_err_and(connection_is_unusable) {
            self.client.lock().await.take();
        }
        result.map_err(SemanticError::from)
    }
}

#[async_trait]
impl SemanticCapability for IpcSemanticCapability {
    async fn health(&self) -> Result<SemanticHealth, SemanticError> {
        let client = self.worker_client().await?;
        match self.adapt_result(client.health().await).await? {
            fm_semantic_worker::WorkerHealth::Serving => Ok(SemanticHealth::Serving),
            fm_semantic_worker::WorkerHealth::Degraded => Ok(SemanticHealth::Degraded),
            fm_semantic_worker::WorkerHealth::Draining => Ok(SemanticHealth::Draining),
        }
    }

    async fn ingest(&self, ingestion: DocumentIngestion) -> Result<SemanticJobId, SemanticError> {
        let DocumentIngestion {
            scope,
            operation_id,
            document_id,
            metadata,
            media_type,
            content,
        } = ingestion;
        let client = self.worker_client().await?;
        self.adapt_result(
            client
                .ingest(
                    operation_id.as_str(),
                    fm_semantic_worker::IngestionScope::new(
                        scope.tenant_id.as_str(),
                        scope.library_id.as_str(),
                    ),
                    document_id.as_str(),
                    metadata,
                    &media_type,
                    content,
                )
                .await,
        )
        .await
        .map(SemanticJobId::new)
    }

    async fn query(
        &self,
        query: SemanticQuery,
    ) -> Result<Vec<SemanticSearchResult>, SemanticError> {
        let SemanticQuery {
            scope,
            request_id,
            text,
            concept,
            maximum_results,
        } = query;
        let client = self.worker_client().await?;
        let result = if let Some(concept) = concept {
            client
                .query_concepts_with_request_id(
                    request_id.as_str(),
                    scope.tenant_id.as_str(),
                    scope.library_id.as_str(),
                    fm_semantic_worker::ConceptFolderQuery {
                        vocabulary_id: concept.vocabulary_id,
                        concept_uris: concept.concept_uris,
                        root_id: concept.root_id,
                        workspace_id: concept.workspace_id,
                        include_unavailable: concept.include_unavailable,
                        offset: concept.offset,
                    },
                    maximum_results,
                )
                .await
        } else {
            client
                .query_with_request_id(
                    request_id.as_str(),
                    scope.tenant_id.as_str(),
                    scope.library_id.as_str(),
                    &text,
                    maximum_results,
                )
                .await
        };
        self.adapt_result(result).await.map(|results| {
            results
                .into_iter()
                .map(|result| SemanticSearchResult {
                    document_id: DocumentId::new(result.document_id),
                    score: result.score,
                    metadata: result.metadata,
                    excerpt: result.excerpt,
                })
                .collect()
        })
    }

    async fn knowledge_search(
        &self,
        request_id: SemanticOperationId,
        request: fm_semantic_worker::knowledge_retrieval::KnowledgeRetrievalRequest,
    ) -> Result<fm_semantic_worker::knowledge_retrieval::KnowledgeRetrieval, SemanticError> {
        let client = self.worker_client().await?;
        let result = client.knowledge_search(request_id.as_str(), &request).await;
        self.adapt_result(result).await
    }

    async fn knowledge_capabilities(
        &self,
    ) -> Result<fm_semantic_worker::knowledge_retrieval::KnowledgeCapabilities, SemanticError> {
        let client = self.worker_client().await?;
        let result = client.knowledge_capabilities().await;
        self.adapt_result(result).await
    }

    async fn ingestion_job(
        &self,
        scope: SemanticScope,
        job_id: SemanticJobId,
    ) -> Result<SemanticIngestionJob, SemanticError> {
        let client = self.worker_client().await?;
        self.adapt_result(
            client
                .ingestion_job(
                    scope.tenant_id.as_str(),
                    scope.library_id.as_str(),
                    job_id.as_str(),
                )
                .await,
        )
        .await
        .map(|job| SemanticIngestionJob {
            job_id: SemanticJobId::new(job.job_id),
            document_id: DocumentId::new(job.document_id),
            state: match job.state {
                fm_semantic_worker::IngestionState::Pending => SemanticIngestionState::Pending,
                fm_semantic_worker::IngestionState::Running => SemanticIngestionState::Running,
                fm_semantic_worker::IngestionState::Completed => SemanticIngestionState::Completed,
                fm_semantic_worker::IngestionState::Failed => SemanticIngestionState::Failed,
                fm_semantic_worker::IngestionState::Cancelled => SemanticIngestionState::Cancelled,
                fm_semantic_worker::IngestionState::Skipped => SemanticIngestionState::Skipped,
            },
            detail: job.detail,
        })
    }

    async fn events(
        &self,
        scope: SemanticScope,
    ) -> Result<Vec<SemanticProgressEvent>, SemanticError> {
        let client = self.worker_client().await?;
        self.adapt_result(
            client
                .events_snapshot(scope.tenant_id.as_str(), scope.library_id.as_str())
                .await,
        )
        .await
        .map(|events| {
            events
                .into_iter()
                .map(|event| SemanticProgressEvent {
                    operation_id: SemanticOperationId::new(event.operation_id),
                    phase: event.phase,
                })
                .collect()
        })
    }

    async fn cancel(&self, operation_id: SemanticOperationId) -> Result<bool, SemanticError> {
        let client = self.worker_client().await?;
        let result = client.cancel(operation_id.as_str()).await;
        self.adapt_result(result).await
    }

    async fn shutdown(&self, grace: Duration) -> Result<(), SemanticError> {
        let Some(client) = self.client.lock().await.as_ref().cloned() else {
            return Ok(());
        };
        let result = client.shutdown(grace).await;
        self.adapt_result(result).await
    }

    async fn restart(&self, grace: Duration) -> Result<(), SemanticError> {
        let mut cached = self.client.lock().await;
        // Taken before the shutdown round trip so the connection is dropped
        // even if the worker refuses or the transport fails: a stale client
        // pointing at the previous model must never be reused.
        let client = match cached.take() {
            Some(client) => Some(client),
            None => self
                .connector
                .connect_existing()
                .await
                .map_err(SemanticError::from)?,
        };
        let Some(client) = client else {
            return Ok(());
        };
        // A cached connection can outlive a worker that exited while the host
        // was idle. Confirm the process state even when its shutdown
        // round-trip fails; an already-absent worker is a successful restart.
        let _shutdown_result = client.shutdown(grace).await;
        drop(client);
        self.connector
            .wait_until_stopped(grace.saturating_add(Duration::from_secs(2)))
            .await
            .map_err(SemanticError::from)
    }
}

fn connection_is_unusable(error: &fm_semantic_worker::ClientError) -> bool {
    matches!(
        error,
        fm_semantic_worker::ClientError::Io(_)
            | fm_semantic_worker::ClientError::Frame(FrameError::Io(_) | FrameError::Decode(_))
            | fm_semantic_worker::ClientError::Unauthenticated
            | fm_semantic_worker::ClientError::Disconnected
            | fm_semantic_worker::ClientError::UnexpectedResponse
    )
}

/// Coordinates semantic operations without exposing worker transport details.
#[derive(Clone)]
pub struct SemanticService {
    capability: Arc<dyn SemanticCapability>,
}

impl SemanticService {
    /// Creates a service over one semantic capability implementation.
    #[must_use]
    pub fn new(capability: Arc<dyn SemanticCapability>) -> Self {
        Self { capability }
    }

    pub(crate) fn unavailable() -> Self {
        Self::new(Arc::new(UnavailableSemanticCapability))
    }

    pub(crate) async fn health(&self) -> Result<SemanticHealth, SemanticError> {
        self.capability.health().await
    }

    pub(crate) async fn ingest(
        &self,
        ingestion: DocumentIngestion,
    ) -> Result<SemanticJobId, SemanticError> {
        self.capability.ingest(ingestion).await
    }

    pub(crate) async fn query(
        &self,
        query: SemanticQuery,
    ) -> Result<Vec<SemanticSearchResult>, SemanticError> {
        self.capability.query(query).await
    }

    pub(crate) async fn knowledge_search(
        &self,
        request_id: SemanticOperationId,
        request: fm_semantic_worker::knowledge_retrieval::KnowledgeRetrievalRequest,
    ) -> Result<fm_semantic_worker::knowledge_retrieval::KnowledgeRetrieval, SemanticError> {
        self.capability.knowledge_search(request_id, request).await
    }

    pub(crate) async fn knowledge_capabilities(
        &self,
    ) -> Result<fm_semantic_worker::knowledge_retrieval::KnowledgeCapabilities, SemanticError> {
        self.capability.knowledge_capabilities().await
    }

    pub(crate) async fn ingestion_job(
        &self,
        scope: SemanticScope,
        job_id: SemanticJobId,
    ) -> Result<SemanticIngestionJob, SemanticError> {
        self.capability.ingestion_job(scope, job_id).await
    }

    pub(crate) async fn events(
        &self,
        scope: SemanticScope,
    ) -> Result<Vec<SemanticProgressEvent>, SemanticError> {
        self.capability.events(scope).await
    }

    pub(crate) async fn cancel(
        &self,
        operation_id: SemanticOperationId,
    ) -> Result<bool, SemanticError> {
        self.capability.cancel(operation_id).await
    }

    pub(crate) async fn shutdown(&self, grace: Duration) -> Result<(), SemanticError> {
        self.capability.shutdown(grace).await
    }

    pub(crate) async fn restart(&self, grace: Duration) -> Result<(), SemanticError> {
        self.capability.restart(grace).await
    }
}
