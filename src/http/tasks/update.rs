//! `PATCH /tasks/{id}`.

use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::patch;

use crate::application::tasks::UpdateTask;
use crate::http::error::ApiError;
use crate::http::extract::ApiJson;
use crate::http::state::AppState;

use super::dto::{TaskResponse, UpdateTaskRequest};
use super::task_id_path::TaskIdPath;

pub fn routes() -> Router<AppState> {
    Router::new().route("/tasks/{id}", patch(update_task_handler))
}

/// The id is extracted before the body, so a malformed id wins over any
/// body error; validation runs before the single update statement.
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
