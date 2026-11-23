//! `POST /tasks` through the real router, in-process.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::{Duration, Instant};

use common::{
    assert_uniform_error, count_tasks, get, post_json, request_with, send, stored_task, test_router,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use uuid::Uuid;

async fn create(pool: &PgPool, body: &str) -> common::Reply {
    let router = test_router(pool.clone(), false);
    send(&router, post_json("/tasks", body)).await
}

fn id_of(body: &Value) -> Uuid {
    body["id"].as_str().unwrap().parse().unwrap()
}

fn detail_fields(body: &Value) -> Vec<String> {
    body["error"]["details"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["field"].as_str().unwrap().to_owned())
        .collect()
}

#[sqlx::test]
async fn minimal_payload_returns_201_with_defaults(pool: PgPool) {
    let reply = create(&pool, r#"{"title":"Write PRD"}"#).await;
    assert_eq!(reply.status.as_u16(), 201, "{}", reply.body);
    let body = reply.json();
    assert_eq!(body["title"], "Write PRD");
    assert_eq!(body["status"], "todo");
    assert_eq!(body["priority"], "medium");
    assert_eq!(body["description"], Value::Null);
    assert_eq!(body["due_date"], Value::Null);
    id_of(&body);
    assert_eq!(body["created_at"], body["updated_at"]);
}

#[sqlx::test]
async fn response_has_location_header(pool: PgPool) {
    let reply = create(&pool, r#"{"title":"Write PRD"}"#).await;
    assert_eq!(reply.status.as_u16(), 201);
    let body = reply.json();
    let location = reply.headers.get("location").unwrap().to_str().unwrap();
    assert_eq!(location, format!("/tasks/{}", body["id"].as_str().unwrap()));
}

#[sqlx::test]
async fn all_fields_are_persisted_and_returned(pool: PgPool) {
    let reply = create(
        &pool,
        r#"{"title":"Ship it","description":"Final review","status":"in_progress","priority":"high","due_date":"2026-12-01"}"#,
    )
    .await;
    assert_eq!(reply.status.as_u16(), 201, "{}", reply.body);
    let body = reply.json();
    assert_eq!(body["title"], "Ship it");
    assert_eq!(body["description"], "Final review");
    assert_eq!(body["status"], "in_progress");
    assert_eq!(body["priority"], "high");
    assert_eq!(body["due_date"], "2026-12-01");
    let row = stored_task(&pool, id_of(&body)).await.unwrap();
    assert_eq!(row.title, "Ship it");
    assert_eq!(row.description.as_deref(), Some("Final review"));
    assert_eq!(row.status, "in_progress");
    assert_eq!(row.priority, "high");
    assert_eq!(row.due_date.unwrap().to_string(), "2026-12-01");
    assert_eq!(count_tasks(&pool).await, 1);
}

#[sqlx::test]
async fn title_is_trimmed_in_response_and_row(pool: PgPool) {
    let reply = create(&pool, r#"{"title":"  Trim me  "}"#).await;
    assert_eq!(reply.status.as_u16(), 201);
    let body = reply.json();
    assert_eq!(body["title"], "Trim me");
    assert_eq!(
        stored_task(&pool, id_of(&body)).await.unwrap().title,
        "Trim me"
    );
}

#[sqlx::test]
async fn blank_description_is_stored_as_null(pool: PgPool) {
    let reply = create(&pool, r#"{"title":"t","description":"   "}"#).await;
    assert_eq!(reply.status.as_u16(), 201);
    let body = reply.json();
    assert_eq!(body["description"], Value::Null);
    assert!(
        stored_task(&pool, id_of(&body))
            .await
            .unwrap()
            .description
            .is_none()
    );
}

#[sqlx::test]
async fn client_supplied_id_and_created_at_are_ignored(pool: PgPool) {
    let reply = create(
        &pool,
        r#"{"title":"t","id":"00000000-0000-0000-0000-000000000001","created_at":"2000-01-01T00:00:00.000Z","updated_at":"2000-01-01T00:00:00.000Z"}"#,
    )
    .await;
    assert_eq!(reply.status.as_u16(), 201);
    let body = reply.json();
    assert_ne!(body["id"], "00000000-0000-0000-0000-000000000001");
    assert_ne!(body["created_at"], "2000-01-01T00:00:00.000Z");
    let fake = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
    assert!(stored_task(&pool, fake).await.is_none());
}

#[sqlx::test]
async fn title_of_200_characters_is_accepted(pool: PgPool) {
    let title = "a".repeat(200);
    let reply = create(&pool, &json!({ "title": title }).to_string()).await;
    assert_eq!(reply.status.as_u16(), 201);
    assert_eq!(reply.json()["title"].as_str().unwrap().chars().count(), 200);
}

#[sqlx::test]
async fn description_of_5000_characters_is_accepted(pool: PgPool) {
    let description = "a".repeat(5000);
    let reply = create(
        &pool,
        &json!({ "title": "t", "description": description }).to_string(),
    )
    .await;
    assert_eq!(reply.status.as_u16(), 201);
    assert_eq!(
        reply.json()["description"]
            .as_str()
            .unwrap()
            .chars()
            .count(),
        5000
    );
}

#[sqlx::test]
async fn past_due_date_is_accepted(pool: PgPool) {
    let reply = create(&pool, r#"{"title":"t","due_date":"2000-01-01"}"#).await;
    assert_eq!(reply.status.as_u16(), 201);
    assert_eq!(reply.json()["due_date"], "2000-01-01");
}

#[sqlx::test]
async fn unknown_fields_are_ignored(pool: PgPool) {
    let reply = create(&pool, r#"{"title":"t","color":"red","extra":{"x":1}}"#).await;
    assert_eq!(reply.status.as_u16(), 201);
    let body = reply.json();
    assert!(body.get("color").is_none());
    assert!(body.get("extra").is_none());
}

#[sqlx::test]
async fn invalid_fields_return_422_and_insert_nothing(pool: PgPool) {
    let long_title = "a".repeat(201);
    let long_description = "a".repeat(5001);
    let cases: Vec<(String, &str)> = vec![
        (r#"{"priority":"high"}"#.to_owned(), "title"),
        (r#"{"title":null}"#.to_owned(), "title"),
        (r#"{"title":""}"#.to_owned(), "title"),
        (r#"{"title":"   "}"#.to_owned(), "title"),
        (json!({ "title": long_title }).to_string(), "title"),
        (
            json!({ "title": "t", "description": long_description }).to_string(),
            "description",
        ),
        (r#"{"title":"t","status":"DONE"}"#.to_owned(), "status"),
        (
            r#"{"title":"t","priority":"urgent"}"#.to_owned(),
            "priority",
        ),
        (
            r#"{"title":"t","due_date":"2026-02-30"}"#.to_owned(),
            "due_date",
        ),
        (
            r#"{"title":"t","due_date":"12/01/2026"}"#.to_owned(),
            "due_date",
        ),
        (
            r#"{"title":"t","due_date":"2026-1-5"}"#.to_owned(),
            "due_date",
        ),
    ];
    for (body, field) in cases {
        let reply = create(&pool, &body).await;
        assert_eq!(reply.status.as_u16(), 422, "{body}: {}", reply.body);
        let json = reply.json();
        assert_eq!(json["error"]["code"], "validation_error", "{body}");
        assert_eq!(
            json["error"]["message"], "Request validation failed",
            "{body}"
        );
        assert!(detail_fields(&json).contains(&field.to_owned()), "{body}");
    }
    assert_eq!(count_tasks(&pool).await, 0);
}

#[sqlx::test]
async fn multiple_invalid_fields_return_one_422(pool: PgPool) {
    let reply = create(&pool, r#"{"title":"   ","priority":"urgent"}"#).await;
    assert_eq!(reply.status.as_u16(), 422);
    assert_eq!(
        reply.body,
        r#"{"error":{"code":"validation_error","message":"Request validation failed","details":[{"field":"title","message":"must be between 1 and 200 characters"},{"field":"priority","message":"must be one of: low, medium, high"}]}}"#
    );
    assert_eq!(count_tasks(&pool).await, 0);
}

#[sqlx::test]
async fn malformed_json_returns_400(pool: PgPool) {
    let reply = create(&pool, r#"{"title":"#).await;
    assert_eq!(reply.status.as_u16(), 400);
    assert_eq!(reply.json()["error"]["code"], "invalid_json");
    assert_eq!(count_tasks(&pool).await, 0);
}

#[sqlx::test]
async fn wrong_json_type_returns_400_naming_the_field(pool: PgPool) {
    let reply = create(&pool, r#"{"title":5}"#).await;
    assert_eq!(reply.status.as_u16(), 400);
    let body = reply.json();
    assert_eq!(body["error"]["code"], "invalid_json");
    assert_eq!(detail_fields(&body), ["title"]);
    assert_eq!(count_tasks(&pool).await, 0);
}

#[sqlx::test]
async fn missing_content_type_returns_415(pool: PgPool) {
    let router = test_router(pool.clone(), false);
    let reply = send(
        &router,
        request_with("POST", "/tasks", None, r#"{"title":"t"}"#),
    )
    .await;
    assert_uniform_error(
        &reply,
        415,
        "unsupported_media_type",
        "Content-Type must be application/json",
    );
    assert_eq!(count_tasks(&pool).await, 0);
}

#[sqlx::test]
async fn oversized_body_returns_413(pool: PgPool) {
    let body = format!(
        r#"{{"title":"{}"}}"#,
        "a".repeat(65_537 - r#"{"title":""}"#.len())
    );
    assert_eq!(body.len(), 65_537);
    let reply = create(&pool, &body).await;
    assert_uniform_error(
        &reply,
        413,
        "payload_too_large",
        "Request body exceeds 64 KB",
    );
    assert_eq!(count_tasks(&pool).await, 0);
}

#[sqlx::test]
async fn unavailable_database_returns_503_and_inserts_nothing(
    pool_options: PgPoolOptions,
    connect_options: PgConnectOptions,
) {
    let _ = pool_options;
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(1))
        .connect_with(connect_options.clone())
        .await
        .unwrap();
    let router = test_router(pool, true);
    let holder = {
        let router = router.clone();
        tokio::spawn(async move { send(&router, get("/__test/slow?ms=4000&hold_db=true")).await })
    };
    tokio::time::sleep(Duration::from_millis(400)).await;

    let started = Instant::now();
    let reply = send(&router, post_json("/tasks", r#"{"title":"t"}"#)).await;
    let elapsed = started.elapsed();
    assert_uniform_error(
        &reply,
        503,
        "service_unavailable",
        "Database temporarily unavailable, please retry",
    );
    assert!(elapsed < Duration::from_millis(2500), "took {elapsed:?}");

    holder.abort();
    let verify = PgPoolOptions::new()
        .connect_with(connect_options)
        .await
        .unwrap();
    assert_eq!(count_tasks(&verify).await, 0);
}
