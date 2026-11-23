//! `DELETE /tasks/{id}` through the real router, in-process.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::Request;
use common::{
    Reply, assert_uniform_error, count_tasks, get, request_with, send, stored_task, test_router,
};
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use task_api::application::tasks::TaskRepository;
use task_api::domain::task::{NewTask, RawNewTask};
use task_api::infrastructure::db::PgTaskRepository;
use uuid::Uuid;

const MSG_INVALID_ID: &str = "Task id must be a valid UUID";

/// Ids of the contract's `three-tasks` rows.
struct Seeded {
    alpha: Uuid,
    beta: Uuid,
    gamma: Uuid,
}

fn raw(
    title: &str,
    description: Option<&str>,
    status: &str,
    priority: &str,
    due_date: Option<&str>,
) -> RawNewTask {
    RawNewTask {
        title: Some(title.to_owned()),
        description: description.map(str::to_owned),
        status: Some(status.to_owned()),
        priority: Some(priority.to_owned()),
        due_date: due_date.map(str::to_owned),
    }
}

async fn seed(pool: &PgPool) -> Seeded {
    let repo = PgTaskRepository::new(pool.clone());
    let mut ids = Vec::new();
    for raw in [
        raw("Alpha", Some("First"), "todo", "low", Some("2026-12-01")),
        raw("Beta", None, "in_progress", "high", None),
        raw("Gamma", Some("Third"), "done", "medium", Some("2027-01-15")),
    ] {
        let task = repo.create(&NewTask::parse(raw).unwrap()).await.unwrap();
        ids.push(task.id().value());
    }
    assert_eq!(count_tasks(pool).await, 3);
    Seeded {
        alpha: ids[0],
        beta: ids[1],
        gamma: ids[2],
    }
}

/// Every stored row (every column), for before/after comparisons.
async fn snapshot(pool: &PgPool, ids: &[Uuid]) -> Vec<String> {
    let mut rows = Vec::new();
    for id in ids {
        rows.push(format!("{:?}", stored_task(pool, *id).await));
    }
    rows
}

fn delete(path: &str) -> Request<Body> {
    Request::builder()
        .method("DELETE")
        .uri(path)
        .body(Body::empty())
        .unwrap()
}

async fn delete_task(pool: &PgPool, id: &str) -> Reply {
    let router = test_router(pool.clone(), false);
    send(&router, delete(&format!("/tasks/{id}"))).await
}

fn not_found_message(id: Uuid) -> String {
    format!("Task {id} not found")
}

/// A 1-connection pool with a 1 s acquire timeout.
async fn single_connection_pool(options: PgConnectOptions) -> PgPool {
    PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(1))
        .connect_with(options)
        .await
        .unwrap()
}

#[sqlx::test]
async fn existing_task_returns_204_with_empty_body(pool: PgPool) {
    let seeded = seed(&pool).await;
    let reply = delete_task(&pool, &seeded.beta.to_string()).await;
    assert_eq!(reply.status.as_u16(), 204, "{}", reply.body);
    assert!(reply.body.is_empty(), "body: {:?}", reply.body);
    assert!(reply.headers.get("content-type").is_none());
}

#[sqlx::test]
async fn deleted_row_is_gone_and_count_drops_by_one(pool: PgPool) {
    let seeded = seed(&pool).await;
    let reply = delete_task(&pool, &seeded.beta.to_string()).await;
    assert_eq!(reply.status.as_u16(), 204);
    assert!(stored_task(&pool, seeded.beta).await.is_none());
    assert_eq!(count_tasks(&pool).await, 2);
}

#[sqlx::test]
async fn other_tasks_are_untouched(pool: PgPool) {
    let seeded = seed(&pool).await;
    let before = snapshot(&pool, &[seeded.alpha, seeded.gamma]).await;
    let reply = delete_task(&pool, &seeded.beta.to_string()).await;
    assert_eq!(reply.status.as_u16(), 204);
    assert_eq!(snapshot(&pool, &[seeded.alpha, seeded.gamma]).await, before);
    assert!(before.iter().all(|row| row.starts_with("Some(")));
}

#[sqlx::test]
async fn uppercase_id_deletes_the_task(pool: PgPool) {
    let seeded = seed(&pool).await;
    let reply = delete_task(&pool, &seeded.beta.to_string().to_uppercase()).await;
    assert_eq!(reply.status.as_u16(), 204, "{}", reply.body);
    assert!(stored_task(&pool, seeded.beta).await.is_none());
}

