//! F10 schema-versus-behavior conformance: what `/openapi.json` documents
//! is what the live endpoints (F03-F08) accept and return.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::BTreeSet;
use std::time::Duration;

use axum::Router;
use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

use common::{
    Reply, assert_valid, count_tasks, get, openapi_spec, post_json, request_with, send,
    stored_task, test_router,
};

fn documented(spec: &Value, method: &str, path: &str, status: u16) -> bool {
    spec["paths"][path][method]["responses"]
        .get(status.to_string())
        .is_some()
}

fn assert_documented(spec: &Value, method: &str, path: &str, reply: &Reply) {
    let status = reply.status.as_u16();
    assert!(
        documented(spec, method, path, status),
        "{method} {path} returned undocumented {status}: {}",
        reply.body
    );
}

fn keys(value: &Value) -> BTreeSet<String> {
    value.as_object().unwrap().keys().cloned().collect()
}

fn component_properties(spec: &Value, name: &str) -> BTreeSet<String> {
    keys(&spec["components"]["schemas"][name]["properties"])
}

fn query_param_schema<'a>(spec: &'a Value, name: &str) -> &'a Value {
    &spec["paths"]["/tasks"]["get"]["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == name)
        .unwrap()["schema"]
}

fn patch_json(path: &str, body: &str) -> axum::http::Request<axum::body::Body> {
    request_with("PATCH", path, Some("application/json"), body)
}

fn delete(path: &str) -> axum::http::Request<axum::body::Body> {
    request_with("DELETE", path, None, "")
}

/// Creates the `seed-task` row through the API and returns its id.
async fn seed_task(router: &Router) -> String {
    let reply = send(
        router,
        post_json(
            "/tasks",
            r#"{"title":"Seed task","description":"Has a description","status":"todo","priority":"low","due_date":"2026-12-01"}"#,
        ),
    )
    .await;
    assert_eq!(reply.status.as_u16(), 201, "body: {}", reply.body);
    reply.json()["id"].as_str().unwrap().to_owned()
}

fn random_uuid() -> String {
    uuid::Uuid::new_v4().hyphenated().to_string()
}

