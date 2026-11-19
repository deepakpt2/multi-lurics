use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgConnection;
use uuid::Uuid;

/// A consequential change. Written in the same transaction as the change itself.
#[derive(Debug, Clone)]
pub struct AuditEntry<'a> {
    pub actor_id: Option<Uuid>,
    pub action: &'a str,
    pub target_type: &'a str,
    pub target_id: Option<Uuid>,
    pub request_id: Option<&'a str>,
    pub before: Option<Value>,
    pub after: Option<Value>,
    pub metadata: Value,
}

impl<'a> AuditEntry<'a> {
    pub fn new(action: &'a str, target_type: &'a str, target_id: Option<Uuid>) -> Self {
        Self {
            actor_id: None,
            action,
            target_type,
            target_id,
            request_id: None,
            before: None,
            after: None,
            metadata: Value::Object(Default::default()),
        }
    }
}

pub async fn record(conn: &mut PgConnection, e: AuditEntry<'_>) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO audit_log (actor_id, action, target_type, target_id, request_id, before, after, metadata)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id",
    )
    .bind(e.actor_id)
    .bind(e.action)
    .bind(e.target_type)
    .bind(e.target_id)
    .bind(e.request_id)
    .bind(e.before)
    .bind(e.after)
    .bind(e.metadata)
    .fetch_one(conn)
    .await
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AuditRow {
    pub id: Uuid,
    pub occurred_at: DateTime<Utc>,
    pub actor_id: Option<Uuid>,
    pub action: String,
    pub target_type: String,
    pub target_id: Option<Uuid>,
    pub request_id: Option<String>,
}

/// Keyset page, newest first. `after` is the (occurred_at, id) of the last row already seen.
pub async fn list(
    conn: &mut PgConnection,
    after: Option<(DateTime<Utc>, Uuid)>,
    limit: i64,
) -> Result<Vec<AuditRow>, sqlx::Error> {
    let (ts, id) = after.unzip();
    sqlx::query_as(
        "SELECT id, occurred_at, actor_id, action, target_type, target_id, request_id
         FROM audit_log
         WHERE $1::timestamptz IS NULL OR (occurred_at, id) < ($1, $2)
         ORDER BY occurred_at DESC, id DESC
         LIMIT $3",
    )
    .bind(ts)
    .bind(id)
    .bind(limit)
    .fetch_all(conn)
    .await
}
