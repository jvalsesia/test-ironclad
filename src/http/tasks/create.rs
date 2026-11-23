//! `POST /tasks`.

use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::header::LOCATION;
use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::routing::post;

use crate::application::tasks::CreateTask;
use crate::http::error::ApiError;
use crate::http::extract::ApiJson;
use crate::http::state::AppState;

use super::dto::{CreateTaskRequest, TaskResponse};

pub fn routes() -> Router<AppState> {
    Router::new().route("/tasks", post(create_task_handler))
}

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
