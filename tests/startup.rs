//! Process-level startup behavior: configuration rejection, connection
//! retries, migrations, listening address and clean shutdown of an idle
//! server. Each test spawns the compiled binary with a controlled
//! environment and a working directory that contains no `.env` file.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::{Duration, Instant};

use common::{
    ServiceProcess, accepts_connections, base_env, database_url_for, free_port, spawn_with,
};
use nix::sys::signal::Signal;
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);

fn env_with_url(url: &str, port: u16, extra: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut env = base_env(port);
    env.push(("DATABASE_URL".to_owned(), url.to_owned()));
    for (key, value) in extra {
        env.retain(|(existing, _)| existing != key);
        env.push(((*key).to_owned(), (*value).to_owned()));
    }
    env
}

fn count_lines(service: &ServiceProcess, needle: &str) -> usize {
    service
        .lines()
        .iter()
        .filter(|line| line.contains(needle))
        .count()
}

fn line_index(service: &ServiceProcess, needle: &str) -> Option<usize> {
    service
        .lines()
        .iter()
        .position(|line| line.contains(needle))
}

// ---- configuration -------------------------------------------------------

#[test]
fn missing_database_url_exits_1() {
    let mut service = ServiceProcess::spawn(&[]);
    let status = service
        .wait_exit(Duration::from_secs(5))
        .expect("exits within 5 s");
    assert_eq!(status.code(), Some(1));
    let output = service.output();
    assert!(
        output.contains("configuration error: DATABASE_URL is required"),
        "{output}"
    );
    assert!(!output.contains("connecting to database"), "{output}");
}

async fn assert_config_rejected(connect_options: PgConnectOptions, variable: &str, value: &str) {
    let url = database_url_for(&connect_options);
    let port = free_port();
    let env = env_with_url(&url, port, &[(variable, value)]);
    let mut service = spawn_with(&env);
    let status = service.wait_exit(Duration::from_secs(10)).expect("exits");
    assert_eq!(status.code(), Some(1), "{}", service.output());
    let output = service.output();
    let named = output
        .lines()
        .any(|line| line.contains("configuration error") && line.contains(variable));
    assert!(
        named,
        "expected a configuration error naming {variable}: {output}"
    );
    assert!(!output.contains("connecting to database"), "{output}");
}

#[sqlx::test]
async fn port_out_of_range_exits_1(_o: PgPoolOptions, connect_options: PgConnectOptions) {
    assert_config_rejected(connect_options, "PORT", "70000").await;
}

#[sqlx::test]
async fn port_zero_exits_1(_o: PgPoolOptions, connect_options: PgConnectOptions) {
    assert_config_rejected(connect_options, "PORT", "0").await;
}

#[sqlx::test]
async fn max_connections_out_of_range_exits_1(
    _o: PgPoolOptions,
    connect_options: PgConnectOptions,
) {
    assert_config_rejected(connect_options, "DATABASE_MAX_CONNECTIONS", "101").await;
}

#[sqlx::test]
async fn acquire_timeout_not_integer_exits_1(_o: PgPoolOptions, connect_options: PgConnectOptions) {
    assert_config_rejected(connect_options, "DATABASE_ACQUIRE_TIMEOUT_SECS", "abc").await;
}

#[sqlx::test]
async fn invalid_test_routes_flag_exits_1(_o: PgPoolOptions, connect_options: PgConnectOptions) {
    assert_config_rejected(connect_options, "ENABLE_TEST_ROUTES", "maybe").await;
}

// ---- database connection --------------------------------------------------

#[tokio::test]
async fn unreachable_database_retries_5_times_then_exits_1() {
    let port = free_port();
    let env = env_with_url("postgres://tasks:s3cr3t-pw@127.0.0.1:1/tasks", port, &[]);
    let mut service = spawn_with(&env);
    let status = service
        .wait_exit(Duration::from_secs(60))
        .expect("exits after the retries");
    let elapsed = service.started.elapsed();

    assert_eq!(status.code(), Some(1), "{}", service.output());
    let output = service.output();
    assert_eq!(
        count_lines(&service, "connecting to database"),
        5,
        "{output}"
    );
    assert!(
        output.contains("could not connect to database after 5 attempts:"),
        "{output}"
    );
    assert!(
        elapsed >= Duration::from_secs(8),
        "exited after only {elapsed:?}"
    );
    assert!(!output.contains("listening on"), "{output}");
    // Connection failures never reveal the password.
    assert!(!output.contains("s3cr3t-pw"), "{output}");
}

// ---- migrations and listening ---------------------------------------------

