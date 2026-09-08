//! Versioned knowledge-retrieval IPC behavior.
//!
//! These tests exercise the real local transport: the host plans and authorizes,
//! the worker retrieves, and no LLM is involved anywhere in the path.

#![cfg(unix)]
#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::ops::Deref;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use fm_semantic_conversion::{ChunkProvenance, Provenance};
use fm_semantic_worker::knowledge_retrieval::{
    KnowledgeCapabilities, KnowledgeEvidence, KnowledgeQuery, KnowledgeRetrieval,
    KnowledgeRetrievalError, KnowledgeRetrievalPolicy, KnowledgeRetrievalReason,
    KnowledgeRetrievalRequest, KnowledgeRetrievalScope, KnowledgeRoute, KnowledgeSourceRestriction,
    RankContribution, RetrievalTrace, RouteFallbackReason, RouteOutcome, TracedEvidence,
    TracedQuery,
};
use fm_semantic_worker::semantic_search::SearchCoverage;
use fm_semantic_worker::semantic_storage::QueryFilters;
use fm_semantic_worker::{
    ClientError, Endpoint, LaunchSecret, WorkerClient, WorkerConfig, WorkerKnowledgeBackend,
    WorkerServer,
};
use tokio_util::sync::CancellationToken;

struct TestDirectory(tempfile::TempDir);

impl Deref for TestDirectory {
    type Target = std::path::Path;

    fn deref(&self) -> &Self::Target {
        self.0.path()
    }
}

fn test_directory() -> TestDirectory {
    TestDirectory(
        tempfile::Builder::new()
            .prefix("fm-knowledge-")
            .tempdir()
            .expect("create isolated worker test directory"),
    )
}

