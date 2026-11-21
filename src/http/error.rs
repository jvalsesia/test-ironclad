//! The uniform error body and the single place where HTTP statuses for
//! errors are chosen.
//!
//! Every non-2xx response has the shape
//! `{"error":{"code":"…","message":"…","details":[{"field":"…","message":"…"}]}}`.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use tracing::{error, warn};

use crate::application::AppError;
use crate::domain::shared::FieldError;

pub const MSG_INVALID_JSON: &str = "Request body is not valid JSON for this endpoint";
pub const MSG_NOT_FOUND_ROUTE: &str = "Route not found";
pub const MSG_METHOD_NOT_ALLOWED: &str = "Method not allowed";
pub const MSG_PAYLOAD_TOO_LARGE: &str = "Request body exceeds 64 KB";
pub const MSG_UNSUPPORTED_MEDIA_TYPE: &str = "Content-Type must be application/json";
pub const MSG_VALIDATION: &str = "Request validation failed";
pub const MSG_UNAVAILABLE: &str = "Database temporarily unavailable, please retry";
pub const MSG_INTERNAL: &str = "An unexpected error occurred";

/// Stable machine-readable error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidJson,
    InvalidId,
    InvalidQuery,
    NotFound,
    MethodNotAllowed,
    PayloadTooLarge,
    UnsupportedMediaType,
    ValidationError,
    ServiceUnavailable,
    InternalError,
}

impl ErrorCode {
    /// The only mapping from error code to HTTP status.
    pub fn status(self) -> StatusCode {
        match self {
            Self::InvalidJson | Self::InvalidId | Self::InvalidQuery => StatusCode::BAD_REQUEST,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            Self::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::UnsupportedMediaType => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Self::ValidationError => StatusCode::UNPROCESSABLE_ENTITY,
            Self::ServiceUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            Self::InternalError => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ErrorDetail {
    pub field: String,
    pub message: String,
}

impl ErrorDetail {
    pub fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
        }
    }
}

impl From<FieldError> for ErrorDetail {
    fn from(err: FieldError) -> Self {
        Self {
            field: err.field,
            message: err.message,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ErrorBody {
    code: ErrorCode,
    message: String,
    details: Vec<ErrorDetail>,
}

#[derive(Debug, Clone, Serialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    pub code: ErrorCode,
    pub message: String,
    pub details: Vec<ErrorDetail>,
}

impl ApiError {
    pub fn new(code: ErrorCode, message: impl Into<String>, details: Vec<ErrorDetail>) -> Self {
        Self {
            code,
            message: message.into(),
            details,
        }
    }

    pub fn status(&self) -> StatusCode {
        self.code.status()
    }

    pub fn invalid_json(details: Vec<ErrorDetail>) -> Self {
        Self::new(ErrorCode::InvalidJson, MSG_INVALID_JSON, details)
    }

    pub fn invalid_id(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidId, message, Vec::new())
    }

    pub fn invalid_query(message: impl Into<String>, details: Vec<ErrorDetail>) -> Self {
        Self::new(ErrorCode::InvalidQuery, message, details)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, message, Vec::new())
    }

    pub fn route_not_found() -> Self {
        Self::not_found(MSG_NOT_FOUND_ROUTE)
    }

    pub fn method_not_allowed() -> Self {
        Self::new(
            ErrorCode::MethodNotAllowed,
            MSG_METHOD_NOT_ALLOWED,
            Vec::new(),
        )
    }

    pub fn payload_too_large() -> Self {
        Self::new(
            ErrorCode::PayloadTooLarge,
            MSG_PAYLOAD_TOO_LARGE,
            Vec::new(),
        )
    }

    pub fn unsupported_media_type() -> Self {
        Self::new(
            ErrorCode::UnsupportedMediaType,
            MSG_UNSUPPORTED_MEDIA_TYPE,
            Vec::new(),
        )
    }

