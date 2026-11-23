//! OpenAPI document root, documentation-only schema types and the
//! documentation routes (`GET /openapi.json`, Swagger UI at `/docs`).
//!
//! Operations are collected from the `#[utoipa::path]` annotations of the
//! handlers registered with `utoipa_axum::routes!`; this module only adds
//! the document metadata and the named component schemas. The routes here
//! are mounted outside the documented router, so they never appear in the
//! document.

use std::sync::Arc;

use axum::Router;
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use tracing::error;
use utoipa::openapi::OpenApi as OpenApiDocument;
use utoipa::{Modify, OpenApi};

use crate::http::error::{ApiError, ErrorDetail, ErrorEnvelope};
use crate::http::health::HealthResponse;
use crate::http::state::AppState;
use crate::http::tasks::page::TaskPageResponse;
use crate::http::tasks::{CreateTaskRequest, TaskResponse, UpdateTaskRequest};

pub mod schemas;
pub mod swagger;

use schemas::{TaskPrioritySchema, TaskStatusSchema};

/// Document metadata: title, version (crate version), tags and the nine
/// named schemas. Paths are added by the documented router.
#[derive(OpenApi)]
#[openapi(
    info(title = "Task API"),
    tags(
        (name = "Tasks", description = "Create, read, list, update and delete tasks"),
        (name = "Health", description = "Service and database health"),
    ),
    components(schemas(
        TaskResponse,
        CreateTaskRequest,
        UpdateTaskRequest,
        TaskPageResponse,
        TaskStatusSchema,
        TaskPrioritySchema,
        ErrorEnvelope,
        ErrorDetail,
        HealthResponse,
    )),
    modifiers(&NoLicense)
)]
pub struct ApiDoc;

/// utoipa fills `info.license` from the (unset) crate license; the document
/// declares none.
struct NoLicense;

impl Modify for NoLicense {
    fn modify(&self, openapi: &mut OpenApiDocument) {
        openapi.info.license = None;
    }
}

/// `GET /openapi.json` (serving `doc`, built once by `build_router`) and the
/// Swagger UI routes.
pub fn docs_routes(doc: OpenApiDocument) -> Router<AppState> {
    let doc = Arc::new(doc);
    Router::new()
        .route(
            swagger::OPENAPI_URL,
            get(move || openapi_json(Arc::clone(&doc))),
        )
        .merge(swagger::routes())
}

async fn openapi_json(doc: Arc<OpenApiDocument>) -> Response {
    match serde_json::to_vec(doc.as_ref()) {
        Ok(body) => (
            [(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            )],
            body,
        )
            .into_response(),
        Err(err) => {
            error!("failed to serialize the OpenAPI document: {err}");
            ApiError::internal().into_response()
        }
    }
}
