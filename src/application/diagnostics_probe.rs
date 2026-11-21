//! Port used by the F01 diagnostic routes to exercise database failure paths
//! before any business endpoint exists.

use std::time::Duration;

use async_trait::async_trait;

use super::errors::AppError;

#[async_trait]
pub trait DiagnosticsProbe: Send + Sync {
    /// Round-trips a trivial statement to prove the database is reachable.
    async fn ping(&self) -> Result<(), AppError>;
    /// Executes a statement that fails at runtime, to exercise error mapping.
    async fn induce_sql_error(&self) -> Result<(), AppError>;
    /// Holds one pooled connection for `duration`.
    async fn hold_connection(&self, duration: Duration) -> Result<(), AppError>;
}
