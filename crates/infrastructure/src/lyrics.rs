//! SQL for lyric contributions, revisions, publication events, settings and idempotency keys.
//! No business rules: callers hold the locks and decide.

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgConnection;
use uuid::Uuid;

type Res<T> = Result<T, sqlx::Error>;
type After = Option<(DateTime<Utc>, Uuid)>;

const CONTRIBUTION: &str = "SELECT c.id, c.recording_id, c.type AS kind, c.language_id,
        coalesce(l.iso639_1, l.iso639_3) AS language,
        c.script_id, s.iso15924 AS script, s.is_rtl AS script_rtl,
        c.is_primary, c.status, c.current_revision_id, c.published_revision_id,
        c.source_contribution_id, c.review_note, c.created_by, c.created_at, c.updated_at
    FROM contributions c
    JOIN languages l ON l.id = c.language_id
    JOIN scripts s ON s.id = c.script_id";

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ContributionRow {
    pub id: Uuid,
    pub recording_id: Uuid,
    pub kind: String,
    pub language_id: Uuid,
    pub language: String,
    pub script_id: Uuid,
    pub script: String,
    pub script_rtl: bool,
    pub is_primary: bool,
    pub status: String,
    pub current_revision_id: Option<Uuid>,
    pub published_revision_id: Option<Uuid>,
    pub source_contribution_id: Option<Uuid>,
    pub review_note: Option<String>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RevisionRow {
    pub id: Uuid,
    pub contribution_id: Uuid,
    pub revision_no: i32,
    pub parent_revision_id: Option<Uuid>,
    pub body: String,
    pub note: Option<String>,
    pub restored_from_revision_id: Option<Uuid>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    /// Was this revision ever public?
    pub published: bool,
}

const REVISION: &str = "SELECT r.id, r.contribution_id, r.revision_no, r.parent_revision_id, r.body, r.note,
        r.restored_from_revision_id, r.created_by, r.created_at,
        EXISTS (SELECT 1 FROM publication_events e WHERE e.revision_id = r.id) AS published
    FROM lyric_revisions r";

pub async fn recording_exists(conn: &mut PgConnection, id: Uuid) -> Res<bool> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM recordings WHERE id = $1)")
        .bind(id)
        .fetch_one(conn)
        .await
}

pub async fn get_contribution(conn: &mut PgConnection, id: Uuid, lock: bool) -> Res<Option<ContributionRow>> {
    // Lock only the contributions row; the joined reference tables are immutable lookups.
    let sql = format!(
        "{CONTRIBUTION} WHERE c.id = $1{}",
        if lock { " FOR UPDATE OF c" } else { "" }
    );
    sqlx::query_as(&sql).bind(id).fetch_optional(conn).await
}

pub struct NewContribution {
    pub recording_id: Uuid,
    pub kind: String,
    pub language_id: Uuid,
    pub script_id: Uuid,
    pub source_contribution_id: Option<Uuid>,
    pub created_by: Uuid,
}

pub async fn insert_contribution(conn: &mut PgConnection, n: &NewContribution) -> Res<Uuid> {
    sqlx::query_scalar(
        "INSERT INTO contributions (recording_id, type, language_id, script_id, source_contribution_id, status, created_by)
         VALUES ($1, $2, $3, $4, $5, 'pending', $6) RETURNING id",
    )
    .bind(n.recording_id)
    .bind(&n.kind)
    .bind(n.language_id)
    .bind(n.script_id)
    .bind(n.source_contribution_id)
    .bind(n.created_by)
    .fetch_one(conn)
    .await
}

/// Appends the next revision. Callers hold the contribution's row lock; the unique
/// `(contribution_id, parent_revision_id)` index backs that up.
pub async fn insert_revision(
    conn: &mut PgConnection,
    contribution_id: Uuid,
    parent: Option<Uuid>,
    body: &str,
    note: Option<&str>,
    restored_from: Option<Uuid>,
    by: Uuid,
) -> Res<Uuid> {
    sqlx::query_scalar(
        "INSERT INTO lyric_revisions (contribution_id, revision_no, parent_revision_id, body, note, restored_from_revision_id, created_by)
         VALUES ($1, (SELECT coalesce(max(revision_no), 0) + 1 FROM lyric_revisions WHERE contribution_id = $1),
                 $2, $3, $4, $5, $6) RETURNING id",
    )
    .bind(contribution_id)
    .bind(parent)
    .bind(body)
    .bind(note)
    .bind(restored_from)
    .bind(by)
    .fetch_one(conn)
    .await
}

