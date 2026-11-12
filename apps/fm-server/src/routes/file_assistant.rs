//! Thin REST adapter for the file assistant (task 0321).

use std::convert::Infallible;

use axum::{
    Json,
    body::{Body, Bytes},
    extract::State,
    http::{StatusCode, header},
    response::Response,
};
use fm_transport_dto::{
    CancelFileAssistantRequestDto, FileAssistantEventDto, FileAssistantRequestDto,
};
use tokio::sync::mpsc;

use crate::state::AppState;

/// Streams the next reply of a file-assistant conversation as newline-delimited JSON events:
/// fragments as they arrive, then exactly one `done` or `failed` event. Every outcome, including
/// a refused request, arrives as an event on this 200 response.
#[utoipa::path(
    post,
    path = "/api/v1/file-assistant/reply/stream",
    operation_id = "streamFileAssistantReply",
    request_body = FileAssistantRequestDto,
    responses(
        (
            status = 200,
            description = "Newline-delimited JSON file-assistant events.",
            content_type = "application/x-ndjson",
            body = FileAssistantEventDto
        )
    )
)]
pub(crate) async fn stream_file_assistant_reply(
    State(state): State<AppState>,
    Json(request): Json<FileAssistantRequestDto>,
) -> Response {
    let (sender, receiver) = mpsc::unbounded_channel::<FileAssistantEventDto>();
    let service = state.service.clone();
    let request_id = request.request_id;
    tokio::spawn(async move {
        let canceller = service.clone();
        service
            .stream_file_assistant_reply(request, &mut |event| {
                // The client went away: stop the request instead of paying for an unread reply.
                if sender.send(event).is_err() {
                    let _ = canceller
                        .cancel_file_assistant_reply(CancelFileAssistantRequestDto { request_id });
                }
            })
            .await;
    });
    let lines = futures::stream::unfold(receiver, |mut receiver| async move {
        let event = receiver.recv().await?;
        let mut line = serde_json::to_vec(&event).unwrap_or_default();
        line.push(b'\n');
        Some((Ok::<_, Infallible>(Bytes::from(line)), receiver))
    });
    Response::builder()
        .header(header::CONTENT_TYPE, "application/x-ndjson")
        .body(Body::from_stream(lines))
        .unwrap_or_default()
}

/// Cancels one running file-assistant reply.
#[utoipa::path(
    post,
    path = "/api/v1/file-assistant/reply/cancel",
    operation_id = "cancelFileAssistantReply",
    request_body = CancelFileAssistantRequestDto,
    responses((status = 204))
)]
pub(crate) async fn cancel_file_assistant_reply(
    State(state): State<AppState>,
    Json(request): Json<CancelFileAssistantRequestDto>,
) -> StatusCode {
    state.service.cancel_file_assistant_reply(request);
    StatusCode::NO_CONTENT
}
