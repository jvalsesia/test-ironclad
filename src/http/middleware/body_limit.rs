//! Request body cap, enforced before routing.

use axum::extract::Request;
use axum::http::header::CONTENT_LENGTH;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::http::error::ApiError;

/// "64 KB" is 64 KiB: 65,536 bytes are accepted, 65,537 are rejected.
pub const MAX_BODY_BYTES: usize = 65_536;

/// Rejects any request whose declared `Content-Length` exceeds
/// [`MAX_BODY_BYTES`] with the uniform 413, whether or not a route matches.
/// Streamed bodies without a length are capped by the `ApiJson` extractor.
pub async fn enforce_body_limit(request: Request, next: Next) -> Response {
    let declared = request
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok());
    if declared.is_some_and(|len| len > MAX_BODY_BYTES as u64) {
        return ApiError::payload_too_large().into_response();
    }
    next.run(request).await
}