pub async fn get_revision(conn: &mut PgConnection, id: Uuid) -> Res<Option<RevisionRow>> {
    sqlx::query_as(&format!("{REVISION} WHERE r.id = $1"))
        .bind(id)
        .fetch_optional(conn)
        .await
}

/// Moves the head and its review state. `review_note` replaces the previous note.
pub async fn set_head(
    conn: &mut PgConnection,
    id: Uuid,
    head: Uuid,
    status: &str,
    review_note: Option<&str>,
) -> Res<()> {
    sqlx::query(
        "UPDATE contributions SET current_revision_id = $2, status = $3, review_note = $4, updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(head)
    .bind(status)
    .bind(review_note)
    .execute(conn)
    .await
    .map(|_| ())
}

pub async fn set_status(conn: &mut PgConnection, id: Uuid, status: &str, review_note: Option<&str>) -> Res<()> {
    sqlx::query("UPDATE contributions SET status = $2, review_note = $3, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(status)
        .bind(review_note)
        .execute(conn)
        .await
        .map(|_| ())
}

/// Serialises everything that can decide the primary of one slot (transaction-scoped).
pub async fn lock_slot(conn: &mut PgConnection, recording: Uuid, kind: &str, language: Uuid, script: Uuid) -> Res<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("lyrics-slot|{recording}|{kind}|{language}|{script}"))
        .execute(conn)
        .await
        .map(|_| ())
}

pub async fn slot_primary(
    conn: &mut PgConnection,
    recording: Uuid,
    kind: &str,
    language: Uuid,
    script: Uuid,
) -> Res<Option<Uuid>> {
    sqlx::query_scalar(
        "SELECT id FROM contributions
         WHERE recording_id = $1 AND type = $2 AND language_id = $3 AND script_id = $4 AND is_primary",
    )
    .bind(recording)
    .bind(kind)
    .bind(language)
    .bind(script)
    .fetch_optional(conn)
    .await
}

/// Makes `revision` the public one, optionally as the slot's primary, and records the event.
pub async fn publish(
    conn: &mut PgConnection,
    id: Uuid,
    revision: Uuid,
    actor: Uuid,
    primary: bool,
    review_note: Option<&str>,
) -> Res<()> {
    sqlx::query(
        "UPDATE contributions SET published_revision_id = $2, status = 'published', review_note = $3,
                is_primary = is_primary OR $4, updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(revision)
    .bind(review_note)
    .bind(primary)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "INSERT INTO publication_events (contribution_id, revision_id, actor_id, became_primary) VALUES ($1, $2, $3, $4)",
    )
    .bind(id)
    .bind(revision)
    .bind(actor)
    .bind(primary)
    .execute(conn)
    .await
    .map(|_| ())
}

pub async fn published_count(conn: &mut PgConnection, user: Uuid) -> Res<i64> {
    sqlx::query_scalar("SELECT count(*) FROM contributions WHERE created_by = $1 AND published_revision_id IS NOT NULL")
        .bind(user)
        .fetch_one(conn)
        .await
}

/// Public list for one recording: only contributions that have a published revision.
pub async fn list_published(
    conn: &mut PgConnection,
    recording: Uuid,
    kind: Option<&str>,
    language: Option<Uuid>,
    after: After,
    limit: i64,
) -> Res<Vec<ContributionRow>> {
    let (ts, id) = after.unzip();
    sqlx::query_as(&format!(
        "{CONTRIBUTION}
         WHERE c.recording_id = $1 AND c.published_revision_id IS NOT NULL
           AND ($2::text IS NULL OR c.type = $2) AND ($3::uuid IS NULL OR c.language_id = $3)
           AND ($4::timestamptz IS NULL OR (c.created_at, c.id) < ($4, $5))
         ORDER BY c.created_at DESC, c.id DESC LIMIT $6"
    ))
    .bind(recording)
    .bind(kind)
    .bind(language)
    .bind(ts)
    .bind(id)
    .bind(limit)
    .fetch_all(conn)
    .await
}

/// The author's own contributions in any state.
pub async fn list_by_creator(
    conn: &mut PgConnection,
    user: Uuid,
    after: After,
    limit: i64,
) -> Res<Vec<ContributionRow>> {
    let (ts, id) = after.unzip();
    sqlx::query_as(&format!(
        "{CONTRIBUTION}
         WHERE c.created_by = $1 AND ($2::timestamptz IS NULL OR (c.created_at, c.id) < ($2, $3))
         ORDER BY c.created_at DESC, c.id DESC LIMIT $4"
    ))
    .bind(user)
    .bind(ts)
    .bind(id)
    .bind(limit)
    .fetch_all(conn)
    .await
}

