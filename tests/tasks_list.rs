//! `GET /tasks` through the real router, in-process.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::{Duration, Instant};

use axum::Router;
use common::{assert_uniform_error, count_tasks, get, post_json, request_with, send, test_router};
use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use uuid::Uuid;

const EMPTY_PAGE: &str = r#"{"items":[],"total":0,"limit":20,"offset":0}"#;
const STATUS_MSG: &str = "must be one of: todo, in_progress, done";
const PRIORITY_MSG: &str = "must be one of: low, medium, high";
const SORT_MSG: &str = "must be one of: created_at, updated_at, due_date, priority";
const ORDER_MSG: &str = "must be one of: asc, desc";
const LIMIT_MSG: &str = "must be an integer between 1 and 100";
const OFFSET_MSG: &str = "must be an integer between 0 and 10000";

/// The 9 `mixed-tasks` rows: title, status, priority, due date.
const MIXED: [(&str, &str, &str, Option<&str>); 9] = [
    ("T-todo-low", "todo", "low", Some("2026-12-05")),
    ("T-todo-medium", "todo", "medium", None),
    ("T-todo-high", "todo", "high", Some("2026-12-01")),
    ("T-prog-low", "in_progress", "low", None),
    ("T-prog-medium", "in_progress", "medium", Some("2026-12-03")),
    ("T-prog-high", "in_progress", "high", Some("2026-12-07")),
    ("T-done-low", "done", "low", Some("2026-12-02")),
    ("T-done-medium", "done", "medium", Some("2026-12-06")),
    ("T-done-high", "done", "high", None),
];

async fn create(router: &Router, body: Value) -> Value {
    let reply = send(router, post_json("/tasks", &body.to_string())).await;
    assert_eq!(reply.status.as_u16(), 201, "body: {}", reply.body);
    reply.json()
}

/// `twenty-five-tasks`: `Task 01` … `Task 25`, created one at a time.
async fn seed_twenty_five(router: &Router) {
    for n in 1..=25 {
        create(router, json!({ "title": format!("Task {n:02}") })).await;
    }
}

/// `mixed-tasks`, created one at a time in table order.
async fn seed_mixed(router: &Router) {
    for (title, status, priority, due_date) in MIXED {
        create(
            router,
            json!({
                "title": title,
                "status": status,
                "priority": priority,
                "due_date": due_date,
            }),
        )
        .await;
    }
}

/// `tied-tasks`: 20 rows inserted by one statement, so they share
/// `created_at` / `updated_at`. Returns their ids in ascending order.
async fn seed_tied(pool: &PgPool) -> Vec<Uuid> {
    let mut ids = sqlx::query_scalar!(
        "INSERT INTO tasks (title, status, priority, due_date) \
         SELECT 'Tied ' || g, 'todo', 'medium', NULL \
         FROM generate_series(1, 20) AS g \
         RETURNING id"
    )
    .fetch_all(pool)
    .await
    .unwrap();
    ids.sort();
    ids
}

/// `seeded-10k-db`: statuses and priorities spread across every value,
/// about one third of due dates NULL.
async fn seed_ten_thousand(pool: &PgPool) {
    sqlx::query!(
        "INSERT INTO tasks (title, status, priority, due_date) \
         SELECT 'Task ' || g, \
                (ARRAY['todo', 'in_progress', 'done'])[g % 3 + 1], \
                (ARRAY['low', 'medium', 'high'])[(g / 3) % 3 + 1], \
                CASE WHEN (g / 9) % 3 = 0 THEN NULL \
                     ELSE DATE '2026-01-01' + (g % 365) END \
         FROM generate_series(1, 10000) AS g"
    )
    .execute(pool)
    .await
    .unwrap();
}

async fn list(router: &Router, query: &str) -> Value {
    let reply = send(router, get(&format!("/tasks{query}"))).await;
    assert_eq!(reply.status.as_u16(), 200, "body: {}", reply.body);
    reply.json()
}

fn items(body: &Value) -> &Vec<Value> {
    body["items"].as_array().unwrap()
}

fn titles(body: &Value) -> Vec<String> {
    items(body)
        .iter()
        .map(|item| item["title"].as_str().unwrap().to_owned())
        .collect()
}

fn ids(body: &Value) -> Vec<Uuid> {
    items(body)
        .iter()
        .map(|item| item["id"].as_str().unwrap().parse().unwrap())
        .collect()
}

fn field_values(body: &Value, field: &str) -> Vec<String> {
    items(body)
        .iter()
        .map(|item| item[field].as_str().unwrap_or("null").to_owned())
        .collect()
}

