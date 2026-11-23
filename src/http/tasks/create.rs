//! `POST /tasks`.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::header::LOCATION;
use axum::http::{HeaderName, HeaderValue, StatusCode};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::application::tasks::CreateTask;
use crate::http::error::{ApiError, ErrorEnvelope};
use crate::http::extract::ApiJson;
use crate::http::openapi::schemas::{
    create_task_request_example, task_example, validation_error_example,
};
use crate::http::state::AppState;

use super::dto::{CreateTaskRequest, TaskResponse};

pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(create_task_handler))
}

#[utoipa::path(
    post,
    path = "/tasks",
    tag = "Tasks",
    operation_id = "createTask",
    summary = "Create a task",
    description = "Creates a task. `status` defaults to `todo` and `priority` to `medium`; every invalid field is reported at once.",
    request_body(content((CreateTaskRequest = "application/json", example = create_task_request_example))),
    responses(
        (status = 201, description = "Task created", content((TaskResponse = "application/json", example = task_example)),
            headers(("Location" = String, description = "Path of the new task: /tasks/{id}"))),
        (status = 400, description = "Malformed JSON or a field of the wrong type (`invalid_json`)", body = ErrorEnvelope),
        (status = 413, description = "Request body exceeds 64 KB (`payload_too_large`)", body = ErrorEnvelope),
        (status = 415, description = "Content-Type is not application/json (`unsupported_media_type`)", body = ErrorEnvelope),
        (status = 422, description = "Validation failed; every invalid field is listed in `details` (`validation_error`)", content((ErrorEnvelope = "application/json", example = validation_error_example))),
        (status = 500, description = "Unexpected server error (`internal_error`)", body = ErrorEnvelope),
        (status = 503, description = "Database temporarily unavailable (`service_unavailable`)", body = ErrorEnvelope),
    )
)]
async fn create_task_handler(
    State(state): State<AppState>,
    ApiJson(body): ApiJson<CreateTaskRequest>,
) -> Result<
    (
        StatusCode,
        [(HeaderName, HeaderValue); 1],
        Json<TaskResponse>,
    ),
    ApiError,
> {
    let task = CreateTask::new(Arc::clone(&state.tasks))
        .execute(body.into())
        .await?;
    let location = HeaderValue::from_str(&format!("/tasks/{}", task.id().value()))
        .map_err(|_| ApiError::internal())?;
    Ok((
        StatusCode::CREATED,
        [(LOCATION, location)],
        Json(TaskResponse::from(&task)),
    ))
}
