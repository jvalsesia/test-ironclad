//! `PATCH /tasks/{id}` through the real router, in-process.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::{Duration, Instant};

use axum::Router;
use chrono::{DateTime, SecondsFormat, Utc};
use common::{
    Reply, StoredTask, assert_uniform_error, count_tasks, get, post_json, request_with, send,
    stored_task, test_router,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use uuid::Uuid;

const KNOWN_TASK: &str = r#"{"title":"Write PRD","description":"Draft the requirements","status":"todo","priority":"high","due_date":"2026-12-01"}"#;

const NO_CHANGES_BODY: &str = r#"{"error":{"code":"validation_error","message":"At least one updatable field must be provided","details":[]}}"#;

/// Creates `known-task` through `POST /tasks` and returns its id.
async fn seed_known(router: &Router) -> Uuid {
    let reply = send(router, post_json("/tasks", KNOWN_TASK)).await;
    assert_eq!(reply.status.as_u16(), 201, "{}", reply.body);
    reply.json()["id"].as_str().unwrap().parse().unwrap()
}

async fn setup(pool: &PgPool) -> (Router, Uuid, StoredTask) {
    let router = test_router(pool.clone(), false);
    let id = seed_known(&router).await;
    let before = stored_task(pool, id).await.unwrap();
    assert_eq!(before.created_at, before.updated_at);
    (router, id, before)
}

async fn patch(router: &Router, path: &str, body: &str) -> Reply {
    send(
        router,
        request_with("PATCH", path, Some("application/json"), body),
    )
    .await
}

fn path_of(id: Uuid) -> String {
    format!("/tasks/{id}")
}

fn millis(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Every column equals the row read before the request.
fn assert_same_row(before: &StoredTask, after: &StoredTask) {
    assert_eq!(after.id, before.id);
    assert_eq!(after.title, before.title);
    assert_eq!(after.description, before.description);
    assert_eq!(after.status, before.status);
    assert_eq!(after.priority, before.priority);
    assert_eq!(after.due_date, before.due_date);
    assert_eq!(after.created_at, before.created_at);
    assert_eq!(after.updated_at, before.updated_at);
}

async fn assert_unchanged(pool: &PgPool, before: &StoredTask) {
    let after = stored_task(pool, before.id).await.unwrap();
    assert_same_row(before, &after);
}

fn validation_body(details: Value) -> Value {
    json!({"error":{"code":"validation_error","message":"Request validation failed","details":details}})
}

fn str_of<'a>(body: &'a Value, key: &str) -> &'a str {
    body[key].as_str().unwrap()
}

// ---- successful updates ----------------------------------------------------