fn invalid_query_body(entries: &[(&str, &str)]) -> String {
    let details: Vec<Value> = entries
        .iter()
        .map(|(field, message)| json!({ "field": field, "message": message }))
        .collect();
    format!(
        r#"{{"error":{{"code":"invalid_query","message":"Query parameters are invalid","details":{}}}}}"#,
        Value::Array(details)
    )
}

async fn assert_invalid_query(router: &Router, query: &str, entries: &[(&str, &str)]) {
    let reply = send(router, get(&format!("/tasks{query}"))).await;
    assert_eq!(reply.status.as_u16(), 400, "{query}: {}", reply.body);
    assert_eq!(
        reply
            .headers
            .get("content-type")
            .map(|v| v.to_str().unwrap()),
        Some("application/json")
    );
    assert_eq!(reply.body, invalid_query_body(entries), "{query}");
}

// --- Paginate -------------------------------------------------------------

#[sqlx::test]
async fn empty_database_returns_empty_page(pool: PgPool) {
    let router = test_router(pool, false);
    let reply = send(&router, get("/tasks")).await;
    assert_eq!(reply.status.as_u16(), 200);
    assert_eq!(
        reply
            .headers
            .get("content-type")
            .map(|v| v.to_str().unwrap()),
        Some("application/json")
    );
    assert_eq!(reply.body, EMPTY_PAGE);
}

#[sqlx::test]
async fn default_page_is_20_newest_first(pool: PgPool) {
    let router = test_router(pool, false);
    seed_twenty_five(&router).await;
    let body = list(&router, "").await;
    let expected: Vec<String> = (6..=25).rev().map(|n| format!("Task {n:02}")).collect();
    assert_eq!(titles(&body), expected);
    assert_eq!(body["total"], 25);
    assert_eq!(body["limit"], 20);
    assert_eq!(body["offset"], 0);
}

#[sqlx::test]
async fn offset_returns_the_remaining_tasks(pool: PgPool) {
    let router = test_router(pool, false);
    seed_twenty_five(&router).await;
    let body = list(&router, "?offset=20").await;
    assert_eq!(
        titles(&body),
        ["Task 05", "Task 04", "Task 03", "Task 02", "Task 01"]
    );
    assert_eq!(body["total"], 25);
    assert_eq!(body["offset"], 20);
}

#[sqlx::test]
async fn offset_beyond_total_returns_no_items(pool: PgPool) {
    let router = test_router(pool, false);
    seed_twenty_five(&router).await;
    let body = list(&router, "?offset=1000").await;
    assert_eq!(body["items"], json!([]));
    assert_eq!(body["total"], 25);
    assert_eq!(body["offset"], 1000);
}

#[sqlx::test]
async fn limit_bounds_are_accepted(pool: PgPool) {
    let router = test_router(pool, false);
    seed_twenty_five(&router).await;

    let body = list(&router, "?limit=1").await;
    assert_eq!(titles(&body), ["Task 25"]);
    assert_eq!(body["limit"], 1);
    assert_eq!(body["total"], 25);

    let reply = send(&router, get("/tasks?limit=100&offset=10000")).await;
    assert_eq!(reply.status.as_u16(), 200);
    assert_eq!(
        reply.body,
        r#"{"items":[],"total":25,"limit":100,"offset":10000}"#
    );
}

#[sqlx::test]
async fn paging_with_limit_7_visits_every_task_once(pool: PgPool) {
    let tied = seed_tied(&pool).await;
    let router = test_router(pool, false);
    let mut seen = Vec::new();
    for (offset, size) in [(0, 7), (7, 7), (14, 6)] {
        let body = list(&router, &format!("?limit=7&offset={offset}")).await;
        assert_eq!(body["total"], 20);
        assert_eq!(items(&body).len(), size);
        seen.extend(ids(&body));
    }
    assert_eq!(seen, tied, "every id once, ascending");
}

#[sqlx::test]
async fn items_match_create_response(pool: PgPool) {
    let router = test_router(pool, false);
    let created = create(
        &router,
        json!({"title":"Write PRD","priority":"high","due_date":"2026-12-01"}),
    )
    .await;
    let body = list(&router, "").await;
    assert_eq!(body["items"], json!([created]));
    assert_eq!(body["total"], 1);
}

// --- Filter ---------------------------------------------------------------

