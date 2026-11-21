//! Request limits and body parsing rules owned by F01.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::convert::Infallible;

use axum::body::{Body, Bytes};
use axum::http::Request;

use common::{assert_uniform_error, json_body_of_len, post_json, request_with, send, test_router};
use sqlx::PgPool;

const PAYLOAD_TOO_LARGE: &str = "Request body exceeds 64 KB";
const UNSUPPORTED_MEDIA: &str = "Content-Type must be application/json";

#[sqlx::test]
async fn post_tasks_over_64kib_returns_413(pool: PgPool) {
    let router = test_router(pool, true);
    let body = json_body_of_len(65_537);
    let request = Request::builder()
        .method("POST")
        .uri("/tasks")
        .header("content-type", "application/json")
        .header("content-length", "65537")
        .body(Body::from(body))
        .unwrap();
    let reply = send(&router, request).await;
    assert_uniform_error(&reply, 413, "payload_too_large", PAYLOAD_TOO_LARGE);
}

#[sqlx::test]
async fn streamed_body_over_limit_returns_413(pool: PgPool) {
    let router = test_router(pool, true);
    let body = json_body_of_len(65_537);
    // No Content-Length: the body arrives as a stream of chunks.
    let chunks: Vec<Result<Bytes, Infallible>> = body
        .into_bytes()
        .chunks(8_192)
        .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
        .collect();
    let request = Request::builder()
        .method("POST")
        .uri("/__test/echo")
        .header("content-type", "application/json")
        .body(Body::from_stream(futures_util::stream::iter(chunks)))
        .unwrap();
    assert!(request.headers().get("content-length").is_none());
    let reply = send(&router, request).await;
    assert_uniform_error(&reply, 413, "payload_too_large", PAYLOAD_TOO_LARGE);
}

#[sqlx::test]
async fn body_of_exactly_64kib_is_accepted(pool: PgPool) {
    let router = test_router(pool, true);
    let body = json_body_of_len(65_536);
    assert_eq!(body.len(), 65_536);
    let reply = send(&router, post_json("/__test/echo", &body)).await;
    assert_eq!(reply.status.as_u16(), 200);
    let echoed = reply.json();
    let sent: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(echoed["value"], sent["value"]);
}

#[sqlx::test]
async fn missing_content_type_returns_415(pool: PgPool) {
    let router = test_router(pool, true);
    let reply = send(
        &router,
        request_with("POST", "/__test/echo", None, r#"{"value":"x"}"#),
    )
    .await;
    assert_uniform_error(&reply, 415, "unsupported_media_type", UNSUPPORTED_MEDIA);
}

#[sqlx::test]
async fn text_plain_content_type_returns_415(pool: PgPool) {
    let router = test_router(pool, true);
    let reply = send(
        &router,
        request_with(
            "POST",
            "/__test/echo",
            Some("text/plain"),
            r#"{"value":"x"}"#,
        ),
    )
    .await;
    assert_uniform_error(&reply, 415, "unsupported_media_type", UNSUPPORTED_MEDIA);
}

#[sqlx::test]
async fn json_content_type_with_charset_is_accepted(pool: PgPool) {
    let router = test_router(pool, true);
    let reply = send(
        &router,
        request_with(
            "POST",
            "/__test/echo",
            Some("application/json; charset=utf-8"),
            r#"{"value":"x"}"#,
        ),
    )
    .await;
    assert_eq!(reply.status.as_u16(), 200);
    assert_eq!(reply.body, r#"{"value":"x"}"#);
}

#[sqlx::test]
async fn malformed_json_returns_400(pool: PgPool) {
    let router = test_router(pool, true);
    let reply = send(&router, post_json("/__test/echo", r#"{"value":"#)).await;
    assert_eq!(reply.status.as_u16(), 400);
    let body = reply.json();
    assert_eq!(body["error"]["code"], "invalid_json");
    assert_eq!(
        body["error"]["message"],
        "Request body is not valid JSON for this endpoint"
    );
    assert_eq!(body["error"]["details"], serde_json::json!([]));
}

#[sqlx::test]
async fn wrong_json_type_returns_400_with_field(pool: PgPool) {
    let router = test_router(pool, true);
    let reply = send(&router, post_json("/__test/echo", r#"{"value":5}"#)).await;
    assert_eq!(reply.status.as_u16(), 400);
    let body = reply.json();
    assert_eq!(body["error"]["code"], "invalid_json");
    let details = body["error"]["details"].as_array().unwrap();
    assert_eq!(details.len(), 1);
    assert_eq!(details[0]["field"], "value");
    assert!(!details[0]["message"].as_str().unwrap().is_empty());
}
