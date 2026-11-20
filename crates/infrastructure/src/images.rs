//! Image records: who uploaded what, its review state, and which image is the public one.
//! Files themselves live in the media store (see `application::media`).

use chrono::{DateTime, Utc};
use sqlx::PgConnection;
use std::collections::HashMap;
use uuid::Uuid;

type Res<T> = Result<T, sqlx::Error>;

/// Entity types that can have an image.
pub const TYPES: [&str; 7] = ["artist", "album", "label", "publisher", "genre", "raga", "deity"];

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct ImageRow {
    pub id: Uuid,
    pub entity_type: Option<String>,
    pub entity_id: Option<Uuid>,
    pub proposal_id: Option<Uuid>,
    pub status: String,
    pub width: i32,
    pub height: i32,
    pub bytes: i32,
    pub sizes: Vec<i32>,
    pub credit: Option<String>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub decided_by: Option<Uuid>,
    pub decided_at: Option<DateTime<Utc>>,
    pub decision_note: Option<String>,
}

const COLS: &str = "id, entity_type, entity_id, proposal_id, status, width, height, bytes, sizes, credit, created_by, \
    created_at, decided_by, decided_at, decision_note";

pub struct NewImage<'a> {
    pub id: Uuid,
    pub entity: Option<(&'a str, Uuid)>,
    pub status: &'a str,
    pub width: i32,
    pub height: i32,
    pub bytes: i32,
    pub sizes: &'a [i32],
    pub sha256: &'a str,
    pub credit: Option<&'a str>,
    pub created_by: Uuid,
}

pub async fn insert(conn: &mut PgConnection, n: &NewImage<'_>) -> Res<ImageRow> {
    sqlx::query_as(&format!(
        "INSERT INTO entity_images (id, entity_type, entity_id, status, width, height, bytes, sizes, sha256, credit,
                                    rights_confirmed, created_by, decided_by, decided_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, true, $11,
                 CASE WHEN $4 = 'approved' THEN $11 END, CASE WHEN $4 = 'approved' THEN now() END)
         RETURNING {COLS}"
    ))
    .bind(n.id)
    .bind(n.entity.map(|e| e.0))
    .bind(n.entity.map(|e| e.1))
    .bind(n.status)
    .bind(n.width)
    .bind(n.height)
    .bind(n.bytes)
    .bind(n.sizes)
    .bind(n.sha256)
    .bind(n.credit)
    .bind(n.created_by)
    .fetch_one(conn)
    .await
}

pub async fn get(conn: &mut PgConnection, id: Uuid, lock: bool) -> Res<Option<ImageRow>> {
    let lock = if lock { " FOR UPDATE" } else { "" };
    sqlx::query_as(&format!("SELECT {COLS} FROM entity_images WHERE id = $1{lock}"))
        .bind(id)
        .fetch_optional(conn)
        .await
}

pub async fn count_for_user(conn: &mut PgConnection, user: Uuid, status: &str) -> Res<i64> {
    sqlx::query_scalar("SELECT count(*) FROM entity_images WHERE created_by = $1 AND status = $2")
        .bind(user)
        .bind(status)
        .fetch_one(conn)
        .await
}

/// Serialises approvals for one entity so two racing approvals cannot both become the public image.
pub async fn lock_entity(conn: &mut PgConnection, entity_id: Uuid) -> Res<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("entity-image:{entity_id}"))
        .execute(conn)
        .await?;
    Ok(())
}

pub async fn current_for(conn: &mut PgConnection, entity_type: &str, ids: &[Uuid]) -> Res<HashMap<Uuid, Uuid>> {
    let rows: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT entity_id, id FROM entity_images WHERE entity_type = $1 AND entity_id = ANY($2) AND status = 'approved'",
    )
    .bind(entity_type)
    .bind(ids)
    .fetch_all(conn)
    .await?;
    Ok(rows.into_iter().collect())
}

pub async fn mine_pending(conn: &mut PgConnection, entity_type: &str, id: Uuid, user: Uuid) -> Res<Option<ImageRow>> {
    sqlx::query_as(&format!(
        "SELECT {COLS} FROM entity_images WHERE entity_type = $1 AND entity_id = $2 AND created_by = $3 AND status = 'pending'"
    ))
    .bind(entity_type)
    .bind(id)
    .bind(user)
    .fetch_optional(conn)
    .await
}

/// Retires the public image of an entity; returns its id so the caller can delete the files.
pub async fn supersede_current(conn: &mut PgConnection, entity_type: &str, id: Uuid) -> Res<Option<Uuid>> {
    sqlx::query_scalar(
        "UPDATE entity_images SET status = 'superseded' WHERE entity_type = $1 AND entity_id = $2 AND status = 'approved'
         RETURNING id",
    )
    .bind(entity_type)
    .bind(id)
    .fetch_optional(conn)
    .await
}

pub async fn same_as_current(conn: &mut PgConnection, entity_type: &str, id: Uuid, sha256: &str) -> Res<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM entity_images WHERE entity_type = $1 AND entity_id = $2 AND status = 'approved'
                        AND sha256 = $3)",
    )
    .bind(entity_type)
    .bind(id)
    .bind(sha256)
    .fetch_one(conn)
    .await
}

