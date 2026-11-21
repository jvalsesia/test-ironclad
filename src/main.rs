//! Composition root. Wires configuration, database, router and listener.

use std::process::ExitCode;

use task_api::Config;
use tracing::error;

/// Minimal stdout subscriber so startup and shutdown messages are visible.
/// F02 replaces this with the configurable subscriber.
fn init_logging() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stdout)
        .with_ansi(false)
        .with_max_level(tracing::Level::INFO)
        .init();
}

async fn run() -> Result<(), ()> {
    // A missing `.env` file is normal; process variables always win.
    let _ = dotenvy::dotenv();

    let _config = Config::from_env().map_err(|err| {
        error!("configuration error: {err}");
    })?;

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
