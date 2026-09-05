//! Public protobuf contract and authority-boundary tests.

use fm_semantic_protocol::{RequestValidationError, v1, validate_ingestion, validate_query};
use prost::Message;

fn session() -> v1::SessionContext {
    v1::SessionContext {
        session_id: "session-1".to_owned(),
        session_token: b"opaque-token".to_vec(),
    }
}

fn scope() -> v1::ResourceScope {
    v1::ResourceScope {
        tenant_id: "tenant-1".to_owned(),
        library_id: "library-1".to_owned(),
    }
}

fn encoded_len(message: &impl Message) -> usize {
    message.encoded_len()
}

fn assert_ingestion_request_shape(request: v1::IngestionRequest) {
    let v1::IngestionRequest {
        session,
        scope,
        request_id,
        payload,
    } = request;
    assert!(session.is_some());
    assert!(scope.is_some());
    assert!(!request_id.is_empty());
    assert!(payload.is_some());
}

fn assert_query_request_shape(request: v1::QueryRequest) {
    let v1::QueryRequest {
        session,
        scope,
        request_id,
        query,
        maximum_results,
        concept_query,
    } = request;
    assert!(session.is_some());
    assert!(scope.is_some());
    assert!(!request_id.is_empty());
    assert!(!query.is_empty());
    assert!(maximum_results > 0);
    assert!(concept_query.is_none());
}

#[test]
fn ingestion_and_query_are_scoped_and_carry_no_path_authority() {
    let start = v1::IngestionStart {
        document_id: "document-1".to_owned(),
        metadata: vec![v1::MetadataEntry {
            key: "title".to_owned(),
            value: "Quarterly report".to_owned(),
        }],
        media_type: "text/plain".to_owned(),
        expected_content_bytes: 12,
    };

    // Exhaustive destructuring makes adding path, root, credential, action, or
    // network authority to this public DTO a compile-time contract change.
    let v1::IngestionStart {
        document_id,
        metadata,
        media_type,
        expected_content_bytes,
    } = start.clone();
    assert_eq!(document_id, "document-1");
    assert_eq!(metadata.len(), 1);
    assert_eq!(media_type, "text/plain");
    assert_eq!(expected_content_bytes, 12);

    let ingestion = v1::IngestionRequest {
        session: Some(session()),
        scope: Some(scope()),
        request_id: "request-1".to_owned(),
        payload: Some(v1::ingestion_request::Payload::Start(start)),
    };
    assert_eq!(validate_ingestion(&ingestion), Ok(()));
    assert_ingestion_request_shape(ingestion);

    let query = v1::QueryRequest {
        session: Some(session()),
        scope: Some(scope()),
        request_id: "request-2".to_owned(),
        query: "revenue".to_owned(),
        maximum_results: 20,
        concept_query: None,
    };
    assert_eq!(validate_query(&query), Ok(()));
    assert_query_request_shape(query.clone());

    let mut unscoped = query;
    unscoped.scope = None;
    assert_eq!(
        validate_query(&unscoped),
        Err(RequestValidationError::MissingScope)
    );
}

#[test]
fn concept_queries_are_bounded_and_do_not_require_text() {
    let mut query = v1::QueryRequest {
        session: Some(session()),
        scope: Some(scope()),
        request_id: "request-concepts".to_owned(),
        query: String::new(),
        maximum_results: 20,
        concept_query: Some(v1::ConceptQuery {
            vocabulary_id: "vocabulary-1".to_owned(),
            concept_uris: vec!["urn:concept:revenue".to_owned()],
            root_id: None,
            workspace_id: Some("workspace-1".to_owned()),
            include_unavailable: true,
            offset: 0,
        }),
    };
    assert_eq!(validate_query(&query), Ok(()));

    query.concept_query.as_mut().unwrap().vocabulary_id.clear();
    assert_eq!(
        validate_query(&query),
        Err(RequestValidationError::InvalidConceptQuery)
    );
    query.concept_query.as_mut().unwrap().vocabulary_id = "vocabulary-1".to_owned();
    query.concept_query.as_mut().unwrap().concept_uris.clear();
    assert_eq!(
        validate_query(&query),
        Err(RequestValidationError::InvalidConceptQuery)
    );
    query.concept_query.as_mut().unwrap().concept_uris = (0..257)
        .map(|index| format!("urn:concept:{index}"))
        .collect();
    assert_eq!(
        validate_query(&query),
        Err(RequestValidationError::InvalidConceptQuery)
    );
}

#[test]
fn contract_exposes_health_jobs_results_events_cancellation_and_shutdown() {
    let lengths = [
        encoded_len(&v1::HealthRequest {
            session: Some(session()),
        }),
        encoded_len(&v1::CancelRequest {
            session: Some(session()),
            request_id: "request-1".to_owned(),
        }),
        encoded_len(&v1::IngestionJobRequest {
            session: Some(session()),
            scope: Some(scope()),
            job_id: "job-1".to_owned(),
        }),
        encoded_len(&v1::QueryEvent {
            payload: Some(v1::query_event::Payload::Completed(v1::QueryCompleted {
                result_count: 0,
            })),
        }),
        encoded_len(&v1::WorkerEvent {
            payload: Some(v1::worker_event::Payload::Progress(v1::Progress {
                operation_id: "job-1".to_owned(),
                phase: "indexing".to_owned(),
                completed_units: 1,
                total_units: 2,
            })),
        }),
        encoded_len(&v1::ShutdownRequest {
            session: Some(session()),
            grace_period_ms: 1_000,
        }),
    ];

    assert!(lengths.into_iter().all(|length| length > 0));
}
