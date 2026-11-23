//! `GET /tasks`.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::application::tasks::ListTasks;
use crate::http::error::{ApiError, ErrorEnvelope};
use crate::http::openapi::schemas::task_page_example;
use crate::http::state::AppState;

use super::list_params::{ListTasksParams, ListTasksQuery};
use super::page::TaskPageResponse;

pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(list_tasks_handler))
}

// Invalid query parameters are rejected by the extractor before any
// database access.
#[utoipa::path(
    get,
    path = "/tasks",
    tag = "Tasks",
    operation_id = "listTasks",
    summary = "List tasks",
    description = "Lists tasks with optional filters, sorting and pagination. Unknown query parameters are ignored; every invalid known parameter is reported at once.",
    params(ListTasksQuery),
    responses(
        (status = 200, description = "One page of tasks", content((TaskPageResponse = "application/json", example = task_page_example))),
        (status = 400, description = "Invalid query parameters (`invalid_query`)", body = ErrorEnvelope),
        (status = 500, description = "Unexpected server error (`internal_error`)", body = ErrorEnvelope),
        (status = 503, description = "Database temporarily unavailable (`service_unavailable`)", body = ErrorEnvelope),
    )
)]
async fn list_tasks_handler(
    State(state): State<AppState>,
    ListTasksParams(query): ListTasksParams,
) -> Result<Json<TaskPageResponse>, ApiError> {
    let page = ListTasks::new(Arc::clone(&state.tasks))
        .execute(&query)
        .await?;
    Ok(Json(TaskPageResponse::new(&page, &query)))
}
