//! Public worker IPC behavior tests.

#![cfg(unix)]

use std::collections::BTreeMap;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use fm_semantic_protocol::{
    MAX_CONCURRENT_REQUESTS, MAX_MESSAGE_BYTES, MAX_STREAM_BYTES, ProtocolLimits, ProtocolVersion,
    REQUEST_DEADLINE, SHUTDOWN_DEADLINE, STREAM_DEADLINE, VersionRange, read_frame, v1,
    write_frame,
};
use fm_semantic_worker::{
    ClientError, ConceptFolderQuery, Endpoint, IngestionScope, LaunchSecret, SearchResult,
    WorkerClient, WorkerConfig, WorkerConnector, WorkerHealth, WorkerQueryBackend,
    WorkerQueryInput, WorkerServer,
};
use tokio_util::sync::CancellationToken;

#[derive(Default)]
struct CapturingQueryBackend {
    input: Mutex<Option<WorkerQueryInput>>,
}

impl WorkerQueryBackend for CapturingQueryBackend {
    fn query(
        &self,
        input: WorkerQueryInput,
        _cancellation: &CancellationToken,
    ) -> Result<Vec<SearchResult>, String> {
        *self.input.lock().expect("query capture lock") = Some(input);
        Ok(vec![SearchResult {
            document_id: "document-1".to_owned(),
            score: 0.91,
            metadata: BTreeMap::new(),
            excerpt: String::new(),
        }])
    }
}

struct TestDirectory(PathBuf);