#[sqlx::test]
async fn request_body_is_ignored(pool: PgPool) {
    let seeded = seed(&pool).await;
    let router = test_router(pool.clone(), false);
    let path = format!("/tasks/{}", seeded.beta);
    let reply = send(
        &router,
        request_with("DELETE", &path, None, r#"{"confirm":true}"#),
    )
    .await;
    assert_eq!(reply.status.as_u16(), 204, "{}", reply.body);
    assert!(stored_task(&pool, seeded.beta).await.is_none());
}

#[sqlx::test]
async fn second_delete_returns_404(pool: PgPool) {
    let seeded = seed(&pool).await;
    let first = delete_task(&pool, &seeded.beta.to_string()).await;
    assert_eq!(first.status.as_u16(), 204);
    let second = delete_task(&pool, &seeded.beta.to_string()).await;
    assert_uniform_error(&second, 404, "not_found", &not_found_message(seeded.beta));
    assert_eq!(count_tasks(&pool).await, 2);
}

#[sqlx::test]
async fn unknown_id_returns_404_and_deletes_nothing(pool: PgPool) {
    let seeded = seed(&pool).await;
    let all = [seeded.alpha, seeded.beta, seeded.gamma];
    let before = snapshot(&pool, &all).await;
    let unknown = Uuid::new_v4();
    let reply = delete_task(&pool, &unknown.to_string()).await;
    assert_uniform_error(&reply, 404, "not_found", &not_found_message(unknown));
    assert_eq!(count_tasks(&pool).await, 3);
    assert_eq!(snapshot(&pool, &all).await, before);
}

#[sqlx::test]
async fn unknown_uppercase_id_reports_lowercase(pool: PgPool) {
    seed(&pool).await;
    let unknown = Uuid::new_v4();
    let reply = delete_task(&pool, &unknown.to_string().to_uppercase()).await;
    assert_eq!(reply.status.as_u16(), 404, "{}", reply.body);
    assert_eq!(
        reply.json()["error"]["message"],
        not_found_message(unknown).as_str()
    );
}

#[sqlx::test]
async fn malformed_ids_return_400(pool: PgPool) {
    let seeded = seed(&pool).await;
    let all = [seeded.alpha, seeded.beta, seeded.gamma];
    let before = snapshot(&pool, &all).await;
    for id in ["abc".to_owned(), seeded.beta.simple().to_string()] {
        let reply = delete_task(&pool, &id).await;
        assert_uniform_error(&reply, 400, "invalid_id", MSG_INVALID_ID);
    }
    assert_eq!(count_tasks(&pool).await, 3);
    assert_eq!(snapshot(&pool, &all).await, before);
}

#[sqlx::test]
async fn malformed_id_does_not_touch_the_database(
    pool_options: PgPoolOptions,
    connect_options: PgConnectOptions,
) {
    let _ = pool_options;
    let pool = single_connection_pool(connect_options).await;
    let router = test_router(pool, true);
    let holder = {
        let router = router.clone();
        tokio::spawn(async move { send(&router, get("/__test/slow?ms=4000&hold_db=true")).await })
    };
    tokio::time::sleep(Duration::from_millis(400)).await;

    let started = Instant::now();
    let reply = send(&router, delete("/tasks/abc")).await;
    let elapsed = started.elapsed();
    assert_uniform_error(&reply, 400, "invalid_id", MSG_INVALID_ID);
    assert!(elapsed < Duration::from_millis(500), "took {elapsed:?}");
    holder.abort();
}

#[sqlx::test]
async fn unavailable_database_returns_503_and_keeps_the_row(
    pool_options: PgPoolOptions,
    connect_options: PgConnectOptions,
) {
    let _ = pool_options;
    let verify = PgPoolOptions::new()
        .connect_with(connect_options.clone())
        .await
        .unwrap();
    let seeded = seed(&verify).await;
    let all = [seeded.alpha, seeded.beta, seeded.gamma];
    let before = snapshot(&verify, &all).await;

    let pool = single_connection_pool(connect_options).await;
    let router = test_router(pool, true);
    let holder = {
        let router = router.clone();
        tokio::spawn(async move { send(&router, get("/__test/slow?ms=4000&hold_db=true")).await })
    };
    tokio::time::sleep(Duration::from_millis(400)).await;

    let started = Instant::now();
    let reply = send(&router, delete(&format!("/tasks/{}", seeded.beta))).await;
    let elapsed = started.elapsed();
    assert_uniform_error(
        &reply,
        503,
        "service_unavailable",
        "Database temporarily unavailable, please retry",
    );
    assert!(elapsed < Duration::from_millis(2500), "took {elapsed:?}");

    holder.await.unwrap();
    assert_eq!(count_tasks(&verify).await, 3);
    assert_eq!(snapshot(&verify, &all).await, before);
}

#[sqlx::test]
async fn unsupported_method_returns_405(pool: PgPool) {
    let seeded = seed(&pool).await;
    let all = [seeded.alpha, seeded.beta, seeded.gamma];
    let before = snapshot(&pool, &all).await;
    let router = test_router(pool.clone(), false);
    let path = format!("/tasks/{}", seeded.beta);
    let reply = send(
        &router,
        request_with("POST", &path, Some("application/json"), r#"{"title":"x"}"#),
    )
    .await;
    assert_uniform_error(&reply, 405, "method_not_allowed", "Method not allowed");
    assert_eq!(count_tasks(&pool).await, 3);
    assert_eq!(snapshot(&pool, &all).await, before);
}
