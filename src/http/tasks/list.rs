//! `GET /tasks`.

use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::get;

use crate::application::tasks::ListTasks;
use crate::http::error::ApiError;
use crate::http::state::AppState;

use super::list_params::ListTasksParams;
use super::page::TaskPageResponse;

pub fn routes() -> Router<AppState> {
    Router::new().route("/tasks", get(list_tasks_handler))
}

/// Invalid query parameters are rejected by the extractor before any
/// database access.
async fn list_tasks_handler(
    State(state): State<AppState>,
    ListTasksParams(query): ListTasksParams,
) -> Result<Json<TaskPageResponse>, ApiError> {
    let page = ListTasks::new(Arc::clone(&state.tasks))
        .execute(&query)
        .await?;
    Ok(Json(TaskPageResponse::new(&page, &query)))
}
