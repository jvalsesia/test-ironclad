//! Shared `{id}` path extractor for task endpoints.

use axum::extract::{FromRequestParts, Path};
use axum::http::request::Parts;
use uuid::Uuid;

use crate::domain::task::TaskId;
use crate::http::error::ApiError;

pub const MSG_INVALID_TASK_ID: &str = "Task id must be a valid UUID";

/// Length of the hyphenated 8-4-4-4-12 UUID form.
const HYPHENATED_LEN: usize = 36;

/// The task id from the single `{id}` path segment. Accepts only the
/// 36-character hyphenated UUID form (any version, hex case-insensitive).
/// Every rejection, including invalid percent-encoded UTF-8, is the
/// uniform `400 invalid_id`. Runs before any database access.
#[derive(Debug, Clone, Copy)]
pub struct TaskIdPath(pub TaskId);

impl<S> FromRequestParts<S> for TaskIdPath
where
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Path(raw) = Path::<String>::from_request_parts(parts, state)
            .await
            .map_err(|_| invalid_task_id())?;
        parse_task_id(&raw).map(TaskIdPath)
    }
}

/// At exactly 36 bytes the `uuid` parser only accepts the hyphenated
/// layout, so the length check excludes the simple, braced and URN forms.
fn parse_task_id(raw: &str) -> Result<TaskId, ApiError> {
    if raw.len() != HYPHENATED_LEN {
        return Err(invalid_task_id());
    }
    Uuid::try_parse(raw)
        .map(TaskId::new)
        .map_err(|_| invalid_task_id())
}

fn invalid_task_id() -> ApiError {
    ApiError::invalid_id(MSG_INVALID_TASK_ID)
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::body::{Body, to_bytes};
    use axum::http::{Request, StatusCode};
    use axum::routing::get;
    use tower::ServiceExt;

    use crate::http::error::ErrorCode;

    use super::*;

    const ID: &str = "0b7d2c1e-5f3a-4e8b-9c6d-7a1f2e3d4c5b";

    fn assert_rejected(raw: &str) {
        let err = parse_task_id(raw).expect_err(raw);
        assert_eq!(err.code, ErrorCode::InvalidId, "{raw}");
        assert_eq!(err.message, MSG_INVALID_TASK_ID, "{raw}");
        assert!(err.details.is_empty(), "{raw}");
    }

    #[test]
    fn accepts_lowercase_hyphenated_uuid() {
        let id = parse_task_id(ID).unwrap();
        assert_eq!(id.value(), Uuid::parse_str(ID).unwrap());
    }

    #[test]
    fn accepts_uppercase_hex() {
        let id = parse_task_id(&ID.to_uppercase()).unwrap();
        assert_eq!(id.to_string(), ID);
    }

    #[test]
    fn accepts_any_version_and_nil() {
        for raw in [
            "c232ab00-9414-11ec-b3c8-9f6bdeced846", // v1
            "01890a5d-ac96-774b-bcce-b302099a8057", // v7
            "00000000-0000-0000-0000-000000000000", // nil
        ] {
            assert!(parse_task_id(raw).is_ok(), "{raw}");
        }
    }

    #[test]
    fn rejects_non_uuid_text() {
        for raw in ["not-a-uuid", "abc", ""] {
            assert_rejected(raw);
        }
    }

    #[test]
    fn rejects_other_uuid_forms() {
        for raw in [
            "0b7d2c1e5f3a4e8b9c6d7a1f2e3d4c5b",
            "{0b7d2c1e-5f3a-4e8b-9c6d-7a1f2e3d4c5b}",
            "urn:uuid:0b7d2c1e-5f3a-4e8b-9c6d-7a1f2e3d4c5b",
        ] {
            assert_rejected(raw);
        }
    }

    #[test]
    fn rejects_near_misses() {
        for raw in [
            "0b7d2c1e-5f3a-4e8b-9c6d-7a1f2e3d4c5",   // 35 characters
            "0b7d2c1e-5f3a-4e8b-9c6d-7a1f2e3d4c5b0", // 37 characters
            "0b7d2c1e-5f3a-4e8b-9c6d-7a1f2e3d4c5g",  // non-hex
            "0b7d2c1e5-f3a-4e8b-9c6d-7a1f2e3d4c5b",  // misplaced hyphen
            "0b7d2c1e-5f3a4-e8b-9c6d-7a1f2e3d4c5b",  // misplaced hyphen
        ] {
            assert_rejected(raw);
        }
    }

    async fn extract(uri: &str) -> (StatusCode, String) {
        let app = Router::new().route(
            "/tasks/{id}",
            get(|TaskIdPath(id): TaskIdPath| async move { id.to_string() }),
        );
        let response = app
            .oneshot(Request::get(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, String::from_utf8(body.to_vec()).unwrap())
    }

    #[tokio::test]
    async fn extractor_yields_the_parsed_id() {
        let (status, body) = extract(&format!("/tasks/{}", ID.to_uppercase())).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, ID);
    }

    #[tokio::test]
    async fn invalid_utf8_segment_is_invalid_id() {
        let (status, body) = extract("/tasks/%FF").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            body,
            r#"{"error":{"code":"invalid_id","message":"Task id must be a valid UUID","details":[]}}"#
        );
    }
}
