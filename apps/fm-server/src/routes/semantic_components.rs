//! Thin REST adapter for managed semantic component lifecycle operations.

use axum::Json;
use axum::extract::{Extension, State};
use axum::http::StatusCode;
use fm_transport_dto::{
    AcceptSemanticInstallationOfferRequestDto, CheckpointSemanticModelMigrationRequestDto,
    CompleteSemanticModelMigrationRequestDto, ConfirmSemanticIndexRemovalRequestDto,
    ConfirmSemanticModelMigrationRequestDto, CreateSemanticIndexRemovalPlanRequestDto,
    CreateSemanticInstallationOfferRequestDto, ImportSemanticLocalModelRequestDto,
    InstallSemanticWorkerPatchRequestDto, MoveSemanticDataRequestDto,
    PlanSemanticModelMigrationRequestDto, SemanticComponentCapabilitiesDto,
    SemanticComponentErrorDto, SemanticComponentStatusDto, SemanticDataMoveReceiptDto,
    SemanticIndexRemovalPlanDto, SemanticIndexRemovalReceiptDto, SemanticInstallReceiptDto,
    SemanticInstallationOfferDto, SemanticModelMigrationPlanDto, SemanticModelMigrationProgressDto,
    SemanticModelProfileDto, SemanticModelSelectionDto, SemanticUninstallReceiptDto,
    SemanticWorkerPatchResponseDto, UninstallSemanticComponentsRequestDto,
};
use tower_http::request_id::RequestId;

use crate::error::{SemanticApiError, extract_request_id};
use crate::state::AppState;

/// Reports semantic component authority and supported operations.
#[utoipa::path(
    get,
    path = "/api/v1/semantic/components/capabilities",
    operation_id = "getSemanticComponentCapabilities",
    responses(
        (status = 200, description = "Current semantic component capabilities", body = SemanticComponentCapabilitiesDto)
    )
)]
pub(crate) async fn get_semantic_component_capabilities(
    State(state): State<AppState>,
) -> Json<SemanticComponentCapabilitiesDto> {
    Json(state.service.semantic_component_capabilities_dto().await)
}

/// Reports semantic component lifecycle, installed versions, and disk use.
#[utoipa::path(
    get,
    path = "/api/v1/semantic/components/status",
    operation_id = "getSemanticComponentStatus",
    responses(
        (status = 200, description = "Current semantic component status", body = SemanticComponentStatusDto),
        (status = 503, description = "Semantic components are unavailable", body = SemanticComponentErrorDto)
    )
)]
pub(crate) async fn get_semantic_component_status(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
) -> Result<Json<SemanticComponentStatusDto>, SemanticApiError> {
    state
        .service
        .semantic_component_status_dto()
        .await
        .map(Json)
        .map_err(|error| SemanticApiError::new(error, extract_request_id(&request_id)))
}

/// Lists curated semantic model profiles and their exact catalog revisions.
#[utoipa::path(
    get,
    path = "/api/v1/semantic/components/profiles",
    operation_id = "listSemanticComponentProfiles",
    responses(
        (status = 200, description = "Curated semantic model profiles", body = Vec<SemanticModelProfileDto>),
        (status = 503, description = "Semantic components are unavailable", body = SemanticComponentErrorDto)
    )
)]
pub(crate) async fn list_semantic_component_profiles(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
) -> Result<Json<Vec<SemanticModelProfileDto>>, SemanticApiError> {
    state
        .service
        .semantic_component_catalog_profiles_dto()
        .await
        .map(Json)
        .map_err(|error| SemanticApiError::new(error, extract_request_id(&request_id)))
}

/// Creates a complete signed installation disclosure before consent.
#[utoipa::path(
    post,
    path = "/api/v1/semantic/components/installation-offers",
    operation_id = "createSemanticComponentInstallationOffer",
    request_body = CreateSemanticInstallationOfferRequestDto,
    responses(
        (status = 200, description = "Installation disclosure awaiting consent", body = SemanticInstallationOfferDto),
        (status = 400, description = "The profile cannot be resolved", body = SemanticComponentErrorDto),
        (status = 403, description = "The runtime cannot manage component installation", body = SemanticComponentErrorDto),
        (status = 503, description = "Semantic components are unavailable", body = SemanticComponentErrorDto)
    )
)]
pub(crate) async fn create_semantic_component_installation_offer(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<CreateSemanticInstallationOfferRequestDto>,
) -> Result<Json<SemanticInstallationOfferDto>, SemanticApiError> {
    state
        .service
        .create_semantic_component_installation_offer(request)
        .await
        .map(Json)
        .map_err(|error| SemanticApiError::new(error, extract_request_id(&request_id)))
}