/// Makes the image the public one for its entity (the previous one must already be superseded).
pub async fn approve(
    conn: &mut PgConnection,
    id: Uuid,
    entity: Option<(&str, Uuid)>,
    by: Uuid,
    note: Option<&str>,
) -> Res<ImageRow> {
    sqlx::query_as(&format!(
        "UPDATE entity_images SET status = 'approved', decided_by = $2, decided_at = now(), decision_note = $3,
                entity_type = coalesce($4, entity_type), entity_id = coalesce($5, entity_id)
         WHERE id = $1 RETURNING {COLS}"
    ))
    .bind(id)
    .bind(by)
    .bind(note)
    .bind(entity.map(|e| e.0))
    .bind(entity.map(|e| e.1))
    .fetch_one(conn)
    .await
}

pub async fn reject(conn: &mut PgConnection, id: Uuid, by: Uuid, note: Option<&str>) -> Res<ImageRow> {
    sqlx::query_as(&format!(
        "UPDATE entity_images SET status = 'rejected', decided_by = $2, decided_at = now(), decision_note = $3
         WHERE id = $1 RETURNING {COLS}"
    ))
    .bind(id)
    .bind(by)
    .bind(note)
    .fetch_one(conn)
    .await
}

pub async fn delete(conn: &mut PgConnection, id: Uuid) -> Res<()> {
    sqlx::query("DELETE FROM entity_images WHERE id = $1")
        .bind(id)
        .execute(conn)
        .await?;
    Ok(())
}

/// A draft becomes part of a proposal and waits for the proposal's review.
pub async fn attach_to_proposal(conn: &mut PgConnection, id: Uuid, proposal: Uuid) -> Res<()> {
    sqlx::query("UPDATE entity_images SET proposal_id = $2, status = 'pending' WHERE id = $1")
        .bind(id)
        .bind(proposal)
        .execute(conn)
        .await?;
    Ok(())
}

/// The image attached to a proposal, with its review state.
pub async fn for_proposals(conn: &mut PgConnection, ids: &[Uuid]) -> Res<HashMap<Uuid, Uuid>> {
    let rows: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT proposal_id, id FROM entity_images WHERE proposal_id = ANY($1) AND status IN ('pending','approved')",
    )
    .bind(ids)
    .fetch_all(conn)
    .await?;
    Ok(rows.into_iter().collect())
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct QueueRow {
    #[sqlx(flatten)]
    pub image: ImageRow,
    pub uploader: String,
    /// The proposed name, when the image belongs to an entity proposal.
    pub proposal_name: Option<String>,
    pub proposal_type: Option<String>,
    /// The image people see today, so a reviewer can compare.
    pub current_id: Option<Uuid>,
}

pub async fn queue(conn: &mut PgConnection, after: Option<(DateTime<Utc>, Uuid)>, limit: i64) -> Res<Vec<QueueRow>> {
    sqlx::query_as(
        "SELECT i.id, i.entity_type, i.entity_id, i.proposal_id, i.status, i.width, i.height, i.bytes, i.sizes, i.credit,
                i.created_by, i.created_at, i.decided_by, i.decided_at, i.decision_note,
                u.display_name AS uploader, p.name AS proposal_name, p.entity_type AS proposal_type,
                (SELECT c.id FROM entity_images c WHERE c.entity_type = i.entity_type AND c.entity_id = i.entity_id
                    AND c.status = 'approved') AS current_id
         FROM entity_images i JOIN users u ON u.id = i.created_by
         LEFT JOIN entity_proposals p ON p.id = i.proposal_id
         WHERE i.status = 'pending' AND ($1::timestamptz IS NULL OR (i.created_at, i.id) > ($1, $2))
         ORDER BY i.created_at, i.id LIMIT $3",
    )
    .bind(after.map(|a| a.0))
    .bind(after.map(|a| a.1))
    .bind(limit + 1)
    .fetch_all(conn)
    .await
}

/// Plain stored names of entities of one type (the primary-language name), for queue labels.
pub async fn entity_names(conn: &mut PgConnection, entity_type: &str, ids: &[Uuid]) -> Res<HashMap<Uuid, String>> {
    let Some((table, col)) = crate::entities::table_of(entity_type) else {
        return Ok(HashMap::new());
    };
    let rows: Vec<(Uuid, String)> = sqlx::query_as(&format!("SELECT id, {col} FROM {table} WHERE id = ANY($1)"))
        .bind(ids)
        .fetch_all(conn)
        .await?;
    Ok(rows.into_iter().collect())
}

/// When entities merge, the survivor keeps its own image, or inherits the merged one's if it had none.
/// Everything else of the merged entity is retired; returns the ids whose files can go.
pub async fn merge(conn: &mut PgConnection, kind: &str, from: Uuid, into: Uuid) -> Res<Vec<Uuid>> {
    if !TYPES.contains(&kind) {
        return Ok(Vec::new());
    }
    sqlx::query(
        "UPDATE entity_images SET entity_id = $3 WHERE entity_type = $1 AND entity_id = $2 AND status = 'approved'
           AND NOT EXISTS (SELECT 1 FROM entity_images k WHERE k.entity_type = $1 AND k.entity_id = $3 AND k.status = 'approved')",
    )
    .bind(kind)
    .bind(from)
    .bind(into)
    .execute(&mut *conn)
    .await?;
    sqlx::query_scalar(
        "UPDATE entity_images SET status = CASE WHEN status = 'approved' THEN 'superseded' ELSE 'rejected' END,
                decided_at = coalesce(decided_at, now()), decision_note = coalesce(decision_note, 'entity merged')
         WHERE entity_type = $1 AND entity_id = $2 AND status IN ('approved','pending') RETURNING id",
    )
    .bind(kind)
    .bind(from)
    .fetch_all(conn)
    .await
}
