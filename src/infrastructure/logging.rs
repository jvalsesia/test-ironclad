//! Global `tracing` subscriber setup.
//!
//! JSON output flattens event fields to the top level and nests the current
//! `request` span under `span`; `pretty` output is the default single-line
//! human-readable layout, with colors only when stdout is a terminal.

use std::io::{IsTerminal, stdout};

use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::config::{LogConfig, LogFormat};

/// Installs the global subscriber. Call once, with a validated config.
/// Returns an error text when a subscriber is already installed or the
/// level directive is rejected.
pub fn init(config: &LogConfig) -> Result<(), String> {
    let filter = EnvFilter::try_new(&config.level)
        .map_err(|err| format!("invalid log level \"{}\": {err}", config.level))?;
    let registry = tracing_subscriber::registry().with(filter);
    let result = match config.format {
        LogFormat::Json => registry
            .with(
                tracing_subscriber::fmt::layer()
                    .json()
                    .flatten_event(true)
                    .with_current_span(true)
                    .with_span_list(false)
                    .with_ansi(false)
                    .with_writer(stdout),
            )
            .try_init(),
        LogFormat::Pretty => registry
            .with(
                tracing_subscriber::fmt::layer()
                    .with_ansi(stdout().is_terminal())
                    .with_writer(stdout),
            )
            .try_init(),
    };
    result.map_err(|err| format!("could not install the log subscriber: {err}"))
}

/// Prints one plain-text `ERROR` line through a throwaway subscriber, for
/// failures that happen before the configured subscriber exists.
pub fn report_startup_error(message: &str) {
    let subscriber = tracing_subscriber::fmt()
        .with_writer(stdout)
        .with_ansi(false)
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        tracing::error!("{message}");
    });
}
