//! The shared `{id}` path extractor for `/tasks/{id}` endpoints.

use axum::extract::{FromRequestParts, Path};
use axum::http::request::Parts;
use uuid::Uuid;

use crate::domain::task::TaskId;
use crate::http::error::ApiError;

pub const MSG_INVALID_TASK_ID: &str = "Task id must be a valid UUID";

/// Length of the hyphenated 8-4-4-4-12 UUID form.
const HYPHENATED_LEN: usize = 36;

/// A task id taken from the `{id}` path segment. Only the 36-character
/// hyphenated UUID form is accepted (hex case-insensitive, any version);
/// every rejection, including invalid UTF-8, is `400 invalid_id`. Being a
/// parts extractor, it runs before any body extractor or database access.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskIdPath(pub TaskId);

impl<S> FromRequestParts<S> for TaskIdPath
where
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Path(segment) = Path::<String>::from_request_parts(parts, state)
            .await
            .map_err(|_| invalid())?;
        parse_task_id(&segment).map(TaskIdPath).ok_or_else(invalid)
    }
}

fn invalid() -> ApiError {
    ApiError::invalid_id(MSG_INVALID_TASK_ID)
}

/// At exactly 36 bytes, the `uuid` parser only accepts the hyphenated
/// layout, so the length check rules out the simple, braced and URN forms.
fn parse_task_id(segment: &str) -> Option<TaskId> {
    if segment.len() != HYPHENATED_LEN {
        return None;
    }
    Uuid::try_parse(segment).ok().map(TaskId::new)
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::extract::Request;
    use axum::routing::get;
    use axum::{Json, Router};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use crate::http::error::ErrorCode;

    use super::*;

    async fn extract(segment: &str) -> Result<TaskId, (u16, serde_json::Value)> {
        let router = Router::new().route(
            "/tasks/{id}",
            get(|TaskIdPath(id): TaskIdPath| async move { Json(id.value()) }),
        );
        let request = Request::builder()
            .uri(format!("/tasks/{segment}"))
            .body(Body::empty())
            .unwrap();
        let response = router.oneshot(request).await.unwrap();
        let status = response.status().as_u16();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        if status == 200 {
            let uuid: Uuid = serde_json::from_value(json).unwrap();
            Ok(TaskId::new(uuid))
        } else {
            Err((status, json))
        }
    }

    async fn assert_rejected(segment: &str) {
        let (status, body) = extract(segment)
            .await
            .expect_err(&format!("{segment:?} should be rejected"));
        assert_eq!(status, 400, "{segment:?}");
        assert_eq!(
            body,
            serde_json::json!({
                "error": {"code": "invalid_id", "message": MSG_INVALID_TASK_ID, "details": []}
            }),
            "{segment:?}"
        );
    }

    #[test]
    fn rejection_uses_the_invalid_id_code() {
        assert_eq!(invalid().code, ErrorCode::InvalidId);
    }

    #[tokio::test]
    async fn accepts_lowercase_hyphenated_uuid() {
        let uuid = Uuid::new_v4();
        let id = extract(&uuid.hyphenated().to_string()).await.unwrap();
        assert_eq!(id.value(), uuid);
    }

    #[tokio::test]
    async fn accepts_uppercase_hex() {
        let uuid = Uuid::new_v4();
        let upper = uuid.hyphenated().to_string().to_uppercase();
        let id = extract(&upper).await.unwrap();
        assert_eq!(id.value(), uuid);
    }

    #[tokio::test]
    async fn accepts_any_version_and_nil() {
        for value in [
            "6ba7b810-9dad-11d1-80b4-00c04fd430c8",
            "01890a5d-ac96-774b-bcce-b302099a8057",
            "00000000-0000-0000-0000-000000000000",
        ] {
            let id = extract(value).await.unwrap();
            assert_eq!(id.to_string(), value);
        }
    }

    #[tokio::test]
    async fn rejects_non_uuid_text() {
        for segment in ["not-a-uuid", "abc"] {
            assert_rejected(segment).await;
        }
        // An empty segment does not match `/tasks/{id}` at all.
        assert!(parse_task_id("").is_none());
    }

    #[tokio::test]
    async fn rejects_other_uuid_forms() {
        let uuid = Uuid::new_v4();
        for segment in [
            uuid.simple().to_string(),
            uuid.braced().to_string(),
            uuid.urn().to_string(),
        ] {
            assert_rejected(&segment).await;
        }
    }

    #[tokio::test]
    async fn rejects_near_misses() {
        let valid = "3f2b6f0e-6d4a-4c8e-9a0f-1b2c3d4e5f60";
        for segment in [
            &valid[..35],
            &format!("{valid}0"),
            "3f2b6f0e-6d4a-4c8e-9a0f-1b2c3d4e5f6g",
            "3f2b6f0e6-d4a-4c8e-9a0f-1b2c3d4e5f60",
            "%FF",
        ] {
            assert_rejected(segment).await;
        }
    }
}
