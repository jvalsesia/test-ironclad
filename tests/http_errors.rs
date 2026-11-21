//! Error paths owned by F01, exercised through the real router in-process.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::{Duration, Instant};

use common::{assert_uniform_error, get, post_json, request_with, send, test_router};
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

#[sqlx::test]
async fn unknown_route_returns_uniform_404(pool: PgPool) {
    let router = test_router(pool, true);
    let reply = send(&router, get("/does-not-exist")).await;
    assert_uniform_error(&reply, 404, "not_found", "Route not found");
}

#[sqlx::test]
async fn unknown_route_post_returns_404_not_405(pool: PgPool) {
    let router = test_router(pool, true);
    let reply = send(&router, post_json("/does-not-exist", r#"{"value":"x"}"#)).await;
    assert_uniform_error(&reply, 404, "not_found", "Route not found");
}

#[sqlx::test]
async fn wrong_method_returns_uniform_405(pool: PgPool) {
    let router = test_router(pool, true);
    let reply = send(&router, request_with("DELETE", "/__test/echo", None, "")).await;
    assert_uniform_error(&reply, 405, "method_not_allowed", "Method not allowed");
}

#[sqlx::test]
async fn sql_error_returns_500_without_leaking(pool: PgPool) {
    let router = test_router(pool, true);
    let reply = send(&router, get("/__test/sql-error")).await;
    assert_uniform_error(
        &reply,
        500,
        "internal_error",
        "An unexpected error occurred",
    );
    for forbidden in [
        "division",
        "SELECT",
        "sqlx",
        "22012",
        "panicked",
        "backtrace",
        ".rs:",
    ] {
        assert!(
            !reply.body.contains(forbidden),
            "{forbidden} leaked: {}",
            reply.body
        );
    }
}

#[sqlx::test]
async fn panic_returns_500_without_trace(pool: PgPool) {
    let router = test_router(pool, true);
    let reply = send(&router, get("/__test/panic")).await;
    assert_uniform_error(
        &reply,
        500,
        "internal_error",
        "An unexpected error occurred",
    );
    for forbidden in ["panicked", "backtrace", ".rs:"] {
        assert!(
            !reply.body.contains(forbidden),
            "{forbidden} leaked: {}",
            reply.body
        );
    }
}

#[sqlx::test]
async fn pool_exhaustion_returns_503(
    _pool_options: PgPoolOptions,
    connect_options: PgConnectOptions,
) {
    // A pool of one connection with a 1 s acquire timeout, on the per-test database.
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(1))
        .connect_with(connect_options)
        .await
        .unwrap();
    let router = test_router(pool, true);

    let holder = {
        let router = router.clone();
        tokio::spawn(async move { send(&router, get("/__test/slow?ms=4000&hold_db=true")).await })
    };
    // Let the holder take the pool's only connection.
    tokio::time::sleep(Duration::from_millis(400)).await;

    let started = Instant::now();
    let reply = send(&router, get("/__test/db-ping")).await;
    let elapsed = started.elapsed();

    assert_uniform_error(
        &reply,
        503,
        "service_unavailable",
        "Database temporarily unavailable, please retry",
    );
    assert!(elapsed < Duration::from_millis(2500), "took {elapsed:?}");
    holder.abort();
}

#[sqlx::test]
async fn test_routes_absent_when_disabled(pool: PgPool) {
    let router = test_router(pool, false);
    let reply = send(&router, get("/__test/sql-error")).await;
    assert_uniform_error(&reply, 404, "not_found", "Route not found");
}

#[sqlx::test]
async fn db_ping_reports_up_when_database_is_reachable(pool: PgPool) {
    let router = test_router(pool, true);
    let reply = send(&router, get("/__test/db-ping")).await;
    assert_eq!(reply.status.as_u16(), 200);
    assert_eq!(reply.body, r#"{"database":"up"}"#);
}

#[sqlx::test]
async fn slow_route_validates_its_query(pool: PgPool) {
    let router = test_router(pool, true);
    for path in [
        "/__test/slow",
        "/__test/slow?ms=abc",
        "/__test/slow?ms=30001",
        "/__test/slow?ms=1&hold_db=maybe",
    ] {
        let reply = send(&router, get(path)).await;
        assert_eq!(reply.status.as_u16(), 400, "{path}");
        let body = reply.json();
        assert_eq!(body["error"]["code"], "invalid_query", "{path}");
        assert!(
            !body["error"]["details"].as_array().unwrap().is_empty(),
            "{path}"
        );
    }
    let reply = send(&router, get("/__test/slow?ms=5")).await;
    assert_eq!(reply.body, r#"{"slept_ms":5}"#);
}
