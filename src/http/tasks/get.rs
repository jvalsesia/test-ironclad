//! `GET /tasks/{id}`.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::application::tasks::GetTask;
use crate::http::error::{ApiError, ErrorEnvelope};
use crate::http::openapi::schemas::{invalid_id_example, task_example, task_not_found_example};
use crate::http::state::AppState;

use super::dto::TaskResponse;
use super::task_id_path::TaskIdPath;

pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(get_task_handler))
}

#[utoipa::path(
    get,
    path = "/tasks/{id}",
    tag = "Tasks",
    operation_id = "getTask",
    summary = "Get a task",
    description = "Returns one task by id.",
    params(
        ("id" = String, Path, format = Uuid, description = "36-character hyphenated UUID", example = "3f2b6a1c-8d4e-4f7a-9b2c-1e5d7a9c0b3f")
    ),
    responses(
        (status = 200, description = "The task", content((TaskResponse = "application/json", example = task_example))),
        (status = 400, description = "Malformed id (`invalid_id`)", content((ErrorEnvelope = "application/json", example = invalid_id_example))),
        (status = 404, description = "No task with this id (`not_found`)", content((ErrorEnvelope = "application/json", example = task_not_found_example))),
        (status = 500, description = "Unexpected server error (`internal_error`)", body = ErrorEnvelope),
        (status = 503, description = "Database temporarily unavailable (`service_unavailable`)", body = ErrorEnvelope),
    )
)]
async fn get_task_handler(
    State(state): State<AppState>,
    TaskIdPath(id): TaskIdPath,
) -> Result<Json<TaskResponse>, ApiError> {
    let task = GetTask::new(Arc::clone(&state.tasks)).execute(id).await?;
    Ok(Json(TaskResponse::from(&task)))
}
