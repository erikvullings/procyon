//! Thin REST adapter for semantic-library enrolment and consent.

use axum::Json;
use axum::extract::{Extension, State};
use fm_transport_dto::{
    ConfirmSemanticEnrolmentRequestDto, ConfirmSemanticExclusionRequestDto,
    GetSemanticFolderStatusRequestDto, PlanSemanticExclusionRequestDto,
    PreviewSemanticEnrolmentRequestDto, ResumeSemanticCleanupRequestDto,
    SemanticEnrolmentPreviewDto, SemanticExclusionPlanDto, SemanticFolderStatusDto,
    SemanticLibraryCapabilitiesDto, SemanticLibraryErrorDto, SemanticLibraryRevisionRequestDto,
    SemanticLibraryStatusDto, UpdateSemanticEligibilityOverridesRequestDto,
};
use tower_http::request_id::RequestId;

use crate::error::{SemanticLibraryApiError, extract_request_id};
use crate::state::AppState;

#[utoipa::path(
    get,
    path = "/api/v1/semantic/library/capabilities",
    operation_id = "getSemanticLibraryCapabilities",
    responses((status = 200, body = SemanticLibraryCapabilitiesDto))
)]
pub(crate) async fn get_semantic_library_capabilities(
    State(state): State<AppState>,
) -> Json<SemanticLibraryCapabilitiesDto> {
    // The advertised operation list is per caller, not per deployment: a
    // principal this server's library would deny must not be told that
    // enrolment or destructive exclusion is available to it.
    Json(
        state
            .service
            .semantic_library_capabilities_dto(state.semantic_access())
            .await,
    )
}

