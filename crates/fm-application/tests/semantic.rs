//! Public application-boundary coverage for the optional semantic capability.

use std::collections::BTreeMap;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::Arc;
#[cfg(unix)]
use std::time::Duration;

use fm_application::FileManagerService;
use fm_application::semantic::{
    DocumentId, DocumentIngestion, FakeSemanticCapability, IpcSemanticCapability, LibraryId,
    SemanticCapability, SemanticError, SemanticHealth, SemanticIngestionState, SemanticOperationId,
    SemanticQuery, SemanticScope, SemanticWorkerEndpoint, SemanticWorkerSecret, TenantId,
};
use fm_semantic_protocol::{FrameError, v1::ErrorCode};
use fm_semantic_worker::ClientError;
#[cfg(unix)]
use fm_semantic_worker::{WorkerConfig, WorkerServer};
use fm_transport_dto::RuntimeKindDto;

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(name: &str) -> Self {
        let path = PathBuf::from("target")
            .join("semantic-application-tests")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create test directory");
        Self(path)
    }
}

impl Deref for TestDirectory {
    type Target = std::path::Path;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn default_facade_reports_semantic_capability_as_unavailable() {
    let directory = TestDirectory::new("default");
    let service = FileManagerService::new(
        RuntimeKindDto::BrowserServer,
        &*directory,
        directory.join("settings"),
    );

    assert_eq!(
        service.semantic_health().await,
        Err(SemanticError::Unavailable)
    );
}

#[tokio::test]
async fn semantic_fake_filters_interleaved_unsorted_documents_before_returning_results() {
    let fake = FakeSemanticCapability::new();
    let wanted = SemanticScope::new(TenantId::new("tenant-a"), LibraryId::new("library-a"));
    let other_library = SemanticScope::new(TenantId::new("tenant-a"), LibraryId::new("library-b"));
    let other_tenant = SemanticScope::new(TenantId::new("tenant-b"), LibraryId::new("library-a"));

    for (scope, document_id) in [
        (wanted.clone(), "zeta"),
        (other_library, "hidden-library"),
        (wanted.clone(), "alpha"),
        (other_tenant, "hidden-tenant"),
    ] {
        fake.ingest(DocumentIngestion {
            scope,
            operation_id: SemanticOperationId::new(format!("ingest-{document_id}")),
            document_id: DocumentId::new(document_id),
            metadata: BTreeMap::from([("kind".to_owned(), "note".to_owned())]),
            media_type: "text/plain".to_owned(),
            content: b"contains needle".to_vec(),
        })
        .await
        .expect("fake ingestion");
    }

    let results = fake
        .query(SemanticQuery {
            scope: wanted,
            request_id: "query-1".into(),
            text: "needle".to_owned(),
            maximum_results: 10,
        })
        .await
        .expect("fake query");

    assert_eq!(
        results
            .into_iter()
            .map(|result| result.document_id)
            .collect::<Vec<_>>(),
        [DocumentId::new("alpha"), DocumentId::new("zeta")]
    );
}

#[tokio::test]
async fn semantic_injected_fake_ingestion_and_query_flow_through_the_facade() {
    let directory = TestDirectory::new("facade");
    let service = FileManagerService::new(
        RuntimeKindDto::BrowserServer,
        &*directory,
        directory.join("settings"),
    )
    .with_semantic_capability(Arc::new(FakeSemanticCapability::new()));
    let scope = SemanticScope::new(TenantId::new("tenant"), LibraryId::new("library"));

    service
        .semantic_ingest(DocumentIngestion {
            scope: scope.clone(),
            operation_id: SemanticOperationId::new("ingest-document"),
            document_id: DocumentId::new("document"),
            metadata: BTreeMap::new(),
            media_type: "text/plain".to_owned(),
            content: b"searchable content".to_vec(),
        })
        .await
        .expect("facade ingestion");
    let results = service
        .semantic_query(SemanticQuery {
            scope,
            request_id: "query".into(),
            text: "searchable".to_owned(),
            maximum_results: 5,
        })
        .await
        .expect("facade query");

    assert_eq!(results[0].document_id, DocumentId::new("document"));
}

#[tokio::test]
async fn semantic_facade_delegates_fake_job_events_cancellation_and_shutdown_lifecycle() {
    let directory = TestDirectory::new("lifecycle");
    let service = FileManagerService::new(
        RuntimeKindDto::BrowserServer,
        &*directory,
        directory.join("settings"),
    )
    .with_semantic_capability(Arc::new(FakeSemanticCapability::new()));
    let scope = SemanticScope::new(TenantId::new("tenant"), LibraryId::new("library"));
    let job_id = service
        .semantic_ingest(DocumentIngestion {
            scope: scope.clone(),
            operation_id: SemanticOperationId::new("ingest-document"),
            document_id: DocumentId::new("document"),
            metadata: BTreeMap::new(),
            media_type: "text/plain".to_owned(),
            content: b"content".to_vec(),
        })
        .await
        .expect("facade ingestion");

    let status = service
        .semantic_ingestion_job(scope.clone(), job_id)
        .await
        .expect("facade job status");
    let events = service.semantic_events(scope).await.expect("facade events");
    let cancelled = service
        .semantic_cancel(SemanticOperationId::new("missing"))
        .await
        .expect("facade cancellation");
    service
        .semantic_shutdown(std::time::Duration::from_secs(1))
        .await
        .expect("facade shutdown");

    assert_eq!(
        (
            status.state,
            events[0].operation_id.clone(),
            cancelled,
            service.semantic_health().await,
        ),
        (
            SemanticIngestionState::Completed,
            SemanticOperationId::new("fake-job-1"),
            false,
            Ok(SemanticHealth::Draining),
        )
    );
}

#[test]
fn semantic_provisioned_ipc_capability_is_lazy_until_first_operation() {
    let directory = TestDirectory::new("lazy");
    let endpoint = SemanticWorkerEndpoint::for_runtime_directory(&directory);

    let _capability = IpcSemanticCapability::administrator_provisioned(
        endpoint.clone(),
        SemanticWorkerSecret::from_bytes([7; 32]),
    );

    assert!(!endpoint.exists());
}

#[cfg(unix)]
#[tokio::test]
async fn desktop_on_demand_shutdown_does_not_start_an_unused_worker() {
    let directory = TestDirectory::new("lazy-desktop-shutdown");
    let endpoint = SemanticWorkerEndpoint::for_runtime_directory(&directory);
    let capability =
        IpcSemanticCapability::desktop_on_demand(&directory, &directory.join("must-not-launch"));

    capability
        .shutdown(Duration::from_millis(100))
        .await
        .expect("unused capability shutdown");

    assert!(!endpoint.exists());
    for artifact in ["launch.secret", "launch.lock", "worker.lock", "worker.pid"] {
        assert!(
            !directory.join(artifact).exists(),
            "shutdown created {artifact}"
        );
    }
}

#[test]
fn semantic_worker_errors_map_to_actionable_categories_without_transport_details() {
    let mapped: [SemanticError; 11] = [
        ClientError::Unauthenticated.into(),
        ClientError::InvalidSecretFile.into(),
        ClientError::InsecureEndpoint.into(),
        ClientError::InvalidNegotiatedLimits(
            fm_semantic_protocol::NegotiatedLimitsError::ExceedsClientCeiling {
                field: fm_semantic_protocol::LimitField::MessageBytes,
                offered: u64::from(u32::MAX),
                maximum: fm_semantic_protocol::MAX_MESSAGE_BYTES as u64,
            },
        )
        .into(),
        ClientError::InvalidNegotiatedVersion.into(),
        ClientError::Incompatible {
            code: ErrorCode::ClientUpdateRequired,
            message: "update Procyon".to_owned(),
        }
        .into(),
        ClientError::Incompatible {
            code: ErrorCode::WorkerUpdateRequired,
            message: "update worker".to_owned(),
        }
        .into(),
        ClientError::Remote {
            code: ErrorCode::LimitExceeded,
            message: "reduce input".to_owned(),
        }
        .into(),
        ClientError::Remote {
            code: ErrorCode::Cancelled,
            message: "cancelled".to_owned(),
        }
        .into(),
        ClientError::Io(std::io::Error::other("secret operating-system detail")).into(),
        ClientError::Frame(FrameError::TooLarge {
            actual: 11,
            maximum: 10,
        })
        .into(),
    ];

    assert_eq!(
        mapped,
        [
            SemanticError::AuthenticationRejected,
            SemanticError::AuthenticationConfiguration,
            SemanticError::AuthenticationConfiguration,
            SemanticError::ProtocolViolation,
            SemanticError::ProtocolViolation,
            SemanticError::ClientUpdateRequired("update Procyon".to_owned()),
            SemanticError::WorkerUpdateRequired("update worker".to_owned()),
            SemanticError::LimitExceeded("reduce input".to_owned()),
            SemanticError::Cancelled,
            SemanticError::Unavailable,
            SemanticError::LimitExceeded("semantic message exceeds size limit".to_owned()),
        ]
    );
}

#[cfg(unix)]
#[tokio::test]
async fn semantic_ipc_capability_adapts_worker_operations_to_application_types() {
    let directory = TestDirectory::new("ipc");
    let endpoint = SemanticWorkerEndpoint::for_runtime_directory(&directory);
    let secret = SemanticWorkerSecret::from_bytes([9; 32]);
    let worker =
        tokio::spawn(WorkerServer::new(WorkerConfig::new(endpoint.clone(), secret.clone())).run());
    for _ in 0..100 {
        if endpoint.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(endpoint.exists(), "worker endpoint did not start");

    let service = FileManagerService::new(
        RuntimeKindDto::BrowserServer,
        directory.join("workspaces"),
        directory.join("settings"),
    )
    .with_semantic_capability(Arc::new(IpcSemanticCapability::administrator_provisioned(
        endpoint, secret,
    )));
    let scope = SemanticScope::new(TenantId::new("tenant"), LibraryId::new("library"));
    let job_id = service
        .semantic_ingest(DocumentIngestion {
            scope: scope.clone(),
            operation_id: SemanticOperationId::new("ingest-document"),
            document_id: DocumentId::new("document"),
            metadata: BTreeMap::from([("kind".to_owned(), "note".to_owned())]),
            media_type: "text/plain".to_owned(),
            content: b"semantic bytes".to_vec(),
        })
        .await
        .expect("IPC ingestion");
    let status = service
        .semantic_ingestion_job(scope.clone(), job_id)
        .await
        .expect("IPC job");
    let results = service
        .semantic_query(SemanticQuery {
            scope: scope.clone(),
            request_id: SemanticOperationId::new("query"),
            text: "semantic".to_owned(),
            maximum_results: 5,
        })
        .await
        .expect("IPC query");
    let events = service.semantic_events(scope).await.expect("IPC events");
    let cancelled = service
        .semantic_cancel(SemanticOperationId::new("missing"))
        .await
        .expect("IPC cancellation");
    service
        .semantic_shutdown(Duration::from_millis(100))
        .await
        .expect("IPC shutdown");
    worker.await.expect("worker task").expect("worker shutdown");

    assert_eq!(
        (
            status.state,
            results[0].document_id.clone(),
            events[0].operation_id.clone(),
            cancelled,
        ),
        (
            SemanticIngestionState::Completed,
            DocumentId::new("document"),
            SemanticOperationId::new("job-1"),
            false,
        )
    );
}

#[cfg(unix)]
#[tokio::test]
async fn semantic_ipc_capability_reconnects_after_a_typed_session_expiry_rejection() {
    let directory = TestDirectory::new("ipc-session-expiry");
    let endpoint = SemanticWorkerEndpoint::for_runtime_directory(&directory);
    let secret = SemanticWorkerSecret::from_bytes([12; 32]);
    let worker = tokio::spawn(
        WorkerServer::new(
            WorkerConfig::new(endpoint.clone(), secret.clone())
                .with_session_lifetime(Duration::from_millis(30)),
        )
        .run(),
    );
    for _ in 0..100 {
        if endpoint.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let service = FileManagerService::new(
        RuntimeKindDto::BrowserServer,
        directory.join("workspaces"),
        directory.join("settings"),
    )
    .with_semantic_capability(Arc::new(IpcSemanticCapability::administrator_provisioned(
        endpoint, secret,
    )));
    assert_eq!(service.semantic_health().await, Ok(SemanticHealth::Serving));
    tokio::time::sleep(Duration::from_millis(50)).await;

    assert_eq!(
        service.semantic_health().await,
        Err(SemanticError::AuthenticationRejected)
    );
    assert_eq!(service.semantic_health().await, Ok(SemanticHealth::Serving));

    service
        .semantic_shutdown(Duration::from_millis(100))
        .await
        .unwrap();
    worker.await.unwrap().unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn application_propagates_shared_worker_shutdown_until_other_client_disconnects() {
    let directory = TestDirectory::new("shared-ipc-shutdown");
    let endpoint = SemanticWorkerEndpoint::for_runtime_directory(&directory);
    let secret = SemanticWorkerSecret::from_bytes([10; 32]);
    let worker =
        tokio::spawn(WorkerServer::new(WorkerConfig::new(endpoint.clone(), secret.clone())).run());
    for _ in 0..100 {
        if endpoint.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(endpoint.exists(), "worker endpoint did not start");

    let first = FileManagerService::new(
        RuntimeKindDto::BrowserServer,
        directory.join("first-workspaces"),
        directory.join("first-settings"),
    )
    .with_semantic_capability(Arc::new(IpcSemanticCapability::administrator_provisioned(
        endpoint.clone(),
        secret.clone(),
    )));
    let second = FileManagerService::new(
        RuntimeKindDto::BrowserServer,
        directory.join("second-workspaces"),
        directory.join("second-settings"),
    )
    .with_semantic_capability(Arc::new(IpcSemanticCapability::administrator_provisioned(
        endpoint, secret,
    )));
    assert_eq!(first.semantic_health().await, Ok(SemanticHealth::Serving));
    assert_eq!(second.semantic_health().await, Ok(SemanticHealth::Serving));

    assert_eq!(
        first.semantic_shutdown(Duration::from_millis(100)).await,
        Err(SemanticError::ShutdownBlocked {
            remaining_clients: 1
        })
    );
    assert_eq!(second.semantic_health().await, Ok(SemanticHealth::Serving));

    drop(first);
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            match second.semantic_shutdown(Duration::from_millis(100)).await {
                Ok(()) => break,
                Err(SemanticError::ShutdownBlocked { .. }) => tokio::task::yield_now().await,
                result => panic!("unexpected shutdown result: {result:?}"),
            }
        }
    })
    .await
    .expect("remaining application client could not stop the worker");
    worker.await.expect("worker task").expect("worker shutdown");
}

#[cfg(unix)]
#[tokio::test]
async fn semantic_cancel_targets_an_ingestion_operation_while_it_is_in_flight() {
    let directory = TestDirectory::new("ipc-ingestion-cancellation");
    let endpoint = SemanticWorkerEndpoint::for_runtime_directory(&directory);
    let secret = SemanticWorkerSecret::from_bytes([11; 32]);
    let worker = tokio::spawn(
        WorkerServer::new(
            WorkerConfig::new(endpoint.clone(), secret.clone())
                .with_atomic_ingestion_delay(Duration::from_millis(200)),
        )
        .run(),
    );
    for _ in 0..100 {
        if endpoint.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let service = Arc::new(
        FileManagerService::new(
            RuntimeKindDto::BrowserServer,
            directory.join("workspaces"),
            directory.join("settings"),
        )
        .with_semantic_capability(Arc::new(
            IpcSemanticCapability::administrator_provisioned(endpoint, secret),
        )),
    );
    let scope = SemanticScope::new(TenantId::new("tenant"), LibraryId::new("library"));
    let operation_id = SemanticOperationId::new("ingest-cancellable");
    let ingesting = Arc::clone(&service);
    let ingestion_scope = scope.clone();
    let ingestion_operation_id = operation_id.clone();
    let ingestion = tokio::spawn(async move {
        ingesting
            .semantic_ingest(DocumentIngestion {
                scope: ingestion_scope,
                operation_id: ingestion_operation_id,
                document_id: DocumentId::new("cancelled-document"),
                metadata: BTreeMap::new(),
                media_type: "text/plain".to_owned(),
                content: b"must not be committed".to_vec(),
            })
            .await
    });

    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(
        service
            .semantic_cancel(operation_id)
            .await
            .expect("ingestion cancellation")
    );
    assert_eq!(
        ingestion.await.expect("ingestion task"),
        Err(SemanticError::Cancelled)
    );
    assert!(
        service
            .semantic_query(SemanticQuery {
                scope,
                request_id: SemanticOperationId::new("verify-cancelled-ingestion"),
                text: "committed".to_owned(),
                maximum_results: 1,
            })
            .await
            .expect("verification query")
            .is_empty()
    );

    service
        .semantic_shutdown(Duration::from_millis(100))
        .await
        .expect("IPC shutdown");
    worker.await.expect("worker task").expect("worker shutdown");
}
