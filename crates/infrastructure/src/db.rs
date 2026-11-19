use sqlx::migrate::Migrator;
use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Postgres, Transaction};
use std::ops::{Deref, DerefMut};
use std::time::Duration;

/// Migrations embedded at compile time from `/migrations` (forward-only).
pub static MIGRATOR: Migrator = sqlx::migrate!("../../migrations");

pub async fn connect(url: &str, max_connections: u32) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(5))
        .connect(url)
        .await
}

/// One unit of work. Dropping without `commit` rolls back, so an early `?`
/// can never leave half a change (or an orphan audit/outbox row) behind.
pub struct AppTx(Transaction<'static, Postgres>);

impl AppTx {
    pub async fn begin(pool: &PgPool) -> Result<Self, sqlx::Error> {
        Ok(Self(pool.begin().await?))
    }

    pub async fn commit(self) -> Result<(), sqlx::Error> {
        self.0.commit().await
    }
}

impl Deref for AppTx {
    type Target = PgConnection;
    fn deref(&self) -> &PgConnection {
        &self.0
    }
}

impl DerefMut for AppTx {
    fn deref_mut(&mut self) -> &mut PgConnection {
        &mut self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationState {
    UpToDate {
        version: i64,
    },
    /// Embedded migrations not yet applied, or a recorded failure.
    Pending {
        applied: Option<i64>,
        expected: i64,
    },
    Dirty {
        version: i64,
    },
    /// Database has a migration this binary does not know: wrong build or a rollback attempt.
    Ahead {
        applied: i64,
        expected: i64,
    },
}

/// Compares `_sqlx_migrations` with the migrations compiled into this binary.
pub async fn migration_state(pool: &PgPool) -> Result<MigrationState, sqlx::Error> {
    let expected = MIGRATOR.iter().map(|m| m.version).max().unwrap_or(0);
    let exists: bool = sqlx::query_scalar("SELECT to_regclass('_sqlx_migrations') IS NOT NULL")
        .fetch_one(pool)
        .await?;
    if !exists {
        return Ok(MigrationState::Pending {
            applied: None,
            expected,
        });
    }
    if let Some(version) =
        sqlx::query_scalar::<_, i64>("SELECT version FROM _sqlx_migrations WHERE NOT success ORDER BY version LIMIT 1")
            .fetch_optional(pool)
            .await?
    {
        return Ok(MigrationState::Dirty { version });
    }
    let applied: Option<i64> = sqlx::query_scalar("SELECT max(version) FROM _sqlx_migrations")
        .fetch_one(pool)
        .await?;
    Ok(match applied {
        Some(v) if v == expected => MigrationState::UpToDate { version: v },
        Some(v) if v > expected => MigrationState::Ahead { applied: v, expected },
        other => MigrationState::Pending {
            applied: other,
            expected,
        },
    })
}