/// Explicitly accepts and installs one previously disclosed offer.
#[utoipa::path(
    post,
    path = "/api/v1/semantic/components/installation-offers/accept",
    operation_id = "acceptSemanticComponentInstallationOffer",
    request_body = AcceptSemanticInstallationOfferRequestDto,
    responses(
        (status = 200, description = "Installed or enabled signed components", body = SemanticInstallReceiptDto),
        (status = 403, description = "The runtime cannot manage component installation", body = SemanticComponentErrorDto),
        (status = 409, description = "Explicit consent is missing or stale", body = SemanticComponentErrorDto),
        (status = 500, description = "Component installation failed", body = SemanticComponentErrorDto)
    )
)]
pub(crate) async fn accept_semantic_component_installation_offer(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<AcceptSemanticInstallationOfferRequestDto>,
) -> Result<Json<SemanticInstallReceiptDto>, SemanticApiError> {
    state
        .service
        .accept_semantic_component_installation_offer(request)
        .await
        .map(Json)
        .map_err(|error| SemanticApiError::new(error, extract_request_id(&request_id)))
}

/// Pauses semantic indexing without uninstalling components.
#[utoipa::path(
    post,
    path = "/api/v1/semantic/components/indexing/pause",
    operation_id = "pauseSemanticComponentIndexing",
    responses(
        (status = 204, description = "Semantic indexing paused"),
        (status = 403, description = "The runtime cannot manage indexing", body = SemanticComponentErrorDto),
        (status = 409, description = "Indexing cannot be paused in the current lifecycle", body = SemanticComponentErrorDto)
    )
)]
pub(crate) async fn pause_semantic_component_indexing(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
) -> Result<StatusCode, SemanticApiError> {
    state
        .service
        .semantic_component_pause_indexing()
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| SemanticApiError::new(error, extract_request_id(&request_id)))
}

/// Resumes explicitly paused semantic indexing.
#[utoipa::path(
    post,
    path = "/api/v1/semantic/components/indexing/resume",
    operation_id = "resumeSemanticComponentIndexing",
    responses(
        (status = 204, description = "Semantic indexing resumed"),
        (status = 403, description = "The runtime cannot manage indexing", body = SemanticComponentErrorDto),
        (status = 409, description = "Indexing cannot be resumed in the current lifecycle", body = SemanticComponentErrorDto)
    )
)]
pub(crate) async fn resume_semantic_component_indexing(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
) -> Result<StatusCode, SemanticApiError> {
    state
        .service
        .semantic_component_resume_indexing()
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| SemanticApiError::new(error, extract_request_id(&request_id)))
}

/// Inventories every index record derived from one semantic enrolment.
#[utoipa::path(
    post,
    path = "/api/v1/semantic/components/index-removal-plans",
    operation_id = "createSemanticComponentIndexRemovalPlan",
    request_body = CreateSemanticIndexRemovalPlanRequestDto,
    responses(
        (status = 200, description = "Authoritative semantic index removal plan", body = SemanticIndexRemovalPlanDto),
        (status = 400, description = "The enrolment request is invalid", body = SemanticComponentErrorDto),
        (status = 403, description = "The runtime cannot remove semantic indexes", body = SemanticComponentErrorDto),
        (status = 500, description = "Index inventory failed", body = SemanticComponentErrorDto)
    )
)]
pub(crate) async fn create_semantic_component_index_removal_plan(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<CreateSemanticIndexRemovalPlanRequestDto>,
) -> Result<Json<SemanticIndexRemovalPlanDto>, SemanticApiError> {
    state
        .service
        .create_semantic_component_index_removal_plan(request)
        .await
        .map(Json)
        .map_err(|error| SemanticApiError::new(error, extract_request_id(&request_id)))
}

/// Confirms one live authoritative semantic-index removal plan.
#[utoipa::path(
    post,
    path = "/api/v1/semantic/components/index-removal-plans/confirm",
    operation_id = "confirmSemanticComponentIndexRemoval",
    request_body = ConfirmSemanticIndexRemovalRequestDto,
    responses(
        (status = 200, description = "Verified semantic index removal", body = SemanticIndexRemovalReceiptDto),
        (status = 403, description = "The runtime cannot remove semantic indexes", body = SemanticComponentErrorDto),
        (status = 500, description = "The removal plan is stale or index removal failed", body = SemanticComponentErrorDto)
    )
)]
pub(crate) async fn confirm_semantic_component_index_removal(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<ConfirmSemanticIndexRemovalRequestDto>,
) -> Result<Json<SemanticIndexRemovalReceiptDto>, SemanticApiError> {
    state
        .service
        .confirm_semantic_component_index_removal(request)
        .await
        .map(Json)
        .map_err(|error| SemanticApiError::new(error, extract_request_id(&request_id)))
}