impl Deref for TestDirectory {
    type Target = std::path::Path;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<std::path::Path> for TestDirectory {
    fn as_ref(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn test_directory(name: &str) -> TestDirectory {
    let path = PathBuf::from("target")
        .join("semantic-worker-tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("create worker test directory");
    TestDirectory(path)
}

#[tokio::test]
async fn concept_query_preserves_stable_identity_scope_and_paging() {
    let directory = test_directory("concept-query");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let secret = LaunchSecret::from_bytes([91; 32]);
    let backend = Arc::new(CapturingQueryBackend::default());
    let task = tokio::spawn(
        WorkerServer::with_query_backend(
            WorkerConfig::new(endpoint.clone(), secret.clone()),
            backend.clone(),
        )
        .run(),
    );
    wait_for_endpoint(&endpoint).await;
    let client = WorkerClient::connect(&endpoint, secret).await.unwrap();

    let results = client
        .query_concepts_with_request_id(
            "concept-request",
            "tenant-1",
            "library-1",
            ConceptFolderQuery {
                vocabulary_id: "topics".to_owned(),
                concept_uris: vec!["urn:topic:parent".to_owned(), "urn:topic:child".to_owned()],
                root_id: Some("root-1".to_owned()),
                workspace_id: Some("workspace-1".to_owned()),
                include_unavailable: true,
                offset: 40,
            },
            20,
        )
        .await
        .expect("concept query");
    assert_eq!(results[0].document_id, "document-1");
    let captured = backend
        .input
        .lock()
        .expect("query capture lock")
        .clone()
        .expect("captured query");
    assert_eq!(captured.tenant_id, "tenant-1");
    assert_eq!(captured.library_id, "library-1");
    assert!(captured.query.is_empty());
    assert_eq!(captured.maximum_results, 20);
    assert_eq!(
        captured.concept_query,
        Some(ConceptFolderQuery {
            vocabulary_id: "topics".to_owned(),
            concept_uris: vec!["urn:topic:parent".to_owned(), "urn:topic:child".to_owned()],
            root_id: Some("root-1".to_owned()),
            workspace_id: Some("workspace-1".to_owned()),
            include_unavailable: true,
            offset: 40,
        })
    );

    client.shutdown(Duration::from_millis(100)).await.unwrap();
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn negotiates_and_rejects_an_invalid_launch_secret() {
    let directory = test_directory("authentication");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let secret = LaunchSecret::generate();
    let server = WorkerServer::new(WorkerConfig::new(endpoint.clone(), secret.clone()));
    let task = tokio::spawn(server.run());
    wait_for_endpoint(&endpoint).await;

    let error = WorkerClient::connect(&endpoint, LaunchSecret::generate())
        .await
        .unwrap_err();
    assert!(matches!(error, ClientError::Unauthenticated));

    let client = WorkerConnector::provisioned(endpoint, secret)
        .connect()
        .await
        .unwrap();
    assert_eq!(client.protocol_version(), 1);
    client.shutdown(Duration::from_millis(100)).await.unwrap();
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn expired_sessions_are_rejected_and_a_new_session_can_authenticate() {
    let directory = test_directory("session-expiry");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let secret = LaunchSecret::from_bytes([88; 32]);
    let task = tokio::spawn(
        WorkerServer::new(
            WorkerConfig::new(endpoint.clone(), secret.clone())
                .with_session_lifetime(Duration::from_millis(30)),
        )
        .run(),
    );
    wait_for_endpoint(&endpoint).await;
    let expired = WorkerClient::connect(&endpoint, secret.clone())
        .await
        .unwrap();
    assert_eq!(expired.health().await.unwrap(), WorkerHealth::Serving);
    tokio::time::sleep(Duration::from_millis(50)).await;

    assert!(matches!(
        expired.health().await,
        Err(ClientError::Unauthenticated)
    ));
    let replacement = WorkerClient::connect(&endpoint, secret).await.unwrap();
    assert_eq!(replacement.health().await.unwrap(), WorkerHealth::Serving);

    drop(expired);
    shutdown_when_last(&replacement).await;
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn rejects_an_over_permissive_unix_socket_before_sending_protocol_bytes() {
    use std::os::unix::fs::PermissionsExt;

    let directory = test_directory("insecure-socket-mode");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let Endpoint::Unix(path) = &endpoint;
    let listener = tokio::net::UnixListener::bind(path).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o666)).unwrap();
    let peer = tokio::spawn(async move {
        let Ok(Ok((mut stream, _))) =
            tokio::time::timeout(Duration::from_millis(100), listener.accept()).await
        else {
            return false;
        };
        tokio::time::timeout(
            Duration::from_millis(100),
            read_frame::<_, v1::ClientFrame>(&mut stream, MAX_MESSAGE_BYTES),
        )
        .await
        .ok()
        .and_then(Result::ok)
        .is_some()
    });

    assert!(matches!(
        WorkerClient::connect(&endpoint, LaunchSecret::from_bytes([75; 32])).await,
        Err(ClientError::InsecureEndpoint)
    ));
    assert!(
        !peer.await.unwrap(),
        "client sent protocol bytes to an insecure socket"
    );
}

#[tokio::test]
async fn accepts_an_owner_only_socket_served_by_the_same_effective_user() {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};

    let directory = test_directory("same-user-socket");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let secret = LaunchSecret::from_bytes([76; 32]);
    let task =
        tokio::spawn(WorkerServer::new(WorkerConfig::new(endpoint.clone(), secret.clone())).run());
    wait_for_endpoint(&endpoint).await;
    let Endpoint::Unix(path) = &endpoint;
    let metadata = std::fs::symlink_metadata(path).unwrap();
    assert!(metadata.file_type().is_socket());
    assert_eq!(metadata.mode() & 0o777, 0o600);
    assert_eq!(metadata.uid(), rustix::process::geteuid().as_raw());

    let client = WorkerClient::connect(&endpoint, secret).await.unwrap();
    assert_eq!(client.health().await.unwrap(), WorkerHealth::Serving);
    client.shutdown(Duration::from_millis(100)).await.unwrap();
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn query_results_are_filtered_by_tenant_and_library_before_streaming() {
    let directory = test_directory("scope-filtering");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let secret = LaunchSecret::generate();
    let task =
        tokio::spawn(WorkerServer::new(WorkerConfig::new(endpoint.clone(), secret.clone())).run());
    wait_for_endpoint(&endpoint).await;
    let client = WorkerClient::connect(&endpoint, secret).await.unwrap();

    // Deliberately interleave scopes so filtering cannot rely on insertion order.
    for (tenant, library, document) in [
        ("tenant-a", "library-a", "a-1"),
        ("tenant-b", "library-a", "b-1"),
        ("tenant-a", "library-b", "a-2"),
        ("tenant-a", "library-a", "a-3"),
    ] {
        let job_id = client
            .ingest(
                &format!("ingest-{document}"),
                IngestionScope::new(tenant, library),
                document,
                BTreeMap::from([("title".to_owned(), document.to_owned())]),
                "text/plain",
                b"shared needle".to_vec(),
            )
            .await
            .unwrap();
        assert_eq!(
            client
                .ingestion_job(tenant, library, &job_id)
                .await
                .unwrap()
                .document_id,
            document
        );
    }

    let results = client
        .query("tenant-a", "library-a", "needle", 10)
        .await
        .unwrap();
    assert_eq!(
        results
            .iter()
            .map(|result| result.document_id.as_str())
            .collect::<Vec<_>>(),
        ["a-1", "a-3"]
    );
    assert_eq!(
        client
            .events_snapshot("tenant-a", "library-a")
            .await
            .unwrap()
            .len(),
        2
    );

    client.shutdown(Duration::from_millis(100)).await.unwrap();
    task.await.unwrap().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn query_filters_scope_before_expensive_content_processing() {
    let directory = test_directory("scope-before-content");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let secret = LaunchSecret::from_bytes([82; 32]);
    let task = tokio::spawn(
        WorkerServer::new(
            WorkerConfig::new(endpoint.clone(), secret.clone())
                .with_test_query_scan_delay(Duration::from_millis(50)),
        )
        .run(),
    );
    wait_for_endpoint(&endpoint).await;
    let client = WorkerClient::connect(&endpoint, secret).await.unwrap();
    for index in 0..6 {
        client
            .ingest(
                &format!("out-of-scope-{index}"),
                IngestionScope::new("other-tenant", "other-library"),
                &format!("large-document-{index}"),
                BTreeMap::new(),
                "text/plain",
                vec![b'x'; 512 * 1024],
            )
            .await
            .unwrap();
    }
    client
        .ingest(
            "in-scope",
            IngestionScope::new("tenant", "library"),
            "wanted",
            BTreeMap::new(),
            "text/plain",
            b"needle".to_vec(),
        )
        .await
        .unwrap();

    let results = tokio::time::timeout(
        Duration::from_millis(250),
        client.query("tenant", "library", "needle", 1),
    )
    .await
    .expect("out-of-scope content was processed before scope filtering")
    .unwrap();
    assert_eq!(results[0].document_id, "wanted");

    client.shutdown(Duration::from_millis(100)).await.unwrap();
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn cancellation_interrupts_an_in_flight_result_stream() {
    let directory = test_directory("cancellation");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let secret = LaunchSecret::generate();
    let task =
        tokio::spawn(WorkerServer::new(WorkerConfig::new(endpoint.clone(), secret.clone())).run());
    wait_for_endpoint(&endpoint).await;
    let client = WorkerClient::connect(&endpoint, secret).await.unwrap();
    for index in 0..20 {
        client
            .ingest(
                &format!("ingest-{index}"),
                IngestionScope::new("tenant", "library"),
                &format!("document-{index}"),
                BTreeMap::new(),
                "text/plain",
                b"needle".to_vec(),
            )
            .await
            .unwrap();
    }

    let querying = client.clone();
    let query = tokio::spawn(async move {
        querying
            .query_with_request_id("cancel-me", "tenant", "library", "needle", 20)
            .await
    });
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert!(client.cancel("cancel-me").await.unwrap());
    assert!(matches!(
        query.await.unwrap(),
        Err(ClientError::Remote {
            code: fm_semantic_protocol::v1::ErrorCode::Cancelled,
            ..
        })
    ));

    client.shutdown(Duration::from_millis(100)).await.unwrap();
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn cancellation_sent_immediately_after_a_query_is_accepted_deterministically() {
    let directory = test_directory("immediate-query-cancellation");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let secret = LaunchSecret::from_bytes([80; 32]);
    let task = tokio::spawn(
        WorkerServer::new(
            WorkerConfig::new(endpoint.clone(), secret.clone())
                .with_test_query_scan_delay(Duration::from_secs(5)),
        )
        .run(),
    );
    wait_for_endpoint(&endpoint).await;
    let observer = WorkerClient::connect(&endpoint, secret.clone())
        .await
        .unwrap();
    observer
        .ingest(
            "immediate-cancel-fixture",
            IngestionScope::new("tenant", "library"),
            "document",
            BTreeMap::new(),
            "text/plain",
            b"needle".to_vec(),
        )
        .await
        .unwrap();
    let (mut raw, session) = raw_authenticated_stream(&endpoint, &secret).await;
    write_frame(
        &mut raw,
        &v1::ClientFrame {
            correlation_id: 200,
            payload: Some(v1::client_frame::Payload::Query(v1::QueryRequest {
                session: Some(session.clone()),
                scope: Some(v1::ResourceScope {
                    tenant_id: "tenant".to_owned(),
                    library_id: "library".to_owned(),
                }),
                request_id: "immediate-cancel".to_owned(),
                query: "needle".to_owned(),
                maximum_results: 1,
                concept_query: None,
            })),
        },
        MAX_MESSAGE_BYTES,
    )
    .await
    .unwrap();
    write_frame(
        &mut raw,
        &v1::ClientFrame {
            correlation_id: 201,
            payload: Some(v1::client_frame::Payload::Cancel(v1::CancelRequest {
                session: Some(session),
                request_id: "immediate-cancel".to_owned(),
            })),
        },
        MAX_MESSAGE_BYTES,
    )
    .await
    .unwrap();

    let mut cancellation_accepted = false;
    let mut query_cancelled = false;
    for _ in 0..4 {
        let frame = tokio::time::timeout(
            Duration::from_millis(500),
            read_frame::<_, v1::ServerFrame>(&mut raw, MAX_MESSAGE_BYTES),
        )
        .await
        .expect("immediate cancellation did not complete")
        .unwrap();
        match frame.payload {
            Some(v1::server_frame::Payload::Cancelled(response)) => {
                cancellation_accepted = response.accepted;
            }
            Some(v1::server_frame::Payload::Error(error))
                if error.code == v1::ErrorCode::Cancelled as i32 =>
            {
                query_cancelled = true;
            }
            _ => {}
        }
    }
    assert!(cancellation_accepted);
    assert!(query_cancelled);

    drop(raw);
    shutdown_when_last(&observer).await;
    task.await.unwrap().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn duplicate_active_query_identifiers_remain_rejected() {
    let directory = test_directory("duplicate-query-identifier");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let secret = LaunchSecret::from_bytes([89; 32]);
    let task = tokio::spawn(
        WorkerServer::new(
            WorkerConfig::new(endpoint.clone(), secret.clone())
                .with_test_query_scan_delay(Duration::from_secs(5)),
        )
        .run(),
    );
    wait_for_endpoint(&endpoint).await;
    let client = WorkerClient::connect(&endpoint, secret).await.unwrap();
    client
        .ingest(
            "duplicate-query-fixture",
            IngestionScope::new("tenant", "library"),
            "document",
            BTreeMap::new(),
            "text/plain",
            b"needle".to_vec(),
        )
        .await
        .unwrap();
    let querying = client.clone();
    let first = tokio::spawn(async move {
        querying
            .query_with_request_id("duplicate-query", "tenant", "library", "needle", 1)
            .await
    });
    tokio::time::sleep(Duration::from_millis(20)).await;

    assert!(matches!(
        client
            .query_with_request_id("duplicate-query", "tenant", "library", "needle", 1)
            .await,
        Err(ClientError::Remote {
            code: v1::ErrorCode::InvalidRequest,
            ..
        })
    ));
    assert!(client.cancel("duplicate-query").await.unwrap());
    assert!(matches!(
        first.await.unwrap(),
        Err(ClientError::Remote {
            code: v1::ErrorCode::Cancelled,
            ..
        })
    ));

    client.shutdown(Duration::from_millis(100)).await.unwrap();
    task.await.unwrap().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn aborting_a_query_future_cancels_work_without_closing_the_client_connection() {
    let directory = test_directory("aborted-query");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let secret = LaunchSecret::from_bytes([78; 32]);
    let limits = ProtocolLimits::new(
        512,
        1024,
        1,
        Duration::from_secs(1),
        Duration::from_secs(2),
        Duration::from_secs(1),
    )
    .unwrap();
    let task = tokio::spawn(
        WorkerServer::new(
            WorkerConfig::new(endpoint.clone(), secret.clone())
                .with_limits(limits)
                .with_test_query_scan_delay(Duration::from_secs(5)),
        )
        .run(),
    );
    wait_for_endpoint(&endpoint).await;
    let client = WorkerClient::connect(&endpoint, secret).await.unwrap();
    client
        .ingest(
            "aborted-query-fixture",
            IngestionScope::new("tenant", "library"),
            "document",
            BTreeMap::new(),
            "text/plain",
            b"needle".to_vec(),
        )
        .await
        .unwrap();
    let querying = client.clone();
    let query = tokio::spawn(async move {
        querying
            .query_with_request_id("aborted-query", "tenant", "library", "needle", 1)
            .await
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    wait_for_limit_reached(&client).await;

    query.abort();
    query.await.unwrap_err();
    wait_for_serving_health(&client).await;

    client.shutdown(Duration::from_millis(100)).await.unwrap();
    task.await.unwrap().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_queries_receive_distinct_generated_request_identifiers() {
    let directory = test_directory("concurrent-query-identifiers");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let secret = LaunchSecret::from_bytes([81; 32]);
    let task = tokio::spawn(
        WorkerServer::new(
            WorkerConfig::new(endpoint.clone(), secret.clone())
                .with_test_query_scan_delay(Duration::from_millis(100)),
        )
        .run(),
    );
    wait_for_endpoint(&endpoint).await;
    let client = WorkerClient::connect(&endpoint, secret).await.unwrap();
    client
        .ingest(
            "concurrent-query-fixture",
            IngestionScope::new("tenant", "library"),
            "document",
            BTreeMap::new(),
            "text/plain",
            b"needle".to_vec(),
        )
        .await
        .unwrap();
    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(17));
    let mut queries = Vec::new();
    for _ in 0..16 {
        let querying = client.clone();
        let barrier = std::sync::Arc::clone(&barrier);
        queries.push(tokio::spawn(async move {
            barrier.wait().await;
            querying.query("tenant", "library", "needle", 1).await
        }));
    }
    barrier.wait().await;

    for query in queries {
        assert_eq!(query.await.unwrap().unwrap().len(), 1);
    }

    client.shutdown(Duration::from_millis(100)).await.unwrap();
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn expensive_query_scan_keeps_health_and_cancellation_responsive() {
    let directory = test_directory("responsive-query-scan");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let secret = LaunchSecret::from_bytes([74; 32]);
    let task = tokio::spawn(
        WorkerServer::new(
            WorkerConfig::new(endpoint.clone(), secret.clone())
                .with_test_query_scan_delay(Duration::from_secs(5)),
        )
        .run(),
    );
    wait_for_endpoint(&endpoint).await;
    let client = WorkerClient::connect(&endpoint, secret.clone())
        .await
        .unwrap();
    client
        .ingest(
            "expensive-query-fixture",
            IngestionScope::new("tenant", "library"),
            "document",
            BTreeMap::new(),
            "text/plain",
            b"needle".to_vec(),
        )
        .await
        .unwrap();
    let querying = client.clone();
    let query = tokio::spawn(async move {
        querying
            .query_with_request_id("expensive-query", "tenant", "library", "needle", 1)
            .await
    });

    tokio::time::timeout(
        Duration::from_millis(500),
        wait_for_active_request(&endpoint, &secret),
    )
    .await
    .expect("query scan blocked the Tokio executor");
    assert_eq!(
        tokio::time::timeout(Duration::from_millis(200), client.health())
            .await
            .expect("health was unresponsive during query scan")
            .unwrap(),
        WorkerHealth::Serving
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(200), client.cancel("expensive-query"))
            .await
            .expect("cancellation was unresponsive during query scan")
            .unwrap()
    );
    assert!(matches!(
        tokio::time::timeout(Duration::from_millis(500), query)
            .await
            .expect("cancelled query scan did not stop")
            .unwrap(),
        Err(ClientError::Remote {
            code: v1::ErrorCode::Cancelled,
            ..
        })
    ));

    shutdown_when_last(&client).await;
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn health_is_authenticated_and_ingestion_obeys_stream_limits() {
    let directory = test_directory("limits");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let secret = LaunchSecret::generate();
    let limits = ProtocolLimits::new(
        512,
        1024,
        1,
        Duration::from_secs(1),
        Duration::from_secs(1),
        Duration::from_secs(1),
    )
    .unwrap();
    let task = tokio::spawn(
        WorkerServer::new(
            WorkerConfig::new(endpoint.clone(), secret.clone())
                .with_limits(limits)
                .with_atomic_ingestion_delay(Duration::from_millis(80)),
        )
        .run(),
    );
    wait_for_endpoint(&endpoint).await;
    let client = WorkerClient::connect(&endpoint, secret).await.unwrap();

    assert_eq!(client.health().await.unwrap(), WorkerHealth::Serving);
    assert!(matches!(
        client
            .ingest(
                "ingest-too-large",
                IngestionScope::new("tenant", "library"),
                "too-large",
                BTreeMap::new(),
                "application/octet-stream",
                vec![0; 1025],
            )
            .await,
        Err(ClientError::Remote {
            code: fm_semantic_protocol::v1::ErrorCode::LimitExceeded,
            ..
        })
    ));
    let ingesting = client.clone();
    let ingestion = tokio::spawn(async move {
        ingesting
            .ingest(
                "ingest-active",
                IngestionScope::new("tenant", "library"),
                "active",
                BTreeMap::new(),
                "text/plain",
                b"needle".to_vec(),
            )
            .await
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(matches!(
        client.query("tenant", "library", "needle", 1).await,
        Err(ClientError::Remote {
            code: fm_semantic_protocol::v1::ErrorCode::LimitExceeded,
            ..
        })
    ));
    ingestion.await.unwrap().unwrap();

    client.shutdown(Duration::from_millis(100)).await.unwrap();
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn concurrency_limit_covers_event_handlers_while_cancellation_remains_responsive() {
    let directory = test_directory("global-concurrency");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let secret = LaunchSecret::from_bytes([63; 32]);
    let limits = ProtocolLimits::new(
        512,
        1024,
        1,
        Duration::from_secs(1),
        Duration::from_secs(1),
        Duration::from_secs(1),
    )
    .unwrap();
    let task = tokio::spawn(
        WorkerServer::new(
            WorkerConfig::new(endpoint.clone(), secret.clone())
                .with_limits(limits)
                .with_atomic_ingestion_delay(Duration::from_millis(200)),
        )
        .run(),
    );
    wait_for_endpoint(&endpoint).await;
    let client = WorkerClient::connect(&endpoint, secret.clone())
        .await
        .unwrap();
    let ingesting = client.clone();
    let ingestion = tokio::spawn(async move {
        ingesting
            .ingest(
                "occupies-global-permit",
                IngestionScope::new("tenant", "library"),
                "document",
                BTreeMap::new(),
                "text/plain",
                b"content".to_vec(),
            )
            .await
    });
    tokio::time::sleep(Duration::from_millis(30)).await;

    assert!(matches!(
        client.events_snapshot("tenant", "library").await,
        Err(ClientError::Remote {
            code: v1::ErrorCode::LimitExceeded,
            ..
        })
    ));
    assert!(client.cancel("occupies-global-permit").await.unwrap());
    assert!(matches!(
        ingestion.await.unwrap(),
        Err(ClientError::Remote {
            code: v1::ErrorCode::Cancelled,
            ..
        })
    ));

    client.shutdown(Duration::from_millis(100)).await.unwrap();
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn query_result_stream_stops_at_the_cumulative_server_byte_limit() {
    let directory = test_directory("query-stream-budget");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let secret = LaunchSecret::from_bytes([64; 32]);
    let limits = ProtocolLimits::new(
        512,
        700,
        2,
        Duration::from_secs(1),
        Duration::from_secs(1),
        Duration::from_secs(1),
    )
    .unwrap();
    let task = tokio::spawn(
        WorkerServer::new(WorkerConfig::new(endpoint.clone(), secret.clone()).with_limits(limits))
            .run(),
    );
    wait_for_endpoint(&endpoint).await;
    let client = WorkerClient::connect(&endpoint, secret).await.unwrap();
    for index in 0..4 {
        client
            .ingest(
                &format!("stream-budget-ingest-{index}"),
                IngestionScope::new("tenant", "library"),
                &format!("document-{index}"),
                BTreeMap::new(),
                "text/plain",
                format!("needle {}", "x".repeat(280)).into_bytes(),
            )
            .await
            .unwrap();
    }

    assert!(matches!(
        client.query("tenant", "library", "needle", 4).await,
        Err(ClientError::Remote {
            code: v1::ErrorCode::LimitExceeded,
            ..
        })
    ));

    client.shutdown(Duration::from_millis(100)).await.unwrap();
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn oversized_generated_query_frame_returns_a_typed_limit_without_disconnect() {
    let directory = test_directory("query-message-budget");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let secret = LaunchSecret::from_bytes([83; 32]);
    let limits = ProtocolLimits::new(
        128,
        4096,
        2,
        Duration::from_secs(1),
        Duration::from_secs(1),
        Duration::from_secs(1),
    )
    .unwrap();
    let task = tokio::spawn(
        WorkerServer::new(WorkerConfig::new(endpoint.clone(), secret.clone()).with_limits(limits))
            .run(),
    );
    wait_for_endpoint(&endpoint).await;
    let client = WorkerClient::connect(&endpoint, secret).await.unwrap();
    client
        .ingest(
            "i",
            IngestionScope::new("t", "l"),
            "d",
            BTreeMap::new(),
            "text/plain",
            format!("needle {}", "x".repeat(200)).into_bytes(),
        )
        .await
        .unwrap();

    assert!(matches!(
        client.query("t", "l", "needle", 1).await,
        Err(ClientError::Remote {
            code: v1::ErrorCode::LimitExceeded,
            ..
        })
    ));
    assert_eq!(client.health().await.unwrap(), WorkerHealth::Serving);

    client.shutdown(Duration::from_millis(100)).await.unwrap();
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn event_stream_stops_at_the_cumulative_server_byte_limit() {
    let directory = test_directory("event-stream-budget");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let secret = LaunchSecret::from_bytes([65; 32]);
    let limits = ProtocolLimits::new(
        256,
        300,
        2,
        Duration::from_secs(1),
        Duration::from_secs(1),
        Duration::from_secs(1),
    )
    .unwrap();
    let task = tokio::spawn(
        WorkerServer::new(WorkerConfig::new(endpoint.clone(), secret.clone()).with_limits(limits))
            .run(),
    );
    wait_for_endpoint(&endpoint).await;
    let client = WorkerClient::connect(&endpoint, secret).await.unwrap();
    for index in 0..20 {
        client
            .ingest(
                &format!("event-budget-ingest-{index}"),
                IngestionScope::new("tenant", "library"),
                &format!("document-{index}"),
                BTreeMap::new(),
                "text/plain",
                Vec::new(),
            )
            .await
            .unwrap();
    }

    assert!(matches!(
        client.events_snapshot("tenant", "library").await,
        Err(ClientError::Remote {
            code: v1::ErrorCode::LimitExceeded,
            ..
        })
    ));

    client.shutdown(Duration::from_millis(100)).await.unwrap();
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn oversized_generated_event_frame_returns_a_typed_limit_without_disconnect() {
    let directory = test_directory("event-message-budget");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let secret = LaunchSecret::from_bytes([84; 32]);
    let limits = ProtocolLimits::new(
        128,
        4096,
        2,
        Duration::from_secs(1),
        Duration::from_secs(1),
        Duration::from_secs(1),
    )
    .unwrap();
    let task = tokio::spawn(
        WorkerServer::new(
            WorkerConfig::new(endpoint.clone(), secret.clone())
                .with_limits(limits)
                .with_test_event_phase_bytes(200),
        )
        .run(),
    );
    wait_for_endpoint(&endpoint).await;
    let client = WorkerClient::connect(&endpoint, secret).await.unwrap();
    client
        .ingest(
            "i",
            IngestionScope::new("t", "l"),
            "d",
            BTreeMap::new(),
            "text/plain",
            Vec::new(),
        )
        .await
        .unwrap();

    assert!(matches!(
        client.events_snapshot("t", "l").await,
        Err(ClientError::Remote {
            code: v1::ErrorCode::LimitExceeded,
            ..
        })
    ));
    assert_eq!(client.health().await.unwrap(), WorkerHealth::Serving);

    client.shutdown(Duration::from_millis(100)).await.unwrap();
    task.await.unwrap().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn event_snapshot_preparation_obeys_deadline_off_the_tokio_runtime() {
    let directory = test_directory("event-snapshot-deadline");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let secret = LaunchSecret::from_bytes([85; 32]);
    let limits = ProtocolLimits::new(
        512,
        4096,
        2,
        Duration::from_millis(100),
        Duration::from_millis(40),
        Duration::from_millis(100),
    )
    .unwrap();
    let task = tokio::spawn(
        WorkerServer::new(
            WorkerConfig::new(endpoint.clone(), secret.clone())
                .with_limits(limits)
                .with_test_event_scan_delay(Duration::from_secs(5)),
        )
        .run(),
    );
    wait_for_endpoint(&endpoint).await;
    let client = WorkerClient::connect(&endpoint, secret).await.unwrap();
    client
        .ingest(
            "event-deadline-fixture",
            IngestionScope::new("tenant", "library"),
            "document",
            BTreeMap::new(),
            "text/plain",
            Vec::new(),
        )
        .await
        .unwrap();

    let reading_events = client.clone();
    let events =
        tokio::spawn(async move { reading_events.events_snapshot("tenant", "library").await });
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert_eq!(
        tokio::time::timeout(Duration::from_millis(100), client.health())
            .await
            .expect("event snapshot blocked the Tokio runtime")
            .unwrap(),
        WorkerHealth::Serving
    );
    assert!(matches!(
        tokio::time::timeout(Duration::from_millis(300), events)
            .await
            .expect("event snapshot exceeded its stream deadline")
            .unwrap(),
        Err(ClientError::Remote {
            code: v1::ErrorCode::DeadlineExceeded,
            ..
        })
    ));
    assert_eq!(client.health().await.unwrap(), WorkerHealth::Serving);

    client.shutdown(Duration::from_millis(100)).await.unwrap();
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn worker_exits_after_the_last_client_disconnects_and_idle_timeout_elapses() {
    let directory = test_directory("idle-shutdown");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let secret = LaunchSecret::generate();
    let task = tokio::spawn(
        WorkerServer::new(
            WorkerConfig::new(endpoint.clone(), secret.clone())
                .with_idle_timeout(Duration::from_millis(40)),
        )
        .run(),
    );
    wait_for_endpoint(&endpoint).await;
    let client = WorkerClient::connect(&endpoint, secret).await.unwrap();
    assert_eq!(client.health().await.unwrap(), WorkerHealth::Serving);
    drop(client);

    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .expect("worker did not stop when idle")
        .unwrap()
        .unwrap();
    assert!(!endpoint.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn repeated_last_client_disconnects_cannot_lose_idle_shutdown() {
    for iteration in 0..10 {
        let directory = test_directory(&format!("idle-shutdown-stress-{iteration}"));
        let endpoint = Endpoint::for_runtime_directory(&directory);
        let secret = LaunchSecret::generate();
        let task = tokio::spawn(
            WorkerServer::new(
                WorkerConfig::new(endpoint.clone(), secret.clone())
                    .with_idle_timeout(Duration::from_millis(20))
                    .with_test_idle_wait_delay(Duration::from_millis(100)),
            )
            .run(),
        );
        wait_for_endpoint(&endpoint).await;
        let client = WorkerClient::connect(&endpoint, secret).await.unwrap();
        assert_eq!(client.health().await.unwrap(), WorkerHealth::Serving);
        drop(client);

        tokio::time::timeout(Duration::from_millis(250), task)
            .await
            .unwrap_or_else(|_| {
                panic!("idle shutdown notification was lost on iteration {iteration}")
            })
            .unwrap()
            .unwrap();
        assert!(!endpoint.exists());
    }
}

#[tokio::test]
async fn silent_unauthenticated_connection_cannot_prevent_idle_shutdown() {
    let directory = test_directory("unauthenticated-timeout");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let limits = ProtocolLimits::new(
        512,
        1024,
        2,
        Duration::from_millis(30),
        Duration::from_millis(100),
        Duration::from_millis(100),
    )
    .unwrap();
    let task = tokio::spawn(
        WorkerServer::new(
            WorkerConfig::new(endpoint.clone(), LaunchSecret::from_bytes([61; 32]))
                .with_limits(limits)
                .with_idle_timeout(Duration::from_millis(30)),
        )
        .run(),
    );
    wait_for_endpoint(&endpoint).await;
    let Endpoint::Unix(path) = &endpoint;
    let _silent_client = tokio::net::UnixStream::connect(path).await.unwrap();

    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .expect("silent unauthenticated connection kept worker alive")
        .unwrap()
        .unwrap();
    assert!(!endpoint.exists());
}

#[tokio::test]
async fn authenticated_but_idle_connection_is_closed_at_the_read_idle_deadline() {
    let directory = test_directory("authenticated-read-idle");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let secret = LaunchSecret::from_bytes([67; 32]);
    let limits = ProtocolLimits::new(
        512,
        1024,
        2,
        Duration::from_millis(100),
        Duration::from_millis(40),
        Duration::from_millis(100),
    )
    .unwrap();
    let task = tokio::spawn(
        WorkerServer::new(
            WorkerConfig::new(endpoint.clone(), secret.clone())
                .with_limits(limits)
                .with_idle_timeout(Duration::from_millis(200)),
        )
        .run(),
    );
    wait_for_endpoint(&endpoint).await;
    let _idle_client = WorkerClient::connect(&endpoint, secret).await.unwrap();

    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .expect("idle authenticated connection kept worker alive")
        .unwrap()
        .unwrap();
    assert!(!endpoint.exists());
}

#[tokio::test]
async fn pending_ingestion_expires_without_another_chunk_and_releases_capacity() {
    let directory = test_directory("pending-ingestion-timeout");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let secret = LaunchSecret::from_bytes([62; 32]);
    let limits = ProtocolLimits::new(
        512,
        1024,
        1,
        Duration::from_secs(1),
        Duration::from_secs(1),
        Duration::from_secs(1),
    )
    .unwrap();
    let task = tokio::spawn(
        WorkerServer::new(
            WorkerConfig::new(endpoint.clone(), secret.clone())
                .with_limits(limits)
                .with_ingestion_timeout(Duration::from_millis(40)),
        )
        .run(),
    );
    wait_for_endpoint(&endpoint).await;
    let observer = WorkerClient::connect(&endpoint, secret.clone())
        .await
        .unwrap();
    let (mut raw, session) = raw_authenticated_stream(&endpoint, &secret).await;
    write_frame(
        &mut raw,
        &v1::ClientFrame {
            correlation_id: 77,
            payload: Some(v1::client_frame::Payload::Ingestion(v1::IngestionRequest {
                session: Some(session),
                scope: Some(v1::ResourceScope {
                    tenant_id: "tenant".to_owned(),
                    library_id: "library".to_owned(),
                }),
                request_id: "abandoned-ingestion".to_owned(),
                payload: Some(v1::ingestion_request::Payload::Start(v1::IngestionStart {
                    document_id: "document".to_owned(),
                    metadata: Vec::new(),
                    media_type: "text/plain".to_owned(),
                    expected_content_bytes: 1,
                })),
            })),
        },
        MAX_MESSAGE_BYTES,
    )
    .await
    .unwrap();

    let expired = tokio::time::timeout(
        Duration::from_millis(300),
        read_frame::<_, v1::ServerFrame>(&mut raw, MAX_MESSAGE_BYTES),
    )
    .await
    .expect("pending ingestion did not expire")
    .unwrap();
    assert!(matches!(
        expired.payload,
        Some(v1::server_frame::Payload::Error(v1::ProtocolError {
            code,
            ..
        })) if code == v1::ErrorCode::DeadlineExceeded as i32
    ));
    assert!(
        observer
            .query("tenant", "library", "missing", 1)
            .await
            .unwrap()
            .is_empty()
    );

    drop(raw);
    observer.shutdown(Duration::from_millis(100)).await.unwrap();
    task.await.unwrap().unwrap();
}

#[test]
fn ingestion_timeout_cannot_exceed_the_configured_stream_deadline() {
    let directory = test_directory("clamped-ingestion-timeout");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let limits = ProtocolLimits::new(
        512,
        1024,
        1,
        Duration::from_millis(100),
        Duration::from_millis(40),
        Duration::from_millis(100),
    )
    .unwrap();
    let secret = LaunchSecret::from_bytes([77; 32]);
    let timeout_then_limits = WorkerConfig::new(endpoint.clone(), secret.clone())
        .with_ingestion_timeout(Duration::from_secs(5))
        .with_limits(limits);
    let limits_then_timeout = WorkerConfig::new(endpoint, secret)
        .with_limits(limits)
        .with_ingestion_timeout(Duration::from_secs(5));

    assert_eq!(
        timeout_then_limits.effective_ingestion_timeout(),
        Duration::from_millis(40)
    );
    assert_eq!(
        limits_then_timeout.effective_ingestion_timeout(),
        Duration::from_millis(40)
    );
}

#[tokio::test]
async fn ingestion_operation_can_be_cancelled_before_content_arrives() {
    let directory = test_directory("pending-ingestion-cancellation");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let secret = LaunchSecret::from_bytes([66; 32]);
    let task =
        tokio::spawn(WorkerServer::new(WorkerConfig::new(endpoint.clone(), secret.clone())).run());
    wait_for_endpoint(&endpoint).await;
    let observer = WorkerClient::connect(&endpoint, secret.clone())
        .await
        .unwrap();
    let (mut raw, session) = raw_authenticated_stream(&endpoint, &secret).await;
    write_frame(
        &mut raw,
        &v1::ClientFrame {
            correlation_id: 81,
            payload: Some(v1::client_frame::Payload::Ingestion(v1::IngestionRequest {
                session: Some(session.clone()),
                scope: Some(v1::ResourceScope {
                    tenant_id: "tenant".to_owned(),
                    library_id: "library".to_owned(),
                }),
                request_id: "cancel-before-content".to_owned(),
                payload: Some(v1::ingestion_request::Payload::Start(v1::IngestionStart {
                    document_id: "document".to_owned(),
                    metadata: Vec::new(),
                    media_type: "text/plain".to_owned(),
                    expected_content_bytes: 1,
                })),
            })),
        },
        MAX_MESSAGE_BYTES,
    )
    .await
    .unwrap();
    write_frame(
        &mut raw,
        &v1::ClientFrame {
            correlation_id: 82,
            payload: Some(v1::client_frame::Payload::Cancel(v1::CancelRequest {
                session: Some(session),
                request_id: "cancel-before-content".to_owned(),
            })),
        },
        MAX_MESSAGE_BYTES,
    )
    .await
    .unwrap();

    let mut cancellation_accepted = false;
    let mut ingestion_cancelled = false;
    for _ in 0..4 {
        let frame = read_frame::<_, v1::ServerFrame>(&mut raw, MAX_MESSAGE_BYTES)
            .await
            .unwrap();
        match frame.payload {
            Some(v1::server_frame::Payload::Cancelled(response)) => {
                cancellation_accepted = response.accepted;
            }
            Some(v1::server_frame::Payload::Error(error))
                if error.code == v1::ErrorCode::Cancelled as i32 =>
            {
                ingestion_cancelled = true;
            }
            _ => {}
        }
    }
    assert!(cancellation_accepted);
    assert!(ingestion_cancelled);
    assert!(
        observer
            .query("tenant", "library", "missing", 1)
            .await
            .unwrap()
            .is_empty()
    );

    drop(raw);
    observer.shutdown(Duration::from_millis(100)).await.unwrap();
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn ingestion_rejects_non_contiguous_sequences_and_scope_changes_without_committing() {
    let directory = test_directory("ingestion-stream-identity");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let secret = LaunchSecret::from_bytes([86; 32]);
    let task =
        tokio::spawn(WorkerServer::new(WorkerConfig::new(endpoint.clone(), secret.clone())).run());
    wait_for_endpoint(&endpoint).await;
    let observer = WorkerClient::connect(&endpoint, secret.clone())
        .await
        .unwrap();
    let (mut raw, session) = raw_authenticated_stream(&endpoint, &secret).await;

    write_ingestion_start(
        &mut raw,
        300,
        session.clone(),
        "starts-at-one",
        ("tenant", "library"),
        "starts-at-one",
        6,
    )
    .await;
    write_ingestion_chunk(
        &mut raw,
        300,
        session.clone(),
        "starts-at-one",
        ("tenant", "library"),
        (1, b"needle", true),
    )
    .await;
    assert_invalid_request(&mut raw, 300).await;

    write_ingestion_start(
        &mut raw,
        301,
        session.clone(),
        "duplicate",
        ("tenant", "library"),
        "duplicate",
        12,
    )
    .await;
    write_ingestion_chunk(
        &mut raw,
        301,
        session.clone(),
        "duplicate",
        ("tenant", "library"),
        (0, b"needle", false),
    )
    .await;
    write_ingestion_chunk(
        &mut raw,
        301,
        session.clone(),
        "duplicate",
        ("tenant", "library"),
        (0, b"needle", true),
    )
    .await;
    assert_invalid_request(&mut raw, 301).await;

    write_ingestion_start(
        &mut raw,
        302,
        session.clone(),
        "changed-scope",
        ("tenant", "library"),
        "changed-scope",
        6,
    )
    .await;
    write_ingestion_chunk(
        &mut raw,
        302,
        session.clone(),
        "changed-scope",
        ("tenant", "other-library"),
        (0, b"needle", true),
    )
    .await;
    assert_invalid_request(&mut raw, 302).await;

    write_ingestion_start(
        &mut raw,
        303,
        session.clone(),
        "missing-scope",
        ("tenant", "library"),
        "missing-scope",
        6,
    )
    .await;
    write_frame(
        &mut raw,
        &v1::ClientFrame {
            correlation_id: 303,
            payload: Some(v1::client_frame::Payload::Ingestion(v1::IngestionRequest {
                session: Some(session.clone()),
                scope: None,
                request_id: "missing-scope".to_owned(),
                payload: Some(v1::ingestion_request::Payload::Chunk(
                    v1::FileContentChunk {
                        sequence: 0,
                        content: b"needle".to_vec(),
                        end_of_stream: true,
                    },
                )),
            })),
        },
        MAX_MESSAGE_BYTES,
    )
    .await
    .unwrap();
    assert_invalid_request(&mut raw, 303).await;
    write_ingestion_chunk(
        &mut raw,
        303,
        session,
        "missing-scope",
        ("tenant", "library"),
        (0, b"needle", true),
    )
    .await;
    assert_invalid_request(&mut raw, 303).await;

    assert!(
        observer
            .query("tenant", "library", "needle", 10)
            .await
            .unwrap()
            .is_empty()
    );

    drop(raw);
    observer.shutdown(Duration::from_millis(100)).await.unwrap();
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn shared_worker_shutdown_waits_until_the_requester_is_the_last_client() {
    let directory = test_directory("shared-shutdown");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let secret = LaunchSecret::generate();
    let task =
        tokio::spawn(WorkerServer::new(WorkerConfig::new(endpoint.clone(), secret.clone())).run());
    wait_for_endpoint(&endpoint).await;
    let first = WorkerClient::connect(&endpoint, secret.clone())
        .await
        .unwrap();
    let second = WorkerClient::connect(&endpoint, secret).await.unwrap();

    assert!(matches!(
        first.shutdown(Duration::from_millis(100)).await,
        Err(ClientError::ShutdownBlocked {
            remaining_clients: 1
        })
    ));
    assert_eq!(second.health().await.unwrap(), WorkerHealth::Serving);

    drop(first);
    shutdown_when_last(&second).await;
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn graceful_shutdown_finishes_atomic_ingestion_and_rejects_new_work() {
    let directory = test_directory("draining");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let secret = LaunchSecret::generate();
    let task = tokio::spawn(
        WorkerServer::new(
            WorkerConfig::new(endpoint.clone(), secret.clone())
                .with_atomic_ingestion_delay(Duration::from_millis(500)),
        )
        .run(),
    );
    wait_for_endpoint(&endpoint).await;
    let client = WorkerClient::connect(&endpoint, secret.clone())
        .await
        .unwrap();
    let ingesting = client.clone();
    let ingestion = tokio::spawn(async move {
        ingesting
            .ingest(
                "ingest-atomic",
                IngestionScope::new("tenant", "library"),
                "atomic",
                BTreeMap::new(),
                "text/plain",
                b"content".to_vec(),
            )
            .await
    });
    wait_for_active_request(&endpoint, &secret).await;
    client.shutdown(Duration::from_secs(1)).await.unwrap();
    assert!(
        !task.is_finished(),
        "worker exited during an atomic ingestion"
    );
    assert!(matches!(
        client.query("tenant", "library", "content", 1).await,
        Err(ClientError::Remote {
            code: fm_semantic_protocol::v1::ErrorCode::Unavailable,
            ..
        })
    ));
    assert!(matches!(
        client
            .ingest(
                "ingest-rejected",
                IngestionScope::new("tenant", "library"),
                "rejected",
                BTreeMap::new(),
                "text/plain",
                b"content".to_vec(),
            )
            .await,
        Err(ClientError::Remote {
            code: fm_semantic_protocol::v1::ErrorCode::Unavailable,
            ..
        })
    ));
    assert!(ingestion.await.unwrap().is_ok());
    task.await.unwrap().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_connectors_elect_one_worker_and_share_it() {
    use std::os::unix::fs::PermissionsExt;

    let directory = test_directory("concurrent-launch");
    let executable =
        std::fs::canonicalize(env!("CARGO_BIN_EXE_fm-semantic-worker")).expect("worker executable");
    let first =
        WorkerConnector::desktop(&directory, &executable).with_idle_timeout(Duration::from_secs(2));
    let second =
        WorkerConnector::desktop(&directory, &executable).with_idle_timeout(Duration::from_secs(2));

    let (first, second) = tokio::join!(first.connect(), second.connect());
    let first = first.unwrap();
    let second = second.unwrap();
    assert_eq!(first.health().await.unwrap(), WorkerHealth::Serving);
    assert_eq!(second.health().await.unwrap(), WorkerHealth::Serving);
    assert_eq!(
        std::fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(directory.join("launch.secret"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );

    drop(second);
    shutdown_when_last(&first).await;
}

#[tokio::test]
async fn desktop_connector_reaps_the_worker_process_after_it_exits() {
    let directory = test_directory("child-reaping");
    let executable =
        std::fs::canonicalize(env!("CARGO_BIN_EXE_fm-semantic-worker")).expect("worker executable");
    let connector =
        WorkerConnector::desktop(&directory, &executable).with_idle_timeout(Duration::from_secs(2));
    let client = connector.connect().await.unwrap();
    let raw_pid = std::fs::read_to_string(directory.join("worker.pid"))
        .unwrap()
        .trim()
        .parse::<i32>()
        .unwrap();
    let pid = rustix::process::Pid::from_raw(raw_pid).unwrap();

    client.shutdown(Duration::from_millis(100)).await.unwrap();
    for _ in 0..100 {
        if !directory.join("worker.pid").exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    tokio::time::sleep(Duration::from_millis(50)).await;

    assert!(
        rustix::process::waitpid(Some(pid), rustix::process::WaitOptions::NOHANG).is_err(),
        "worker child remained waitable after exit"
    );
}

#[tokio::test]
async fn desktop_connector_waits_for_process_exit_and_endpoint_removal_after_shutdown() {
    let directory = test_directory("confirmed-shutdown");
    let executable = PathBuf::from(env!("CARGO_BIN_EXE_fm-semantic-worker"));
    let connector =
        WorkerConnector::desktop(&directory, &executable).with_idle_timeout(Duration::from_secs(2));
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let client = connector.connect().await.unwrap();

    client.shutdown(Duration::from_millis(100)).await.unwrap();
    drop(client);
    connector
        .wait_until_stopped(Duration::from_secs(2))
        .await
        .unwrap();

    assert!(
        !endpoint.exists(),
        "restart returned before endpoint removal"
    );
    assert!(
        !directory.join("worker.pid").exists(),
        "restart returned before process cleanup"
    );
}

#[cfg(all(unix, feature = "developer-bundle"))]
fn native_zvec_library_directory(executable: &std::path::Path) -> PathBuf {
    let build_directory = executable.parent().expect("target directory").join("build");
    std::fs::read_dir(build_directory)
        .expect("build directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("out/zvec-prebuilt"))
        .find(|candidate| {
            std::fs::read_dir(candidate).is_ok_and(|entries| {
                entries.filter_map(Result::ok).any(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with("libzvec_c_api")
                })
            })
        })
        .expect("Zvec native library directory")
}

#[cfg(all(unix, feature = "developer-bundle"))]
#[tokio::test]
async fn developer_connector_allows_real_model_cold_start_time_before_binding() {
    use std::os::unix::fs::PermissionsExt;

    let directory = test_directory("delayed-developer-launch");
    let runtime_directory = std::fs::canonicalize(&directory).expect("runtime directory");
    let executable =
        std::fs::canonicalize(env!("CARGO_BIN_EXE_fm-semantic-worker")).expect("worker executable");
    let native_library_directory = native_zvec_library_directory(&executable);
    let wrapper = directory.join("delayed-worker");
    let log = runtime_directory.join("delayed-worker.log");
    let quoted = executable.display().to_string().replace('\'', "'\\''");
    let quoted_log = log.display().to_string().replace('\'', "'\\''");
    let quoted_native = native_library_directory
        .display()
        .to_string()
        .replace('\'', "'\\''");
    let library_path_variable = if cfg!(target_os = "macos") {
        "DYLD_LIBRARY_PATH"
    } else {
        "LD_LIBRARY_PATH"
    };
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\n/bin/sleep 3\nexport {library_path_variable}='{quoted_native}'\n\
             exec '{quoted}' \"$@\" 2>'{quoted_log}'\n"
        ),
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&wrapper).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&wrapper, permissions).unwrap();
    let wrapper = std::fs::canonicalize(wrapper).expect("wrapper");
    let connector = WorkerConnector::desktop(&directory, &wrapper)
        .with_developer_native_library_directory(&native_library_directory)
        .with_startup_timeout(Duration::from_secs(30))
        .with_idle_timeout(Duration::from_secs(2));

    let client = connector.connect().await.unwrap_or_else(|error| {
        panic!(
            "delayed developer worker: {error}; log: {}",
            std::fs::read_to_string(log).unwrap_or_else(|read_error| read_error.to_string())
        )
    });
    client.shutdown(Duration::from_millis(100)).await.unwrap();
    drop(client);
    connector
        .wait_until_stopped(Duration::from_secs(2))
        .await
        .unwrap();
}

#[cfg(all(unix, feature = "developer-bundle"))]
#[tokio::test]
async fn a_new_developer_host_replaces_a_worker_from_the_previous_host() {
    use std::os::unix::fs::PermissionsExt;

    let directory = test_directory("developer-host-restart");
    let executable =
        std::fs::canonicalize(env!("CARGO_BIN_EXE_fm-semantic-worker")).expect("worker executable");
    let native_library_directory = native_zvec_library_directory(&executable);
    let data_directory = std::fs::canonicalize(&directory)
        .unwrap()
        .join("developer-data");
    let resolver: fm_semantic_worker::DeveloperModelPackResolver = Arc::new(|| Ok(None));
    let first_connector = WorkerConnector::desktop_developer(
        &directory,
        &executable,
        &data_directory,
        Some(&native_library_directory),
    )
    .with_developer_model_pack_resolver(resolver.clone());
    let first = first_connector.connect().await.expect("first worker");
    let first_pid = std::fs::read_to_string(directory.join("worker.pid")).unwrap();
    drop(first);

    let second_connector = WorkerConnector::desktop_developer(
        &directory,
        &executable,
        &data_directory,
        Some(&native_library_directory),
    )
    .with_developer_model_pack_resolver(resolver);
    let second = second_connector.connect().await.unwrap_or_else(|error| {
        let socket = directory.join("semantic-worker.sock");
        let metadata = std::fs::symlink_metadata(&socket).ok();
        panic!(
            "replacement worker: {error}; socket_exists={}; socket_mode={:?}; socket_uid={:?}; current_uid={}",
            socket.exists(),
            metadata.as_ref().map(|value| value.permissions().mode() & 0o777),
            metadata.as_ref().map(std::os::unix::fs::MetadataExt::uid),
            rustix::process::geteuid().as_raw(),
        )
    });
    let second_pid = std::fs::read_to_string(directory.join("worker.pid")).unwrap();
    assert_ne!(first_pid, second_pid);

    second.shutdown(Duration::from_millis(100)).await.unwrap();
    drop(second);
    second_connector
        .wait_until_stopped(Duration::from_secs(2))
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn connector_replaces_a_crashed_worker_on_the_next_call() {
    let directory = test_directory("crash-restart");
    let executable = PathBuf::from(env!("CARGO_BIN_EXE_fm-semantic-worker"));
    let connector =
        WorkerConnector::desktop(&directory, &executable).with_idle_timeout(Duration::from_secs(2));
    let client = connector.connect().await.unwrap();
    let first_pid = std::fs::read_to_string(directory.join("worker.pid"))
        .unwrap()
        .trim()
        .to_owned();
    assert!(
        std::process::Command::new("kill")
            .args(["-9", &first_pid])
            .status()
            .unwrap()
            .success()
    );
    drop(client);
    tokio::time::sleep(Duration::from_millis(50)).await;

    let restarted = connector.connect().await.unwrap();
    let second_pid = std::fs::read_to_string(directory.join("worker.pid"))
        .unwrap()
        .trim()
        .to_owned();
    assert_ne!(first_pid, second_pid);
    assert_eq!(restarted.health().await.unwrap(), WorkerHealth::Serving);
    restarted
        .shutdown(Duration::from_millis(100))
        .await
        .unwrap();
}

#[tokio::test]
async fn disconnect_cancels_active_ingestion_without_committing_it() {
    let directory = test_directory("disconnect");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let secret = LaunchSecret::generate();
    let task = tokio::spawn(
        WorkerServer::new(
            WorkerConfig::new(endpoint.clone(), secret.clone())
                .with_atomic_ingestion_delay(Duration::from_millis(100)),
        )
        .run(),
    );
    wait_for_endpoint(&endpoint).await;
    let client = WorkerClient::connect(&endpoint, secret.clone())
        .await
        .unwrap();
    let ingestion = tokio::spawn(async move {
        client
            .ingest(
                "ingest-must-not-commit",
                IngestionScope::new("tenant", "library"),
                "must-not-commit",
                BTreeMap::new(),
                "text/plain",
                b"needle".to_vec(),
            )
            .await
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    ingestion.abort();
    tokio::time::sleep(Duration::from_millis(120)).await;

    let observer = WorkerClient::connect(&endpoint, secret).await.unwrap();
    assert!(
        observer
            .query("tenant", "library", "needle", 10)
            .await
            .unwrap()
            .is_empty()
    );
    observer.shutdown(Duration::from_millis(100)).await.unwrap();
    task.await.unwrap().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn aborting_an_ingestion_future_cancels_work_without_closing_the_client_connection() {
    let directory = test_directory("aborted-ingestion");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let secret = LaunchSecret::from_bytes([79; 32]);
    let limits = ProtocolLimits::new(
        512,
        1024,
        1,
        Duration::from_secs(1),
        Duration::from_secs(2),
        Duration::from_secs(1),
    )
    .unwrap();
    let task = tokio::spawn(
        WorkerServer::new(
            WorkerConfig::new(endpoint.clone(), secret.clone())
                .with_limits(limits)
                .with_atomic_ingestion_delay(Duration::from_secs(5)),
        )
        .run(),
    );
    wait_for_endpoint(&endpoint).await;
    let client = WorkerClient::connect(&endpoint, secret).await.unwrap();
    let ingesting = client.clone();
    let ingestion = tokio::spawn(async move {
        ingesting
            .ingest(
                "aborted-ingestion",
                IngestionScope::new("tenant", "library"),
                "must-not-commit",
                BTreeMap::new(),
                "text/plain",
                b"needle".to_vec(),
            )
            .await
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    wait_for_limit_reached(&client).await;

    ingestion.abort();
    ingestion.await.unwrap_err();
    wait_for_serving_health(&client).await;
    assert!(
        client
            .query("tenant", "library", "needle", 1)
            .await
            .unwrap()
            .is_empty()
    );

    client.shutdown(Duration::from_millis(100)).await.unwrap();
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn malformed_frame_disconnect_cancels_connection_owned_ingestion() {
    use tokio::io::AsyncWriteExt;

    let directory = test_directory("malformed-frame-disconnect");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let secret = LaunchSecret::from_bytes([71; 32]);
    let task = tokio::spawn(
        WorkerServer::new(
            WorkerConfig::new(endpoint.clone(), secret.clone())
                .with_atomic_ingestion_delay(Duration::from_millis(150)),
        )
        .run(),
    );
    wait_for_endpoint(&endpoint).await;
    let observer = WorkerClient::connect(&endpoint, secret.clone())
        .await
        .unwrap();
    let (mut raw, session) = raw_authenticated_stream(&endpoint, &secret).await;
    write_frame(
        &mut raw,
        &v1::ClientFrame {
            correlation_id: 91,
            payload: Some(v1::client_frame::Payload::Ingestion(v1::IngestionRequest {
                session: Some(session.clone()),
                scope: Some(v1::ResourceScope {
                    tenant_id: "tenant".to_owned(),
                    library_id: "library".to_owned(),
                }),
                request_id: "malformed-owner".to_owned(),
                payload: Some(v1::ingestion_request::Payload::Start(v1::IngestionStart {
                    document_id: "must-not-commit".to_owned(),
                    metadata: Vec::new(),
                    media_type: "text/plain".to_owned(),
                    expected_content_bytes: 6,
                })),
            })),
        },
        MAX_MESSAGE_BYTES,
    )
    .await
    .unwrap();
    write_frame(
        &mut raw,
        &v1::ClientFrame {
            correlation_id: 91,
            payload: Some(v1::client_frame::Payload::Ingestion(v1::IngestionRequest {
                session: Some(session),
                scope: Some(v1::ResourceScope {
                    tenant_id: "tenant".to_owned(),
                    library_id: "library".to_owned(),
                }),
                request_id: "malformed-owner".to_owned(),
                payload: Some(v1::ingestion_request::Payload::Chunk(
                    v1::FileContentChunk {
                        sequence: 0,
                        content: b"needle".to_vec(),
                        end_of_stream: true,
                    },
                )),
            })),
        },
        MAX_MESSAGE_BYTES,
    )
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;
    raw.write_all(&[0, 0, 0, 1, 0xff]).await.unwrap();
    raw.shutdown().await.unwrap();
    tokio::time::sleep(Duration::from_millis(180)).await;

    assert!(
        observer
            .query("tenant", "library", "needle", 1)
            .await
            .unwrap()
            .is_empty()
    );
    observer.shutdown(Duration::from_millis(100)).await.unwrap();
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn incompatible_clients_receive_a_typed_required_update() {
    let directory = test_directory("incompatible");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let secret = LaunchSecret::generate();
    let task =
        tokio::spawn(WorkerServer::new(WorkerConfig::new(endpoint.clone(), secret.clone())).run());
    wait_for_endpoint(&endpoint).await;

    assert!(matches!(
        WorkerClient::connect_with_versions(&endpoint, secret.clone(), 2, 3).await,
        Err(ClientError::Incompatible {
            code: fm_semantic_protocol::v1::ErrorCode::WorkerUpdateRequired,
            ..
        })
    ));
    let client = WorkerClient::connect(&endpoint, secret).await.unwrap();
    client.shutdown(Duration::from_millis(100)).await.unwrap();
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn desktop_discovery_preserves_a_live_workers_authentication_error_and_secret() {
    use std::os::unix::fs::PermissionsExt;

    let directory = test_directory("live-worker-authentication");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let live_secret = LaunchSecret::from_bytes([31; 32]);
    let stale_secret = LaunchSecret::from_bytes([32; 32]);
    let secret_path = directory.join("launch.secret");
    std::fs::write(&secret_path, stale_secret.as_bytes()).unwrap();
    std::fs::set_permissions(&secret_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let task = tokio::spawn(
        WorkerServer::new(WorkerConfig::new(endpoint.clone(), live_secret.clone())).run(),
    );
    wait_for_endpoint(&endpoint).await;

    let connector = WorkerConnector::desktop(&directory, &directory.join("must-not-launch"));
    assert!(matches!(
        connector.connect().await,
        Err(ClientError::Unauthenticated)
    ));
    assert_eq!(
        std::fs::read(&secret_path).unwrap(),
        stale_secret.as_bytes()
    );

    let observer = WorkerClient::connect(&endpoint, live_secret).await.unwrap();
    assert_eq!(observer.health().await.unwrap(), WorkerHealth::Serving);
    observer.shutdown(Duration::from_millis(100)).await.unwrap();
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn desktop_discovery_preserves_a_live_workers_compatibility_error_and_secret() {
    use std::os::unix::fs::PermissionsExt;

    let directory = test_directory("live-worker-compatibility");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let secret = LaunchSecret::from_bytes([41; 32]);
    let secret_path = directory.join("launch.secret");
    std::fs::write(&secret_path, secret.as_bytes()).unwrap();
    std::fs::set_permissions(&secret_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let versions = VersionRange::new(ProtocolVersion::new(2), ProtocolVersion::new(2)).unwrap();
    let task = tokio::spawn(
        WorkerServer::new(
            WorkerConfig::new(endpoint.clone(), secret.clone()).with_versions(versions),
        )
        .run(),
    );
    wait_for_endpoint(&endpoint).await;

    let connector = WorkerConnector::desktop(&directory, &directory.join("must-not-launch"));
    assert!(matches!(
        connector.connect().await,
        Err(ClientError::Incompatible {
            code: fm_semantic_protocol::v1::ErrorCode::ClientUpdateRequired,
            ..
        })
    ));
    assert_eq!(std::fs::read(&secret_path).unwrap(), secret.as_bytes());

    task.abort();
}

#[tokio::test]
async fn client_write_backpressure_is_bounded_for_the_writer_and_waiters() {
    let directory = test_directory("client-write-backpressure");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let Endpoint::Unix(path) = &endpoint;
    let listener = bind_owner_only_listener(path);
    let peer = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let negotiate = read_frame::<_, v1::ClientFrame>(&mut stream, MAX_MESSAGE_BYTES)
            .await
            .unwrap();
        write_frame(
            &mut stream,
            &v1::ServerFrame {
                correlation_id: negotiate.correlation_id,
                payload: Some(v1::server_frame::Payload::Negotiated(
                    v1::NegotiateResponse {
                        selected_version: 1,
                        capabilities: Vec::new(),
                        limits: Some(v1::ProtocolLimits {
                            maximum_message_bytes: MAX_MESSAGE_BYTES as u64,
                            maximum_stream_bytes: 8 * 1024 * 1024,
                            maximum_concurrent_requests: 2,
                            request_deadline_ms: 100,
                            stream_deadline_ms: 40,
                            shutdown_deadline_ms: 100,
                        }),
                    },
                )),
            },
            MAX_MESSAGE_BYTES,
        )
        .await
        .unwrap();
        write_frame(
            &mut stream,
            &v1::ServerFrame {
                correlation_id: negotiate.correlation_id,
                payload: Some(v1::server_frame::Payload::StreamEnd(v1::StreamEnd {})),
            },
            MAX_MESSAGE_BYTES,
        )
        .await
        .unwrap();
        let open_session = read_frame::<_, v1::ClientFrame>(&mut stream, MAX_MESSAGE_BYTES)
            .await
            .unwrap();
        write_frame(
            &mut stream,
            &v1::ServerFrame {
                correlation_id: open_session.correlation_id,
                payload: Some(v1::server_frame::Payload::SessionOpened(
                    v1::OpenSessionResponse {
                        session_id: "stalled-peer".to_owned(),
                        session_token: vec![7; 32],
                        expires_at_unix_ms: u64::MAX,
                    },
                )),
            },
            MAX_MESSAGE_BYTES,
        )
        .await
        .unwrap();
        write_frame(
            &mut stream,
            &v1::ServerFrame {
                correlation_id: open_session.correlation_id,
                payload: Some(v1::server_frame::Payload::StreamEnd(v1::StreamEnd {})),
            },
            MAX_MESSAGE_BYTES,
        )
        .await
        .unwrap();

        tokio::time::sleep(Duration::from_secs(2)).await;
    });
    let client = WorkerClient::connect(&endpoint, LaunchSecret::from_bytes([72; 32]))
        .await
        .unwrap();
    let ingesting = client.clone();
    let ingestion = tokio::spawn(async move {
        ingesting
            .ingest(
                "blocked-write",
                IngestionScope::new("tenant", "library"),
                "document",
                BTreeMap::new(),
                "application/octet-stream",
                vec![0; 8 * 1024 * 1024],
            )
            .await
    });
    tokio::time::sleep(Duration::from_millis(10)).await;
    let health = tokio::time::timeout(Duration::from_millis(500), async {
        let health = client.health().await;
        let ingestion = ingestion.await.unwrap();
        (health, ingestion)
    })
    .await
    .expect("socket backpressure outlived negotiated operation deadlines");

    assert!(matches!(
        health,
        (
            Err(ClientError::Remote {
                code: v1::ErrorCode::DeadlineExceeded,
                ..
            } | ClientError::Disconnected),
            Err(ClientError::Remote {
                code: v1::ErrorCode::DeadlineExceeded,
                ..
            } | ClientError::Disconnected),
        )
    ));
    peer.abort();
}

#[tokio::test]
async fn non_reading_client_releases_server_capacity_at_the_request_deadline() {
    let directory = test_directory("server-write-backpressure");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let secret = LaunchSecret::from_bytes([73; 32]);
    let limits = ProtocolLimits::new(
        512,
        1024,
        1,
        Duration::from_millis(100),
        Duration::from_millis(40),
        Duration::from_millis(100),
    )
    .unwrap();
    let task = tokio::spawn(
        WorkerServer::new(
            WorkerConfig::new(endpoint.clone(), secret.clone())
                .with_limits(limits)
                .with_test_query_write_delay(Duration::from_secs(5)),
        )
        .run(),
    );
    wait_for_endpoint(&endpoint).await;
    let observer = WorkerClient::connect(&endpoint, secret.clone())
        .await
        .unwrap();
    observer
        .ingest(
            "write-deadline-fixture",
            IngestionScope::new("tenant", "library"),
            "document",
            BTreeMap::new(),
            "text/plain",
            b"needle".to_vec(),
        )
        .await
        .unwrap();
    let (mut non_reader, session) = raw_authenticated_stream(&endpoint, &secret).await;
    write_frame(
        &mut non_reader,
        &v1::ClientFrame {
            correlation_id: 1_000,
            payload: Some(v1::client_frame::Payload::Query(v1::QueryRequest {
                session: Some(session),
                scope: Some(v1::ResourceScope {
                    tenant_id: "tenant".to_owned(),
                    library_id: "library".to_owned(),
                }),
                request_id: "blocked-query-write".to_owned(),
                query: "needle".to_owned(),
                maximum_results: 1,
                concept_query: None,
            })),
        },
        512,
    )
    .await
    .unwrap();

    tokio::time::timeout(Duration::from_millis(500), async {
        loop {
            if matches!(
                observer.health().await,
                Err(ClientError::Remote {
                    code: v1::ErrorCode::LimitExceeded,
                    ..
                })
            ) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("delayed query write did not occupy server capacity");

    tokio::time::timeout(Duration::from_millis(500), async {
        loop {
            match observer.health().await {
                Ok(WorkerHealth::Serving) => break,
                Err(ClientError::Remote {
                    code: v1::ErrorCode::LimitExceeded,
                    ..
                }) => tokio::task::yield_now().await,
                result => panic!("unexpected health result: {result:?}"),
            }
        }
    })
    .await
    .expect("non-reading client retained server capacity past the request deadline");

    drop(non_reader);
    shutdown_when_last(&observer).await;
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn rejects_malicious_negotiated_limits_before_sending_the_launch_secret() {
    let directory = test_directory("malicious-negotiation");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let Endpoint::Unix(path) = &endpoint;
    let listener = bind_owner_only_listener(path);
    let peer = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let negotiate = read_frame::<_, v1::ClientFrame>(&mut stream, MAX_MESSAGE_BYTES)
            .await
            .unwrap();
        assert!(matches!(
            negotiate.payload,
            Some(v1::client_frame::Payload::Negotiate(_))
        ));
        write_frame(
            &mut stream,
            &v1::ServerFrame {
                correlation_id: negotiate.correlation_id,
                payload: Some(v1::server_frame::Payload::Negotiated(
                    v1::NegotiateResponse {
                        selected_version: 1,
                        capabilities: Vec::new(),
                        limits: Some(v1::ProtocolLimits {
                            maximum_message_bytes: u64::from(u32::MAX),
                            maximum_stream_bytes: MAX_STREAM_BYTES,
                            maximum_concurrent_requests: MAX_CONCURRENT_REQUESTS as u32,
                            request_deadline_ms: REQUEST_DEADLINE.as_millis() as u64,
                            stream_deadline_ms: STREAM_DEADLINE.as_millis() as u64,
                            shutdown_deadline_ms: SHUTDOWN_DEADLINE.as_millis() as u64,
                        }),
                    },
                )),
            },
            MAX_MESSAGE_BYTES,
        )
        .await
        .unwrap();
        write_frame(
            &mut stream,
            &v1::ServerFrame {
                correlation_id: negotiate.correlation_id,
                payload: Some(v1::server_frame::Payload::StreamEnd(v1::StreamEnd {})),
            },
            MAX_MESSAGE_BYTES,
        )
        .await
        .unwrap();

        tokio::time::timeout(
            Duration::from_millis(100),
            read_frame::<_, v1::ClientFrame>(&mut stream, MAX_MESSAGE_BYTES),
        )
        .await
        .ok()
        .and_then(Result::ok)
        .is_some()
    });

    assert!(matches!(
        WorkerClient::connect(&endpoint, LaunchSecret::from_bytes([51; 32])).await,
        Err(ClientError::InvalidNegotiatedLimits(_))
    ));
    assert!(
        !peer.await.unwrap(),
        "client sent authentication material after unsafe negotiation"
    );
}

#[tokio::test]
async fn rejects_an_out_of_range_selected_version_before_sending_the_launch_secret() {
    let directory = test_directory("malicious-selected-version");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let Endpoint::Unix(path) = &endpoint;
    let listener = bind_owner_only_listener(path);
    let peer = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let negotiate = read_frame::<_, v1::ClientFrame>(&mut stream, MAX_MESSAGE_BYTES)
            .await
            .unwrap();
        write_frame(
            &mut stream,
            &v1::ServerFrame {
                correlation_id: negotiate.correlation_id,
                payload: Some(v1::server_frame::Payload::Negotiated(
                    v1::NegotiateResponse {
                        selected_version: 2,
                        capabilities: Vec::new(),
                        limits: Some(v1::ProtocolLimits {
                            maximum_message_bytes: MAX_MESSAGE_BYTES as u64,
                            maximum_stream_bytes: MAX_STREAM_BYTES,
                            maximum_concurrent_requests: MAX_CONCURRENT_REQUESTS as u32,
                            request_deadline_ms: REQUEST_DEADLINE.as_millis() as u64,
                            stream_deadline_ms: STREAM_DEADLINE.as_millis() as u64,
                            shutdown_deadline_ms: SHUTDOWN_DEADLINE.as_millis() as u64,
                        }),
                    },
                )),
            },
            MAX_MESSAGE_BYTES,
        )
        .await
        .unwrap();
        write_frame(
            &mut stream,
            &v1::ServerFrame {
                correlation_id: negotiate.correlation_id,
                payload: Some(v1::server_frame::Payload::StreamEnd(v1::StreamEnd {})),
            },
            MAX_MESSAGE_BYTES,
        )
        .await
        .unwrap();

        tokio::time::timeout(
            Duration::from_millis(100),
            read_frame::<_, v1::ClientFrame>(&mut stream, MAX_MESSAGE_BYTES),
        )
        .await
        .ok()
        .and_then(Result::ok)
        .is_some()
    });

    assert!(matches!(
        WorkerClient::connect_with_versions(&endpoint, LaunchSecret::from_bytes([87; 32]), 1, 1,)
            .await,
        Err(ClientError::InvalidNegotiatedVersion)
    ));
    assert!(
        !peer.await.unwrap(),
        "client sent authentication material after an invalid version selection"
    );
}

#[tokio::test]
async fn client_rejects_a_peer_that_exceeds_the_negotiated_stream_budget() {
    let directory = test_directory("malicious-response-stream");
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let Endpoint::Unix(path) = &endpoint;
    let listener = bind_owner_only_listener(path);
    let peer = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let negotiate = read_frame::<_, v1::ClientFrame>(&mut stream, MAX_MESSAGE_BYTES)
            .await
            .unwrap();
        write_frame(
            &mut stream,
            &v1::ServerFrame {
                correlation_id: negotiate.correlation_id,
                payload: Some(v1::server_frame::Payload::Negotiated(
                    v1::NegotiateResponse {
                        selected_version: 1,
                        capabilities: Vec::new(),
                        limits: Some(v1::ProtocolLimits {
                            maximum_message_bytes: 512,
                            maximum_stream_bytes: 600,
                            maximum_concurrent_requests: 2,
                            request_deadline_ms: 1_000,
                            stream_deadline_ms: 1_000,
                            shutdown_deadline_ms: 1_000,
                        }),
                    },
                )),
            },
            MAX_MESSAGE_BYTES,
        )
        .await
        .unwrap();
        write_frame(
            &mut stream,
            &v1::ServerFrame {
                correlation_id: negotiate.correlation_id,
                payload: Some(v1::server_frame::Payload::StreamEnd(v1::StreamEnd {})),
            },
            MAX_MESSAGE_BYTES,
        )
        .await
        .unwrap();
        let open = read_frame::<_, v1::ClientFrame>(&mut stream, 512)
            .await
            .unwrap();
        write_frame(
            &mut stream,
            &v1::ServerFrame {
                correlation_id: open.correlation_id,
                payload: Some(v1::server_frame::Payload::SessionOpened(
                    v1::OpenSessionResponse {
                        session_id: "fake-session".to_owned(),
                        session_token: vec![1],
                        expires_at_unix_ms: u64::MAX,
                    },
                )),
            },
            512,
        )
        .await
        .unwrap();
        write_frame(
            &mut stream,
            &v1::ServerFrame {
                correlation_id: open.correlation_id,
                payload: Some(v1::server_frame::Payload::StreamEnd(v1::StreamEnd {})),
            },
            512,
        )
        .await
        .unwrap();
        let query = read_frame::<_, v1::ClientFrame>(&mut stream, 512)
            .await
            .unwrap();
        for index in 0..4 {
            write_frame(
                &mut stream,
                &v1::ServerFrame {
                    correlation_id: query.correlation_id,
                    payload: Some(v1::server_frame::Payload::QueryEvent(v1::QueryEvent {
                        payload: Some(v1::query_event::Payload::Result(v1::QueryResult {
                            document_id: format!("document-{index}"),
                            score: 1.0,
                            metadata: Vec::new(),
                            excerpt: "x".repeat(220),
                        })),
                    })),
                },
                512,
            )
            .await
            .unwrap();
        }
        let _ = write_frame(
            &mut stream,
            &v1::ServerFrame {
                correlation_id: query.correlation_id,
                payload: Some(v1::server_frame::Payload::StreamEnd(v1::StreamEnd {})),
            },
            512,
        )
        .await;
    });

    let client = WorkerClient::connect(&endpoint, LaunchSecret::from_bytes([52; 32]))
        .await
        .unwrap();
    assert!(matches!(
        client.query("tenant", "library", "anything", 10).await,
        Err(ClientError::Remote {
            code: v1::ErrorCode::LimitExceeded,
            ..
        })
    ));
    peer.await.unwrap();
}

async fn wait_for_endpoint(endpoint: &Endpoint) {
    for _ in 0..100 {
        if endpoint.exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("worker endpoint was not created");
}

fn bind_owner_only_listener(path: &std::path::Path) -> tokio::net::UnixListener {
    use std::os::unix::fs::PermissionsExt;

    let listener = tokio::net::UnixListener::bind(path).expect("bind test listener");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .expect("protect test listener");
    listener
}

async fn wait_for_limit_reached(client: &WorkerClient) {
    tokio::time::timeout(Duration::from_millis(500), async {
        loop {
            match client.health().await {
                Err(ClientError::Remote {
                    code: v1::ErrorCode::LimitExceeded,
                    ..
                }) => break,
                Ok(_) => tokio::task::yield_now().await,
                Err(error) => panic!("unexpected health result: {error}"),
            }
        }
    })
    .await
    .expect("request did not occupy server capacity");
}

async fn wait_for_serving_health(client: &WorkerClient) {
    tokio::time::timeout(Duration::from_millis(500), async {
        loop {
            match client.health().await {
                Ok(WorkerHealth::Serving) => break,
                Err(ClientError::Remote {
                    code: v1::ErrorCode::LimitExceeded,
                    ..
                }) => tokio::task::yield_now().await,
                result => panic!("unexpected health result: {result:?}"),
            }
        }
    })
    .await
    .expect("cancelled request did not release server capacity");
}

async fn shutdown_when_last(client: &WorkerClient) {
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            match client.shutdown(Duration::from_millis(100)).await {
                Ok(()) => break,
                Err(ClientError::ShutdownBlocked { .. }) => tokio::task::yield_now().await,
                Err(error) => panic!("unexpected shutdown failure: {error}"),
            }
        }
    })
    .await
    .expect("remaining client could not stop the shared worker");
}

async fn raw_authenticated_stream(
    endpoint: &Endpoint,
    secret: &LaunchSecret,
) -> (tokio::net::UnixStream, v1::SessionContext) {
    let Endpoint::Unix(path) = endpoint;
    let mut stream = tokio::net::UnixStream::connect(path)
        .await
        .expect("connect raw test client");
    write_frame(
        &mut stream,
        &v1::ClientFrame {
            correlation_id: 1,
            payload: Some(v1::client_frame::Payload::Negotiate(v1::NegotiateRequest {
                minimum_version: 1,
                maximum_version: 1,
                requested_capabilities: Vec::new(),
            })),
        },
        MAX_MESSAGE_BYTES,
    )
    .await
    .expect("write raw negotiation");
    let negotiated = read_frame::<_, v1::ServerFrame>(&mut stream, MAX_MESSAGE_BYTES)
        .await
        .expect("read raw negotiation response");
    assert!(matches!(
        negotiated.payload,
        Some(v1::server_frame::Payload::Negotiated(_))
    ));
    let _end = read_frame::<_, v1::ServerFrame>(&mut stream, MAX_MESSAGE_BYTES)
        .await
        .expect("read raw negotiation end");
    write_frame(
        &mut stream,
        &v1::ClientFrame {
            correlation_id: 2,
            payload: Some(v1::client_frame::Payload::OpenSession(
                v1::OpenSessionRequest {
                    authentication_token: secret.as_bytes().to_vec(),
                },
            )),
        },
        MAX_MESSAGE_BYTES,
    )
    .await
    .expect("write raw authentication");
    let opened = read_frame::<_, v1::ServerFrame>(&mut stream, MAX_MESSAGE_BYTES)
        .await
        .expect("read raw authentication response");
    let Some(v1::server_frame::Payload::SessionOpened(opened)) = opened.payload else {
        panic!("worker did not open raw test session");
    };
    let _end = read_frame::<_, v1::ServerFrame>(&mut stream, MAX_MESSAGE_BYTES)
        .await
        .expect("read raw authentication end");
    (
        stream,
        v1::SessionContext {
            session_id: opened.session_id,
            session_token: opened.session_token,
        },
    )
}

async fn write_ingestion_start(
    stream: &mut tokio::net::UnixStream,
    correlation_id: u64,
    session: v1::SessionContext,
    request_id: &str,
    scope: (&str, &str),
    document_id: &str,
    expected_content_bytes: u64,
) {
    write_frame(
        stream,
        &v1::ClientFrame {
            correlation_id,
            payload: Some(v1::client_frame::Payload::Ingestion(v1::IngestionRequest {
                session: Some(session),
                scope: Some(v1::ResourceScope {
                    tenant_id: scope.0.to_owned(),
                    library_id: scope.1.to_owned(),
                }),
                request_id: request_id.to_owned(),
                payload: Some(v1::ingestion_request::Payload::Start(v1::IngestionStart {
                    document_id: document_id.to_owned(),
                    metadata: Vec::new(),
                    media_type: "text/plain".to_owned(),
                    expected_content_bytes,
                })),
            })),
        },
        MAX_MESSAGE_BYTES,
    )
    .await
    .expect("write ingestion start");
}

async fn write_ingestion_chunk(
    stream: &mut tokio::net::UnixStream,
    correlation_id: u64,
    session: v1::SessionContext,
    request_id: &str,
    scope: (&str, &str),
    chunk: (u64, &[u8], bool),
) {
    write_frame(
        stream,
        &v1::ClientFrame {
            correlation_id,
            payload: Some(v1::client_frame::Payload::Ingestion(v1::IngestionRequest {
                session: Some(session),
                scope: Some(v1::ResourceScope {
                    tenant_id: scope.0.to_owned(),
                    library_id: scope.1.to_owned(),
                }),
                request_id: request_id.to_owned(),
                payload: Some(v1::ingestion_request::Payload::Chunk(
                    v1::FileContentChunk {
                        sequence: chunk.0,
                        content: chunk.1.to_vec(),
                        end_of_stream: chunk.2,
                    },
                )),
            })),
        },
        MAX_MESSAGE_BYTES,
    )
    .await
    .expect("write ingestion chunk");
}

async fn assert_invalid_request(stream: &mut tokio::net::UnixStream, correlation_id: u64) {
    let response = read_frame::<_, v1::ServerFrame>(stream, MAX_MESSAGE_BYTES)
        .await
        .expect("read invalid ingestion response");
    assert_eq!(response.correlation_id, correlation_id);
    assert!(matches!(
        response.payload,
        Some(v1::server_frame::Payload::Error(v1::ProtocolError {
            code,
            ..
        })) if code == v1::ErrorCode::InvalidRequest as i32
    ));
    let end = read_frame::<_, v1::ServerFrame>(stream, MAX_MESSAGE_BYTES)
        .await
        .expect("read invalid ingestion stream end");
    assert!(matches!(
        end.payload,
        Some(v1::server_frame::Payload::StreamEnd(_))
    ));
}

async fn wait_for_active_request(endpoint: &Endpoint, secret: &LaunchSecret) {
    let (mut stream, session) = raw_authenticated_stream(endpoint, secret).await;
    for correlation_id in 100..200 {
        write_frame(
            &mut stream,
            &v1::ClientFrame {
                correlation_id,
                payload: Some(v1::client_frame::Payload::Health(v1::HealthRequest {
                    session: Some(session.clone()),
                })),
            },
            MAX_MESSAGE_BYTES,
        )
        .await
        .expect("write health probe");
        let response = read_frame::<_, v1::ServerFrame>(&mut stream, MAX_MESSAGE_BYTES)
            .await
            .expect("read health probe");
        let _end = read_frame::<_, v1::ServerFrame>(&mut stream, MAX_MESSAGE_BYTES)
            .await
            .expect("read health probe end");
        if matches!(
            response.payload,
            Some(v1::server_frame::Payload::Health(v1::HealthResponse {
                active_requests,
                ..
            })) if active_requests >= 2
        ) {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("ingestion did not become active");
}