#[utoipa::path(
    get,
    path = "/api/v1/semantic/library/status",
    operation_id = "getSemanticLibraryStatus",
    responses(
        (status = 200, body = SemanticLibraryStatusDto),
        (status = 403, body = SemanticLibraryErrorDto),
        (status = 503, body = SemanticLibraryErrorDto)
    )
)]
pub(crate) async fn get_semantic_library_status(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
) -> Result<Json<SemanticLibraryStatusDto>, SemanticLibraryApiError> {
    state
        .service
        .semantic_library_status_dto(state.semantic_access())
        .await
        .map(Json)
        .map_err(|error| SemanticLibraryApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/semantic/library/folder-status",
    operation_id = "getSemanticFolderStatus",
    request_body = GetSemanticFolderStatusRequestDto,
    responses(
        (status = 200, body = SemanticFolderStatusDto),
        (status = 400, body = SemanticLibraryErrorDto),
        (status = 403, body = SemanticLibraryErrorDto),
        (status = 409, body = SemanticLibraryErrorDto)
    )
)]
pub(crate) async fn get_semantic_folder_status(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<GetSemanticFolderStatusRequestDto>,
) -> Result<Json<SemanticFolderStatusDto>, SemanticLibraryApiError> {
    state
        .service
        .semantic_folder_status_dto(state.semantic_access(), request)
        .await
        .map(Json)
        .map_err(|error| SemanticLibraryApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/semantic/library/enrolment/preview",
    operation_id = "previewSemanticEnrolment",
    request_body = PreviewSemanticEnrolmentRequestDto,
    responses(
        (status = 200, body = SemanticEnrolmentPreviewDto),
        (status = 400, body = SemanticLibraryErrorDto),
        (status = 403, body = SemanticLibraryErrorDto),
        (status = 409, body = SemanticLibraryErrorDto)
    )
)]
pub(crate) async fn preview_semantic_enrolment(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<PreviewSemanticEnrolmentRequestDto>,
) -> Result<Json<SemanticEnrolmentPreviewDto>, SemanticLibraryApiError> {
    state
        .service
        .preview_semantic_enrolment(state.semantic_access(), request)
        .await
        .map(Json)
        .map_err(|error| SemanticLibraryApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/semantic/library/enrolment/confirm",
    operation_id = "confirmSemanticEnrolment",
    request_body = ConfirmSemanticEnrolmentRequestDto,
    responses(
        (status = 200, body = SemanticLibraryStatusDto),
        (status = 403, body = SemanticLibraryErrorDto),
        (status = 409, body = SemanticLibraryErrorDto)
    )
)]
pub(crate) async fn confirm_semantic_enrolment(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<ConfirmSemanticEnrolmentRequestDto>,
) -> Result<Json<SemanticLibraryStatusDto>, SemanticLibraryApiError> {
    state
        .service
        .confirm_semantic_enrolment(state.semantic_access(), request)
        .await
        .map(Json)
        .map_err(|error| SemanticLibraryApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/semantic/library/exclusions/plan",
    operation_id = "planSemanticExclusion",
    request_body = PlanSemanticExclusionRequestDto,
    responses(
        (status = 200, body = SemanticExclusionPlanDto),
        (status = 400, body = SemanticLibraryErrorDto),
        (status = 403, body = SemanticLibraryErrorDto),
        (status = 409, body = SemanticLibraryErrorDto)
    )
)]
pub(crate) async fn plan_semantic_exclusion(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<PlanSemanticExclusionRequestDto>,
) -> Result<Json<SemanticExclusionPlanDto>, SemanticLibraryApiError> {
    state
        .service
        .plan_semantic_exclusion(state.semantic_access(), request)
        .await
        .map(Json)
        .map_err(|error| SemanticLibraryApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/semantic/library/exclusions/confirm",
    operation_id = "confirmSemanticExclusion",
    request_body = ConfirmSemanticExclusionRequestDto,
    responses(
        (status = 200, body = SemanticLibraryStatusDto),
        (status = 403, body = SemanticLibraryErrorDto),
        (status = 409, body = SemanticLibraryErrorDto)
    )
)]
pub(crate) async fn confirm_semantic_exclusion(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<ConfirmSemanticExclusionRequestDto>,
) -> Result<Json<SemanticLibraryStatusDto>, SemanticLibraryApiError> {
    state
        .service
        .confirm_semantic_exclusion(state.semantic_access(), request)
        .await
        .map(Json)
        .map_err(|error| SemanticLibraryApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/semantic/library/cleanup/resume",
    operation_id = "resumeSemanticCleanup",
    request_body = ResumeSemanticCleanupRequestDto,
    responses(
        (status = 200, body = SemanticLibraryStatusDto),
        (status = 403, body = SemanticLibraryErrorDto),
        (status = 404, body = SemanticLibraryErrorDto),
        (status = 409, body = SemanticLibraryErrorDto)
    )
)]
pub(crate) async fn resume_semantic_cleanup(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<ResumeSemanticCleanupRequestDto>,
) -> Result<Json<SemanticLibraryStatusDto>, SemanticLibraryApiError> {
    state
        .service
        .resume_semantic_cleanup(state.semantic_access(), request)
        .await
        .map(Json)
        .map_err(|error| SemanticLibraryApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/semantic/library/pause",
    operation_id = "pauseSemanticLibrary",
    request_body = SemanticLibraryRevisionRequestDto,
    responses(
        (status = 200, body = SemanticLibraryStatusDto),
        (status = 403, body = SemanticLibraryErrorDto),
        (status = 409, body = SemanticLibraryErrorDto)
    )
)]
pub(crate) async fn pause_semantic_library(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<SemanticLibraryRevisionRequestDto>,
) -> Result<Json<SemanticLibraryStatusDto>, SemanticLibraryApiError> {
    state
        .service
        .pause_semantic_library(state.semantic_access(), request)
        .await
        .map(Json)
        .map_err(|error| SemanticLibraryApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/semantic/library/resume",
    operation_id = "resumeSemanticLibrary",
    request_body = SemanticLibraryRevisionRequestDto,
    responses(
        (status = 200, body = SemanticLibraryStatusDto),
        (status = 403, body = SemanticLibraryErrorDto),
        (status = 409, body = SemanticLibraryErrorDto)
    )
)]
pub(crate) async fn resume_semantic_library(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<SemanticLibraryRevisionRequestDto>,
) -> Result<Json<SemanticLibraryStatusDto>, SemanticLibraryApiError> {
    state
        .service
        .resume_semantic_library(state.semantic_access(), request)
        .await
        .map(Json)
        .map_err(|error| SemanticLibraryApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/semantic/library/eligibility-overrides",
    operation_id = "updateSemanticEligibilityOverrides",
    request_body = UpdateSemanticEligibilityOverridesRequestDto,
    responses(
        (status = 200, body = SemanticLibraryStatusDto),
        (status = 400, body = SemanticLibraryErrorDto),
        (status = 403, body = SemanticLibraryErrorDto),
        (status = 409, body = SemanticLibraryErrorDto)
    )
)]
pub(crate) async fn update_semantic_eligibility_overrides(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<UpdateSemanticEligibilityOverridesRequestDto>,
) -> Result<Json<SemanticLibraryStatusDto>, SemanticLibraryApiError> {
    state
        .service
        .update_semantic_eligibility_overrides(state.semantic_access(), request)
        .await
        .map(Json)
        .map_err(|error| SemanticLibraryApiError::new(error, extract_request_id(&request_id)))
}