/// Moves the semantic-data root through pause-copy-verify-switch.
#[utoipa::path(
    post,
    path = "/api/v1/semantic/components/data/move",
    operation_id = "moveSemanticComponentData",
    request_body = MoveSemanticDataRequestDto,
    responses(
        (status = 200, description = "Verified semantic-data root move", body = SemanticDataMoveReceiptDto),
        (status = 403, description = "The runtime cannot change the semantic-data root", body = SemanticComponentErrorDto),
        (status = 409, description = "Data cannot be moved in the current lifecycle", body = SemanticComponentErrorDto),
        (status = 500, description = "Data migration failed before switching roots", body = SemanticComponentErrorDto)
    )
)]
pub(crate) async fn move_semantic_component_data(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<MoveSemanticDataRequestDto>,
) -> Result<Json<SemanticDataMoveReceiptDto>, SemanticApiError> {
    state
        .service
        .move_semantic_component_data(request)
        .await
        .map(Json)
        .map_err(|error| SemanticApiError::new(error, extract_request_id(&request_id)))
}

/// Uninstalls semantic components with an explicit index-data decision.
#[utoipa::path(
    post,
    path = "/api/v1/semantic/components/uninstall",
    operation_id = "uninstallSemanticComponents",
    request_body = UninstallSemanticComponentsRequestDto,
    responses(
        (status = 200, description = "Components uninstalled with the requested index decision", body = SemanticUninstallReceiptDto),
        (status = 403, description = "The runtime cannot uninstall semantic components", body = SemanticComponentErrorDto),
        (status = 409, description = "Components cannot be uninstalled in the current lifecycle", body = SemanticComponentErrorDto),
        (status = 500, description = "Component uninstall failed", body = SemanticComponentErrorDto)
    )
)]
pub(crate) async fn uninstall_semantic_components(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<UninstallSemanticComponentsRequestDto>,
) -> Result<Json<SemanticUninstallReceiptDto>, SemanticApiError> {
    state
        .service
        .uninstall_semantic_components(request)
        .await
        .map(Json)
        .map_err(|error| SemanticApiError::new(error, extract_request_id(&request_id)))
}

/// Installs the newest compatible signed worker patch, when available.
#[utoipa::path(
    post,
    path = "/api/v1/semantic/components/worker-patches/install",
    operation_id = "installSemanticComponentWorkerPatch",
    request_body = InstallSemanticWorkerPatchRequestDto,
    responses(
        (status = 200, description = "Worker patch result", body = SemanticWorkerPatchResponseDto),
        (status = 403, description = "The runtime cannot install worker patches", body = SemanticComponentErrorDto),
        (status = 409, description = "A worker patch is invalid for the active index", body = SemanticComponentErrorDto),
        (status = 500, description = "Worker patch activation failed and was rolled back", body = SemanticComponentErrorDto)
    )
)]
pub(crate) async fn install_semantic_component_worker_patch(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<InstallSemanticWorkerPatchRequestDto>,
) -> Result<Json<SemanticWorkerPatchResponseDto>, SemanticApiError> {
    state
        .service
        .install_semantic_component_worker_patch(request)
        .await
        .map(Json)
        .map_err(|error| SemanticApiError::new(error, extract_request_id(&request_id)))
}

/// Validates an expert local model and creates a confirmation-gated migration.
#[utoipa::path(
    post,
    path = "/api/v1/semantic/components/models/import-local",
    operation_id = "importSemanticComponentLocalModel",
    request_body = ImportSemanticLocalModelRequestDto,
    responses(
        (status = 200, description = "Confirmation-gated local model migration plan", body = SemanticModelMigrationPlanDto),
        (status = 400, description = "Required local model metadata is invalid", body = SemanticComponentErrorDto),
        (status = 403, description = "Local model import is desktop-only", body = SemanticComponentErrorDto),
        (status = 409, description = "A model migration cannot be planned now", body = SemanticComponentErrorDto)
    )
)]
pub(crate) async fn import_semantic_component_local_model(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<ImportSemanticLocalModelRequestDto>,
) -> Result<Json<SemanticModelMigrationPlanDto>, SemanticApiError> {
    state
        .service
        .import_semantic_component_local_model(request)
        .await
        .map(Json)
        .map_err(|error| SemanticApiError::new(error, extract_request_id(&request_id)))
}

