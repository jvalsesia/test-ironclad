//! Connection pool construction with bounded startup retries.

use std::time::Duration;

use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tracing::{error, info, warn};

/// Number of connection attempts made at startup.
pub const CONNECT_ATTEMPTS: u32 = 5;
/// Pause between two failed attempts.
pub const RETRY_DELAY: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub struct DbSettings {
    pub url: String,
    pub max_connections: u32,
    pub acquire_timeout: Duration,
}

// The URL may embed a password; it is never printed.
impl std::fmt::Debug for DbSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DbSettings")
            .field("url", &"<redacted>")
            .field("max_connections", &self.max_connections)
            .field("acquire_timeout", &self.acquire_timeout)
            .finish()
    }
}

/// Connects to PostgreSQL, trying up to [`CONNECT_ATTEMPTS`] times with
/// [`RETRY_DELAY`] between attempts. Each attempt is bounded by the acquire
/// timeout. The connection URL is never logged.
pub async fn connect_with_retry(settings: &DbSettings) -> Result<PgPool, sqlx::Error> {
    let mut last_error = None;
    for attempt in 1..=CONNECT_ATTEMPTS {
        info!("connecting to database (attempt {attempt}/{CONNECT_ATTEMPTS})");
        let result = PgPoolOptions::new()
            .max_connections(settings.max_connections)
            .acquire_timeout(settings.acquire_timeout)
            .connect(&settings.url)
            .await;
        match result {
            Ok(pool) => return Ok(pool),
            Err(err) => {
                warn!("database connection attempt {attempt}/{CONNECT_ATTEMPTS} failed: {err}");
                last_error = Some(err);
                if attempt < CONNECT_ATTEMPTS {
                    tokio::time::sleep(RETRY_DELAY).await;
                }
            }
        }
    }
    let err = last_error.unwrap_or(sqlx::Error::PoolClosed);
    error!("could not connect to database after {CONNECT_ATTEMPTS} attempts: {err}");
    Err(err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_redacts_the_url() {
        let settings = DbSettings {
            url: "postgres://u:s3cr3t@h/db".to_owned(),
            max_connections: 1,
            acquire_timeout: Duration::from_secs(1),
        };
        assert!(!format!("{settings:?}").contains("s3cr3t"));
    }
}
