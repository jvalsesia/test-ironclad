//! Health check use case: is the database answering within the timeout?

use std::sync::Arc;
use std::time::Duration;

use tracing::warn;

use super::diagnostics_probe::DiagnosticsProbe;

/// Upper bound for the database round trip behind `GET /health`.
pub const HEALTH_TIMEOUT: Duration = Duration::from_secs(2);

/// Outcome of a health check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HealthStatus {
    pub database_up: bool,
}

/// Runs the database probe under a timeout; any error or timeout is `down`.
pub struct CheckHealth {
    probe: Arc<dyn DiagnosticsProbe>,
    timeout: Duration,
}

impl CheckHealth {
    pub fn new(probe: Arc<dyn DiagnosticsProbe>, timeout: Duration) -> Self {
        Self { probe, timeout }
    }

    pub async fn execute(&self) -> HealthStatus {
        let database_up = match tokio::time::timeout(self.timeout, self.probe.ping()).await {
            Ok(Ok(())) => true,
            Ok(Err(err)) => {
                warn!("health check failed: {err}");
                false
            }
            Err(_) => {
                warn!(
                    "health check failed: database did not answer within {} ms",
                    self.timeout.as_millis()
                );
                false
            }
        };
        HealthStatus { database_up }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use async_trait::async_trait;

    use super::*;
    use crate::application::AppError;

    enum Fake {
        Ok,
        Unavailable,
        Internal,
        Hang,
    }

    #[async_trait]
    impl DiagnosticsProbe for Fake {
        async fn ping(&self) -> Result<(), AppError> {
            match self {
                Fake::Ok => Ok(()),
                Fake::Unavailable => Err(AppError::Unavailable("down".into())),
                Fake::Internal => Err(AppError::Internal("boom".into())),
                Fake::Hang => {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    Ok(())
                }
            }
        }
        async fn induce_sql_error(&self) -> Result<(), AppError> {
            Ok(())
        }
        async fn hold_connection(&self, _duration: Duration) -> Result<(), AppError> {
            Ok(())
        }
    }

    async fn run(fake: Fake, timeout: Duration) -> HealthStatus {
        CheckHealth::new(Arc::new(fake), timeout).execute().await
    }

    #[tokio::test]
    async fn reachable_database_is_up() {
        assert!(run(Fake::Ok, HEALTH_TIMEOUT).await.database_up);
    }

    #[tokio::test]
    async fn probe_error_is_down() {
        assert!(!run(Fake::Unavailable, HEALTH_TIMEOUT).await.database_up);
        assert!(!run(Fake::Internal, HEALTH_TIMEOUT).await.database_up);
    }

    #[tokio::test]
    async fn hung_probe_is_down_after_the_timeout() {
        let start = Instant::now();
        let status = run(Fake::Hang, Duration::from_millis(50)).await;
        assert!(!status.database_up);
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn default_timeout_is_two_seconds() {
        assert_eq!(HEALTH_TIMEOUT, Duration::from_secs(2));
    }
}