#[sqlx::test]
async fn status_only_patch_changes_only_status(pool: PgPool) {
    let (router, id, before) = setup(&pool).await;
    let reply = patch(&router, &path_of(id), r#"{"status":"done"}"#).await;
    assert_eq!(reply.status.as_u16(), 200, "{}", reply.body);
    let body = reply.json();
    assert_eq!(body["status"], "done");
    assert_eq!(body["id"], id.to_string());
    assert_eq!(body["title"], "Write PRD");
    assert_eq!(body["description"], "Draft the requirements");
    assert_eq!(body["priority"], "high");
    assert_eq!(body["due_date"], "2026-12-01");
    assert_eq!(str_of(&body, "created_at"), millis(before.created_at));

    let after = stored_task(&pool, id).await.unwrap();
    assert_eq!(after.status, "done");
    assert_eq!(after.title, before.title);
    assert_eq!(after.description, before.description);
    assert_eq!(after.priority, before.priority);
    assert_eq!(after.due_date, before.due_date);
    assert_eq!(after.created_at, before.created_at);
}

#[sqlx::test]
async fn updated_at_strictly_increases_and_created_at_is_kept(pool: PgPool) {
    let (router, id, before) = setup(&pool).await;
    let reply = patch(&router, &path_of(id), r#"{"priority":"low"}"#).await;
    assert_eq!(reply.status.as_u16(), 200, "{}", reply.body);
    let body = reply.json();
    assert!(
        str_of(&body, "updated_at") > millis(before.updated_at).as_str(),
        "{} vs {}",
        body["updated_at"],
        millis(before.updated_at)
    );
    assert_eq!(str_of(&body, "created_at"), millis(before.created_at));

    let after = stored_task(&pool, id).await.unwrap();
    assert!(after.updated_at > before.updated_at);
    assert_eq!(after.created_at, before.created_at);
}

#[sqlx::test]
async fn back_to_back_patches_increase_updated_at(pool: PgPool) {
    let (router, id, _) = setup(&pool).await;
    let first = patch(&router, &path_of(id), r#"{"status":"in_progress"}"#).await;
    assert_eq!(first.status.as_u16(), 200, "{}", first.body);
    let second = patch(&router, &path_of(id), r#"{"status":"done"}"#).await;
    assert_eq!(second.status.as_u16(), 200, "{}", second.body);
    let (first, second) = (first.json(), second.json());
    assert!(
        str_of(&second, "updated_at") > str_of(&first, "updated_at"),
        "{} then {}",
        first["updated_at"],
        second["updated_at"]
    );
    assert_eq!(second["created_at"], first["created_at"]);
}

#[sqlx::test]
async fn all_fields_patch(pool: PgPool) {
    let (router, id, _) = setup(&pool).await;
    let reply = patch(
        &router,
        &path_of(id),
        r#"{"title":"  Ship it  ","description":"Final review","status":"in_progress","priority":"low","due_date":"2027-01-15"}"#,
    )
    .await;
    assert_eq!(reply.status.as_u16(), 200, "{}", reply.body);
    let body = reply.json();
    assert_eq!(body["id"], id.to_string());
    assert_eq!(body["title"], "Ship it");
    assert_eq!(body["description"], "Final review");
    assert_eq!(body["status"], "in_progress");
    assert_eq!(body["priority"], "low");
    assert_eq!(body["due_date"], "2027-01-15");

    let row = stored_task(&pool, id).await.unwrap();
    assert_eq!(row.title, "Ship it");
    assert_eq!(row.description.as_deref(), Some("Final review"));
    assert_eq!(row.status, "in_progress");
    assert_eq!(row.priority, "low");
    assert_eq!(row.due_date.unwrap().to_string(), "2027-01-15");
}

#[sqlx::test]
async fn same_value_patch_still_updates(pool: PgPool) {
    let (router, id, before) = setup(&pool).await;
    let reply = patch(&router, &path_of(id), r#"{"status":"todo"}"#).await;
    assert_eq!(reply.status.as_u16(), 200, "{}", reply.body);
    let body = reply.json();
    assert_eq!(body["status"], "todo");
    assert!(str_of(&body, "updated_at") > millis(before.updated_at).as_str());
}

#[sqlx::test]
async fn unknown_and_server_managed_fields_are_ignored(pool: PgPool) {
    let (router, id, before) = setup(&pool).await;
    let reply = patch(
        &router,
        &path_of(id),
        r#"{"status":"done","color":"red","id":"00000000-0000-0000-0000-000000000001","created_at":"2000-01-01T00:00:00.000Z"}"#,
    )
    .await;
    assert_eq!(reply.status.as_u16(), 200, "{}", reply.body);
    let body = reply.json();
    assert_eq!(body["status"], "done");
    assert!(body.get("color").is_none());
    assert_eq!(body["id"], id.to_string());
    assert_eq!(str_of(&body, "created_at"), millis(before.created_at));
    assert!(
        stored_task(&pool, Uuid::from_u128(1)).await.is_none(),
        "client id must not be stored"
    );
}

#[sqlx::test]
async fn response_is_the_full_task_representation(pool: PgPool) {
    let (router, id, _) = setup(&pool).await;
    let reply = patch(&router, &path_of(id), r#"{"title":"Renamed"}"#).await;
    assert_eq!(reply.status.as_u16(), 200, "{}", reply.body);
    assert_eq!(reply.headers["content-type"], "application/json");
    assert!(reply.headers.get("location").is_none());
    let keys = [
        "id",
        "title",
        "description",
        "status",
        "priority",
        "due_date",
        "created_at",
        "updated_at",
    ];
    let positions: Vec<usize> = keys
        .iter()
        .map(|key| reply.body.find(&format!("\"{key}\":")).unwrap())
        .collect();
    assert!(positions.windows(2).all(|w| w[0] < w[1]), "{}", reply.body);
    let body = reply.json();
    assert_eq!(body.as_object().unwrap().len(), keys.len());
    assert_eq!(body["title"], "Renamed");
}

// ---- clearing optional fields ----------------------------------------------

#[sqlx::test]
async fn null_clears_description_and_due_date(pool: PgPool) {
    let (router, id, _) = setup(&pool).await;
    let reply = patch(&router, &path_of(id), r#"{"description":null}"#).await;
    assert_eq!(reply.status.as_u16(), 200, "{}", reply.body);
    assert_eq!(reply.json()["description"], Value::Null);
    let row = stored_task(&pool, id).await.unwrap();
    assert!(row.description.is_none());
    assert_eq!(row.due_date.unwrap().to_string(), "2026-12-01");
}

#[sqlx::test]
async fn null_clears_due_date(pool: PgPool) {
    let (router, id, _) = setup(&pool).await;
    let reply = patch(&router, &path_of(id), r#"{"due_date":null}"#).await;
    assert_eq!(reply.status.as_u16(), 200, "{}", reply.body);
    assert_eq!(reply.json()["due_date"], Value::Null);
    let row = stored_task(&pool, id).await.unwrap();
    assert!(row.due_date.is_none());
    assert_eq!(row.description.as_deref(), Some("Draft the requirements"));
}

#[sqlx::test]
async fn null_clears_both_optional_fields_at_once(pool: PgPool) {
    let (router, id, _) = setup(&pool).await;
    let reply = patch(
        &router,
        &path_of(id),
        r#"{"description":null,"due_date":null}"#,
    )
    .await;
    assert_eq!(reply.status.as_u16(), 200, "{}", reply.body);
    let body = reply.json();
    assert_eq!(body["description"], Value::Null);
    assert_eq!(body["due_date"], Value::Null);
    assert_eq!(body["title"], "Write PRD");
    let row = stored_task(&pool, id).await.unwrap();
    assert!(row.description.is_none() && row.due_date.is_none());
}

#[sqlx::test]
async fn blank_description_clears(pool: PgPool) {
    let (router, id, _) = setup(&pool).await;
    let reply = patch(&router, &path_of(id), r#"{"description":"   "}"#).await;
    assert_eq!(reply.status.as_u16(), 200, "{}", reply.body);
    assert_eq!(reply.json()["description"], Value::Null);
    assert!(stored_task(&pool, id).await.unwrap().description.is_none());
}

// ---- validation --------------------------------------------------------------

#[sqlx::test]
async fn null_for_non_nullable_fields_returns_422(pool: PgPool) {
    let (router, id, before) = setup(&pool).await;
    for field in ["title", "status", "priority"] {
        let reply = patch(&router, &path_of(id), &format!(r#"{{"{field}":null}}"#)).await;
        assert_eq!(reply.status.as_u16(), 422, "{}", reply.body);
        assert_eq!(reply.headers["content-type"], "application/json");
        assert_eq!(
            reply.body,
            format!(
                r#"{{"error":{{"code":"validation_error","message":"Request validation failed","details":[{{"field":"{field}","message":"cannot be null"}}]}}}}"#
            )
        );
        assert_unchanged(&pool, &before).await;
    }
}

#[sqlx::test]
async fn empty_or_unknown_only_body_returns_422(pool: PgPool) {
    let (router, id, before) = setup(&pool).await;
    for body in ["{}", r#"{"color":"red"}"#] {
        let reply = patch(&router, &path_of(id), body).await;
        assert_eq!(reply.status.as_u16(), 422, "{}", reply.body);
        assert_eq!(reply.body, NO_CHANGES_BODY, "{body}");
        assert_unchanged(&pool, &before).await;
    }
}

#[sqlx::test]
async fn partially_valid_payload_writes_nothing(pool: PgPool) {
    let (router, id, before) = setup(&pool).await;
    let reply = patch(
        &router,
        &path_of(id),
        r#"{"title":"Valid","priority":"urgent"}"#,
    )
    .await;
    assert_eq!(reply.status.as_u16(), 422, "{}", reply.body);
    assert_eq!(
        reply.body,
        r#"{"error":{"code":"validation_error","message":"Request validation failed","details":[{"field":"priority","message":"must be one of: low, medium, high"}]}}"#
    );
    assert_eq!(stored_task(&pool, id).await.unwrap().title, "Write PRD");
    assert_unchanged(&pool, &before).await;
}

#[sqlx::test]
async fn invalid_values_return_422(pool: PgPool) {
    let (router, id, before) = setup(&pool).await;
    let cases = [
        (
            json!({"title": "a".repeat(201)}),
            "title",
            "must be between 1 and 200 characters",
        ),
        (
            json!({"description": "a".repeat(5001)}),
            "description",
            "must be at most 5000 characters",
        ),
        (
            json!({"due_date": "2026-13-01"}),
            "due_date",
            "must be a valid date in YYYY-MM-DD format",
        ),
        (
            json!({"title": ""}),
            "title",
            "must be between 1 and 200 characters",
        ),
        (
            json!({"status": "DONE"}),
            "status",
            "must be one of: todo, in_progress, done",
        ),
    ];
    for (body, field, message) in cases {
        let reply = patch(&router, &path_of(id), &body.to_string()).await;
        assert_eq!(reply.status.as_u16(), 422, "{}", reply.body);
        assert_eq!(
            reply.json(),
            validation_body(json!([{"field": field, "message": message}])),
            "{field}"
        );
        assert_unchanged(&pool, &before).await;
    }
}

#[sqlx::test]
async fn all_errors_are_reported_in_order(pool: PgPool) {
    let (router, id, before) = setup(&pool).await;
    let reply = patch(
        &router,
        &path_of(id),
        r#"{"due_date":"2026-02-30","priority":null,"status":"DONE","title":""}"#,
    )
    .await;
    assert_eq!(reply.status.as_u16(), 422, "{}", reply.body);
    assert_eq!(
        reply.json(),
        validation_body(json!([
            {"field":"title","message":"must be between 1 and 200 characters"},
            {"field":"status","message":"must be one of: todo, in_progress, done"},
            {"field":"priority","message":"cannot be null"},
            {"field":"due_date","message":"must be a valid date in YYYY-MM-DD format"}
        ]))
    );
    assert_unchanged(&pool, &before).await;
}

// ---- missing task and malformed id ------------------------------------------

#[sqlx::test]
async fn unknown_id_returns_404(pool: PgPool) {
    let (router, _, before) = setup(&pool).await;
    let unknown = Uuid::new_v4();
    let reply = patch(&router, &path_of(unknown), r#"{"status":"done"}"#).await;
    assert_uniform_error(
        &reply,
        404,
        "not_found",
        &format!("Task {unknown} not found"),
    );
    assert_eq!(count_tasks(&pool).await, 1);
    assert_unchanged(&pool, &before).await;
}

#[sqlx::test]
async fn unknown_id_with_invalid_body_returns_422(pool: PgPool) {
    let (router, _, _) = setup(&pool).await;
    let reply = patch(&router, &path_of(Uuid::new_v4()), r#"{"title":""}"#).await;
    assert_eq!(reply.status.as_u16(), 422, "{}", reply.body);
    assert_eq!(reply.json()["error"]["code"], "validation_error");
    assert_eq!(count_tasks(&pool).await, 1);
}

#[sqlx::test]
async fn malformed_id_returns_400(pool: PgPool) {
    let (router, _, before) = setup(&pool).await;
    for body in [r#"{"status":"done"}"#, r#"{"status":"#] {
        let reply = patch(&router, "/tasks/abc", body).await;
        assert_uniform_error(&reply, 400, "invalid_id", "Task id must be a valid UUID");
    }
    let reply = send(&router, request_with("PATCH", "/tasks/abc", None, "{")).await;
    assert_uniform_error(&reply, 400, "invalid_id", "Task id must be a valid UUID");
    assert_unchanged(&pool, &before).await;
}

// ---- request rejection --------------------------------------------------------

#[sqlx::test]
async fn malformed_json_returns_400(pool: PgPool) {
    let (router, id, before) = setup(&pool).await;
    let reply = patch(&router, &path_of(id), r#"{"status":"#).await;
    assert_uniform_error(
        &reply,
        400,
        "invalid_json",
        "Request body is not valid JSON for this endpoint",
    );
    assert_unchanged(&pool, &before).await;
}

#[sqlx::test]
async fn wrong_type_returns_400_naming_field(pool: PgPool) {
    let (router, id, before) = setup(&pool).await;
    let reply = patch(&router, &path_of(id), r#"{"title":5}"#).await;
    assert_eq!(reply.status.as_u16(), 400, "{}", reply.body);
    let body = reply.json();
    assert_eq!(body["error"]["code"], "invalid_json");
    let details = body["error"]["details"].as_array().unwrap();
    assert_eq!(details.len(), 1);
    assert_eq!(details[0]["field"], "title");
    assert_unchanged(&pool, &before).await;
}

#[sqlx::test]
async fn non_object_body_returns_400(pool: PgPool) {
    let (router, id, before) = setup(&pool).await;
    for body in ["null", "[]", r#""done""#, "5"] {
        let reply = patch(&router, &path_of(id), body).await;
        assert_uniform_error(
            &reply,
            400,
            "invalid_json",
            "Request body is not valid JSON for this endpoint",
        );
    }
    assert_unchanged(&pool, &before).await;
}

#[sqlx::test]
async fn missing_content_type_returns_415(pool: PgPool) {
    let (router, id, before) = setup(&pool).await;
    let reply = send(
        &router,
        request_with("PATCH", &path_of(id), None, r#"{"status":"done"}"#),
    )
    .await;
    assert_uniform_error(
        &reply,
        415,
        "unsupported_media_type",
        "Content-Type must be application/json",
    );
    assert_unchanged(&pool, &before).await;
}

#[sqlx::test]
async fn oversized_body_returns_413(pool: PgPool) {
    let (router, id, before) = setup(&pool).await;
    let body = format!(
        r#"{{"title":"{}"}}"#,
        "a".repeat(65_537 - r#"{"title":""}"#.len())
    );
    assert_eq!(body.len(), 65_537);
    let request = axum::http::Request::builder()
        .method("PATCH")
        .uri(path_of(id))
        .header("content-type", "application/json")
        .header("content-length", "65537")
        .body(axum::body::Body::from(body))
        .unwrap();
    let reply = send(&router, request).await;
    assert_uniform_error(
        &reply,
        413,
        "payload_too_large",
        "Request body exceeds 64 KB",
    );
    assert_unchanged(&pool, &before).await;
}

#[sqlx::test]
async fn unavailable_database_returns_503_and_writes_nothing(
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
    let router = test_router(pool.clone(), true);
    let id = seed_known(&router).await;
    let before = stored_task(&pool, id).await.unwrap();

    let holder = {
        let router = router.clone();
        tokio::spawn(async move { send(&router, get("/__test/slow?ms=4000&hold_db=true")).await })
    };
    tokio::time::sleep(Duration::from_millis(400)).await;

    let started = Instant::now();
    let reply = patch(&router, &path_of(id), r#"{"status":"done"}"#).await;
    let elapsed = started.elapsed();
    assert_uniform_error(
        &reply,
        503,
        "service_unavailable",
        "Database temporarily unavailable, please retry",
    );
    assert!(elapsed < Duration::from_millis(2500), "took {elapsed:?}");

    let held = holder.await.unwrap();
    assert_eq!(held.status.as_u16(), 200, "{}", held.body);
    let verify = PgPoolOptions::new()
        .connect_with(connect_options)
        .await
        .unwrap();
    assert_unchanged(&verify, &before).await;
}

#[sqlx::test]
async fn unsupported_method_returns_405(pool: PgPool) {
    let (router, id, before) = setup(&pool).await;
    let reply = send(
        &router,
        request_with(
            "PUT",
            &path_of(id),
            Some("application/json"),
            r#"{"status":"done"}"#,
        ),
    )
    .await;
    assert_uniform_error(&reply, 405, "method_not_allowed", "Method not allowed");
    assert_unchanged(&pool, &before).await;
}
