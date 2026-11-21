//! Process-level logging configuration: invalid values and rendering.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::Duration;

use common::{
    ServiceProcess, accepts_connections, base_env, database_url_for, free_port, spawn_with,
};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

fn env_for(options: &PgConnectOptions, port: u16, extra: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut env = base_env(port);
    env.push(("DATABASE_URL".to_owned(), database_url_for(options)));
    for (key, value) in extra {
        env.push(((*key).to_owned(), (*value).to_owned()));
    }
    env
}

async fn assert_rejected(options: PgConnectOptions, variable: &str, value: &str) {
    let env = env_for(&options, free_port(), &[(variable, value)]);
    let mut service: ServiceProcess = spawn_with(&env);
    let status = service
        .wait_exit(Duration::from_secs(5))
        .expect("exits within 5 s");
    assert_eq!(status.code(), Some(1));
    let output = service.output();
    assert!(
        output
            .lines()
            .any(|line| line.contains("configuration error") && line.contains(variable)),
        "{output}"
    );
    assert!(!output.contains("connecting to database"), "{output}");
}

#[sqlx::test]
async fn invalid_log_format_exits_1(_o: PgPoolOptions, options: PgConnectOptions) {
    assert_rejected(options, "LOG_FORMAT", "xml").await;
}

#[sqlx::test]
async fn invalid_log_level_exits_1(_o: PgPoolOptions, options: PgConnectOptions) {
    assert_rejected(options, "LOG_LEVEL", "info,sqlx=loud").await;
}

#[sqlx::test]
async fn pretty_format_is_not_json(_o: PgPoolOptions, options: PgConnectOptions) {
    let port = free_port();
    let service = spawn_with(&env_for(&options, port, &[("LOG_FORMAT", "pretty")]));
    let needle = format!("listening on 127.0.0.1:{port}");
    assert!(
        service.wait_for_line(&needle, Duration::from_secs(30)),
        "{}",
        service.output()
    );
    let line = service
        .lines()
        .into_iter()
        .find(|line| line.contains(&needle))
        .unwrap();
    assert!(
        serde_json::from_str::<serde_json::Value>(&line).is_err(),
        "{line}"
    );
    assert!(accepts_connections(port));
}

#[sqlx::test]
async fn default_format_is_json(_o: PgPoolOptions, options: PgConnectOptions) {
    let port = free_port();
    let service = spawn_with(&env_for(&options, port, &[]));
    let needle = format!("listening on 127.0.0.1:{port}");
    assert!(
        service.wait_for_line(&needle, Duration::from_secs(30)),
        "{}",
        service.output()
    );
    let line = service
        .lines()
        .into_iter()
        .find(|line| line.contains(&needle))
        .unwrap();
    let json: serde_json::Value = serde_json::from_str(&line).expect("JSON line");
    assert!(json["timestamp"].as_str().is_some_and(|s| !s.is_empty()));
    assert_eq!(json["level"], "INFO");
    assert!(json["message"].as_str().unwrap().contains(&needle));
}
