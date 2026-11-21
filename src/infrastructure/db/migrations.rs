//! Embedded migrations, applied at startup before the listener binds.

use sqlx::PgPool;
use sqlx::migrate::{Migrate, MigrateError, Migrator};

/// Migrations embedded into the binary from `./migrations`.
pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// A failed migration run: the migration version (when known) and the cause.
#[derive(Debug)]
pub struct MigrationFailure {
    pub version: Option<i64>,
    pub cause: String,
}

impl std::fmt::Display for MigrationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.version {
            Some(version) => write!(f, "{version} {}", self.cause),
            None => f.write_str(&self.cause),
        }
    }
}

impl std::error::Error for MigrationFailure {}

impl MigrationFailure {
    fn from_migrate_error(err: &MigrateError) -> Self {
        let version = match err {
            MigrateError::VersionMismatch(version)
            | MigrateError::VersionMissing(version)
            | MigrateError::VersionNotPresent(version)
            | MigrateError::Dirty(version) => Some(*version),
            _ => None,
        };
        Self {
            version,
            cause: err.to_string(),
        }
    }
}

/// Applies every pending migration and returns how many were newly applied.
pub async fn run_migrations(pool: &PgPool) -> Result<usize, MigrationFailure> {
    let applied_before = applied_count(pool).await?;
    MIGRATOR
        .run(pool)
        .await
        .map_err(|err| MigrationFailure::from_migrate_error(&err))?;
    let applied_after = applied_count(pool).await?;
    Ok(applied_after.saturating_sub(applied_before))
}

async fn applied_count(pool: &PgPool) -> Result<usize, MigrationFailure> {
    let failure = |err: MigrateError| MigrationFailure::from_migrate_error(&err);
    let mut conn = pool
        .acquire()
        .await
        .map_err(|err| failure(MigrateError::Execute(err)))?;
    conn.ensure_migrations_table(&MIGRATOR.table_name)
        .await
        .map_err(failure)?;
    let applied = conn
        .list_applied_migrations(&MIGRATOR.table_name)
        .await
        .map_err(failure)?;
    Ok(applied.len())
}