async fn wait_for_endpoint(endpoint: &Endpoint) {
    for _ in 0..200 {
        if endpoint.exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("worker endpoint never appeared");
}

struct FullTextOnlyBackend {
    requests: Mutex<Vec<KnowledgeRetrievalRequest>>,
}

impl FullTextOnlyBackend {
    fn new() -> Self {
        Self {
            requests: Mutex::new(Vec::new()),
        }
    }
}

fn evidence() -> KnowledgeEvidence {
    KnowledgeEvidence {
        record_id: "record-1".to_owned(),
        occurrence_id: "occurrence-1".to_owned(),
        source_id: "source-1".to_owned(),
        duplicate_source_ids: vec!["source-2".to_owned()],
        document_id: "document-1".to_owned(),
        library_id: "library-1".to_owned(),
        chunk_kind: "chunk".to_owned(),
        excerpt: "Rotor blades".to_owned(),
        content: "Rotor blades convert wind into torque.".to_owned(),
        token_count: 9,
        section_path: vec!["Design".to_owned()],
        provenance: ChunkProvenance::Exact(Provenance::TextLines {
            start_line: 3,
            end_line: 8,
        }),
        media_type: Some("text/plain".to_owned()),
        modified_at_ms: Some(1_700_000_000_000),
        indexed_content_hash: "sha256:rotor".to_owned(),
        generation: 2,
        source_position: 4,
        generated: false,
        unavailable: false,
        stale: true,
        adjacent: false,
        final_rank: 1,
    }
}

impl WorkerKnowledgeBackend for FullTextOnlyBackend {
    fn capabilities(&self) -> KnowledgeCapabilities {
        KnowledgeCapabilities {
            full_text: true,
            query_embeddings: false,
        }
    }

    fn search(
        &self,
        request: KnowledgeRetrievalRequest,
        _cancellation: &CancellationToken,
    ) -> Result<KnowledgeRetrieval, KnowledgeRetrievalError> {
        let include_trace = request.policy.include_trace;
        let coverage = request.coverage;
        self.requests.lock().unwrap().push(request);
        let route = RouteOutcome {
            requested: KnowledgeRoute::Hybrid,
            applied: KnowledgeRoute::FullText,
            fallback_reason: Some(RouteFallbackReason::QueryEmbeddingsUnavailable),
        };
        let capabilities = self.capabilities();
        Ok(KnowledgeRetrieval {
            route,
            capabilities,
            evidence: vec![evidence()],
            token_count: 9,
            coverage,
            trace: include_trace.then(|| RetrievalTrace {
                route,
                capabilities,
                rank_constant: 60,
                queries: vec![TracedQuery {
                    text: "wind turbines".to_owned(),
                    reason: KnowledgeRetrievalReason::Subject,
                    semantic_candidates: 0,
                    full_text_candidates: 3,
                }],
                entries: vec![TracedEvidence {
                    record_id: "record-1".to_owned(),
                    occurrence_id: "occurrence-1".to_owned(),
                    source_id: "source-1".to_owned(),
                    document_id: "document-1".to_owned(),
                    contributions: vec![RankContribution {
                        query_index: 0,
                        route: KnowledgeRoute::FullText,
                        rank: 1,
                    }],
                    fused_score: 0.016_393_442_622_950_82,
                    final_rank: 1,
                    provenance: ChunkProvenance::Exact(Provenance::TextLines {
                        start_line: 3,
                        end_line: 8,
                    }),
                    adjacent: false,
                    generated: false,
                    unavailable: false,
                    stale: true,
                }],
            }),
        })
    }
}

fn request() -> KnowledgeRetrievalRequest {
    KnowledgeRetrievalRequest {
        queries: vec![
            KnowledgeQuery {
                text: "wind turbines".to_owned(),
                reason: KnowledgeRetrievalReason::Subject,
            },
            KnowledgeQuery {
                text: "wind turbines definition".to_owned(),
                reason: KnowledgeRetrievalReason::Definition,
            },
        ],
        route: KnowledgeRoute::Hybrid,
        scopes: vec![
            KnowledgeRetrievalScope {
                filters: QueryFilters {
                    tenant_id: "tenant-1".to_owned(),
                    library_id: Some("library-1".to_owned()),
                    root_id: Some("root-1".to_owned()),
                    include_unavailable: true,
                    ..QueryFilters::default()
                },
                source_restriction: KnowledgeSourceRestriction {
                    allowed_source_ids: ["source-1".to_owned(), "source-2".to_owned()]
                        .into_iter()
                        .collect(),
                },
            },
            KnowledgeRetrievalScope {
                filters: QueryFilters {
                    tenant_id: "tenant-1".to_owned(),
                    library_id: Some("library-1".to_owned()),
                    root_id: Some("root-2".to_owned()),
                    include_unavailable: true,
                    ..QueryFilters::default()
                },
                source_restriction: KnowledgeSourceRestriction::default(),
            },
        ],
        current_hashes: HashMap::new(),
        coverage: SearchCoverage {
            eligible: 5,
            indexed: 4,
            unavailable: 1,
            ..SearchCoverage::default()
        },
        policy: KnowledgeRetrievalPolicy {
            include_trace: true,
            ..KnowledgeRetrievalPolicy::default_search()
        },
    }
}

#[tokio::test]
async fn knowledge_search_round_trips_evidence_route_fallback_and_trace_over_ipc() {
    let directory = test_directory();
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let secret = LaunchSecret::from_bytes([37; 32]);
    let backend = Arc::new(FullTextOnlyBackend::new());
    let task = tokio::spawn(
        WorkerServer::with_knowledge_backend(
            WorkerConfig::new(endpoint.clone(), secret.clone()),
            backend.clone(),
        )
        .run(),
    );
    wait_for_endpoint(&endpoint).await;
    let client = WorkerClient::connect(&endpoint, secret).await.unwrap();

    let capabilities = client.knowledge_capabilities().await.unwrap();
    assert!(capabilities.full_text);
    assert!(!capabilities.query_embeddings);

    let retrieval = client
        .knowledge_search("knowledge-request-1", &request())
        .await
        .unwrap();

    assert_eq!(retrieval.route.requested, KnowledgeRoute::Hybrid);
    assert_eq!(retrieval.route.applied, KnowledgeRoute::FullText);
    assert_eq!(
        retrieval.route.fallback_reason,
        Some(RouteFallbackReason::QueryEmbeddingsUnavailable)
    );
    assert_eq!(retrieval.evidence, vec![evidence()]);
    assert_eq!(retrieval.coverage.eligible, 5);
    assert_eq!(retrieval.coverage.unavailable, 1);
    let trace = retrieval.trace.expect("trace was requested");
    assert_eq!(trace.entries[0].contributions[0].rank, 1);

    let received = backend.requests.lock().unwrap().clone();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].scopes.len(), 2);
    assert_eq!(received[0].scopes[0].filters.tenant_id, "tenant-1");
    assert_eq!(
        received[0].scopes[0].filters.root_id.as_deref(),
        Some("root-1")
    );
    assert_eq!(
        received[0].scopes[0]
            .source_restriction
            .allowed_source_ids
            .len(),
        2
    );
    assert_eq!(
        received[0].scopes[1].filters.root_id.as_deref(),
        Some("root-2")
    );
    assert!(
        received[0].scopes[1]
            .source_restriction
            .allowed_source_ids
            .is_empty()
    );
    assert_eq!(received[0].queries.len(), 2);
    assert_eq!(
        received[0].queries[1].reason,
        KnowledgeRetrievalReason::Definition
    );

    drop(client);
    task.abort();
}