#[sqlx::test(migrations = false)]
async fn fresh_database_applies_migrations_and_listens(
    _o: PgPoolOptions,
    connect_options: PgConnectOptions,
) {
    let url = database_url_for(&connect_options);
    let port = free_port();
    let mut service = spawn_with(&env_with_url(&url, port, &[]));
    let listening = format!("listening on 127.0.0.1:{port}");
    assert!(
        service.wait_for_line(&listening, STARTUP_TIMEOUT),
        "{}",
        service.output()
    );

    let connecting = line_index(&service, "connecting to database").expect("connect line");
    let applied = line_index(&service, "applied 1 migrations").expect("applied line");
    let listen = line_index(&service, &listening).expect("listen line");
    assert!(
        connecting < applied && applied < listen,
        "{}",
        service.output()
    );
    assert!(accepts_connections(port));

    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(service.is_running(), "{}", service.output());
}

#[sqlx::test]
async fn already_migrated_database_applies_zero(
    _o: PgPoolOptions,
    connect_options: PgConnectOptions,
) {
    let url = database_url_for(&connect_options);
    let port = free_port();
    let service = spawn_with(&env_with_url(&url, port, &[]));
    assert!(
        service.wait_for_line(&format!("listening on 127.0.0.1:{port}"), STARTUP_TIMEOUT),
        "{}",
        service.output()
    );
    assert!(service.output().contains("applied 0 migrations"));
}

#[sqlx::test(migrations = false)]
async fn migration_checksum_mismatch_exits_1_without_listener(
    _o: PgPoolOptions,
    connect_options: PgConnectOptions,
) {
    // Build `tampered-db`: migration recorded as applied with a wrong checksum.
    let pool = PgPool::connect_with(connect_options.clone()).await.unwrap();
    task_api::infrastructure::db::run_migrations(&pool)
        .await
        .unwrap();
    let wrong_checksum = vec![0_u8; 48];
    sqlx::query!("UPDATE _sqlx_migrations SET checksum = $1", &wrong_checksum)
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;

    let url = database_url_for(&connect_options);
    let port = free_port();
    let mut service = spawn_with(&env_with_url(&url, port, &[]));

    let mut ever_accepted = false;
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        ever_accepted |= accepts_connections(port);
        if let Some(status) = service.wait_exit(Duration::from_millis(100)) {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "service did not exit: {}",
            service.output()
        );
    };

    assert_eq!(status.code(), Some(1), "{}", service.output());
    let output = service.output();
    assert!(
        output.contains("migration failed: 20261121000001"),
        "{output}"
    );
    assert!(!output.contains("listening on"), "{output}");
    assert!(!ever_accepted, "the port accepted a connection");
}

#[sqlx::test]
async fn default_bind_address_is_0_0_0_0_8080(
    _o: PgPoolOptions,
    connect_options: PgConnectOptions,
) {
    // Requires TCP port 8080 to be free on this host.
    let url = database_url_for(&connect_options);
    let env = vec![("DATABASE_URL".to_owned(), url)];
    let service = spawn_with(&env);
    assert!(
        service.wait_for_line("listening on 0.0.0.0:8080", STARTUP_TIMEOUT),
        "{}",
        service.output()
    );
    assert!(accepts_connections(8080));
}

// ---- idle shutdown ---------------------------------------------------------

async fn idle_shutdown(
    connect_options: PgConnectOptions,
    signal: Signal,
    expect_signal_line: bool,
) {
    let url = database_url_for(&connect_options);
    let port = free_port();
    let mut service = spawn_with(&env_with_url(&url, port, &[]));
    assert!(
        service.wait_for_line("listening on", STARTUP_TIMEOUT),
        "{}",
        service.output()
    );

    let sent = Instant::now();
    service.signal(signal);
    let status = service
        .wait_exit(Duration::from_secs(10))
        .unwrap_or_else(|| panic!("did not exit within 10 s: {}", service.output()));
    assert!(sent.elapsed() < Duration::from_secs(10));
    assert_eq!(status.code(), Some(0), "{}", service.output());
    let output = service.output();
    if expect_signal_line {
        assert!(output.contains("shutdown signal received"), "{output}");
    }
    assert!(output.contains("server stopped"), "{output}");
}

#[sqlx::test]
async fn sigint_when_idle_exits_0(_o: PgPoolOptions, connect_options: PgConnectOptions) {
    idle_shutdown(connect_options, Signal::SIGINT, true).await;
}

#[sqlx::test]
async fn sigterm_when_idle_exits_0(_o: PgPoolOptions, connect_options: PgConnectOptions) {
    idle_shutdown(connect_options, Signal::SIGTERM, false).await;
}
