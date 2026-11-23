//! `GET /health`: database reachability probe for operators and orchestrators.

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde::Serialize;
use utoipa::ToSchema;

use super::openapi::schemas::{DatabaseState, HealthState};
use super::state::AppState;
use crate::application::health::{CheckHealth, HEALTH_TIMEOUT, HealthStatus};

/// Body of both the 200 and the 503 response (not the uniform error body).
#[derive(Debug, Serialize, ToSchema)]
#[schema(description = "Service and database health")]
pub struct HealthResponse {
    #[schema(value_type = HealthState, inline)]
    pub status: &'static str,
    #[schema(value_type = DatabaseState, inline)]
    pub database: &'static str,
    /// Service version
    #[schema(value_type = String)]
    pub version: &'static str,
}

impl HealthResponse {
    pub fn from_status(status: HealthStatus) -> Self {
        let (status, database) = if status.database_up {
            ("ok", "up")
        } else {
            ("degraded", "down")
        };
        Self {
            status,
            database,
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/health", get(health))
}

async fn health(State(state): State<AppState>) -> Response {
    let status = CheckHealth::new(state.diagnostics.clone(), HEALTH_TIMEOUT)
        .execute()
        .await;
    let code = if status.database_up {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    let mut response = (code, Json(HealthResponse::from_status(status))).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(database_up: bool) -> String {
        serde_json::to_string(&HealthResponse::from_status(HealthStatus { database_up })).unwrap()
    }

    #[test]
    fn healthy_payload_serializes_in_order() {
        assert_eq!(
            json(true),
            format!(
                r#"{{"status":"ok","database":"up","version":"{}"}}"#,
                env!("CARGO_PKG_VERSION")
            )
        );
    }

    #[test]
    fn degraded_payload_serializes_in_order() {
        assert_eq!(
            json(false),
            format!(
                r#"{{"status":"degraded","database":"down","version":"{}"}}"#,
                env!("CARGO_PKG_VERSION")
            )
        );
    }

    #[test]
    fn version_is_the_crate_version() {
        let payload = HealthResponse::from_status(HealthStatus { database_up: true });
        assert_eq!(payload.version, env!("CARGO_PKG_VERSION"));
    }
}