    pub fn validation(message: impl Into<String>, details: Vec<ErrorDetail>) -> Self {
        Self::new(ErrorCode::ValidationError, message, details)
    }

    pub fn service_unavailable() -> Self {
        Self::new(ErrorCode::ServiceUnavailable, MSG_UNAVAILABLE, Vec::new())
    }

    pub fn internal() -> Self {
        Self::new(ErrorCode::InternalError, MSG_INTERNAL, Vec::new())
    }
}

impl From<AppError> for ApiError {
    /// Internal and availability causes are logged here and never returned.
    fn from(err: AppError) -> Self {
        match err {
            AppError::Validation(fields) => Self::validation(
                MSG_VALIDATION,
                fields.into_iter().map(ErrorDetail::from).collect(),
            ),
            AppError::NotFound(message) => Self::not_found(message),
            AppError::Unavailable(source) => {
                warn!("database unavailable: {source}");
                Self::service_unavailable()
            }
            AppError::Internal(source) => {
                error!("internal error: {source}");
                Self::internal()
            }
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        let body = ErrorEnvelope {
            error: ErrorBody {
                code: self.code,
                message: self.message,
                details: self.details,
            },
        };
        (status, Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use axum::body::to_bytes;

    use super::*;

    async fn body_string(response: Response) -> String {
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[test]
    fn every_code_maps_to_its_documented_status() {
        let table = [
            (ErrorCode::InvalidJson, 400),
            (ErrorCode::InvalidId, 400),
            (ErrorCode::InvalidQuery, 400),
            (ErrorCode::NotFound, 404),
            (ErrorCode::MethodNotAllowed, 405),
            (ErrorCode::PayloadTooLarge, 413),
            (ErrorCode::UnsupportedMediaType, 415),
            (ErrorCode::ValidationError, 422),
            (ErrorCode::ServiceUnavailable, 503),
            (ErrorCode::InternalError, 500),
        ];
        for (code, status) in table {
            assert_eq!(code.status().as_u16(), status, "{code:?}");
        }
    }

    #[tokio::test]
    async fn route_not_found_serializes_to_the_exact_documented_body() {
        let response = ApiError::route_not_found().into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(response.headers()["content-type"], "application/json",);
        assert_eq!(
            body_string(response).await,
            r#"{"error":{"code":"not_found","message":"Route not found","details":[]}}"#
        );
    }

    #[tokio::test]
    async fn validation_details_keep_field_and_message() {
        let err: ApiError = AppError::Validation(vec![
            FieldError::new("title", "must be between 1 and 200 characters"),
            FieldError::new("priority", "must be one of: low, medium, high"),
        ])
        .into();
        assert_eq!(err.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = body_string(err.into_response()).await;
        assert_eq!(
            body,
            r#"{"error":{"code":"validation_error","message":"Request validation failed","details":[{"field":"title","message":"must be between 1 and 200 characters"},{"field":"priority","message":"must be one of: low, medium, high"}]}}"#
        );
    }

    #[tokio::test]
    async fn internal_and_unavailable_causes_are_not_returned() {
        let internal: ApiError =
            AppError::Internal("relation \"tasks\" does not exist".into()).into();
        let body = body_string(internal.into_response()).await;
        assert_eq!(
            body,
            r#"{"error":{"code":"internal_error","message":"An unexpected error occurred","details":[]}}"#
        );

        let unavailable: ApiError = AppError::Unavailable("connection refused".into()).into();
        let body = body_string(unavailable.into_response()).await;
        assert_eq!(
            body,
            r#"{"error":{"code":"service_unavailable","message":"Database temporarily unavailable, please retry","details":[]}}"#
        );
    }

    #[test]
    fn not_found_app_error_keeps_its_message() {
        let err: ApiError = AppError::NotFound("Task 1 not found".to_owned()).into();
        assert_eq!(err.code, ErrorCode::NotFound);
        assert_eq!(err.message, "Task 1 not found");
    }
}
