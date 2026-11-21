//! `X-Request-Id` behavior on every response class, in-process.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use axum::body::Body;
use axum::http::Request;
use common::{assert_uuid_v4, get, request_id_of, request_with, send, test_router};
use sqlx::PgPool;

fn with_header(path: &str, value: &str) -> Request<Body> {
    Request::builder()
        .uri(path)
        .header("x-request-id", value)
        .body(Body::empty())
        .unwrap()
}

#[sqlx::test]
async fn success_response_has_generated_uuid_v4(pool: PgPool) {
    let router = test_router(pool, true);
    let reply = send(&router, get("/__test/db-ping")).await;
    assert_eq!(reply.status.as_u16(), 200);
    assert_uuid_v4(&request_id_of(&reply));
}

#[sqlx::test]
async fn unknown_route_has_request_id(pool: PgPool) {
    let router = test_router(pool, true);
    let reply = send(&router, get("/does-not-exist")).await;
    assert_eq!(reply.status.as_u16(), 404);
    assert!(!request_id_of(&reply).is_empty());
}

#[sqlx::test]
async fn sql_error_500_has_request_id(pool: PgPool) {
    let router = test_router(pool, true);
    let reply = send(&router, get("/__test/sql-error")).await;
    assert_eq!(reply.status.as_u16(), 500);
    assert!(!request_id_of(&reply).is_empty());
}

#[sqlx::test]
async fn panic_500_has_request_id(pool: PgPool) {
    let router = test_router(pool, true);
    let reply = send(&router, get("/__test/panic")).await;
    assert_eq!(reply.status.as_u16(), 500);
    assert!(!request_id_of(&reply).is_empty());
}

#[sqlx::test]
async fn oversized_declared_body_413_has_request_id(pool: PgPool) {
    let router = test_router(pool, true);
    let request = Request::builder()
        .method("POST")
        .uri("/tasks")
        .header("content-type", "application/json")
        .header("content-length", "65537")
        .body(Body::from("a".repeat(65_537)))
        .unwrap();
    let reply = send(&router, request).await;
    assert_eq!(reply.status.as_u16(), 413);
    assert!(!request_id_of(&reply).is_empty());
}

#[sqlx::test]
async fn unsupported_media_type_415_has_request_id(pool: PgPool) {
    let router = test_router(pool, true);
    let reply = send(
        &router,
        request_with("POST", "/__test/echo", None, r#"{"value":"x"}"#),
    )
    .await;
    assert_eq!(reply.status.as_u16(), 415);
    assert!(!request_id_of(&reply).is_empty());
}

#[sqlx::test]
async fn wrong_method_405_has_request_id(pool: PgPool) {
    let router = test_router(pool, true);
    let reply = send(&router, request_with("DELETE", "/__test/echo", None, "")).await;
    assert_eq!(reply.status.as_u16(), 405);
    assert!(!request_id_of(&reply).is_empty());
}

#[sqlx::test]
async fn consecutive_requests_get_distinct_ids(pool: PgPool) {
    let router = test_router(pool, true);
    let first = request_id_of(&send(&router, get("/__test/db-ping")).await);
    let second = request_id_of(&send(&router, get("/__test/db-ping")).await);
    assert_uuid_v4(&first);
    assert_uuid_v4(&second);
    assert_ne!(first, second);
}

#[sqlx::test]
async fn valid_client_id_is_echoed(pool: PgPool) {
    let router = test_router(pool, true);
    let reply = send(&router, with_header("/__test/db-ping", "client-abc-123")).await;
    assert_eq!(reply.status.as_u16(), 200);
    assert_eq!(request_id_of(&reply), "client-abc-123");
}

#[sqlx::test]
async fn boundary_128_char_id_is_echoed(pool: PgPool) {
    let router = test_router(pool, true);
    let id = "a".repeat(128);
    let reply = send(&router, with_header("/__test/db-ping", &id)).await;
    assert_eq!(reply.status.as_u16(), 200);
    assert_eq!(request_id_of(&reply), id);
}

#[sqlx::test]
async fn valid_client_id_is_echoed_on_error_response(pool: PgPool) {
    let router = test_router(pool, true);
    let reply = send(&router, with_header("/does-not-exist", "client-abc-123")).await;
    assert_eq!(reply.status.as_u16(), 404);
    assert_eq!(request_id_of(&reply), "client-abc-123");
}

#[sqlx::test]
async fn overlong_id_is_replaced(pool: PgPool) {
    let router = test_router(pool, true);
    let id = "a".repeat(200);
    let reply = send(&router, with_header("/__test/db-ping", &id)).await;
    assert_eq!(reply.status.as_u16(), 200);
    let echoed = request_id_of(&reply);
    assert_uuid_v4(&echoed);
    assert_ne!(echoed, id);
}

#[sqlx::test]
async fn id_with_space_is_replaced(pool: PgPool) {
    let router = test_router(pool, true);
    let reply = send(&router, with_header("/__test/db-ping", "has space")).await;
    assert_eq!(reply.status.as_u16(), 200);
    assert_uuid_v4(&request_id_of(&reply));
}

#[sqlx::test]
async fn empty_id_is_replaced(pool: PgPool) {
    let router = test_router(pool, true);
    let reply = send(&router, with_header("/__test/db-ping", "")).await;
    assert_eq!(reply.status.as_u16(), 200);
    assert_uuid_v4(&request_id_of(&reply));
}

#[sqlx::test]
async fn id_with_disallowed_character_is_replaced(pool: PgPool) {
    let router = test_router(pool, true);
    let reply = send(&router, with_header("/__test/db-ping", "bad/id!")).await;
    assert_eq!(reply.status.as_u16(), 200);
    let echoed = request_id_of(&reply);
    assert_uuid_v4(&echoed);
    assert_ne!(echoed, "bad/id!");
}