#[sqlx::test]
async fn status_filter_is_or_within_parameter(pool: PgPool) {
    let router = test_router(pool, false);
    seed_mixed(&router).await;
    let body = list(&router, "?status=todo,in_progress").await;
    let statuses = field_values(&body, "status");
    assert_eq!(statuses.len(), 6);
    assert!(statuses.iter().all(|s| s == "todo" || s == "in_progress"));
    assert_eq!(body["total"], 6);

    let body = list(&router, "?status=done,done").await;
    let statuses = field_values(&body, "status");
    assert_eq!(statuses.len(), 3);
    assert!(statuses.iter().all(|s| s == "done"));
    assert_eq!(body["total"], 3);
}

#[sqlx::test]
async fn status_and_priority_combine_with_and(pool: PgPool) {
    let router = test_router(pool, false);
    seed_mixed(&router).await;
    let body = list(&router, "?status=todo&priority=high").await;
    assert_eq!(titles(&body), ["T-todo-high"]);
    assert_eq!(body["total"], 1);
}

#[sqlx::test]
async fn priority_filter_is_or_within_parameter(pool: PgPool) {
    let router = test_router(pool, false);
    seed_mixed(&router).await;
    let body = list(&router, "?priority=low,high").await;
    let priorities = field_values(&body, "priority");
    assert_eq!(priorities.len(), 6);
    assert!(priorities.iter().all(|p| p == "low" || p == "high"));
    assert_eq!(body["total"], 6);
}

#[sqlx::test]
async fn total_ignores_limit(pool: PgPool) {
    let router = test_router(pool, false);
    seed_mixed(&router).await;
    let body = list(&router, "?status=todo,in_progress&limit=2").await;
    assert_eq!(items(&body).len(), 2);
    assert_eq!(body["total"], 6);
}

#[sqlx::test]
async fn combined_filter_sort_example(pool: PgPool) {
    let router = test_router(pool, false);
    seed_mixed(&router).await;
    let body = list(
        &router,
        "?status=todo,in_progress&priority=high&sort=due_date&order=asc&limit=50",
    )
    .await;
    assert_eq!(titles(&body), ["T-todo-high", "T-prog-high"]);
    assert_eq!(body["total"], 2);
    assert_eq!(body["limit"], 50);
}

// --- Sort -----------------------------------------------------------------

#[sqlx::test]
async fn priority_sort_is_semantic(pool: PgPool) {
    let router = test_router(pool, false);
    seed_mixed(&router).await;
    let desc = field_values(
        &list(&router, "?sort=priority&order=desc").await,
        "priority",
    );
    assert_eq!(
        desc,
        [
            "high", "high", "high", "medium", "medium", "medium", "low", "low", "low"
        ]
    );
    let asc = field_values(&list(&router, "?sort=priority&order=asc").await, "priority");
    assert_eq!(
        asc,
        [
            "low", "low", "low", "medium", "medium", "medium", "high", "high", "high"
        ]
    );
}

#[sqlx::test]
async fn due_date_sort_puts_nulls_last(pool: PgPool) {
    let router = test_router(pool, false);
    seed_mixed(&router).await;

    for (order, dated) in [
        (
            "asc",
            [
                "T-todo-high",
                "T-done-low",
                "T-prog-medium",
                "T-todo-low",
                "T-done-medium",
                "T-prog-high",
            ],
        ),
        (
            "desc",
            [
                "T-prog-high",
                "T-done-medium",
                "T-todo-low",
                "T-prog-medium",
                "T-done-low",
                "T-todo-high",
            ],
        ),
    ] {
        let body = list(&router, &format!("?sort=due_date&order={order}")).await;
        assert_eq!(titles(&body)[..6], dated, "order={order}");
        let due_dates = field_values(&body, "due_date");
        assert!(due_dates[6..].iter().all(|d| d == "null"), "order={order}");
        let undated = &ids(&body)[6..];
        let mut sorted = undated.to_vec();
        sorted.sort();
        assert_eq!(undated, sorted.as_slice(), "order={order}");
    }
}

#[sqlx::test]
async fn created_and_updated_sorts(pool: PgPool) {
    let router = test_router(pool, false);
    seed_mixed(&router).await;
    let in_order: Vec<String> = MIXED.iter().map(|(t, ..)| (*t).to_owned()).collect();
    let mut reversed = in_order.clone();
    reversed.reverse();

    assert_eq!(titles(&list(&router, "?order=asc").await), in_order);
    assert_eq!(
        titles(&list(&router, "?sort=updated_at&order=desc").await),
        reversed
    );
}