/// Plans migration to a signed-catalog model resolution.
#[utoipa::path(
    post,
    path = "/api/v1/semantic/components/model-migrations/plan",
    operation_id = "planSemanticComponentModelMigration",
    request_body = PlanSemanticModelMigrationRequestDto,
    responses(
        (status = 200, description = "Confirmation-gated model migration plan", body = SemanticModelMigrationPlanDto),
        (status = 400, description = "The requested profile cannot be resolved", body = SemanticComponentErrorDto),
        (status = 403, description = "The runtime cannot plan model migrations", body = SemanticComponentErrorDto),
        (status = 409, description = "A model migration cannot be planned now", body = SemanticComponentErrorDto)
    )
)]
pub(crate) async fn plan_semantic_component_model_migration(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<PlanSemanticModelMigrationRequestDto>,
) -> Result<Json<SemanticModelMigrationPlanDto>, SemanticApiError> {
    state
        .service
        .plan_semantic_component_model_migration(request)
        .await
        .map(Json)
        .map_err(|error| SemanticApiError::new(error, extract_request_id(&request_id)))
}

/// Confirms and begins one previously returned model migration plan.
#[utoipa::path(
    post,
    path = "/api/v1/semantic/components/model-migrations/confirm",
    operation_id = "confirmSemanticComponentModelMigration",
    request_body = ConfirmSemanticModelMigrationRequestDto,
    responses(
        (status = 200, description = "Initial resumable model migration progress", body = SemanticModelMigrationProgressDto),
        (status = 403, description = "The runtime cannot confirm model migrations", body = SemanticComponentErrorDto),
        (status = 409, description = "The migration plan is stale or unknown", body = SemanticComponentErrorDto)
    )
)]
pub(crate) async fn confirm_semantic_component_model_migration(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<ConfirmSemanticModelMigrationRequestDto>,
) -> Result<Json<SemanticModelMigrationProgressDto>, SemanticApiError> {
    state
        .service
        .confirm_semantic_component_model_migration(request)
        .await
        .map(Json)
        .map_err(|error| SemanticApiError::new(error, extract_request_id(&request_id)))
}

/// Persists a resumable model migration checkpoint.
#[utoipa::path(
    post,
    path = "/api/v1/semantic/components/model-migrations/checkpoint",
    operation_id = "checkpointSemanticComponentModelMigration",
    request_body = CheckpointSemanticModelMigrationRequestDto,
    responses(
        (status = 200, description = "Updated durable migration progress", body = SemanticModelMigrationProgressDto),
        (status = 403, description = "The runtime cannot checkpoint model migrations", body = SemanticComponentErrorDto),
        (status = 409, description = "Migration progress is stale or invalid", body = SemanticComponentErrorDto)
    )
)]
pub(crate) async fn checkpoint_semantic_component_model_migration(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<CheckpointSemanticModelMigrationRequestDto>,
) -> Result<Json<SemanticModelMigrationProgressDto>, SemanticApiError> {
    state
        .service
        .checkpoint_semantic_component_model_migration(request)
        .await
        .map(Json)
        .map_err(|error| SemanticApiError::new(error, extract_request_id(&request_id)))
}

/// Completes and activates one fully reindexed model migration.
#[utoipa::path(
    post,
    path = "/api/v1/semantic/components/model-migrations/complete",
    operation_id = "completeSemanticComponentModelMigration",
    request_body = CompleteSemanticModelMigrationRequestDto,
    responses(
        (status = 200, description = "Activated model selection", body = SemanticModelSelectionDto),
        (status = 403, description = "The runtime cannot complete model migrations", body = SemanticComponentErrorDto),
        (status = 409, description = "Migration progress is incomplete or invalid", body = SemanticComponentErrorDto)
    )
)]
pub(crate) async fn complete_semantic_component_model_migration(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<CompleteSemanticModelMigrationRequestDto>,
) -> Result<Json<SemanticModelSelectionDto>, SemanticApiError> {
    state
        .service
        .complete_semantic_component_model_migration(request)
        .await
        .map(Json)
        .map_err(|error| SemanticApiError::new(error, extract_request_id(&request_id)))
}
