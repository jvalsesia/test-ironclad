//! `PATCH /tasks/{id}`.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::application::tasks::UpdateTask;
use crate::http::error::{ApiError, ErrorEnvelope};
use crate::http::extract::ApiJson;
use crate::http::openapi::schemas::{
    task_not_found_example, update_task_request_example, updated_task_example,
    validation_error_example,
};
use crate::http::state::AppState;

use super::dto::{TaskResponse, UpdateTaskRequest};
use super::task_id_path::TaskIdPath;

pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(update_task_handler))
}

// The id is extracted before the body, so a malformed id wins over any
// body error; validation runs before the single update statement.
#[utoipa::path(
    patch,
    path = "/tasks/{id}",
    tag = "Tasks",
    operation_id = "updateTask",
    summary = "Update a task",
    description = "Partially updates a task: absent fields are left unchanged, `null` clears `description` or `due_date`. At least one updatable field must be provided.",
    params(
        ("id" = String, Path, format = Uuid, description = "36-character hyphenated UUID", example = "3f2b6a1c-8d4e-4f7a-9b2c-1e5d7a9c0b3f")
    ),
    request_body(content((UpdateTaskRequest = "application/json", example = update_task_request_example))),
    responses(
        (status = 200, description = "Updated task", content((TaskResponse = "application/json", example = updated_task_example))),
        (status = 400, description = "Malformed id or JSON (`invalid_id`, `invalid_json`)", body = ErrorEnvelope),
        (status = 404, description = "No task with this id (`not_found`)", content((ErrorEnvelope = "application/json", example = task_not_found_example))),
        (status = 413, description = "Request body exceeds 64 KB (`payload_too_large`)", body = ErrorEnvelope),
        (status = 415, description = "Content-Type is not application/json (`unsupported_media_type`)", body = ErrorEnvelope),
        (status = 422, description = "Validation failed or no updatable field given (`validation_error`)", content((ErrorEnvelope = "application/json", example = validation_error_example))),
        (status = 500, description = "Unexpected server error (`internal_error`)", body = ErrorEnvelope),
        (status = 503, description = "Database temporarily unavailable (`service_unavailable`)", body = ErrorEnvelope),
    )
)]
async fn update_task_handler(
    State(state): State<AppState>,
    TaskIdPath(id): TaskIdPath,
    ApiJson(body): ApiJson<UpdateTaskRequest>,
) -> Result<Json<TaskResponse>, ApiError> {
    let task = UpdateTask::new(Arc::clone(&state.tasks))
        .execute(id, body.into())
        .await?;
    Ok(Json(TaskResponse::from(&task)))
}