/// Moderation queue: heads awaiting a decision, oldest request first.
pub async fn list_queue(conn: &mut PgConnection, after: After, limit: i64) -> Res<Vec<ContributionRow>> {
    let (ts, id) = after.unzip();
    sqlx::query_as(&format!(
        "{CONTRIBUTION}
         WHERE c.status IN ('pending', 'approved') AND ($1::timestamptz IS NULL OR (c.created_at, c.id) > ($1, $2))
         ORDER BY c.created_at ASC, c.id ASC LIMIT $3"
    ))
    .bind(ts)
    .bind(id)
    .bind(limit)
    .fetch_all(conn)
    .await
}

/// History newest first. `public_only` limits it to revisions that were ever published.
pub async fn list_revisions(
    conn: &mut PgConnection,
    contribution: Uuid,
    public_only: bool,
    after: After,
    limit: i64,
) -> Res<Vec<RevisionRow>> {
    let (ts, id) = after.unzip();
    sqlx::query_as(&format!(
        "{REVISION}
         WHERE r.contribution_id = $1
           AND (NOT $2 OR EXISTS (SELECT 1 FROM publication_events e WHERE e.revision_id = r.id))
           AND ($3::timestamptz IS NULL OR (r.created_at, r.id) < ($3, $4))
         ORDER BY r.created_at DESC, r.id DESC LIMIT $5"
    ))
    .bind(contribution)
    .bind(public_only)
    .bind(ts)
    .bind(id)
    .bind(limit)
    .fetch_all(conn)
    .await
}

pub async fn get_setting(conn: &mut PgConnection, key: &str) -> Res<Option<Value>> {
    sqlx::query_scalar("SELECT value FROM settings WHERE key = $1")
        .bind(key)
        .fetch_optional(conn)
        .await
}

pub async fn put_setting(conn: &mut PgConnection, key: &str, value: &Value, by: Uuid) -> Res<()> {
    sqlx::query(
        "INSERT INTO settings (key, value, updated_by) VALUES ($1, $2, $3)
         ON CONFLICT (key) DO UPDATE SET value = $2, updated_by = $3, updated_at = now()",
    )
    .bind(key)
    .bind(value)
    .bind(by)
    .execute(conn)
    .await
    .map(|_| ())
}

/// Outcome of claiming an idempotency key.
pub enum Claim {
    /// First use: the caller does the work and calls `finish`.
    Fresh,
    /// Seen before (committed): same request replays its result; a different one is a misuse.
    Seen {
        fingerprint: String,
        result_kind: Option<String>,
        result_id: Option<Uuid>,
    },
}

/// Inserts the key inside the caller's transaction. A concurrent claimant of the same key
/// blocks on the unique index until this transaction ends, then sees the committed row.
pub async fn claim_key(conn: &mut PgConnection, user: Uuid, key: &str, fingerprint: &str) -> Res<Claim> {
    let inserted = sqlx::query(
        "INSERT INTO idempotency_keys (user_id, key, fingerprint) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
    )
    .bind(user)
    .bind(key)
    .bind(fingerprint)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    if inserted == 1 {
        return Ok(Claim::Fresh);
    }
    let (fingerprint, result_kind, result_id): (String, Option<String>, Option<Uuid>) = sqlx::query_as(
        "SELECT fingerprint, result_kind, result_id FROM idempotency_keys WHERE user_id = $1 AND key = $2",
    )
    .bind(user)
    .bind(key)
    .fetch_one(conn)
    .await?;
    Ok(Claim::Seen {
        fingerprint,
        result_kind,
        result_id,
    })
}

pub async fn finish_key(conn: &mut PgConnection, user: Uuid, key: &str, kind: &str, id: Uuid) -> Res<()> {
    sqlx::query("UPDATE idempotency_keys SET result_kind = $3, result_id = $4 WHERE user_id = $1 AND key = $2")
        .bind(user)
        .bind(key)
        .bind(kind)
        .bind(id)
        .execute(conn)
        .await
        .map(|_| ())
}

/// Which of `ids` have a published revision (used to avoid pointing the public at hidden work).
pub async fn published_ids(conn: &mut PgConnection, ids: &[Uuid]) -> Res<Vec<Uuid>> {
    sqlx::query_scalar("SELECT id FROM contributions WHERE id = ANY($1) AND published_revision_id IS NOT NULL")
        .bind(ids)
        .fetch_all(conn)
        .await
}
