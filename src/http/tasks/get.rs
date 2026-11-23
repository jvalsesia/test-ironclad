//! `GET /tasks/{id}`.

use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::get;

use crate::application::tasks::GetTask;
use crate::http::error::ApiError;
use crate::http::state::AppState;

use super::dto::TaskResponse;
use super::task_id_path::TaskIdPath;

pub fn routes() -> Router<AppState> {
    Router::new().route("/tasks/{id}", get(get_task_handler))
}

async fn get_task_handler(
    State(state): State<AppState>,
    TaskIdPath(id): TaskIdPath,
) -> Result<Json<TaskResponse>, ApiError> {
    let task = GetTask::new(Arc::clone(&state.tasks)).execute(id).await?;
    Ok(Json(TaskResponse::from(&task)))
}
