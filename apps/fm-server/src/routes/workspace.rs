//! Workspace REST surface (spec §5.3.12): thin handlers that translate a
//! request into a `FileManagerService` call and back into a DTO, with no
//! validation/mutation logic of their own (spec §3 rule 2).

use axum::Json;
use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use fm_transport_dto::{
    ApplicationErrorDto, CreateWorkspaceRequestDto, WorkspaceCommandDto, WorkspaceDto,
    WorkspaceSummaryDto,
};
use serde::Deserialize;
use std::time::Instant;
use tower_http::request_id::RequestId;
use tracing::{info, warn};
use utoipa::IntoParams;
use uuid::Uuid;

use crate::error::{ApiError, extract_request_id};
use crate::state::AppState;

/// Lists every stored workspace as a lightweight summary.
#[utoipa::path(
    get,
    path = "/api/v1/workspaces",
    operation_id = "listWorkspaces",
    responses((status = 200, description = "Every stored workspace", body = Vec<WorkspaceSummaryDto>))
)]
pub(crate) async fn list_workspaces(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
) -> Result<Json<Vec<WorkspaceSummaryDto>>, ApiError> {
    let request_id = extract_request_id(&request_id);
    let summaries = state
        .service
        .list_workspaces()
        .await
        .map_err(|error| ApiError::new(error, request_id))?;
    Ok(Json(summaries))
}

/// Query parameters accepted by [`start_workspace`].
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StartWorkspaceQuery {
    /// Opens this workspace explicitly. When absent, the last-active
    /// workspace is reopened, falling back to a fresh default if none exists
    /// or the last-active one is missing/corrupt (spec §5.3.7).
    workspace_id: Option<Uuid>,
}

/// Runs the workspace startup lifecycle (spec §5.3.7): opens the requested
/// workspace, otherwise the last-active one, otherwise creates a default.
#[utoipa::path(
    post,
    path = "/api/v1/workspaces/start",
    operation_id = "startWorkspace",
    params(StartWorkspaceQuery),
    responses((status = 200, description = "The selected or created workspace", body = WorkspaceDto))
)]
pub(crate) async fn start_workspace(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Query(query): Query<StartWorkspaceQuery>,
) -> Result<Json<WorkspaceDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    let workspace = state
        .service
        .start_workspace(query.workspace_id)
        .await
        .map_err(|error| ApiError::new(error, request_id))?;
    Ok(Json(workspace))
}

