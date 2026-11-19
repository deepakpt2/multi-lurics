use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

/// Enqueue inside the caller's transaction: the event exists iff the change committed.
pub async fn enqueue(
    conn: &mut PgConnection,
    topic: &str,
    aggregate_type: &str,
    aggregate_id: Uuid,
    payload: Value,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO outbox (topic, aggregate_type, aggregate_id, payload) VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(topic)
    .bind(aggregate_type)
    .bind(aggregate_id)
    .bind(payload)
    .fetch_one(conn)
    .await
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct OutboxEvent {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub topic: String,
    pub aggregate_type: String,
    pub aggregate_id: Uuid,
    pub payload: Value,
    pub attempts: i32,
}

/// Leases up to `limit` due events. `SKIP LOCKED` + lease means concurrent
/// workers never receive the same event while a lease is live.
pub async fn claim(pool: &PgPool, limit: i64, lease_secs: i64) -> Result<Vec<OutboxEvent>, sqlx::Error> {
    sqlx::query_as(
        "UPDATE outbox o
         SET locked_until = clock_timestamp() + make_interval(secs => $2), attempts = o.attempts + 1
         FROM (
             SELECT id FROM outbox
             WHERE processed_at IS NULL
               AND available_at <= clock_timestamp()
               AND (locked_until IS NULL OR locked_until < clock_timestamp())
             ORDER BY available_at, created_at
             LIMIT $1
             FOR UPDATE SKIP LOCKED
         ) due
         WHERE o.id = due.id
         RETURNING o.id, o.created_at, o.topic, o.aggregate_type, o.aggregate_id, o.payload, o.attempts",
    )
    .bind(limit)
    .bind(lease_secs as f64)
    .fetch_all(pool)
    .await
}

pub async fn mark_processed(pool: &PgPool, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE outbox SET processed_at = clock_timestamp(), locked_until = NULL, last_error = NULL WHERE id = $1",
    )
    .bind(id)
    .execute(pool)
    .await
    .map(|_| ())
}

/// Exponential backoff capped at one hour.
pub async fn mark_failed(pool: &PgPool, id: Uuid, attempts: i32, error: &str) -> Result<(), sqlx::Error> {
    let delay_secs = 2f64.powi(attempts.clamp(0, 12)).min(3600.0);
    sqlx::query(
        "UPDATE outbox
         SET locked_until = NULL, last_error = left($3, 2000),
             available_at = clock_timestamp() + make_interval(secs => $2)
         WHERE id = $1",
    )
    .bind(id)
    .bind(delay_secs)
    .bind(error)
    .execute(pool)
    .await
    .map(|_| ())
}
