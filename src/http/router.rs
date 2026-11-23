//! Router composition.

use std::any::Any;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::middleware::from_fn;
use axum::response::{IntoResponse, Response};
use tower_http::catch_panic::CatchPanicLayer;
use tracing::error;
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;

use super::error::ApiError;
use super::health;
use super::middleware::{MAX_BODY_BYTES, enforce_body_limit, request_context};
use super::openapi::ApiDoc;
use super::state::AppState;
use super::tasks;
use super::test_routes;

#[derive(Debug, Clone, Copy, Default)]
pub struct RouterOptions {
    /// Mount the `/__test/*` diagnostic routes.
    pub test_routes: bool,
}

/// Builds the application router. Layer order, outermost first:
/// request context (ID, span, completion log) → panic containment → body
/// limit → route / fallback. The request context is outermost so every
/// response, including panics and early rejections, is covered.
///
/// Production routes are registered through the documented router, so each
/// served route has an OpenAPI operation; the test routes are merged after
/// the split and never appear in the document.
pub fn build_router(state: AppState, options: RouterOptions) -> Router {
    let (mut router, _doc) = documented_routes().split_for_parts();
    if options.test_routes {
        router = router.merge(test_routes::routes());
    }
    router
        .fallback(route_not_found)
        .method_not_allowed_fallback(method_not_allowed)
        .with_state(state)
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(from_fn(enforce_body_limit))
        .layer(CatchPanicLayer::custom(panic_response))
        .layer(from_fn(request_context))
}

/// Every production route, with its OpenAPI operation.
fn documented_routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::with_openapi(ApiDoc::openapi())
        .merge(health::routes())
        .merge(tasks::routes())
}

async fn route_not_found() -> ApiError {
    ApiError::route_not_found()
}

async fn method_not_allowed() -> ApiError {
    ApiError::method_not_allowed()
}

/// Logs the panic payload server-side and returns the uniform 500.
fn panic_response(payload: Box<dyn Any + Send + 'static>) -> Response {
    let detail = payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-string panic payload".to_owned());
    error!("handler panicked: {detail}");
    ApiError::internal().into_response()
}
