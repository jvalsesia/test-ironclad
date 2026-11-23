//! OpenAPI document root and its documentation-only schema types.
//!
//! Operations are collected from the `#[utoipa::path]` annotations of the
//! handlers registered with `utoipa_axum::routes!`; this module only adds
//! the document metadata and the named component schemas.

use utoipa::openapi::OpenApi as OpenApiDocument;
use utoipa::{Modify, OpenApi};

use crate::http::error::{ErrorDetail, ErrorEnvelope};
use crate::http::health::HealthResponse;
use crate::http::tasks::page::TaskPageResponse;
use crate::http::tasks::{CreateTaskRequest, TaskResponse, UpdateTaskRequest};

pub mod schemas;

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