#[sqlx::test]
async fn every_documented_operation_is_routed(pool: PgPool) {
    let router = test_router(pool, false);
    let spec = openapi_spec(&router).await;
    let seed = seed_task(&router).await;
    let id_path = format!("/tasks/{seed}");
    let requests = [
        ("get", "/health", get("/health")),
        (
            "post",
            "/tasks",
            post_json("/tasks", r#"{"title":"Routed"}"#),
        ),
        ("get", "/tasks", get("/tasks")),
        ("get", "/tasks/{id}", get(&id_path)),
        (
            "patch",
            "/tasks/{id}",
            patch_json(&id_path, r#"{"status":"done"}"#),
        ),
        ("delete", "/tasks/{id}", delete(&id_path)),
    ];
    for (method, path, request) in requests {
        let reply = send(&router, request).await;
        assert_ne!(reply.status.as_u16(), 405, "{method} {path}");
        if reply.status.as_u16() == 404 {
            assert_ne!(
                reply.json()["error"]["message"],
                "Route not found",
                "{method} {path}"
            );
        }
        assert_documented(&spec, method, path, &reply);
    }
}

#[sqlx::test]
async fn health_200_matches_health_response(pool: PgPool) {
    let router = test_router(pool, false);
    let spec = openapi_spec(&router).await;
    let reply = send(&router, get("/health")).await;
    assert_eq!(reply.status.as_u16(), 200, "body: {}", reply.body);
    let body = reply.json();
    assert_eq!(keys(&body), component_properties(&spec, "HealthResponse"));
    assert_valid(&spec, "HealthResponse", &body);
}

#[sqlx::test]
async fn health_503_matches_health_response(
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
    let spec = openapi_spec(&router).await;

    let holder_router = router.clone();
    let holder = tokio::spawn(async move {
        send(&holder_router, get("/__test/slow?ms=4000&hold_db=true")).await
    });
    tokio::time::sleep(Duration::from_millis(500)).await;

    let reply = send(&router, get("/health")).await;
    assert_eq!(reply.status.as_u16(), 503, "body: {}", reply.body);
    assert_valid(&spec, "HealthResponse", &reply.json());
    assert_documented(&spec, "get", "/health", &reply);
    holder.abort();
}

#[sqlx::test]
async fn post_example_from_spec_returns_201(pool: PgPool) {
    let router = test_router(pool.clone(), false);
    let spec = openapi_spec(&router).await;
    let example =
        &spec["paths"]["/tasks"]["post"]["requestBody"]["content"]["application/json"]["example"];
    let reply = send(&router, post_json("/tasks", &example.to_string())).await;
    assert_eq!(reply.status.as_u16(), 201, "body: {}", reply.body);
    let body = reply.json();
    let location = reply.headers["location"].to_str().unwrap();
    assert_eq!(location, format!("/tasks/{}", body["id"].as_str().unwrap()));
    assert_eq!(body["title"], "Write PRD");
    assert_eq!(body["priority"], "high");
    assert_eq!(body["due_date"], "2026-12-01");
    assert_eq!(body["status"], "todo");
    assert_eq!(count_tasks(&pool).await, 1);
}

#[sqlx::test]
async fn task_response_matches_task_schema_and_defaults(pool: PgPool) {
    let router = test_router(pool, false);
    let spec = openapi_spec(&router).await;
    let reply = send(&router, post_json("/tasks", r#"{"title":"Write PRD"}"#)).await;
    assert_eq!(reply.status.as_u16(), 201, "body: {}", reply.body);
    let body = reply.json();
    assert_eq!(keys(&body), component_properties(&spec, "Task"));
    assert_valid(&spec, "Task", &body);
    let create = &spec["components"]["schemas"]["CreateTaskRequest"]["properties"];
    assert_eq!(body["status"], create["status"]["default"]);
    assert_eq!(body["priority"], create["priority"]["default"]);
}

#[sqlx::test]
async fn documented_boundaries_are_accepted(pool: PgPool) {
    let router = test_router(pool, false);
    let spec = openapi_spec(&router).await;
    let body = json!({
        "title": "a".repeat(200),
        "description": "d".repeat(5000),
        "status": "in_progress",
        "priority": "low",
        "due_date": "2024-02-29"
    });
    let reply = send(&router, post_json("/tasks", &body.to_string())).await;
    assert_eq!(reply.status.as_u16(), 201, "body: {}", reply.body);
    let task = reply.json();
    assert_valid(&spec, "Task", &task);
    assert_eq!(task["title"].as_str().unwrap().chars().count(), 200);
    assert_eq!(task["description"].as_str().unwrap().chars().count(), 5000);
}

#[sqlx::test]
async fn values_outside_documented_constraints_are_rejected(pool: PgPool) {
    let router = test_router(pool.clone(), false);
    let spec = openapi_spec(&router).await;
    let body = json!({
        "title": "a".repeat(201),
        "description": "d".repeat(5001),
        "status": "DONE",
        "priority": "urgent",
        "due_date": "2026-02-30"
    });
    let reply = send(&router, post_json("/tasks", &body.to_string())).await;
    assert_eq!(reply.status.as_u16(), 422, "body: {}", reply.body);
    let error = reply.json();
    let fields: Vec<&str> = error["error"]["details"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["field"].as_str().unwrap())
        .collect();
    assert_eq!(
        fields,
        ["title", "description", "status", "priority", "due_date"]
    );
    assert_valid(&spec, "ErrorResponse", &error);
    assert_documented(&spec, "post", "/tasks", &reply);
    assert_eq!(count_tasks(&pool).await, 0);
}

#[sqlx::test]
async fn get_and_delete_statuses_are_documented(pool: PgPool) {
    let router = test_router(pool, false);
    let spec = openapi_spec(&router).await;
    let seed = seed_task(&router).await;

    let found = send(&router, get(&format!("/tasks/{seed}"))).await;
    assert_eq!(found.status.as_u16(), 200, "body: {}", found.body);
    assert_valid(&spec, "Task", &found.json());
    assert_documented(&spec, "get", "/tasks/{id}", &found);

    for (method, request_for) in [
        (
            "get",
            get as fn(&str) -> axum::http::Request<axum::body::Body>,
        ),
        ("delete", delete),
    ] {
        let malformed = send(&router, request_for("/tasks/abc")).await;
        assert_eq!(
            malformed.status.as_u16(),
            400,
            "{method}: {}",
            malformed.body
        );
        assert_eq!(malformed.json()["error"]["code"], "invalid_id");
        assert_valid(&spec, "ErrorResponse", &malformed.json());
        assert_documented(&spec, method, "/tasks/{id}", &malformed);

        let missing = send(&router, request_for(&format!("/tasks/{}", random_uuid()))).await;
        assert_eq!(missing.status.as_u16(), 404, "{method}: {}", missing.body);
        assert_eq!(missing.json()["error"]["code"], "not_found");
        assert_valid(&spec, "ErrorResponse", &missing.json());
        assert_documented(&spec, method, "/tasks/{id}", &missing);
    }

    let deleted = send(&router, delete(&format!("/tasks/{seed}"))).await;
    assert_eq!(deleted.status.as_u16(), 204);
    assert!(deleted.body.is_empty());
    assert_documented(&spec, "delete", "/tasks/{id}", &deleted);
}

#[sqlx::test]
async fn list_defaults_match_documented_defaults(pool: PgPool) {
    let router = test_router(pool, false);
    let spec = openapi_spec(&router).await;
    let reply = send(&router, get("/tasks")).await;
    assert_eq!(reply.status.as_u16(), 200, "body: {}", reply.body);
    let page = reply.json();
    assert_valid(&spec, "TaskPage", &page);
    assert_eq!(page["limit"], query_param_schema(&spec, "limit")["default"]);
    assert_eq!(
        page["offset"],
        query_param_schema(&spec, "offset")["default"]
    );
}

#[sqlx::test]
async fn documented_sort_and_order_values_are_accepted(pool: PgPool) {
    let router = test_router(pool, false);
    let spec = openapi_spec(&router).await;
    seed_task(&router).await;
    let sorts = query_param_schema(&spec, "sort")["enum"]
        .as_array()
        .unwrap()
        .clone();
    let orders = query_param_schema(&spec, "order")["enum"]
        .as_array()
        .unwrap()
        .clone();
    let mut uris = Vec::new();
    for sort in &sorts {
        for order in &orders {
            uris.push(format!(
                "/tasks?sort={}&order={}",
                sort.as_str().unwrap(),
                order.as_str().unwrap()
            ));
        }
    }
    uris.push(format!(
        "/tasks?limit={}&offset={}",
        query_param_schema(&spec, "limit")["maximum"],
        query_param_schema(&spec, "offset")["maximum"]
    ));
    for uri in uris {
        let reply = send(&router, get(&uri)).await;
        assert_eq!(reply.status.as_u16(), 200, "{uri}: {}", reply.body);
        assert_valid(&spec, "TaskPage", &reply.json());
    }
}

#[sqlx::test]
async fn undocumented_values_are_rejected(pool: PgPool) {
    let router = test_router(pool, false);
    let spec = openapi_spec(&router).await;
    let reply = send(
        &router,
        get("/tasks?status=closed&priority=urgent&sort=title&order=up&limit=101&offset=10001"),
    )
    .await;
    assert_eq!(reply.status.as_u16(), 400, "body: {}", reply.body);
    let error = reply.json();
    assert_eq!(error["error"]["code"], "invalid_query");
    let fields: Vec<&str> = error["error"]["details"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["field"].as_str().unwrap())
        .collect();
    assert_eq!(
        fields,
        ["status", "priority", "sort", "order", "limit", "offset"]
    );
    assert_valid(&spec, "ErrorResponse", &error);
    assert_documented(&spec, "get", "/tasks", &reply);
}

#[sqlx::test]
async fn comma_separated_filters_work_as_documented(pool: PgPool) {
    let router = test_router(pool, false);
    let seed = seed_task(&router).await;
    let reply = send(
        &router,
        get("/tasks?status=todo,in_progress&priority=low,high"),
    )
    .await;
    assert_eq!(reply.status.as_u16(), 200, "body: {}", reply.body);
    let page = reply.json();
    assert_eq!(page["total"], 1);
    assert_eq!(page["items"][0]["id"], seed.as_str());
}

#[sqlx::test]
async fn patch_nullable_fields_clear(pool: PgPool) {
    let router = test_router(pool, false);
    let spec = openapi_spec(&router).await;
    let seed = seed_task(&router).await;
    let reply = send(
        &router,
        patch_json(
            &format!("/tasks/{seed}"),
            r#"{"description":null,"due_date":null}"#,
        ),
    )
    .await;
    assert_eq!(reply.status.as_u16(), 200, "body: {}", reply.body);
    let task = reply.json();
    assert_eq!(task["description"], Value::Null);
    assert_eq!(task["due_date"], Value::Null);
    assert_valid(&spec, "Task", &task);
}

#[sqlx::test]
async fn patch_non_nullable_fields_reject_null(pool: PgPool) {
    let router = test_router(pool.clone(), false);
    let seed = seed_task(&router).await;
    let id = uuid::Uuid::parse_str(&seed).unwrap();
    let before = format!("{:?}", stored_task(&pool, id).await.unwrap());
    let reply = send(
        &router,
        patch_json(
            &format!("/tasks/{seed}"),
            r#"{"title":null,"status":null,"priority":null}"#,
        ),
    )
    .await;
    assert_eq!(reply.status.as_u16(), 422, "body: {}", reply.body);
    assert_eq!(
        reply.json()["error"]["details"],
        json!([
            {"field": "title", "message": "cannot be null"},
            {"field": "status", "message": "cannot be null"},
            {"field": "priority", "message": "cannot be null"}
        ])
    );
    let after = format!("{:?}", stored_task(&pool, id).await.unwrap());
    assert_eq!(after, before);
}
