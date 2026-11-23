//! The shared `{id}` path extractor for `/tasks/{id}` endpoints.

use axum::extract::{FromRequestParts, Path};
use axum::http::request::Parts;
use uuid::Uuid;

use crate::domain::task::TaskId;
use crate::http::error::ApiError;

pub const MSG_INVALID_TASK_ID: &str = "Task id must be a valid UUID";

/// Length of the hyphenated 8-4-4-4-12 form.
const HYPHENATED_LEN: usize = 36;

/// A task id taken from the `{id}` path segment. Only the 36-character
/// hyphenated UUID form is accepted (any version, case-insensitive hex);
/// every rejection is `400 invalid_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskIdPath(pub TaskId);

impl<S> FromRequestParts<S> for TaskIdPath
where
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Path(raw) = Path::<String>::from_request_parts(parts, state)
            .await
            .map_err(|_| invalid_id())?;
        parse_task_id(&raw).map(TaskIdPath).ok_or_else(invalid_id)
    }
}

/// At exactly 36 bytes the `uuid` parser only accepts the hyphenated layout.
fn parse_task_id(raw: &str) -> Option<TaskId> {
    if raw.len() != HYPHENATED_LEN {
        return None;
    }
    Uuid::try_parse(raw).ok().map(TaskId::new)
}

fn invalid_id() -> ApiError {
    ApiError::invalid_id(MSG_INVALID_TASK_ID)
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::body::{Body, to_bytes};
    use axum::http::{Request, StatusCode};
    use axum::routing::get;
    use tower::ServiceExt;

    use super::*;

    const V4: &str = "3f2b6f0e-6d4a-4c8e-9a0f-1b2c3d4e5f60";

    async fn extract(path: &str) -> (StatusCode, String) {
        let router: Router = Router::new().route(
            "/tasks/{id}",
            get(|TaskIdPath(id): TaskIdPath| async move { id.to_string() }),
        );
        let response = router
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, String::from_utf8(bytes.to_vec()).unwrap())
    }

    const INVALID_BODY: &str =
        r#"{"error":{"code":"invalid_id","message":"Task id must be a valid UUID","details":[]}}"#;

    #[tokio::test]
    async fn accepts_lowercase_hyphenated_uuid() {
        assert_eq!(
            extract(&format!("/tasks/{V4}")).await,
            (StatusCode::OK, V4.to_owned())
        );
    }

    #[tokio::test]
    async fn accepts_uppercase_hex() {
        assert_eq!(
            extract(&format!("/tasks/{}", V4.to_uppercase())).await,
            (StatusCode::OK, V4.to_owned())
        );
    }

    #[test]
    fn accepts_any_version_and_nil() {
        for raw in [
            "c232ab00-9414-11ec-b3c8-9f6bdeced846",
            "01890a5d-ac96-774b-bcce-b302099a8057",
            "00000000-0000-0000-0000-000000000000",
        ] {
            assert_eq!(
                parse_task_id(raw).map(|id| id.to_string()),
                Some(raw.to_owned())
            );
        }
    }

    #[tokio::test]
    async fn rejects_non_uuid_text() {
        for path in ["/tasks/not-a-uuid", "/tasks/abc", "/tasks/%FF"] {
            assert_eq!(
                extract(path).await,
                (StatusCode::BAD_REQUEST, INVALID_BODY.to_owned()),
                "{path}"
            );
        }
        assert!(parse_task_id("").is_none());
    }

    #[test]
    fn rejects_other_uuid_forms() {
        for raw in [
            "3f2b6f0e6d4a4c8e9a0f1b2c3d4e5f60",
            "{3f2b6f0e-6d4a-4c8e-9a0f-1b2c3d4e5f60}",
            "urn:uuid:3f2b6f0e-6d4a-4c8e-9a0f-1b2c3d4e5f60",
        ] {
            assert!(parse_task_id(raw).is_none(), "{raw}");
        }
    }

    #[test]
    fn rejects_near_misses() {
        for raw in [
            "3f2b6f0e-6d4a-4c8e-9a0f-1b2c3d4e5f6",
            "3f2b6f0e-6d4a-4c8e-9a0f-1b2c3d4e5f600",
            "3f2b6f0e-6d4a-4c8e-9a0f-1b2c3d4e5f6g",
            "3f2b6f0e6-d4a-4c8e-9a0f-1b2c3d4e5f60",
        ] {
            assert!(parse_task_id(raw).is_none(), "{raw}");
        }
    }
}
