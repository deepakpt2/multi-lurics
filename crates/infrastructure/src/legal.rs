//! SQL for legal notices, takedowns and takedown appeals. Rules live in the use cases.

use chrono::{DateTime, Utc};
use sqlx::PgConnection;
use uuid::Uuid;

type Res<T> = Result<T, sqlx::Error>;
pub type After = Option<(DateTime<Utc>, Uuid)>;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct NoticeRow {
    pub id: Uuid,
    pub reference: String,
    pub kind: String,
    pub contribution_id: Option<Uuid>,
    pub contact_name: String,
    pub contact_email: String,
    pub statement: String,
    pub status: String,
    pub decided_by: Option<Uuid>,
    pub decided_at: Option<DateTime<Utc>>,
    pub decision_note: Option<String>,
    pub taken_down_revision_id: Option<Uuid>,
    pub was_primary: bool,
    pub created_at: DateTime<Utc>,
}

const NOTICE: &str = "SELECT id, reference, kind, contribution_id, contact_name, contact_email, statement, status,
    decided_by, decided_at, decision_note, taken_down_revision_id, was_primary, created_at FROM legal_notices";

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AppealRow {
    pub id: Uuid,
    pub notice_id: Uuid,
    pub appellant_id: Uuid,
    pub statement: String,
    pub status: String,
    pub decided_by: Option<Uuid>,
    pub decision_note: Option<String>,
    pub decided_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

const APPEAL: &str = "SELECT id, notice_id, appellant_id, statement, status, decided_by, decision_note, decided_at,
    created_at FROM takedown_appeals";

pub async fn insert_notice(
    conn: &mut PgConnection,
    reference: &str,
    kind: &str,
    contribution: Option<Uuid>,
    name: &str,
    email: &str,
    statement: &str,
) -> Res<NoticeRow> {
    sqlx::query_as(
        "INSERT INTO legal_notices (reference, kind, contribution_id, contact_name, contact_email, statement, good_faith)
         VALUES ($1, $2, $3, $4, $5, $6, true)
         RETURNING id, reference, kind, contribution_id, contact_name, contact_email, statement, status, decided_by,
                   decided_at, decision_note, taken_down_revision_id, was_primary, created_at",
    )
    .bind(reference)
    .bind(kind)
    .bind(contribution)
    .bind(name)
    .bind(email)
    .bind(statement)
    .fetch_one(conn)
    .await
}

pub async fn contribution_exists(conn: &mut PgConnection, id: Uuid) -> Res<bool> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM contributions WHERE id = $1)")
        .bind(id)
        .fetch_one(conn)
        .await
}

pub async fn get_notice(conn: &mut PgConnection, id: Uuid, lock: bool) -> Res<Option<NoticeRow>> {
    let sql = format!("{NOTICE} WHERE id = $1{}", if lock { " FOR UPDATE" } else { "" });
    sqlx::query_as(&sql).bind(id).fetch_optional(conn).await
}

pub async fn notice_queue(conn: &mut PgConnection, status: &str, after: After, limit: i64) -> Res<Vec<NoticeRow>> {
    sqlx::query_as(&format!(
        "{NOTICE} WHERE status = $1 AND ($2::timestamptz IS NULL OR (created_at, id) > ($2, $3))
         ORDER BY created_at, id LIMIT $4"
    ))
    .bind(status)
    .bind(after.map(|a| a.0))
    .bind(after.map(|a| a.1))
    .bind(limit)
    .fetch_all(conn)
    .await
}

pub async fn decide_notice(
    conn: &mut PgConnection,
    id: Uuid,
    status: &str,
    by: Uuid,
    note: Option<&str>,
    revision: Option<Uuid>,
    was_primary: bool,
) -> Res<()> {
    sqlx::query(
        "UPDATE legal_notices SET status = $2, decided_by = $3, decided_at = now(), decision_note = $4,
                taken_down_revision_id = COALESCE($5, taken_down_revision_id), was_primary = $6 WHERE id = $1",
    )
    .bind(id)
    .bind(status)
    .bind(by)
    .bind(note)
    .bind(revision)
    .bind(was_primary)
    .execute(conn)
    .await
    .map(|_| ())
}

