//! PostgreSQL implementation of the diagnostics port.

use std::time::Duration;

use async_trait::async_trait;
use sqlx::PgPool;

use super::errors::map_sqlx_error;
use crate::application::{AppError, DiagnosticsProbe};

#[derive(Clone)]
pub struct PgDiagnosticsProbe {
    pool: PgPool,
}

impl PgDiagnosticsProbe {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl DiagnosticsProbe for PgDiagnosticsProbe {
    async fn ping(&self) -> Result<(), AppError> {
        sqlx::query_scalar!(r#"SELECT 1 AS "one!""#)
            .fetch_one(&self.pool)
            .await
            .map(|_| ())
            .map_err(map_sqlx_error)
    }

    async fn induce_sql_error(&self) -> Result<(), AppError> {
        // The divisor is bound at runtime, so PostgreSQL prepares the
        // statement and only fails when executing it (division by zero).
        let zero: i32 = 0;
        sqlx::query_scalar!(r#"SELECT 1 / $1::int4 AS "quotient!""#, zero)
            .fetch_one(&self.pool)
            .await
            .map(|_| ())
            .map_err(map_sqlx_error)
    }

    async fn hold_connection(&self, duration: Duration) -> Result<(), AppError> {
        let connection = self.pool.acquire().await.map_err(map_sqlx_error)?;
        tokio::time::sleep(duration).await;
        drop(connection);
        Ok(())
    }
}