#[tokio::test]
async fn a_worker_without_a_knowledge_backend_reports_no_routes_instead_of_empty_results() {
    let directory = test_directory();
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let secret = LaunchSecret::from_bytes([41; 32]);
    let task =
        tokio::spawn(WorkerServer::new(WorkerConfig::new(endpoint.clone(), secret.clone())).run());
    wait_for_endpoint(&endpoint).await;
    let client = WorkerClient::connect(&endpoint, secret).await.unwrap();

    // A worker without a knowledge backend must not advertise the capability,
    // and the client must refuse the RPC locally instead of guessing that an
    // older but protocol-compatible worker will understand it.
    assert!(!client.supports(fm_semantic_protocol::v1::Capability::KnowledgeSearch));
    let error = client
        .knowledge_capabilities()
        .await
        .expect_err("an unconfigured worker never reports knowledge capabilities");
    assert!(
        matches!(
            error,
            ClientError::CapabilityUnavailable {
                capability: fm_semantic_protocol::v1::Capability::KnowledgeSearch
            }
        ),
        "{error:?}"
    );

    let error = client
        .knowledge_search("knowledge-request-2", &request())
        .await
        .expect_err("an unconfigured worker cannot retrieve knowledge");
    assert!(
        matches!(
            error,
            ClientError::CapabilityUnavailable {
                capability: fm_semantic_protocol::v1::Capability::KnowledgeSearch
            }
        ),
        "{error:?}"
    );

    drop(client);
    task.abort();
}

#[tokio::test]
async fn an_unbounded_or_routeless_knowledge_request_is_rejected_before_retrieval() {
    let directory = test_directory();
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let secret = LaunchSecret::from_bytes([43; 32]);
    let backend = Arc::new(FullTextOnlyBackend::new());
    let task = tokio::spawn(
        WorkerServer::with_knowledge_backend(
            WorkerConfig::new(endpoint.clone(), secret.clone()),
            backend.clone(),
        )
        .run(),
    );
    wait_for_endpoint(&endpoint).await;
    let client = WorkerClient::connect(&endpoint, secret).await.unwrap();

    let mut empty = request();
    empty.queries.clear();
    let error = client
        .knowledge_search("knowledge-request-3", &empty)
        .await
        .expect_err("an empty plan must be rejected");
    assert!(matches!(error, ClientError::Remote { .. }), "{error:?}");
    assert!(backend.requests.lock().unwrap().is_empty());

    drop(client);
    task.abort();
}

/// Retrieval that only finishes when its cancellation token is cancelled, so
/// the test observes the worker's cancellation ownership rather than a race
/// with a fast backend.
struct BlockingUntilCancelledBackend {
    started: Arc<tokio::sync::Notify>,
}

impl WorkerKnowledgeBackend for BlockingUntilCancelledBackend {
    fn capabilities(&self) -> KnowledgeCapabilities {
        KnowledgeCapabilities {
            full_text: true,
            query_embeddings: false,
        }
    }

