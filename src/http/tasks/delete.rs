//! `DELETE /tasks/{id}`.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::application::tasks::DeleteTask;
use crate::http::error::{ApiError, ErrorEnvelope};
use crate::http::openapi::schemas::{invalid_id_example, task_not_found_example};
use crate::http::state::AppState;

use super::task_id_path::TaskIdPath;

pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(delete_task_handler))
}

// `204` with an empty body; any request body is ignored.
#[utoipa::path(
    delete,
    path = "/tasks/{id}",
    tag = "Tasks",
    operation_id = "deleteTask",
    summary = "Delete a task",
    description = "Permanently deletes a task. Any request body is ignored.",
    params(
        ("id" = String, Path, format = Uuid, description = "36-character hyphenated UUID", example = "3f2b6a1c-8d4e-4f7a-9b2c-1e5d7a9c0b3f")
    ),
    responses(
        (status = 204, description = "Task deleted (empty body)"),
        (status = 400, description = "Malformed id (`invalid_id`)", content((ErrorEnvelope = "application/json", example = invalid_id_example))),
        (status = 404, description = "No task with this id (`not_found`)", content((ErrorEnvelope = "application/json", example = task_not_found_example))),
        (status = 500, description = "Unexpected server error (`internal_error`)", body = ErrorEnvelope),
        (status = 503, description = "Database temporarily unavailable (`service_unavailable`)", body = ErrorEnvelope),
    )
)]
async fn delete_task_handler(
    State(state): State<AppState>,
    TaskIdPath(id): TaskIdPath,
) -> Result<StatusCode, ApiError> {
    DeleteTask::new(Arc::clone(&state.tasks))
        .execute(id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
