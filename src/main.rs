//! Composition root. Wires configuration, database, router and listener.
//!
//! Startup order: `.env` → logging → configuration → database connection
//! (5 attempts) → migrations → bind → serve until SIGTERM/SIGINT → drain →
//! close the pool. Every startup failure is logged and exits with code 1.

use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use task_api::Config;
use task_api::http::server::{SHUTDOWN_GRACE, serve, shutdown_signal};
use task_api::http::{AppState, RouterOptions, build_router};
use task_api::infrastructure::db::{
    DbSettings, PgDiagnosticsProbe, connect_with_retry, run_migrations,
};
use tokio::net::TcpListener;
use tracing::{error, info, warn};

/// Upper bound on closing the pool, so a stuck connection cannot delay exit.
const POOL_CLOSE_TIMEOUT: Duration = Duration::from_secs(1);

/// Minimal stdout subscriber so startup and shutdown messages are visible.
/// F02 replaces this with the configurable subscriber.
fn init_logging() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stdout)
        .with_ansi(false)
        .with_max_level(tracing::Level::INFO)
        .init();
}

/// Runs the service. Failures are logged where they happen; the `Err(())`
/// only selects the exit code.
async fn run() -> Result<(), ()> {
    // A missing `.env` file is normal; process variables always win.
    let _ = dotenvy::dotenv();

    let config = Config::from_env().map_err(|err| {
        error!("configuration error: {err}");
    })?;

    let settings = DbSettings {
        url: config.database_url.clone(),
        max_connections: config.database_max_connections,
        acquire_timeout: config.database_acquire_timeout,
    };
    // `connect_with_retry` logs every attempt and the final failure itself.
    let pool = connect_with_retry(&settings).await.map_err(|_| ())?;

    match run_migrations(&pool).await {
        Ok(applied) => info!("applied {applied} migrations"),
        Err(failure) => {
            error!("migration failed: {failure}");
            return Err(());
        }
    }

    if config.enable_test_routes {
        warn!("diagnostic /__test/* routes are enabled");
    }
    let state = AppState::new(Arc::new(PgDiagnosticsProbe::new(pool.clone())));
    let router = build_router(
        state,
        RouterOptions {
            test_routes: config.enable_test_routes,
        },
    );

    let address = SocketAddr::new(config.host, config.port);
    let listener = TcpListener::bind(address).await.map_err(|err| {
        error!("could not bind {address}: {err}");
    })?;
    let bound = listener.local_addr().map_err(|err| {
        error!("could not read the bound address: {err}");
    })?;
    info!("listening on {bound}");

    serve(listener, router, SHUTDOWN_GRACE, shutdown_signal())
        .await
        .map_err(|err| {
            error!("server error: {err}");
        })?;

    if tokio::time::timeout(POOL_CLOSE_TIMEOUT, pool.close())
        .await
        .is_err()
    {
        warn!("database pool did not close within {POOL_CLOSE_TIMEOUT:?}");
    }
    info!("server stopped");
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    init_logging();
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(()) => ExitCode::from(1),
    }
}
