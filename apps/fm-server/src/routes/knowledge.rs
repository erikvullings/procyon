//! Thin HTTP adapter for Structured Knowledge Search.
//!
//! Every route works without a generation profile: capabilities are reported
//! independently and search never requires answer generation.

use axum::Json;
use axum::extract::{Extension, State};
use axum::http::StatusCode;
use fm_transport_dto::{
    ApplicationErrorDto, CancelKnowledgeAnswerRequestDto, CancelKnowledgeSearchRequestDto,
    ExecuteKnowledgeSearchRequestDto, GenerateKnowledgeAnswerRequestDto, KnowledgeAnswerDto,
    KnowledgeCapabilitiesDto, KnowledgeQueryInterpretationDto, KnowledgeRootDto,
    KnowledgeSearchPlanDto, KnowledgeSearchResultDto, KnowledgeSourceLocationDto,
    ListKnowledgeRootsRequestDto, ParseKnowledgeQueryRequestDto, PlanKnowledgeSearchRequestDto,
    ResolveKnowledgeSourceRequestDto,
};
use tower_http::request_id::RequestId;

use crate::error::{ApiError, extract_request_id};
use crate::state::AppState;

#[utoipa::path(
    get,
    path = "/api/v1/semantic/knowledge/capabilities",
    operation_id = "getKnowledgeCapabilities",
    responses((status = 200, body = KnowledgeCapabilitiesDto))
)]
pub(crate) async fn get_knowledge_capabilities(
    State(state): State<AppState>,
) -> Json<KnowledgeCapabilitiesDto> {
    Json(state.service.knowledge_capabilities().await)
}

#[utoipa::path(
    post,
    path = "/api/v1/semantic/knowledge/roots",
    operation_id = "listKnowledgeRoots",
    request_body = ListKnowledgeRootsRequestDto,
    responses(
        (status = 200, body = Vec<KnowledgeRootDto>),
        (status = 400, body = ApplicationErrorDto),
        (status = 403, body = ApplicationErrorDto),
        (status = 503, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn list_knowledge_roots(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<ListKnowledgeRootsRequestDto>,
) -> Result<Json<Vec<KnowledgeRootDto>>, ApiError> {
    state
        .service
        .list_knowledge_roots(state.semantic_access(), request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/semantic/knowledge/parse",
    operation_id = "parseKnowledgeQuery",
    request_body = ParseKnowledgeQueryRequestDto,
    responses((status = 200, body = KnowledgeQueryInterpretationDto))
)]
pub(crate) async fn parse_knowledge_query(
    State(state): State<AppState>,
    Json(request): Json<ParseKnowledgeQueryRequestDto>,
) -> Json<KnowledgeQueryInterpretationDto> {
    Json(state.service.parse_knowledge_query(request))
}

#[utoipa::path(
    post,
    path = "/api/v1/semantic/knowledge/plan",
    operation_id = "planKnowledgeSearch",
    request_body = PlanKnowledgeSearchRequestDto,
    responses(
        (status = 200, body = KnowledgeSearchPlanDto),
        (status = 400, body = ApplicationErrorDto),
        (status = 403, body = ApplicationErrorDto),
        (status = 404, body = ApplicationErrorDto),
        (status = 503, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn plan_knowledge_search(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<PlanKnowledgeSearchRequestDto>,
) -> Result<Json<KnowledgeSearchPlanDto>, ApiError> {
    state
        .service
        .plan_knowledge_search(state.semantic_access(), request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/semantic/knowledge/search",
    operation_id = "executeKnowledgeSearch",
    request_body = ExecuteKnowledgeSearchRequestDto,
    responses(
        (status = 200, body = KnowledgeSearchResultDto),
        (status = 400, body = ApplicationErrorDto),
        (status = 403, body = ApplicationErrorDto),
        (status = 404, body = ApplicationErrorDto),
        (status = 503, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn execute_knowledge_search(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<ExecuteKnowledgeSearchRequestDto>,
) -> Result<Json<KnowledgeSearchResultDto>, ApiError> {
    state
        .service
        .execute_knowledge_search(state.semantic_access(), request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/semantic/knowledge/search/cancel",
    operation_id = "cancelKnowledgeSearch",
    request_body = CancelKnowledgeSearchRequestDto,
    responses((status = 204))
)]
pub(crate) async fn cancel_knowledge_search(
    State(state): State<AppState>,
    Json(request): Json<CancelKnowledgeSearchRequestDto>,
) -> StatusCode {
    state.service.cancel_knowledge_search(request);
    StatusCode::NO_CONTENT
}

#[utoipa::path(
    post,
    path = "/api/v1/semantic/knowledge/sources/resolve",
    operation_id = "resolveKnowledgeSource",
    request_body = ResolveKnowledgeSourceRequestDto,
    responses(
        (status = 200, body = KnowledgeSourceLocationDto),
        (status = 403, body = ApplicationErrorDto),
        (status = 404, body = ApplicationErrorDto),
        (status = 503, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn resolve_knowledge_source(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<ResolveKnowledgeSourceRequestDto>,
) -> Result<Json<KnowledgeSourceLocationDto>, ApiError> {
    state
        .service
        .resolve_knowledge_source(state.semantic_access(), request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/semantic/knowledge/answer",
    operation_id = "generateKnowledgeAnswer",
    request_body = GenerateKnowledgeAnswerRequestDto,
    responses(
        (status = 200, body = KnowledgeAnswerDto),
        (status = 400, body = ApplicationErrorDto),
        (status = 403, body = ApplicationErrorDto),
        (status = 409, description = "The inspected evidence set must be retrieved again", body = ApplicationErrorDto),
        (status = 500, body = ApplicationErrorDto),
        (status = 503, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn generate_knowledge_answer(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<GenerateKnowledgeAnswerRequestDto>,
) -> Result<Json<KnowledgeAnswerDto>, ApiError> {
    state
        .service
        .generate_knowledge_answer(state.semantic_access(), request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/semantic/knowledge/answer/cancel",
    operation_id = "cancelKnowledgeAnswer",
    request_body = CancelKnowledgeAnswerRequestDto,
    responses((status = 204))
)]
pub(crate) async fn cancel_knowledge_answer(
    State(state): State<AppState>,
    Json(request): Json<CancelKnowledgeAnswerRequestDto>,
) -> StatusCode {
    state.service.cancel_knowledge_answer(request);
    StatusCode::NO_CONTENT
}
