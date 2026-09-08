//! HTTP parity and OpenAPI coverage for Structured Knowledge Search.
//!
//! The browser host must expose a complete search surface without any LLM
//! profile: capabilities are reported independently, parsing and planning are
//! deterministic, and unavailable retrieval is reported honestly.

#![allow(clippy::unwrap_used)]

mod common;

use fm_transport_dto::{
    ApplicationErrorCode as ApplicationErrorCodeDto, ApplicationErrorDto, KnowledgeCapabilitiesDto,
    KnowledgeNeedDto, KnowledgeQueryInterpretationDto,
};
use reqwest::StatusCode;

use common::TestServer;

#[tokio::test]
async fn knowledge_capabilities_are_reported_independently_without_a_generation_profile() {
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();

    let capabilities: KnowledgeCapabilitiesDto = client
        .get(format!(
            "{}/api/v1/semantic/knowledge/capabilities",
            server.base_url
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert!(!capabilities.full_text);
    assert!(!capabilities.semantic);
    assert!(!capabilities.answer_generation);

    server.handle.abort();
}

#[tokio::test]
async fn parsing_is_deterministic_and_never_moves_answer_fields_into_retrieval() {
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();

    let interpretation: KnowledgeQueryInterpretationDto = client
        .post(format!(
            "{}/api/v1/semantic/knowledge/parse",
            server.base_url
        ))
        .json(&serde_json::json!({
            "text": "about: wind turbines\nneed: procedure\ndo: apply\nto: a maintenance report"
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(interpretation.draft.about, vec!["wind turbines".to_owned()]);
    assert_eq!(
        interpretation.draft.needs,
        vec![KnowledgeNeedDto::Procedure]
    );
    assert!(
        interpretation
            .excluded_from_retrieval
            .iter()
            .any(|field| field.field == "to")
    );
    assert!(!interpretation.dsl_compact.is_empty());

    server.handle.abort();
}

#[tokio::test]
async fn searching_without_a_configured_library_is_denied_rather_than_silently_empty() {
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();
    let workspace_id = uuid::Uuid::new_v4();

    let roots = client
        .post(format!(
            "{}/api/v1/semantic/knowledge/roots",
            server.base_url
        ))
        .json(&serde_json::json!({ "workspaceId": workspace_id }))
        .send()
        .await
        .unwrap();
    assert!(roots.status().is_success() || roots.status() == StatusCode::SERVICE_UNAVAILABLE);

    let response = client
        .post(format!(
            "{}/api/v1/semantic/knowledge/search",
            server.base_url
        ))
        .json(&serde_json::json!({
            "requestId": uuid::Uuid::new_v4(),
            "draft": { "about": ["wind turbines"], "needs": ["definition"] },
            "scope": { "kind": "entireLibrary", "workspaceId": workspace_id },
            "mode": "hybrid"
        }))
        .send()
        .await
        .unwrap();

    let status = response.status();
    let error: ApplicationErrorDto = response.json().await.unwrap();
    assert!(
        status == StatusCode::SERVICE_UNAVAILABLE || status == StatusCode::NOT_FOUND,
        "search without an indexed library must be denied, got {status}: {error:?}"
    );

    server.handle.abort();
}

#[tokio::test]
async fn an_invalid_scope_is_rejected_before_any_retrieval_is_attempted() {
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!(
            "{}/api/v1/semantic/knowledge/plan",
            server.base_url
        ))
        .json(&serde_json::json!({
            "draft": { "about": ["wind turbines"] },
            "scope": {
                "kind": "semanticResults",
                "workspaceId": uuid::Uuid::new_v4(),
                "semanticSourceIds": []
            },
            "mode": "fullText"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    server.handle.abort();
}

#[tokio::test]
async fn cancellation_is_available_over_http_for_host_parity() {
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!(
            "{}/api/v1/semantic/knowledge/search/cancel",
            server.base_url
        ))
        .json(&serde_json::json!({ "requestId": uuid::Uuid::new_v4() }))
        .send()
        .await
        .unwrap();

    // The documented contract is 204 with no body, not merely "a success".
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(response.bytes().await.unwrap().is_empty());

    server.handle.abort();
}

#[tokio::test]
async fn answering_without_a_generation_profile_is_refused_over_http() {
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!(
            "{}/api/v1/semantic/knowledge/answer",
            server.base_url
        ))
        .json(&serde_json::json!({
            "requestId": uuid::Uuid::new_v4(),
            "workspaceId": uuid::Uuid::new_v4(),
            "evidenceFingerprint": "sha256:never-retrieved",
            "profileId": uuid::Uuid::new_v4(),
            "allowModelKnowledge": false
        }))
        .send()
        .await
        .unwrap();

    let status = response.status();
    let error: ApplicationErrorDto = response.json().await.unwrap();
    // Never 200, and never a silent retrieval: without an indexed library or a
    // retained evidence set the request is refused with a typed failure.
    assert!(
        matches!(
            status,
            StatusCode::CONFLICT | StatusCode::SERVICE_UNAVAILABLE | StatusCode::NOT_FOUND
        ),
        "answering an unknown evidence set must be refused, got {status}: {error:?}"
    );
    assert!(
        matches!(
            error.code,
            ApplicationErrorCodeDto::KnowledgeEvidenceRefreshRequired
                | ApplicationErrorCodeDto::ProviderUnavailable
                | ApplicationErrorCodeDto::NotFound
        ),
        "unexpected error code: {error:?}"
    );

    server.handle.abort();
}

#[tokio::test]
async fn answer_cancellation_is_available_over_http_with_the_documented_status() {
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!(
            "{}/api/v1/semantic/knowledge/answer/cancel",
            server.base_url
        ))
        .json(&serde_json::json!({ "requestId": uuid::Uuid::new_v4() }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(response.bytes().await.unwrap().is_empty());

    server.handle.abort();
}

#[tokio::test]
async fn a_malformed_answer_request_is_rejected_before_any_authorization_work() {
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!(
            "{}/api/v1/semantic/knowledge/answer",
            server.base_url
        ))
        .json(&serde_json::json!({ "requestId": uuid::Uuid::new_v4() }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

    server.handle.abort();
}

#[test]
fn every_knowledge_route_is_published_in_the_openapi_document() {
    let document = fm_server::openapi_document();
    let paths = document.paths.paths;

    for path in [
        "/api/v1/semantic/knowledge/capabilities",
        "/api/v1/semantic/knowledge/roots",
        "/api/v1/semantic/knowledge/parse",
        "/api/v1/semantic/knowledge/plan",
        "/api/v1/semantic/knowledge/search",
        "/api/v1/semantic/knowledge/search/cancel",
        "/api/v1/semantic/knowledge/sources/resolve",
        "/api/v1/semantic/knowledge/answer",
        "/api/v1/semantic/knowledge/answer/cancel",
    ] {
        assert!(paths.contains_key(path), "{path} must be published");
    }

    let components = document.components.expect("components");
    // The resolved-scope and honest-coverage contract must be published, not
    // just the route: a host has to be able to tell an exactly searched scope
    // from a narrowed superset, and unknown publication state from zero.
    let plan = serde_json::to_value(
        components
            .schemas
            .get("KnowledgeSearchPlanDto")
            .expect("plan schema"),
    )
    .expect("serialize plan schema");
    assert!(plan["properties"]["scopeIsExact"].is_object());
    let coverage = serde_json::to_value(
        components
            .schemas
            .get("KnowledgeCoverageDto")
            .expect("coverage schema"),
    )
    .expect("serialize coverage schema");
    assert!(coverage["properties"]["indexed"].is_object());
    assert!(coverage["properties"]["scopeIsExact"].is_object());
    assert!(coverage["properties"]["staleEvidence"].is_object());

    let schemas = components.schemas.into_keys().collect::<Vec<_>>();
    for schema in [
        "KnowledgeCapabilitiesDto",
        "KnowledgeSearchPlanDto",
        "KnowledgeSearchResultDto",
        "KnowledgeEvidenceDto",
        "KnowledgeQueryInterpretationDto",
        "KnowledgeRootDto",
        "KnowledgeSourceLocationDto",
        "GenerateKnowledgeAnswerRequestDto",
        "KnowledgeAnswerDto",
        "KnowledgeAnswerCitationDto",
        "CancelKnowledgeAnswerRequestDto",
    ] {
        assert!(
            schemas.iter().any(|name| name == schema),
            "{schema} must be published"
        );
    }
}
