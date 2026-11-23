//! `GET /tasks/{id}` through the real router, in-process.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::{Duration, Instant};

use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};
use common::{
    StoredTask, assert_uniform_error, count_tasks, get, post_json, request_with, send, stored_task,
    test_router,
};
use serde_json::Value;
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use uuid::Uuid;

const INVALID_ID_MESSAGE: &str = "Task id must be a valid UUID";
const KEYS: [&str; 8] = [
    "id",
    "title",
    "description",
    "status",
    "priority",
    "due_date",
    "created_at",
    "updated_at",
];

/// Inserts a row directly with SQL and returns its server-generated id.
async fn insert_task(
    pool: &PgPool,
    title: &str,
    description: Option<&str>,
    status: &str,
    priority: &str,
    due_date: Option<NaiveDate>,
) -> Uuid {
    sqlx::query_scalar!(
        "INSERT INTO tasks (title, description, status, priority, due_date) \
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
        title,
        description,
        status,
        priority,
        due_date
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

/// The `known-task` row of the contract.
async fn insert_known_task(pool: &PgPool) -> Uuid {
    insert_task(
        pool,
        "Write PRD",
        Some("Draft the requirements"),
        "in_progress",
        "high",
        Some(NaiveDate::from_ymd_opt(2026, 12, 1).unwrap()),
    )
    .await
}

fn millis(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Positions of every representation key in the raw body, which must be
/// strictly increasing (serde_json's `Value` does not keep key order).
fn assert_key_order(body: &str) {
    let positions: Vec<usize> = KEYS
        .iter()
        .map(|key| {
            body.find(&format!("\"{key}\":"))
                .unwrap_or_else(|| panic!("missing key {key}: {body}"))
        })
        .collect();
    assert!(positions.is_sorted(), "keys out of order: {body}");
}

fn assert_same_row(before: &StoredTask, after: &StoredTask) {
    assert_eq!(before.id, after.id);
    assert_eq!(before.title, after.title);
    assert_eq!(before.description, after.description);
    assert_eq!(before.status, after.status);
    assert_eq!(before.priority, after.priority);
    assert_eq!(before.due_date, after.due_date);
    assert_eq!(before.created_at, after.created_at);
    assert_eq!(before.updated_at, after.updated_at);
}

/// A router over a 1-connection pool (1 s acquire timeout) with the
/// diagnostic routes mounted, plus an in-flight request that holds the
/// only connection for 4 s.
async fn router_with_held_pool(
    connect_options: PgConnectOptions,
) -> (axum::Router, tokio::task::JoinHandle<common::Reply>) {
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
    tokio::time::sleep(Duration::from_millis(400)).await;
    (router, holder)
}

// ---- success ---------------------------------------------------------------

#[sqlx::test]
async fn existing_task_returns_200_with_all_fields(pool: PgPool) {
    let id = insert_known_task(&pool).await;
    let router = test_router(pool.clone(), false);

    let reply = send(&router, get(&format!("/tasks/{id}"))).await;
    assert_eq!(reply.status.as_u16(), 200, "{}", reply.body);
    assert_eq!(
        reply.headers.get("content-type").unwrap(),
        "application/json"
    );
    assert_key_order(&reply.body);

    let body = reply.json();
    assert_eq!(body.as_object().unwrap().len(), 8);
    let row = stored_task(&pool, id).await.unwrap();
    assert_eq!(body["id"], id.to_string());
    assert_eq!(body["title"], "Write PRD");
    assert_eq!(body["description"], "Draft the requirements");
    assert_eq!(body["status"], "in_progress");
    assert_eq!(body["priority"], "high");
    assert_eq!(body["due_date"], "2026-12-01");
    assert_eq!(body["created_at"], millis(row.created_at));
    assert_eq!(body["updated_at"], millis(row.updated_at));
    let created_at = body["created_at"].as_str().unwrap();
    assert_eq!(created_at.len(), "2026-11-21T18:11:00.123Z".len());
    assert!(created_at.ends_with('Z'));
}

#[sqlx::test]
async fn null_optional_fields_are_returned_as_null(pool: PgPool) {
    let id = insert_task(&pool, "Bare", None, "todo", "medium", None).await;
    let router = test_router(pool, false);

    let reply = send(&router, get(&format!("/tasks/{id}"))).await;
    assert_eq!(reply.status.as_u16(), 200, "{}", reply.body);
    assert_key_order(&reply.body);
    let body = reply.json();
    assert_eq!(body["description"], Value::Null);
    assert_eq!(body["due_date"], Value::Null);
    assert_eq!(body["title"], "Bare");
    assert_eq!(body["status"], "todo");
    assert_eq!(body["priority"], "medium");
}

#[sqlx::test]
async fn get_matches_create_response(pool: PgPool) {
    let router = test_router(pool, false);
    let created = send(
        &router,
        post_json(
            "/tasks",
            r#"{"title":"Write PRD","priority":"high","due_date":"2026-12-01"}"#,
        ),
    )
    .await;
    assert_eq!(created.status.as_u16(), 201, "{}", created.body);
    let id = created.json()["id"].as_str().unwrap().to_owned();

    let reply = send(&router, get(&format!("/tasks/{id}"))).await;
    assert_eq!(reply.status.as_u16(), 200, "{}", reply.body);
    assert_eq!(reply.body, created.body);
}

#[sqlx::test]
async fn uppercase_id_returns_the_task(pool: PgPool) {
    let id = insert_known_task(&pool).await;
    let router = test_router(pool, false);

    let path = format!("/tasks/{}", id.to_string().to_uppercase());
    let reply = send(&router, get(&path)).await;
    assert_eq!(reply.status.as_u16(), 200, "{}", reply.body);
    let body = reply.json();
    assert_eq!(body["id"], id.to_string());
    assert_eq!(body["title"], "Write PRD");
}

#[sqlx::test]
async fn get_does_not_modify_the_row(pool: PgPool) {
    let id = insert_known_task(&pool).await;
    let before = stored_task(&pool, id).await.unwrap();
    let router = test_router(pool.clone(), false);

    for _ in 0..2 {
        let reply = send(&router, get(&format!("/tasks/{id}"))).await;
        assert_eq!(reply.status.as_u16(), 200, "{}", reply.body);
    }
    let after = stored_task(&pool, id).await.unwrap();
    assert_same_row(&before, &after);
    assert_eq!(count_tasks(&pool).await, 1);
}

// ---- malformed id ------------------------------------------------------------

#[sqlx::test]
async fn malformed_ids_return_400_invalid_id(pool: PgPool) {
    let router = test_router(pool, false);
    for segment in [
        "not-a-uuid",
        "0b7d2c1e5f3a4e8b9c6d7a1f2e3d4c5b",
        "urn:uuid:0b7d2c1e-5f3a-4e8b-9c6d-7a1f2e3d4c5b",
        "0b7d2c1e-5f3a-4e8b-9c6d-7a1f2e3d4c5",
        "0b7d2c1e-5f3a-4e8b-9c6d-7a1f2e3d4c5g",
        "0b7d2c1e5-f3a-4e8b-9c6d-7a1f2e3d4c5b",
        "%FF",
    ] {
        let reply = send(&router, get(&format!("/tasks/{segment}"))).await;
        assert_uniform_error(&reply, 400, "invalid_id", INVALID_ID_MESSAGE);
    }
}

#[sqlx::test]
async fn malformed_id_does_not_touch_the_database(
    pool_options: PgPoolOptions,
    connect_options: PgConnectOptions,
) {
    let _ = pool_options;
    let (router, holder) = router_with_held_pool(connect_options).await;

    let started = Instant::now();
    let reply = send(&router, get("/tasks/abc")).await;
    let elapsed = started.elapsed();
    assert_uniform_error(&reply, 400, "invalid_id", INVALID_ID_MESSAGE);
    assert!(elapsed < Duration::from_millis(500), "took {elapsed:?}");

    holder.abort();
}

// ---- missing task --------------------------------------------------------------

#[sqlx::test]
async fn unknown_id_returns_404_with_id_in_message(pool: PgPool) {
    insert_known_task(&pool).await;
    let router = test_router(pool.clone(), false);

    let id = Uuid::new_v4();
    let reply = send(&router, get(&format!("/tasks/{id}"))).await;
    assert_uniform_error(&reply, 404, "not_found", &format!("Task {id} not found"));
    assert_eq!(count_tasks(&pool).await, 1);
}

#[sqlx::test]
async fn unknown_uppercase_id_reports_lowercase(pool: PgPool) {
    insert_known_task(&pool).await;
    let router = test_router(pool, false);

    let id = Uuid::new_v4();
    let path = format!("/tasks/{}", id.to_string().to_uppercase());
    let reply = send(&router, get(&path)).await;
    assert_uniform_error(&reply, 404, "not_found", &format!("Task {id} not found"));
}

#[sqlx::test]
async fn nil_uuid_is_a_valid_id(pool: PgPool) {
    insert_known_task(&pool).await;
    let router = test_router(pool, false);

    let reply = send(&router, get("/tasks/00000000-0000-0000-0000-000000000000")).await;
    assert_uniform_error(
        &reply,
        404,
        "not_found",
        "Task 00000000-0000-0000-0000-000000000000 not found",
    );
}

// ---- request rejection ---------------------------------------------------------

#[sqlx::test]
async fn unavailable_database_returns_503(
    pool_options: PgPoolOptions,
    connect_options: PgConnectOptions,
) {
    let _ = pool_options;
    let seed = PgPoolOptions::new()
        .connect_with(connect_options.clone())
        .await
        .unwrap();
    let id = insert_known_task(&seed).await;
    seed.close().await;

    let (router, holder) = router_with_held_pool(connect_options).await;
    let started = Instant::now();
    let reply = send(&router, get(&format!("/tasks/{id}"))).await;
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
async fn unsupported_method_returns_405(pool: PgPool) {
    let id = insert_known_task(&pool).await;
    let before = stored_task(&pool, id).await.unwrap();
    let router = test_router(pool.clone(), false);

    let reply = send(
        &router,
        request_with(
            "PUT",
            &format!("/tasks/{id}"),
            Some("application/json"),
            r#"{"title":"x"}"#,
        ),
    )
    .await;
    assert_uniform_error(&reply, 405, "method_not_allowed", "Method not allowed");
    let after = stored_task(&pool, id).await.unwrap();
    assert_same_row(&before, &after);
}

#[sqlx::test]
async fn nested_path_is_an_unknown_route(pool: PgPool) {
    let id = insert_known_task(&pool).await;
    let router = test_router(pool, false);

    let reply = send(&router, get(&format!("/tasks/{id}/extra"))).await;
    assert_uniform_error(&reply, 404, "not_found", "Route not found");
}

// ---- latency ---------------------------------------------------------------------

/// Seeds `count` rows cycling every status and priority, with about one
/// third of due dates `NULL`.
async fn seed_tasks(pool: &PgPool, count: i32) {
    sqlx::query!(
        "INSERT INTO tasks (title, description, status, priority, due_date) \
         SELECT 'Task ' || n, \
                CASE WHEN n % 2 = 0 THEN 'Description ' || n END, \
                (ARRAY['todo', 'in_progress', 'done'])[n % 3 + 1], \
                (ARRAY['low', 'medium', 'high'])[(n / 3) % 3 + 1], \
                CASE WHEN (n / 9) % 3 = 0 THEN NULL ELSE DATE '2026-01-01' + (n % 365) END \
         FROM generate_series(1, $1::int4) AS n",
        count
    )
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test]
async fn get_is_fast_with_10000_tasks(pool: PgPool) {
    seed_tasks(&pool, 10_000).await;
    assert_eq!(count_tasks(&pool).await, 10_000);
    let ids: Vec<Uuid> = sqlx::query_scalar!("SELECT id FROM tasks ORDER BY id")
        .fetch_all(&pool)
        .await
        .unwrap();
    let router = test_router(pool, false);

    let mut timings: Vec<Duration> = Vec::with_capacity(1_000);
    for id in ids.iter().step_by(10).take(1_000) {
        let started = Instant::now();
        let reply = send(&router, get(&format!("/tasks/{id}"))).await;
        timings.push(started.elapsed());
        assert_eq!(reply.status.as_u16(), 200, "{}", reply.body);
    }
    assert_eq!(timings.len(), 1_000);
    timings.sort();
    let p95 = timings[949];
    assert!(p95 < Duration::from_millis(50), "p95 was {p95:?}");
}