/// Creates and persists a new workspace.
#[utoipa::path(
    post,
    path = "/api/v1/workspaces",
    operation_id = "createWorkspace",
    request_body = CreateWorkspaceRequestDto,
    responses(
        (status = 201, description = "The created workspace", body = WorkspaceDto),
        (status = 400, description = "The request was invalid", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn create_workspace(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<CreateWorkspaceRequestDto>,
) -> Result<(StatusCode, Json<WorkspaceDto>), ApiError> {
    let request_id = extract_request_id(&request_id);
    let workspace = state
        .service
        .create_workspace(request.name)
        .await
        .map_err(|error| ApiError::new(error, request_id))?;
    Ok((StatusCode::CREATED, Json(workspace)))
}

/// Loads a single workspace by id.
#[utoipa::path(
    get,
    path = "/api/v1/workspaces/{workspaceId}",
    operation_id = "getWorkspace",
    params(("workspaceId" = Uuid, Path, description = "The workspace to load")),
    responses(
        (status = 200, description = "The workspace", body = WorkspaceDto),
        (status = 404, description = "No workspace exists with this id", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn get_workspace(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(workspace_id): Path<Uuid>,
) -> Result<Json<WorkspaceDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    let workspace = state
        .service
        .get_workspace(workspace_id)
        .await
        .map_err(|error| ApiError::new(error, request_id))?;
    Ok(Json(workspace))
}

/// Query parameters accepted by [`delete_workspace`].
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeleteWorkspaceQuery {
    /// The revision the caller last observed; enforces the same optimistic
    /// concurrency check as every other mutation (spec §5.3.10).
    expected_revision: Option<u64>,
}

/// Deletes a workspace.
#[utoipa::path(
    delete,
    path = "/api/v1/workspaces/{workspaceId}",
    operation_id = "deleteWorkspace",
    params(
        ("workspaceId" = Uuid, Path, description = "The workspace to delete"),
        DeleteWorkspaceQuery,
    ),
    responses(
        (status = 204, description = "The workspace was deleted"),
        (status = 404, description = "No workspace exists with this id", body = ApplicationErrorDto),
        (status = 409, description = "The workspace's revision changed", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn delete_workspace(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(workspace_id): Path<Uuid>,
    Query(query): Query<DeleteWorkspaceQuery>,
) -> Result<StatusCode, ApiError> {
    let request_id = extract_request_id(&request_id);
    state
        .service
        .delete_workspace(workspace_id, query.expected_revision)
        .await
        .map_err(|error| ApiError::new(error, request_id))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Selects an existing workspace as the last-active workspace.
#[utoipa::path(
    post,
    path = "/api/v1/workspaces/{workspaceId}/open",
    operation_id = "openWorkspace",
    params(("workspaceId" = Uuid, Path, description = "The workspace to open")),
    responses(
        (status = 200, description = "The opened workspace", body = WorkspaceDto),
        (status = 404, description = "No workspace exists with this id", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn open_workspace(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(workspace_id): Path<Uuid>,
) -> Result<Json<WorkspaceDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    let workspace = state
        .service
        .open_workspace(workspace_id)
        .await
        .map_err(|error| ApiError::new(error, request_id))?;
    Ok(Json(workspace))
}

/// Applies a semantic mutation command (spec §5.3.9). The path's
/// `workspaceId` must match the id embedded in the command body.
#[utoipa::path(
    post,
    path = "/api/v1/workspaces/{workspaceId}/commands",
    operation_id = "applyWorkspaceCommand",
    params(("workspaceId" = Uuid, Path, description = "The workspace the command targets")),
    request_body = WorkspaceCommandDto,
    responses(
        (status = 200, description = "The changed workspace", body = WorkspaceDto),
        (status = 400, description = "The path and body workspace ids did not match", body = ApplicationErrorDto),
        (status = 404, description = "No workspace exists with this id", body = ApplicationErrorDto),
        (status = 409, description = "The workspace's revision changed", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn apply_workspace_command(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(workspace_id): Path<Uuid>,
    Json(command): Json<WorkspaceCommandDto>,
) -> Result<Json<WorkspaceDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    if command_workspace_id(&command) != workspace_id {
        return Err(ApiError::new(
            fm_application::ApplicationError::InvalidRequest(
                "the command's workspaceId did not match the request path".to_owned(),
            ),
            request_id,
        ));
    }

    let started = Instant::now();
    let command_kind = workspace_command_kind(&command);
    tracing::Span::current().record("workspace_id", workspace_id.to_string().as_str());
    info!(
        request_id = %request_id,
        workspace_id = %workspace_id,
        command = command_kind,
        "apply_workspace_command received"
    );
    match state.service.apply_workspace_command(command).await {
        Ok(workspace) => {
            info!(
                request_id = %request_id,
                workspace_id = %workspace_id,
                elapsed_ms = started.elapsed().as_millis(),
                "apply_workspace_command honored"
            );
            Ok(Json(workspace))
        }
        Err(error) => {
            warn!(
                request_id = %request_id,
                workspace_id = %workspace_id,
                elapsed_ms = started.elapsed().as_millis(),
                command = command_kind,
                error = ?error,
                "apply_workspace_command failed"
            );
            Err(ApiError::new(error, request_id))
        }
    }
}

/// Extracts the target workspace id carried by every [`WorkspaceCommandDto`]
/// variant.
fn command_workspace_id(command: &WorkspaceCommandDto) -> Uuid {
    match command {
        WorkspaceCommandDto::RenameWorkspace { workspace_id, .. }
        | WorkspaceCommandDto::SetActivePane { workspace_id, .. }
        | WorkspaceCommandDto::AddTab { workspace_id, .. }
        | WorkspaceCommandDto::AddTransientTab { workspace_id, .. }
        | WorkspaceCommandDto::CloseTab { workspace_id, .. }
        | WorkspaceCommandDto::MoveTab { workspace_id, .. }
        | WorkspaceCommandDto::ActivateTab { workspace_id, .. }
        | WorkspaceCommandDto::NavigateTab { workspace_id, .. }
        | WorkspaceCommandDto::UpdateView { workspace_id, .. }
        | WorkspaceCommandDto::UpdateLayout { workspace_id, .. } => *workspace_id,
    }
}

fn workspace_command_kind(command: &WorkspaceCommandDto) -> &'static str {
    match command {
        WorkspaceCommandDto::RenameWorkspace { .. } => "renameWorkspace",
        WorkspaceCommandDto::SetActivePane { .. } => "setActivePane",
        WorkspaceCommandDto::AddTab { .. } => "addTab",
        WorkspaceCommandDto::AddTransientTab { .. } => "addTransientTab",
        WorkspaceCommandDto::CloseTab { .. } => "closeTab",
        WorkspaceCommandDto::MoveTab { .. } => "moveTab",
        WorkspaceCommandDto::ActivateTab { .. } => "activateTab",
        WorkspaceCommandDto::NavigateTab { .. } => "navigateTab",
        WorkspaceCommandDto::UpdateView { .. } => "updateView",
        WorkspaceCommandDto::UpdateLayout { .. } => "updateLayout",
    }
}
