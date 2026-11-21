//! F01 diagnostic routes, mounted only when `ENABLE_TEST_ROUTES=true`.
//!
//! They let the cross-cutting behaviors (error mapping, panic containment,
//! body limits, pool exhaustion, graceful shutdown) be exercised before any
//! business endpoint exists. When disabled they are unknown routes.

use std::collections::HashMap;
use std::time::Duration;

use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use super::error::{ApiError, ErrorDetail};
use super::extract::ApiJson;
use super::state::AppState;

const MAX_SLOW_MS: u64 = 30_000;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/__test/db-ping", get(db_ping))
        .route("/__test/sql-error", get(sql_error))
        .route("/__test/panic", get(induced_panic))
        .route("/__test/slow", get(slow))
        .route("/__test/echo", post(echo))
}

#[derive(Serialize)]
struct DbPingResponse {
    database: &'static str,
}

async fn db_ping(State(state): State<AppState>) -> Result<Json<DbPingResponse>, ApiError> {
    state.diagnostics.ping().await?;
    Ok(Json(DbPingResponse { database: "up" }))
}

async fn sql_error(State(state): State<AppState>) -> Result<Json<DbPingResponse>, ApiError> {
    state.diagnostics.induce_sql_error().await?;
    Ok(Json(DbPingResponse { database: "up" }))
}

async fn induced_panic() -> Json<DbPingResponse> {
    panic!("induced panic for diagnostics");
}

#[derive(Serialize)]
struct SlowResponse {
    slept_ms: u64,
}

async fn slow(
    State(state): State<AppState>,
    query: Result<Query<HashMap<String, String>>, QueryRejection>,
) -> Result<Json<SlowResponse>, ApiError> {
    let Query(params) =
        query.map_err(|_| ApiError::invalid_query("Query string is malformed", Vec::new()))?;

    let mut details = Vec::new();
    let ms = match params.get("ms") {
        None => {
            details.push(ErrorDetail::new("ms", "is required"));
            0
        }
        Some(raw) => match raw.parse::<u64>() {
            Ok(value) if value <= MAX_SLOW_MS => value,
            _ => {
                details.push(ErrorDetail::new(
                    "ms",
                    format!("must be an integer between 0 and {MAX_SLOW_MS}"),
                ));
                0
            }
        },
    };
    let hold_db = match params.get("hold_db").map(String::as_str) {
        None | Some("false") => false,
        Some("true") => true,
        Some(_) => {
            details.push(ErrorDetail::new("hold_db", "must be true or false"));
            false
        }
    };
    if !details.is_empty() {
        return Err(ApiError::invalid_query("Invalid query parameters", details));
    }

    let duration = Duration::from_millis(ms);
    if hold_db {
        state.diagnostics.hold_connection(duration).await?;
    } else {
        tokio::time::sleep(duration).await;
    }
    Ok(Json(SlowResponse { slept_ms: ms }))
}

#[derive(Serialize, Deserialize)]
struct EchoBody {
    value: String,
}

async fn echo(ApiJson(body): ApiJson<EchoBody>) -> Json<EchoBody> {
    Json(body)
}