#[sqlx::test]
async fn ties_are_broken_by_id(pool: PgPool) {
    let tied = seed_tied(&pool).await;
    let router = test_router(pool, false);
    let body = list(&router, "?sort=priority&order=desc&limit=100").await;
    assert_eq!(items(&body).len(), 20);
    assert_eq!(ids(&body), tied);
}

// --- Query validation -----------------------------------------------------

#[sqlx::test]
async fn each_invalid_parameter_returns_400(pool: PgPool) {
    let router = test_router(pool, false);
    for (query, field, message) in [
        ("?limit=0", "limit", LIMIT_MSG),
        ("?limit=101", "limit", LIMIT_MSG),
        ("?offset=-1", "offset", OFFSET_MSG),
        ("?offset=10001", "offset", OFFSET_MSG),
        ("?status=closed", "status", STATUS_MSG),
        ("?sort=title", "sort", SORT_MSG),
        ("?order=up", "order", ORDER_MSG),
        ("?priority=urgent", "priority", PRIORITY_MSG),
        ("?limit=abc", "limit", LIMIT_MSG),
    ] {
        assert_invalid_query(&router, query, &[(field, message)]).await;
    }
}

#[sqlx::test]
async fn two_invalid_parameters_return_two_details(pool: PgPool) {
    let router = test_router(pool, false);
    assert_invalid_query(
        &router,
        "?status=closed&limit=500",
        &[("status", STATUS_MSG), ("limit", LIMIT_MSG)],
    )
    .await;
}

#[sqlx::test]
async fn all_invalid_parameters_in_fixed_order(pool: PgPool) {
    let router = test_router(pool, false);
    assert_invalid_query(
        &router,
        "?offset=x&limit=x&order=x&sort=x&priority=x&status=x",
        &[
            ("status", STATUS_MSG),
            ("priority", PRIORITY_MSG),
            ("sort", SORT_MSG),
            ("order", ORDER_MSG),
            ("limit", LIMIT_MSG),
            ("offset", OFFSET_MSG),
        ],
    )
    .await;
}

#[sqlx::test]
async fn repeated_and_empty_parameters_are_rejected(pool: PgPool) {
    let router = test_router(pool, false);
    assert_invalid_query(
        &router,
        "?status=todo&status=done",
        &[("status", "must not be repeated")],
    )
    .await;
    assert_invalid_query(
        &router,
        "?status=&limit=",
        &[("status", STATUS_MSG), ("limit", LIMIT_MSG)],
    )
    .await;
    assert_invalid_query(&router, "?status=todo,,done", &[("status", STATUS_MSG)]).await;
    assert_invalid_query(
        &router,
        "?status=TODO&order=DESC",
        &[("status", STATUS_MSG), ("order", ORDER_MSG)],
    )
    .await;
}

#[sqlx::test]
async fn unknown_parameters_are_ignored(pool: PgPool) {
    let router = test_router(pool, false);
    let reply = send(&router, get("/tasks?foo=bar&Status=closed")).await;
    assert_eq!(reply.status.as_u16(), 200);
    assert_eq!(reply.body, EMPTY_PAGE);
}

// --- Request rejection ----------------------------------------------------

#[sqlx::test]
async fn unavailable_database_returns_503(
    pool_options: PgPoolOptions,
    connect_options: PgConnectOptions,
) {
    let _ = pool_options;
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

    let started = Instant::now();
    let reply = send(&router, get("/tasks")).await;
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
    let router = test_router(pool.clone(), false);
    let reply = send(
        &router,
        request_with(
            "PUT",
            "/tasks",
            Some("application/json"),
            r#"{"title":"x"}"#,
        ),
    )
    .await;
    assert_uniform_error(&reply, 405, "method_not_allowed", "Method not allowed");
    assert_eq!(count_tasks(&pool).await, 0);
}

// --- Performance ----------------------------------------------------------

#[sqlx::test]
async fn list_is_fast_with_10000_tasks(pool: PgPool) {
    seed_ten_thousand(&pool).await;
    assert_eq!(count_tasks(&pool).await, 10_000);
    let router = test_router(pool, false);

    let mut timings = Vec::with_capacity(1000);
    for _ in 0..1000 {
        let started = Instant::now();
        let reply = send(&router, get("/tasks?status=todo&sort=due_date&limit=100")).await;
        timings.push(started.elapsed());
        assert_eq!(reply.status.as_u16(), 200, "body: {}", reply.body);
        assert_eq!(items(&reply.json()).len(), 100);
    }
    timings.sort();
    let p95 = timings[949];
    assert!(p95 < Duration::from_millis(100), "p95 was {p95:?}");
}
