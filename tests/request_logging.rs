//! Process-level request logging: completion line shape, levels,
//! correlation and exclusions, against the real binary.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::thread;
use std::time::Duration;

use common::{
    RawReply, ServiceProcess, base_env, completion_lines, database_url_for, free_port, json_lines,
    raw_http, spawn_with,
};
use serde_json::Value;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

const WAIT: Duration = Duration::from_secs(30);

fn start(options: &PgConnectOptions, extra: &[(&str, &str)]) -> (ServiceProcess, u16) {
    let port = free_port();
    let mut env = base_env(port);
    env.push(("DATABASE_URL".to_owned(), database_url_for(options)));
    env.push(("ENABLE_TEST_ROUTES".to_owned(), "true".to_owned()));
    env.push(("LOG_FORMAT".to_owned(), "json".to_owned()));
    for (key, value) in extra {
        env.push(((*key).to_owned(), (*value).to_owned()));
    }
    let service = spawn_with(&env);
    assert!(
        service.wait_for_line(&format!("listening on 127.0.0.1:{port}"), WAIT),
        "{}",
        service.output()
    );
    (service, port)
}

fn request_id(reply: &RawReply) -> String {
    reply.header("x-request-id").unwrap_or_default().to_owned()
}

/// Waits for and returns the single completion line of `reply`.
fn completion(service: &ServiceProcess, reply: &RawReply) -> Value {
    let id = request_id(reply);
    assert!(
        service.wait_for_completion(&id, Duration::from_secs(5)),
        "no completion line for {id}: {}",
        service.output()
    );
    let mut lines = completion_lines(&service.lines(), &id);
    assert_eq!(lines.len(), 1, "{}", service.output());
    lines.remove(0)
}

#[sqlx::test]
async fn completion_line_has_required_fields(_o: PgPoolOptions, options: PgConnectOptions) {
    let (service, port) = start(&options, &[]);
    let reply = raw_http(port, "GET", "/__test/db-ping", &[], "");
    assert_eq!(reply.status, 200);
    let line = completion(&service, &reply);
    assert!(
        line["timestamp"]
            .as_str()
            .is_some_and(|t| t.ends_with('Z') && t.contains('T'))
    );
    assert_eq!(line["level"], "INFO");
    assert!(line["request_id"].is_string());
    assert_eq!(line["method"], "GET");
    assert_eq!(line["path"], "/__test/db-ping");
    assert_eq!(line["status"], 200);
    assert!(line["latency_ms"].is_u64());
}

#[sqlx::test]
async fn completion_line_request_id_matches_header(_o: PgPoolOptions, options: PgConnectOptions) {
    let (service, port) = start(&options, &[]);
    let reply = raw_http(port, "GET", "/__test/db-ping", &[], "");
    assert!(!request_id(&reply).is_empty());
    let line = completion(&service, &reply);
    assert_eq!(line["request_id"], request_id(&reply).as_str());
}

#[sqlx::test]
async fn client_request_id_appears_in_log_line(_o: PgPoolOptions, options: PgConnectOptions) {
    let (service, port) = start(&options, &[]);
    let reply = raw_http(
        port,
        "GET",
        "/__test/db-ping",
        &[("X-Request-Id", "client-abc-123")],
        "",
    );
    assert_eq!(request_id(&reply), "client-abc-123");
    let line = completion(&service, &reply);
    assert_eq!(line["request_id"], "client-abc-123");
}

#[sqlx::test]
async fn unknown_route_is_logged_at_warn(_o: PgPoolOptions, options: PgConnectOptions) {
    let (service, port) = start(&options, &[]);
    let reply = raw_http(port, "GET", "/does-not-exist", &[], "");
    assert_eq!(reply.status, 404);
    let line = completion(&service, &reply);
    assert_eq!(line["level"], "WARN");
    assert_eq!(line["status"], 404);
}

#[sqlx::test]
async fn induced_500_is_logged_at_error(_o: PgPoolOptions, options: PgConnectOptions) {
    let (service, port) = start(&options, &[]);
    let reply = raw_http(port, "GET", "/__test/sql-error", &[], "");
    assert_eq!(reply.status, 500);
    let line = completion(&service, &reply);
    assert_eq!(line["level"], "ERROR");
    assert_eq!(line["status"], 500);
}

#[sqlx::test]
async fn bodies_and_query_strings_are_never_logged(_o: PgPoolOptions, options: PgConnectOptions) {
    let (service, port) = start(&options, &[]);
    let reply = raw_http(
        port,
        "POST",
        "/__test/echo?token=query-marker-91c2",
        &[("Content-Type", "application/json")],
        r#"{"value":"body-marker-7f3a"}"#,
    );
    assert_eq!(reply.status, 200);
    assert!(reply.body.contains("body-marker-7f3a"), "{}", reply.body);
    let line = completion(&service, &reply);
    assert_eq!(line["path"], "/__test/echo");
    thread::sleep(Duration::from_millis(300));
    let output = service.output();
    assert!(!output.contains("body-marker-7f3a"), "{output}");
    assert!(!output.contains("query-marker-91c2"), "{output}");
}

#[sqlx::test]
async fn events_during_a_request_carry_the_request_id(
    _o: PgPoolOptions,
    options: PgConnectOptions,
) {
    let (service, port) = start(&options, &[]);
    let reply = raw_http(port, "GET", "/__test/sql-error", &[], "");
    assert_eq!(reply.status, 500);
    let id = request_id(&reply);
    completion(&service, &reply);
    let carried = json_lines(&service.lines())
        .into_iter()
        .any(|line| line.get("latency_ms").is_none() && line["span"]["request_id"] == id.as_str());
    assert!(carried, "{}", service.output());
}

#[sqlx::test]
async fn log_level_warn_suppresses_info_lines(_o: PgPoolOptions, options: PgConnectOptions) {
    let (service, port) = start_warn(&options);
    let reply = raw_http(port, "GET", "/__test/db-ping", &[], "");
    assert_eq!(reply.status, 200);
    assert!(!request_id(&reply).is_empty());
    thread::sleep(Duration::from_secs(1));
    let has_info = json_lines(&service.lines())
        .iter()
        .any(|line| line["level"] == "INFO");
    assert!(!has_info, "{}", service.output());
}

#[sqlx::test]
async fn log_level_warn_still_logs_warn_completions(_o: PgPoolOptions, options: PgConnectOptions) {
    let (service, port) = start_warn(&options);
    let reply = raw_http(port, "GET", "/does-not-exist", &[], "");
    assert_eq!(reply.status, 404);
    let line = completion(&service, &reply);
    assert_eq!(line["level"], "WARN");
}

/// With `LOG_LEVEL=warn` the `listening on` line is suppressed, so readiness
/// is detected by polling the port.
fn start_warn(options: &PgConnectOptions) -> (ServiceProcess, u16) {
    let port = free_port();
    let mut env = base_env(port);
    env.push(("DATABASE_URL".to_owned(), database_url_for(options)));
    env.push(("ENABLE_TEST_ROUTES".to_owned(), "true".to_owned()));
    env.push(("LOG_FORMAT".to_owned(), "json".to_owned()));
    env.push(("LOG_LEVEL".to_owned(), "warn".to_owned()));
    let service = spawn_with(&env);
    let deadline = std::time::Instant::now() + WAIT;
    while !common::accepts_connections(port) {
        assert!(std::time::Instant::now() < deadline, "{}", service.output());
        thread::sleep(Duration::from_millis(50));
    }
    (service, port)
}