    fn search(
        &self,
        _request: KnowledgeRetrievalRequest,
        cancellation: &CancellationToken,
    ) -> Result<KnowledgeRetrieval, KnowledgeRetrievalError> {
        self.started.notify_waiters();
        for _ in 0..2_000 {
            if cancellation.is_cancelled() {
                return Err(KnowledgeRetrievalError::Cancelled);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        Err(KnowledgeRetrievalError::RetrievalUnavailable)
    }
}

/// A cancel frame that arrives immediately after the search frame must be
/// observed by the running search: cancellation is registered on the read loop
/// before the handler is spawned, exactly like an ordinary query.
#[tokio::test]
async fn cancelling_a_knowledge_search_immediately_after_issuing_it_stops_the_worker() {
    let directory = test_directory();
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let secret = LaunchSecret::from_bytes([47; 32]);
    let backend = Arc::new(BlockingUntilCancelledBackend {
        started: Arc::new(tokio::sync::Notify::new()),
    });
    let task = tokio::spawn(
        WorkerServer::with_knowledge_backend(
            WorkerConfig::new(endpoint.clone(), secret.clone()),
            backend.clone(),
        )
        .run(),
    );
    wait_for_endpoint(&endpoint).await;
    let client = WorkerClient::connect(&endpoint, secret).await.unwrap();
    let searching = client.clone();

    // The two frames are written back to back on one connection, so the worker
    // reads the search frame and then the cancel frame with no work in
    // between: registration must already be owned by the time the cancel is
    // handled, not established later inside the spawned handler.
    let search_request = request();
    let (search, accepted) = tokio::time::timeout(Duration::from_secs(20), async {
        tokio::join!(
            searching.knowledge_search("knowledge-cancel-1", &search_request),
            client.cancel("knowledge-cancel-1"),
        )
    })
    .await
    .expect("an immediately cancelled search must not run to completion");
    let error = search.expect_err("a cancelled search returns no evidence");
    assert!(
        accepted.expect("cancel response"),
        "the worker must own the registration already"
    );
    assert!(
        matches!(
            &error,
            ClientError::Remote { code, .. } if *code == fm_semantic_protocol::v1::ErrorCode::Cancelled
        ),
        "{error:?}"
    );

    drop(client);
    task.abort();
}

/// Evidence bounded by tokens can still be large in bytes. A valid but
/// oversized response must be reported as a typed limit failure instead of
/// exceeding the negotiated frame size and dropping the connection.
struct OversizedEvidenceBackend;

impl WorkerKnowledgeBackend for OversizedEvidenceBackend {
    fn capabilities(&self) -> KnowledgeCapabilities {
        KnowledgeCapabilities {
            full_text: true,
            query_embeddings: false,
        }
    }

    fn search(
        &self,
        _request: KnowledgeRetrievalRequest,
        _cancellation: &CancellationToken,
    ) -> Result<KnowledgeRetrieval, KnowledgeRetrievalError> {
        let route = RouteOutcome {
            requested: KnowledgeRoute::FullText,
            applied: KnowledgeRoute::FullText,
            fallback_reason: None,
        };
        let mut oversized = evidence();
        // One token of accounted context, two megabytes of encoded content.
        oversized.token_count = 1;
        oversized.content = "a".repeat(2 * 1024 * 1024);
        Ok(KnowledgeRetrieval {
            route,
            capabilities: self.capabilities(),
            evidence: vec![oversized],
            token_count: 1,
            coverage: SearchCoverage::default(),
            trace: None,
        })
    }
}

#[tokio::test]
async fn an_oversized_knowledge_response_is_reported_instead_of_dropping_the_connection() {
    let directory = test_directory();
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let secret = LaunchSecret::from_bytes([53; 32]);
    let task = tokio::spawn(
        WorkerServer::with_knowledge_backend(
            WorkerConfig::new(endpoint.clone(), secret.clone()),
            Arc::new(OversizedEvidenceBackend),
        )
        .run(),
    );
    wait_for_endpoint(&endpoint).await;
    let client = WorkerClient::connect(&endpoint, secret).await.unwrap();

    let error = client
        .knowledge_search("knowledge-oversized-1", &request())
        .await
        .expect_err("an oversized response must be reported");
    assert!(
        matches!(
            &error,
            ClientError::Remote { code, .. }
                if *code == fm_semantic_protocol::v1::ErrorCode::LimitExceeded
        ),
        "{error:?}"
    );

    // The connection survives: the worker refused the frame instead of writing
    // one that exceeds the negotiated limit.
    let capabilities = client.knowledge_capabilities().await.unwrap();
    assert!(capabilities.full_text);

    drop(client);
    task.abort();
}
