//! `GET /health` (F03): healthy, degraded, timeout and recovery behavior.
//!
//! Outages are simulated on the per-test database only: connections to it are
//! disallowed and its sessions terminated; the server-level database named by
//! `DATABASE_URL` is never altered.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::{Duration, Instant};

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{Connection, PgConnection, PgPool};

use common::{Reply, assert_uniform_error, get, request_with, send, test_router};

fn body(status: &str, database: &str) -> String {
    format!(
        r#"{{"status":"{status}","database":"{database}","version":"{}"}}"#,
        env!("CARGO_PKG_VERSION")
    )
}

fn healthy_body() -> String {
    body("ok", "up")
}

fn degraded_body() -> String {
    body("degraded", "down")
}

fn header<'a>(reply: &'a Reply, name: &str) -> Option<&'a str> {
    reply.headers.get(name).map(|v| v.to_str().unwrap())
}

/// Makes the per-test database refuse (or accept) connections and, when
/// refusing, terminates every other session on it.
async fn set_database_reachable(options: &PgConnectOptions, reachable: bool) {
    let database = options.get_database().expect("test database name");
    let server_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for tests");
    let mut admin = PgConnection::connect(&server_url).await.unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        r#"ALTER DATABASE "{database}" WITH ALLOW_CONNECTIONS {reachable}"#
    )))
    .execute(&mut admin)
    .await
    .unwrap();
    if !reachable {
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
             WHERE datname = '{database}' AND pid <> pg_backend_pid()"
        )))
        .execute(&mut admin)
        .await
        .unwrap();
    }
}

async fn connect(pool_options: PgPoolOptions, options: &PgConnectOptions) -> PgPool {
    pool_options.connect_with(options.clone()).await.unwrap()
}

#[sqlx::test]
async fn healthy_returns_200_with_payload(pool: PgPool) {
    let router = test_router(pool, false);
    let reply = send(&router, get("/health")).await;
    assert_eq!(reply.status.as_u16(), 200, "body: {}", reply.body);
    assert_eq!(header(&reply, "content-type"), Some("application/json"));
    assert_eq!(reply.body, healthy_body());
}

#[sqlx::test]
async fn healthy_response_has_no_store_cache_control(pool: PgPool) {
    let router = test_router(pool, false);
    let reply = send(&router, get("/health")).await;
    assert_eq!(reply.status.as_u16(), 200);
    assert_eq!(header(&reply, "cache-control"), Some("no-store"));
}

#[sqlx::test]
async fn health_is_fast_when_database_is_up(pool: PgPool) {
    let router = test_router(pool, false);
    let mut times = Vec::new();
    for _ in 0..100 {
        let start = Instant::now();
        let reply = send(&router, get("/health")).await;
        times.push(start.elapsed());
        assert_eq!(reply.status.as_u16(), 200);
    }
    times.sort();
    assert!(
        times[94] < Duration::from_millis(50),
        "p95 was {:?}",
        times[94]
    );
}

#[sqlx::test]
async fn non_get_method_returns_405(pool: PgPool) {
    let router = test_router(pool, false);
    let reply = send(&router, request_with("POST", "/health", None, "")).await;
    assert_uniform_error(&reply, 405, "method_not_allowed", "Method not allowed");
}

#[sqlx::test]
async fn query_string_is_ignored(pool: PgPool) {
    let router = test_router(pool, false);
    let reply = send(&router, get("/health?x=1")).await;
    assert_eq!(reply.status.as_u16(), 200);
    assert_eq!(reply.body, healthy_body());
}

#[sqlx::test]
async fn unreachable_database_returns_503_quickly(
    pool_options: PgPoolOptions,
    options: PgConnectOptions,
) {
    let router = test_router(connect(pool_options, &options).await, false);
    assert_eq!(send(&router, get("/health")).await.status.as_u16(), 200);

    set_database_reachable(&options, false).await;
    let start = Instant::now();
    let reply = send(&router, get("/health")).await;
    assert!(start.elapsed() < Duration::from_millis(2500));
    assert_eq!(reply.status.as_u16(), 503, "body: {}", reply.body);
    assert_eq!(reply.body, degraded_body());
    assert!(reply.json().get("error").is_none());
    assert_eq!(header(&reply, "content-type"), Some("application/json"));
}

#[sqlx::test]
async fn degraded_response_has_no_store_cache_control(
    pool_options: PgPoolOptions,
    options: PgConnectOptions,
) {
    let router = test_router(connect(pool_options, &options).await, false);
    assert_eq!(send(&router, get("/health")).await.status.as_u16(), 200);

    set_database_reachable(&options, false).await;
    let reply = send(&router, get("/health")).await;
    assert_eq!(reply.status.as_u16(), 503);
    assert_eq!(header(&reply, "cache-control"), Some("no-store"));
}

#[sqlx::test]
async fn exhausted_pool_returns_503_within_the_timeout(
    pool_options: PgPoolOptions,
    options: PgConnectOptions,
) {
    let pool = pool_options
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(5))
        .connect_with(options)
        .await
        .unwrap();
    let router = test_router(pool, true);

    let holder_router = router.clone();
    let holder = tokio::spawn(async move {
        send(&holder_router, get("/__test/slow?ms=6000&hold_db=true")).await
    });
    tokio::time::sleep(Duration::from_millis(500)).await;

    let start = Instant::now();
    let reply = send(&router, get("/health")).await;
    assert!(start.elapsed() < Duration::from_millis(2500));
    assert_eq!(reply.status.as_u16(), 503, "body: {}", reply.body);
    assert_eq!(reply.body, degraded_body());
    holder.abort();
}

#[sqlx::test]
async fn service_recovers_without_restart(pool_options: PgPoolOptions, options: PgConnectOptions) {
    let router = test_router(connect(pool_options, &options).await, false);
    assert_eq!(send(&router, get("/health")).await.status.as_u16(), 200);

    set_database_reachable(&options, false).await;
    let down = send(&router, get("/health")).await;
    assert_eq!(down.status.as_u16(), 503);
    assert_eq!(down.body, degraded_body());

    set_database_reachable(&options, true).await;
    let up = send(&router, get("/health")).await;
    assert_eq!(up.status.as_u16(), 200, "body: {}", up.body);
    assert_eq!(up.body, healthy_body());
}
