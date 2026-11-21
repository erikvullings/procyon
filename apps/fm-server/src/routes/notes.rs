//! Thin HTTP adapter for notes (task 0337). Drafting a note from a terminal session is not here:
//! like the terminal itself it only exists in the desktop host.

use axum::Json;
use axum::extract::{Extension, State};
use fm_transport_dto::{
    ApplicationErrorDto, NoteDto, NotesSearchResultDto, NotesSetupDto, ReadNoteRequestDto,
    SearchNotesRequestDto, SetNotesRootRequestDto,
};
use tower_http::request_id::RequestId;

use crate::error::{ApiError, extract_request_id, require_within_roots};
use crate::state::AppState;

#[utoipa::path(
    get,
    path = "/api/v1/notes/setup",
    operation_id = "getNotesSetup",
    responses((status = 200, body = NotesSetupDto))
)]
pub(crate) async fn get_notes_setup(State(state): State<AppState>) -> Json<NotesSetupDto> {
    Json(state.service.notes_setup().await)
}

#[utoipa::path(
    post,
    path = "/api/v1/notes/root",
    operation_id = "setNotesRoot",
    request_body = SetNotesRootRequestDto,
    responses(
        (status = 200, body = NotesSetupDto),
        (status = 400, body = ApplicationErrorDto),
        (status = 403, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn set_notes_root(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<SetNotesRootRequestDto>,
) -> Result<Json<NotesSetupDto>, ApiError> {
    let correlation_id = extract_request_id(&request_id);
    require_within_roots(&request.location, &state.accessible_roots, correlation_id)?;
    state
        .service
        .set_notes_root(request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/notes/search",
    operation_id = "searchNotes",
    request_body = SearchNotesRequestDto,
    responses(
        (status = 200, body = NotesSearchResultDto),
        (status = 400, body = ApplicationErrorDto),
        (status = 403, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn search_notes(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<SearchNotesRequestDto>,
) -> Result<Json<NotesSearchResultDto>, ApiError> {
    require_root_within_roots(&state, extract_request_id(&request_id)).await?;
    state
        .service
        .search_notes(state.semantic_access(), request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/notes/read",
    operation_id = "readNote",
    request_body = ReadNoteRequestDto,
    responses(
        (status = 200, body = NoteDto),
        (status = 400, body = ApplicationErrorDto),
        (status = 403, body = ApplicationErrorDto),
        (status = 404, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn read_note(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<ReadNoteRequestDto>,
) -> Result<Json<NoteDto>, ApiError> {
    require_root_within_roots(&state, extract_request_id(&request_id)).await?;
    state
        .service
        .read_note(request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}

/// The stored notes folder must still lie inside the accessible roots, which may have changed
/// since it was chosen. Notes inside it are reached without following links, so they stay inside.
async fn require_root_within_roots(
    state: &AppState,
    request_id: uuid::Uuid,
) -> Result<(), ApiError> {
    match state.service.notes_setup().await.root {
        Some(root) => require_within_roots(&root, &state.accessible_roots, request_id),
        None => Ok(()),
    }
}
