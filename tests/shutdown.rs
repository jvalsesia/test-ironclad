//! Graceful shutdown with a request in flight, against the spawned binary.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::{Duration, Instant};

use common::{accepts_connections, base_env, database_url_for, free_port, spawn_with};
use nix::sys::signal::Signal;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);

fn service_env(connect_options: &PgConnectOptions, port: u16) -> Vec<(String, String)> {
    let mut env = base_env(port);
    env.push(("DATABASE_URL".to_owned(), database_url_for(connect_options)));
    env.push(("ENABLE_TEST_ROUTES".to_owned(), "true".to_owned()));
    env
}

#[sqlx::test]
async fn sigterm_lets_in_flight_request_complete(
    _o: PgPoolOptions,
    connect_options: PgConnectOptions,
) {
    let port = free_port();
    let mut service = spawn_with(&service_env(&connect_options, port));
    assert!(
        service.wait_for_line("listening on", STARTUP_TIMEOUT),
        "{}",
        service.output()
    );

    let url = format!("http://127.0.0.1:{port}/__test/slow?ms=3000");
    let in_flight = tokio::spawn(async move {
        let response = reqwest::get(url).await?;
        let status = response.status();
        let body = response.text().await?;
        Ok::<_, reqwest::Error>((status, body))
    });
    tokio::time::sleep(Duration::from_millis(500)).await;

    let signalled = Instant::now();
    service.signal(Signal::SIGTERM);
    tokio::time::sleep(Duration::from_millis(400)).await;
    // A new connection after the signal is not served.
    assert!(
        !accepts_connections(port),
        "listener still accepting after SIGTERM"
    );

    let (status, body) = in_flight
        .await
        .unwrap()
        .expect("in-flight request completes");
    assert_eq!(status.as_u16(), 200);
    assert_eq!(body, r#"{"slept_ms":3000}"#);

    let exit = service
        .wait_exit(Duration::from_secs(10))
        .unwrap_or_else(|| panic!("did not exit: {}", service.output()));
    assert_eq!(exit.code(), Some(0), "{}", service.output());
    assert!(signalled.elapsed() < Duration::from_secs(10));
}

#[sqlx::test]
async fn sigterm_deadline_cuts_requests_after_10s(
    _o: PgPoolOptions,
    connect_options: PgConnectOptions,
) {
    let port = free_port();
    let mut service = spawn_with(&service_env(&connect_options, port));
    assert!(
        service.wait_for_line("listening on", STARTUP_TIMEOUT),
        "{}",
        service.output()
    );

    let url = format!("http://127.0.0.1:{port}/__test/slow?ms=20000");
    let in_flight = tokio::spawn(async move {
        let response = reqwest::get(url).await?;
        let status = response.status();
        let body = response.text().await?;
        Ok::<_, reqwest::Error>((status, body))
    });
    tokio::time::sleep(Duration::from_millis(500)).await;

    let signalled = Instant::now();
    service.signal(Signal::SIGTERM);
    let exit = service
        .wait_exit(Duration::from_secs(14))
        .unwrap_or_else(|| panic!("did not exit: {}", service.output()));
    let elapsed = signalled.elapsed();

    assert_eq!(exit.code(), Some(0), "{}", service.output());
    assert!(
        elapsed >= Duration::from_secs(10) && elapsed <= Duration::from_secs(12),
        "exited {elapsed:?} after the signal"
    );
    let outcome = in_flight.await.unwrap();
    assert!(
        !matches!(&outcome, Ok((status, _)) if status.as_u16() == 200),
        "in-flight request must not complete: {outcome:?}"
    );
    assert!(
        service.output().contains("shutdown deadline exceeded"),
        "{}",
        service.output()
    );
}