/// Withdraws the public pointer and, if it was the slot's primary, promotes the oldest remaining
/// published alternative so the slot still has exactly one primary.
pub async fn withdraw(conn: &mut PgConnection, id: Uuid, note: &str) -> Res<()> {
    sqlx::query(
        "UPDATE contributions SET published_revision_id = NULL, is_primary = false, status = 'rejected',
                review_note = $2, updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(note)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "UPDATE contributions SET is_primary = true, updated_at = now() WHERE id = (
            SELECT o.id FROM contributions o JOIN contributions t ON t.id = $1
             WHERE o.recording_id = t.recording_id AND o.type = t.type AND o.language_id = t.language_id
               AND o.script_id = t.script_id AND o.published_revision_id IS NOT NULL AND o.id <> t.id
             ORDER BY o.created_at, o.id LIMIT 1)
           AND NOT EXISTS (
            SELECT 1 FROM contributions o JOIN contributions t ON t.id = $1
             WHERE o.recording_id = t.recording_id AND o.type = t.type AND o.language_id = t.language_id
               AND o.script_id = t.script_id AND o.is_primary)",
    )
    .bind(id)
    .execute(conn)
    .await
    .map(|_| ())
}

pub async fn insert_appeal(conn: &mut PgConnection, notice: Uuid, by: Uuid, statement: &str) -> Res<AppealRow> {
    sqlx::query_as(
        "INSERT INTO takedown_appeals (notice_id, appellant_id, statement) VALUES ($1, $2, $3)
         RETURNING id, notice_id, appellant_id, statement, status, decided_by, decision_note, decided_at, created_at",
    )
    .bind(notice)
    .bind(by)
    .bind(statement)
    .fetch_one(conn)
    .await
}

pub async fn get_appeal(conn: &mut PgConnection, id: Uuid, lock: bool) -> Res<Option<AppealRow>> {
    let sql = format!("{APPEAL} WHERE id = $1{}", if lock { " FOR UPDATE" } else { "" });
    sqlx::query_as(&sql).bind(id).fetch_optional(conn).await
}

pub async fn appeal_queue(conn: &mut PgConnection, status: &str, after: After, limit: i64) -> Res<Vec<AppealRow>> {
    sqlx::query_as(&format!(
        "{APPEAL} WHERE status = $1 AND ($2::timestamptz IS NULL OR (created_at, id) > ($2, $3))
         ORDER BY created_at, id LIMIT $4"
    ))
    .bind(status)
    .bind(after.map(|a| a.0))
    .bind(after.map(|a| a.1))
    .bind(limit)
    .fetch_all(conn)
    .await
}

pub async fn decide_appeal(conn: &mut PgConnection, id: Uuid, status: &str, by: Uuid, note: Option<&str>) -> Res<()> {
    sqlx::query(
        "UPDATE takedown_appeals SET status = $2, decided_by = $3, decision_note = $4, decided_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(status)
    .bind(by)
    .bind(note)
    .execute(conn)
    .await
    .map(|_| ())
}

pub async fn set_notice_status(conn: &mut PgConnection, id: Uuid, status: &str) -> Res<()> {
    sqlx::query("UPDATE legal_notices SET status = $2 WHERE id = $1")
        .bind(id)
        .bind(status)
        .execute(conn)
        .await
        .map(|_| ())
}

/// Restores the public pointer to the revision that was withdrawn. That revision already has its
/// publication event (one per revision, ever), so none is added. It becomes the slot's primary
/// only if the slot has none now.
pub async fn reinstate(conn: &mut PgConnection, id: Uuid, revision: Uuid, note: &str) -> Res<bool> {
    sqlx::query_scalar(
        "UPDATE contributions c SET published_revision_id = $2, status = 'published', review_note = $3,
                is_primary = NOT EXISTS (
                    SELECT 1 FROM contributions o WHERE o.recording_id = c.recording_id AND o.type = c.type
                       AND o.language_id = c.language_id AND o.script_id = c.script_id AND o.is_primary),
                updated_at = now()
          WHERE c.id = $1 RETURNING c.is_primary",
    )
    .bind(id)
    .bind(revision)
    .bind(note)
    .fetch_one(conn)
    .await
}
