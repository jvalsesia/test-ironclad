//! `DELETE /tasks/{id}`.

use std::sync::Arc;

use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::delete;

use crate::application::tasks::DeleteTask;
use crate::http::error::ApiError;
use crate::http::state::AppState;

use super::task_id_path::TaskIdPath;

pub fn routes() -> Router<AppState> {
    Router::new().route("/tasks/{id}", delete(delete_task_handler))
}

/// `204` with an empty body; any request body is ignored.
async fn delete_task_handler(
    State(state): State<AppState>,
    TaskIdPath(id): TaskIdPath,
) -> Result<StatusCode, ApiError> {
    DeleteTask::new(Arc::clone(&state.tasks))
        .execute(id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
