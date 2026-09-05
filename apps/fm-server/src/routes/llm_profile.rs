//! Thin HTTP surface for named generation profiles (task 0184).

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use fm_transport_dto::{
    ActivateLlmProfileRequestDto, ApplicationErrorDto, DeleteLlmProfileRequestDto, LlmProfileDto,
    LlmProfileExportDto, LlmProfilePresetDto, LlmProfileTestResultDto, SaveLlmProfileRequestDto,
};
use tower_http::request_id::RequestId;
use uuid::Uuid;

use crate::error::{ApiError, extract_request_id};
use crate::state::AppState;

#[utoipa::path(
    get,
    path = "/api/v1/llm-profiles/presets",
    operation_id = "listLlmProfilePresets",
    responses((status = 200, description = "Safe defaults for supported providers", body = Vec<LlmProfilePresetDto>))
)]
pub(crate) async fn list_llm_profile_presets(
    State(state): State<AppState>,
) -> Json<Vec<LlmProfilePresetDto>> {
    Json(state.service.list_llm_profile_presets())
}

#[utoipa::path(
    get,
    path = "/api/v1/llm-profiles",
    operation_id = "listLlmProfiles",
    responses(
        (status = 200, description = "Saved generation profiles", body = Vec<LlmProfileDto>),
        (status = 500, description = "Profiles could not be loaded", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn list_llm_profiles(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
) -> Result<Json<Vec<LlmProfileDto>>, ApiError> {
    let request_id = extract_request_id(&request_id);
    Ok(Json(
        state
            .service
            .list_llm_profiles()
            .map_err(|error| ApiError::new(error, request_id))?,
    ))
}

#[utoipa::path(
    post,
    path = "/api/v1/llm-profiles",
    operation_id = "createLlmProfile",
    request_body = SaveLlmProfileRequestDto,
    responses(
        (status = 201, description = "Created generation profile", body = LlmProfileDto),
        (status = 400, description = "Invalid profile", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn create_llm_profile(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<SaveLlmProfileRequestDto>,
) -> Result<(StatusCode, Json<LlmProfileDto>), ApiError> {
    let request_id = extract_request_id(&request_id);
    let profile = state
        .service
        .create_llm_profile(request)
        .await
        .map_err(|error| ApiError::new(error, request_id))?;
    Ok((StatusCode::CREATED, Json(profile)))
}

#[utoipa::path(
    put,
    path = "/api/v1/llm-profiles/{profileId}",
    operation_id = "updateLlmProfile",
    params(("profileId" = Uuid, Path, description = "Profile to update")),
    request_body = SaveLlmProfileRequestDto,
    responses(
        (status = 200, description = "Updated generation profile", body = LlmProfileDto),
        (status = 400, description = "Invalid profile", body = ApplicationErrorDto),
        (status = 404, description = "Profile not found", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn update_llm_profile(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(profile_id): Path<Uuid>,
    Json(request): Json<SaveLlmProfileRequestDto>,
) -> Result<Json<LlmProfileDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    Ok(Json(
        state
            .service
            .update_llm_profile(profile_id, request)
            .await
            .map_err(|error| ApiError::new(error, request_id))?,
    ))
}

#[utoipa::path(
    delete,
    path = "/api/v1/llm-profiles/{profileId}",
    operation_id = "deleteLlmProfile",
    params(("profileId" = Uuid, Path, description = "Profile to delete")),
    request_body = DeleteLlmProfileRequestDto,
    responses(
        (status = 204, description = "Profile deleted"),
        (status = 404, description = "Profile not found", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn delete_llm_profile(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(profile_id): Path<Uuid>,
    Json(request): Json<DeleteLlmProfileRequestDto>,
) -> Result<StatusCode, ApiError> {
    let request_id = extract_request_id(&request_id);
    state
        .service
        .delete_llm_profile(profile_id, request)
        .await
        .map_err(|error| ApiError::new(error, request_id))?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/api/v1/llm-profiles/{profileId}/clone",
    operation_id = "cloneLlmProfile",
    params(("profileId" = Uuid, Path, description = "Profile to clone")),
    responses(
        (status = 201, description = "Credential-free cloned profile", body = LlmProfileDto),
        (status = 404, description = "Profile not found", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn clone_llm_profile(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(profile_id): Path<Uuid>,
) -> Result<(StatusCode, Json<LlmProfileDto>), ApiError> {
    let request_id = extract_request_id(&request_id);
    let profile = state
        .service
        .clone_llm_profile(profile_id)
        .map_err(|error| ApiError::new(error, request_id))?;
    Ok((StatusCode::CREATED, Json(profile)))
}

#[utoipa::path(
    get,
    path = "/api/v1/llm-profiles/{profileId}/export",
    operation_id = "exportLlmProfile",
    params(("profileId" = Uuid, Path, description = "Profile to export")),
    responses(
        (status = 200, description = "Non-secret profile configuration", body = LlmProfileExportDto),
        (status = 404, description = "Profile not found", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn export_llm_profile(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(profile_id): Path<Uuid>,
) -> Result<Json<LlmProfileExportDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    Ok(Json(
        state
            .service
            .export_llm_profile(profile_id)
            .map_err(|error| ApiError::new(error, request_id))?,
    ))
}

#[utoipa::path(
    post,
    path = "/api/v1/llm-profiles/{profileId}/activate",
    operation_id = "activateLlmProfile",
    params(("profileId" = Uuid, Path, description = "Profile to activate")),
    request_body = ActivateLlmProfileRequestDto,
    responses(
        (status = 200, description = "Activated generation profile", body = LlmProfileDto),
        (status = 400, description = "Cloud consent required", body = ApplicationErrorDto),
        (status = 403, description = "Host denied by administrator policy", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn activate_llm_profile(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(profile_id): Path<Uuid>,
    Json(request): Json<ActivateLlmProfileRequestDto>,
) -> Result<Json<LlmProfileDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    Ok(Json(
        state
            .service
            .activate_llm_profile(profile_id, request.consent)
            .map_err(|error| ApiError::new(error, request_id))?,
    ))
}

#[utoipa::path(
    post,
    path = "/api/v1/llm-profiles/{profileId}/test",
    operation_id = "testLlmProfile",
    params(("profileId" = Uuid, Path, description = "Profile to test")),
    responses(
        (status = 200, description = "Content-free normalized test result", body = LlmProfileTestResultDto),
        (status = 404, description = "Profile not found", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn test_llm_profile(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(profile_id): Path<Uuid>,
) -> Result<Json<LlmProfileTestResultDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    Ok(Json(
        state
            .service
            .test_llm_profile(profile_id)
            .await
            .map_err(|error| ApiError::new(error, request_id))?,
    ))
}

#[utoipa::path(
    get,
    path = "/api/v1/llm-profiles/{profileId}/models",
    operation_id = "discoverLlmProfileModels",
    params(("profileId" = Uuid, Path, description = "Profile whose provider models are discovered")),
    responses(
        (status = 200, description = "Bounded provider model identifiers", body = Vec<String>),
        (status = 404, description = "Profile not found", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn discover_llm_profile_models(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(profile_id): Path<Uuid>,
) -> Result<Json<Vec<String>>, ApiError> {
    let request_id = extract_request_id(&request_id);
    Ok(Json(
        state
            .service
            .discover_llm_profile_models(profile_id)
            .await
            .map_err(|error| ApiError::new(error, request_id))?,
    ))
}
