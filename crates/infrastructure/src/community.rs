//! SQL for comparison and community features. No business rules: use cases hold the locks and decide.

use chrono::{DateTime, Utc};
use sqlx::PgConnection;
use uuid::Uuid;

type Res<T> = Result<T, sqlx::Error>;
pub type After = Option<(DateTime<Utc>, Uuid)>;

fn ts(a: &After) -> Option<DateTime<Utc>> {
    a.map(|x| x.0)
}
fn aid(a: &After) -> Option<Uuid> {
    a.map(|x| x.1)
}

// ---------- comparison ----------

/// A contribution as the public sees it: the published revision only.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ResolvedRow {
    pub id: Uuid,
    pub recording_id: Uuid,
    pub recording_title: String,
    pub composition_id: Uuid,
    pub composition_title: String,
    pub kind: String,
    pub language: String,
    pub script: String,
    pub script_rtl: bool,
    pub is_primary: bool,
    pub revision_id: Uuid,
    pub revision_no: i32,
    pub body: String,
    pub revised_at: DateTime<Utc>,
    /// Published contributions in the same slot (type + language + script) of this recording.
    pub slot_size: i64,
}

pub async fn resolve_published(conn: &mut PgConnection, ids: &[Uuid]) -> Res<Vec<ResolvedRow>> {
    sqlx::query_as(
        "SELECT c.id, c.recording_id, rec.title AS recording_title, rec.composition_id, w.title AS composition_title,
                c.type AS kind, coalesce(l.iso639_1, l.iso639_3) AS language, s.iso15924 AS script, s.is_rtl AS script_rtl,
                c.is_primary, r.id AS revision_id, r.revision_no, r.body, r.created_at AS revised_at,
                (SELECT count(*) FROM contributions x WHERE x.recording_id = c.recording_id AND x.type = c.type
                    AND x.language_id = c.language_id AND x.script_id = c.script_id
                    AND x.published_revision_id IS NOT NULL) AS slot_size
         FROM contributions c
         JOIN lyric_revisions r ON r.id = c.published_revision_id
         JOIN recordings rec ON rec.id = c.recording_id
         JOIN compositions w ON w.id = rec.composition_id
         JOIN languages l ON l.id = c.language_id
         JOIN scripts s ON s.id = c.script_id
         WHERE c.id = ANY($1)",
    )
    .bind(ids)
    .fetch_all(conn)
    .await
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SavedRow {
    pub id: Uuid,
    pub name: String,
    pub contribution_ids: Vec<Uuid>,
    pub created_at: DateTime<Utc>,
}

pub async fn insert_saved(conn: &mut PgConnection, owner: Uuid, name: &str, ids: &[Uuid]) -> Res<SavedRow> {
    sqlx::query_as(
        "INSERT INTO saved_comparisons (owner_id, name, contribution_ids) VALUES ($1, $2, $3)
         RETURNING id, name, contribution_ids, created_at",
    )
    .bind(owner)
    .bind(name)
    .bind(ids)
    .fetch_one(conn)
    .await
}

pub async fn list_saved(conn: &mut PgConnection, owner: Uuid, after: After, limit: i64) -> Res<Vec<SavedRow>> {
    sqlx::query_as(
        "SELECT id, name, contribution_ids, created_at FROM saved_comparisons
         WHERE owner_id = $1 AND ($2::timestamptz IS NULL OR (created_at, id) < ($2, $3))
         ORDER BY created_at DESC, id DESC LIMIT $4",
    )
    .bind(owner)
    .bind(ts(&after))
    .bind(aid(&after))
    .bind(limit)
    .fetch_all(conn)
    .await
}

pub async fn get_saved(conn: &mut PgConnection, owner: Uuid, id: Uuid) -> Res<Option<SavedRow>> {
    sqlx::query_as(
        "SELECT id, name, contribution_ids, created_at FROM saved_comparisons WHERE id = $1 AND owner_id = $2",
    )
    .bind(id)
    .bind(owner)
    .fetch_optional(conn)
    .await
}

pub async fn delete_saved(conn: &mut PgConnection, owner: Uuid, id: Uuid) -> Res<bool> {
    Ok(
        sqlx::query("DELETE FROM saved_comparisons WHERE id = $1 AND owner_id = $2")
            .bind(id)
            .bind(owner)
            .execute(conn)
            .await?
            .rows_affected()
            > 0,
    )
}

// ---------- abuse controls ----------

pub async fn lock_user_action(conn: &mut PgConnection, user: Uuid, action: &str) -> Res<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1 || ':' || $2, 0))")
        .bind(user.to_string())
        .bind(action)
        .execute(conn)
        .await
        .map(|_| ())
}

/// Actions in the window and the oldest one still inside it.
pub async fn recent_actions(
    conn: &mut PgConnection,
    user: Uuid,
    action: &str,
    window_secs: i64,
) -> Res<(i64, Option<DateTime<Utc>>)> {
    sqlx::query_as(
        "SELECT count(*), min(at) FROM abuse_events
         WHERE user_id = $1 AND action = $2 AND at > clock_timestamp() - make_interval(secs => $3)",
    )
    .bind(user)
    .bind(action)
    .bind(window_secs as f64)
    .fetch_one(conn)
    .await
}

pub async fn note_action(conn: &mut PgConnection, user: Uuid, action: &str) -> Res<()> {
    sqlx::query("INSERT INTO abuse_events (user_id, action) VALUES ($1, $2)")
        .bind(user)
        .bind(action)
        .execute(&mut *conn)
        .await?;
    // Housekeeping: nothing older than a day can matter for any window.
    sqlx::query("DELETE FROM abuse_events WHERE user_id = $1 AND at < now() - interval '1 day'")
        .bind(user)
        .execute(conn)
        .await
        .map(|_| ())
}

pub async fn recent_duplicate_comment(conn: &mut PgConnection, user: Uuid, body: &str, window_secs: i64) -> Res<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM comments WHERE author_id = $1 AND lower(body) = lower($2)
            AND created_at > now() - make_interval(secs => $3))",
    )
    .bind(user)
    .bind(body)
    .bind(window_secs as f64)
    .fetch_one(conn)
    .await
}

// ---------- targets ----------

/// The creator of a currently published contribution, or `None` when it is not public.
pub async fn published_creator(conn: &mut PgConnection, id: Uuid) -> Res<Option<Uuid>> {
    sqlx::query_scalar("SELECT created_by FROM contributions WHERE id = $1 AND published_revision_id IS NOT NULL")
        .bind(id)
        .fetch_optional(conn)
        .await
}

pub async fn published_ids(conn: &mut PgConnection, ids: &[Uuid]) -> Res<Vec<Uuid>> {
    sqlx::query_scalar("SELECT id FROM contributions WHERE id = ANY($1) AND published_revision_id IS NOT NULL")
        .bind(ids)
        .fetch_all(conn)
        .await
}

// ---------- ratings, reactions, bookmarks ----------

pub async fn upsert_rating(conn: &mut PgConnection, c: Uuid, user: Uuid, value: i16) -> Res<Option<i16>> {
    // Returns the previous value, so reputation can be awarded only on a first high rating.
    let before: Option<i16> =
        sqlx::query_scalar("SELECT value FROM ratings WHERE contribution_id = $1 AND user_id = $2")
            .bind(c)
            .bind(user)
            .fetch_optional(&mut *conn)
            .await?;
    sqlx::query(
        "INSERT INTO ratings (contribution_id, user_id, value) VALUES ($1, $2, $3)
         ON CONFLICT (contribution_id, user_id) DO UPDATE SET value = $3, updated_at = now()",
    )
    .bind(c)
    .bind(user)
    .bind(value)
    .execute(conn)
    .await?;
    Ok(before)
}

pub async fn delete_rating(conn: &mut PgConnection, c: Uuid, user: Uuid) -> Res<bool> {
    Ok(
        sqlx::query("DELETE FROM ratings WHERE contribution_id = $1 AND user_id = $2")
            .bind(c)
            .bind(user)
            .execute(conn)
            .await?
            .rows_affected()
            > 0,
    )
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Summary {
    pub rating_count: i64,
    pub rating_avg: Option<f64>,
    pub comment_count: i64,
}

pub async fn summary(conn: &mut PgConnection, c: Uuid) -> Res<Summary> {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM ratings WHERE contribution_id = $1) AS rating_count,
                (SELECT avg(value)::float8 FROM ratings WHERE contribution_id = $1) AS rating_avg,
                (SELECT count(*) FROM comments WHERE contribution_id = $1 AND status = 'visible') AS comment_count",
    )
    .bind(c)
    .fetch_one(conn)
    .await
}

pub async fn reaction_counts(conn: &mut PgConnection, c: Uuid) -> Res<Vec<(String, i64)>> {
    sqlx::query_as("SELECT kind, count(*) FROM reactions WHERE contribution_id = $1 GROUP BY kind ORDER BY kind")
        .bind(c)
        .fetch_all(conn)
        .await
}

pub async fn my_rating(conn: &mut PgConnection, c: Uuid, user: Uuid) -> Res<Option<i16>> {
    sqlx::query_scalar("SELECT value FROM ratings WHERE contribution_id = $1 AND user_id = $2")
        .bind(c)
        .bind(user)
        .fetch_optional(conn)
        .await
}

pub async fn my_reactions(conn: &mut PgConnection, c: Uuid, user: Uuid) -> Res<Vec<String>> {
    sqlx::query_scalar("SELECT kind FROM reactions WHERE contribution_id = $1 AND user_id = $2 ORDER BY kind")
        .bind(c)
        .bind(user)
        .fetch_all(conn)
        .await
}

pub async fn set_reaction(conn: &mut PgConnection, c: Uuid, user: Uuid, kind: &str, on: bool) -> Res<()> {
    let sql = if on {
        "INSERT INTO reactions (contribution_id, user_id, kind) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING"
    } else {
        "DELETE FROM reactions WHERE contribution_id = $1 AND user_id = $2 AND kind = $3"
    };
    sqlx::query(sql)
        .bind(c)
        .bind(user)
        .bind(kind)
        .execute(conn)
        .await
        .map(|_| ())
}

pub async fn set_bookmark(conn: &mut PgConnection, c: Uuid, user: Uuid, on: bool) -> Res<()> {
    let sql = if on {
        "INSERT INTO bookmarks (user_id, contribution_id) VALUES ($2, $1) ON CONFLICT DO NOTHING"
    } else {
        "DELETE FROM bookmarks WHERE contribution_id = $1 AND user_id = $2"
    };
    sqlx::query(sql).bind(c).bind(user).execute(conn).await.map(|_| ())
}

pub async fn is_bookmarked(conn: &mut PgConnection, c: Uuid, user: Uuid) -> Res<bool> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM bookmarks WHERE contribution_id = $1 AND user_id = $2)")
        .bind(c)
        .bind(user)
        .fetch_one(conn)
        .await
}

/// Bookmarked contributions that are still public, newest bookmark first.
pub async fn list_bookmarks(
    conn: &mut PgConnection,
    user: Uuid,
    after: After,
    limit: i64,
) -> Res<Vec<(Uuid, DateTime<Utc>)>> {
    sqlx::query_as(
        "SELECT b.contribution_id, b.created_at FROM bookmarks b
         JOIN contributions c ON c.id = b.contribution_id AND c.published_revision_id IS NOT NULL
         WHERE b.user_id = $1 AND ($2::timestamptz IS NULL OR (b.created_at, b.contribution_id) < ($2, $3))
         ORDER BY b.created_at DESC, b.contribution_id DESC LIMIT $4",
    )
    .bind(user)
    .bind(ts(&after))
    .bind(aid(&after))
    .bind(limit)
    .fetch_all(conn)
    .await
}

// ---------- comments ----------

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CommentRow {
    pub id: Uuid,
    pub contribution_id: Uuid,
    pub author_id: Uuid,
    pub author_name: String,
    pub body: String,
    pub status: String,
    pub hidden_by: Option<Uuid>,
    pub version: i32,
    pub created_at: DateTime<Utc>,
    pub edited_at: Option<DateTime<Utc>>,
}

const COMMENT: &str = "SELECT m.id, m.contribution_id, m.author_id, u.display_name AS author_name, m.body, m.status,
        m.hidden_by, m.version, m.created_at, m.edited_at
    FROM comments m JOIN users u ON u.id = m.author_id";

pub async fn insert_comment(conn: &mut PgConnection, c: Uuid, author: Uuid, body: &str) -> Res<Uuid> {
    sqlx::query_scalar("INSERT INTO comments (contribution_id, author_id, body) VALUES ($1, $2, $3) RETURNING id")
        .bind(c)
        .bind(author)
        .bind(body)
        .fetch_one(conn)
        .await
}

pub async fn get_comment(conn: &mut PgConnection, id: Uuid, lock: bool) -> Res<Option<CommentRow>> {
    let sql = format!(
        "{COMMENT} WHERE m.id = $1{}",
        if lock { " FOR UPDATE OF m" } else { "" }
    );
    sqlx::query_as(&sql).bind(id).fetch_optional(conn).await
}

/// Visible comments on a published contribution, oldest first.
pub async fn list_comments(conn: &mut PgConnection, c: Uuid, after: After, limit: i64) -> Res<Vec<CommentRow>> {
    let sql = format!(
        "{COMMENT} WHERE m.contribution_id = $1 AND m.status = 'visible'
         AND ($2::timestamptz IS NULL OR (m.created_at, m.id) > ($2, $3))
         ORDER BY m.created_at, m.id LIMIT $4"
    );
    sqlx::query_as(&sql)
        .bind(c)
        .bind(ts(&after))
        .bind(aid(&after))
        .bind(limit)
        .fetch_all(conn)
        .await
}

/// The author's own comments, including ones a moderator hid (so they can appeal); not removed ones.
pub async fn list_my_comments(conn: &mut PgConnection, user: Uuid, after: After, limit: i64) -> Res<Vec<CommentRow>> {
    let sql = format!(
        "{COMMENT} WHERE m.author_id = $1 AND m.status <> 'removed'
         AND ($2::timestamptz IS NULL OR (m.created_at, m.id) < ($2, $3))
         ORDER BY m.created_at DESC, m.id DESC LIMIT $4"
    );
    sqlx::query_as(&sql)
        .bind(user)
        .bind(ts(&after))
        .bind(aid(&after))
        .bind(limit)
        .fetch_all(conn)
        .await
}

pub async fn edit_comment(conn: &mut PgConnection, id: Uuid, body: &str) -> Res<()> {
    sqlx::query("UPDATE comments SET body = $2, version = version + 1, edited_at = now() WHERE id = $1")
        .bind(id)
        .bind(body)
        .execute(conn)
        .await
        .map(|_| ())
}

pub async fn set_comment_status(conn: &mut PgConnection, id: Uuid, status: &str, hidden_by: Option<Uuid>) -> Res<()> {
    sqlx::query("UPDATE comments SET status = $2, hidden_by = $3, version = version + 1 WHERE id = $1")
        .bind(id)
        .bind(status)
        .bind(hidden_by)
        .execute(conn)
        .await
        .map(|_| ())
}

// ---------- profiles and reputation ----------

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ProfileRow {
    pub user_id: Uuid,
    pub display_name: String,
    pub bio: String,
    pub joined_at: DateTime<Utc>,
    pub published_contributions: i64,
    pub comments: i64,
    pub reputation: i64,
}

/// Active members only; the email address is never selected.
pub async fn get_profile(conn: &mut PgConnection, id: Uuid) -> Res<Option<ProfileRow>> {
    sqlx::query_as(
        "SELECT u.id AS user_id, u.display_name, coalesce(p.bio, '') AS bio, u.created_at AS joined_at,
                (SELECT count(*) FROM contributions c WHERE c.created_by = u.id AND c.published_revision_id IS NOT NULL) AS published_contributions,
                (SELECT count(*) FROM comments m WHERE m.author_id = u.id AND m.status = 'visible') AS comments,
                coalesce((SELECT sum(points) FROM reputation_events e WHERE e.user_id = u.id), 0)::bigint AS reputation
         FROM users u LEFT JOIN user_profiles p ON p.user_id = u.id
         WHERE u.id = $1 AND u.status = 'active'",
    )
    .bind(id)
    .fetch_optional(conn)
    .await
}

pub async fn set_bio(conn: &mut PgConnection, user: Uuid, bio: &str) -> Res<()> {
    sqlx::query(
        "INSERT INTO user_profiles (user_id, bio) VALUES ($1, $2)
         ON CONFLICT (user_id) DO UPDATE SET bio = $2, updated_at = now()",
    )
    .bind(user)
    .bind(bio)
    .execute(conn)
    .await
    .map(|_| ())
}

pub async fn reputation(conn: &mut PgConnection, user: Uuid) -> Res<i64> {
    sqlx::query_scalar("SELECT coalesce(sum(points), 0)::bigint FROM reputation_events WHERE user_id = $1")
        .bind(user)
        .fetch_one(conn)
        .await
}

/// `true` when the event was new. A zero-point event is not worth a row.
pub async fn add_reputation(
    conn: &mut PgConnection,
    user: Uuid,
    kind: &str,
    points: i64,
    subject_type: &str,
    subject: Uuid,
) -> Res<bool> {
    if points == 0 {
        return Ok(false);
    }
    Ok(sqlx::query(
        "INSERT INTO reputation_events (user_id, kind, points, subject_type, subject_id) VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (user_id, kind, subject_id) DO NOTHING",
    )
    .bind(user)
    .bind(kind)
    .bind(points as i32)
    .bind(subject_type)
    .bind(subject)
    .execute(conn)
    .await?
    .rows_affected()
        > 0)
}

/// Gives back what moderation took for `subject`, once (the log stays append-only).
pub async fn reverse_penalties(conn: &mut PgConnection, user: Uuid, subject_type: &str, subject: Uuid) -> Res<i64> {
    sqlx::query_scalar(
        "WITH r AS (
            INSERT INTO reputation_events (user_id, kind, points, subject_type, subject_id)
            SELECT $1, 'penalty_reversed', -sum(points), $2, $3 FROM reputation_events
            WHERE user_id = $1 AND subject_id = $3 AND points < 0 HAVING sum(points) < 0
            ON CONFLICT (user_id, kind, subject_id) DO NOTHING RETURNING points)
         SELECT coalesce(sum(points), 0)::bigint FROM r",
    )
    .bind(user)
    .bind(subject_type)
    .bind(subject)
    .fetch_one(conn)
    .await
}

/// Did `user` create this contribution or write any of its revisions?
pub async fn authored(conn: &mut PgConnection, c: Uuid, user: Uuid) -> Res<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM contributions WHERE id = $1 AND created_by = $2)
             OR EXISTS (SELECT 1 FROM lyric_revisions WHERE contribution_id = $1 AND created_by = $2)",
    )
    .bind(c)
    .bind(user)
    .fetch_one(conn)
    .await
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ReputationEventRow {
    pub id: Uuid,
    pub kind: String,
    pub points: i32,
    pub subject_type: String,
    pub subject_id: Uuid,
    pub created_at: DateTime<Utc>,
}

pub async fn list_reputation(
    conn: &mut PgConnection,
    user: Uuid,
    after: After,
    limit: i64,
) -> Res<Vec<ReputationEventRow>> {
    sqlx::query_as(
        "SELECT id, kind, points, subject_type, subject_id, created_at FROM reputation_events
         WHERE user_id = $1 AND ($2::timestamptz IS NULL OR (created_at, id) < ($2, $3))
         ORDER BY created_at DESC, id DESC LIMIT $4",
    )
    .bind(user)
    .bind(ts(&after))
    .bind(aid(&after))
    .bind(limit)
    .fetch_all(conn)
    .await
}

// ---------- reports ----------

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ReportRow {
    pub id: Uuid,
    pub reporter_id: Uuid,
    pub target_type: String,
    pub target_id: Uuid,
    pub reason: String,
    pub detail: Option<String>,
    pub status: String,
    pub resolved_by: Option<Uuid>,
    pub resolution_note: Option<String>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

const REPORT: &str = "SELECT id, reporter_id, target_type, target_id, reason, detail, status, resolved_by,
        resolution_note, resolved_at, created_at FROM reports";

pub async fn insert_report(
    conn: &mut PgConnection,
    reporter: Uuid,
    target_type: &str,
    target: Uuid,
    reason: &str,
    detail: Option<&str>,
) -> Res<ReportRow> {
    sqlx::query_as(
        "INSERT INTO reports (reporter_id, target_type, target_id, reason, detail) VALUES ($1, $2, $3, $4, $5)
         RETURNING id, reporter_id, target_type, target_id, reason, detail, status, resolved_by, resolution_note, resolved_at, created_at",
    )
    .bind(reporter)
    .bind(target_type)
    .bind(target)
    .bind(reason)
    .bind(detail)
    .fetch_one(conn)
    .await
}

pub async fn get_report(conn: &mut PgConnection, id: Uuid, lock: bool) -> Res<Option<ReportRow>> {
    let sql = format!("{REPORT} WHERE id = $1{}", if lock { " FOR UPDATE" } else { "" });
    sqlx::query_as(&sql).bind(id).fetch_optional(conn).await
}

pub async fn report_queue(conn: &mut PgConnection, status: &str, after: After, limit: i64) -> Res<Vec<ReportRow>> {
    let sql = format!(
        "{REPORT} WHERE status = $1 AND ($2::timestamptz IS NULL OR (created_at, id) > ($2, $3))
         ORDER BY created_at, id LIMIT $4"
    );
    sqlx::query_as(&sql)
        .bind(status)
        .bind(ts(&after))
        .bind(aid(&after))
        .bind(limit)
        .fetch_all(conn)
        .await
}

pub async fn resolve_report(conn: &mut PgConnection, id: Uuid, by: Uuid, status: &str, note: Option<&str>) -> Res<()> {
    sqlx::query(
        "UPDATE reports SET status = $2, resolved_by = $3, resolution_note = $4, resolved_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(status)
    .bind(by)
    .bind(note)
    .execute(conn)
    .await
    .map(|_| ())
}

/// Who authored the reported thing (for reputation consequences).
pub async fn target_author(conn: &mut PgConnection, target_type: &str, id: Uuid) -> Res<Option<Uuid>> {
    let sql = match target_type {
        "comment" => "SELECT author_id FROM comments WHERE id = $1",
        _ => "SELECT created_by FROM contributions WHERE id = $1",
    };
    sqlx::query_scalar(sql).bind(id).fetch_optional(conn).await
}

// ---------- moderator notes ----------

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct NoteRow {
    pub id: Uuid,
    pub subject_type: String,
    pub subject_id: Uuid,
    pub author_id: Uuid,
    pub body: String,
    pub created_at: DateTime<Utc>,
}

pub async fn insert_note(conn: &mut PgConnection, st: &str, sid: Uuid, author: Uuid, body: &str) -> Res<NoteRow> {
    sqlx::query_as(
        "INSERT INTO moderator_notes (subject_type, subject_id, author_id, body) VALUES ($1, $2, $3, $4)
         RETURNING id, subject_type, subject_id, author_id, body, created_at",
    )
    .bind(st)
    .bind(sid)
    .bind(author)
    .bind(body)
    .fetch_one(conn)
    .await
}

pub async fn list_notes(conn: &mut PgConnection, st: &str, sid: Uuid, after: After, limit: i64) -> Res<Vec<NoteRow>> {
    sqlx::query_as(
        "SELECT id, subject_type, subject_id, author_id, body, created_at FROM moderator_notes
         WHERE subject_type = $1 AND subject_id = $2 AND ($3::timestamptz IS NULL OR (created_at, id) < ($3, $4))
         ORDER BY created_at DESC, id DESC LIMIT $5",
    )
    .bind(st)
    .bind(sid)
    .bind(ts(&after))
    .bind(aid(&after))
    .bind(limit)
    .fetch_all(conn)
    .await
}

pub async fn subject_exists(conn: &mut PgConnection, st: &str, id: Uuid) -> Res<bool> {
    let table = match st {
        "contribution" => "contributions",
        "comment" => "comments",
        "user" => "users",
        "report" => "reports",
        _ => return Ok(false),
    };
    sqlx::query_scalar(&format!("SELECT EXISTS (SELECT 1 FROM {table} WHERE id = $1)"))
        .bind(id)
        .fetch_one(conn)
        .await
}

// ---------- appeals ----------

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AppealRow {
    pub id: Uuid,
    pub appellant_id: Uuid,
    pub target_type: String,
    pub target_id: Uuid,
    pub message: String,
    pub status: String,
    pub decided_by: Option<Uuid>,
    pub decision_note: Option<String>,
    pub decided_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

const APPEAL: &str = "SELECT id, appellant_id, target_type, target_id, message, status, decided_by, decision_note,
        decided_at, created_at FROM appeals";

pub async fn insert_appeal(conn: &mut PgConnection, who: Uuid, target: Uuid, message: &str) -> Res<AppealRow> {
    sqlx::query_as(
        "INSERT INTO appeals (appellant_id, target_type, target_id, message) VALUES ($1, 'comment', $2, $3)
         RETURNING id, appellant_id, target_type, target_id, message, status, decided_by, decision_note, decided_at, created_at",
    )
    .bind(who)
    .bind(target)
    .bind(message)
    .fetch_one(conn)
    .await
}

pub async fn get_appeal(conn: &mut PgConnection, id: Uuid, lock: bool) -> Res<Option<AppealRow>> {
    let sql = format!("{APPEAL} WHERE id = $1{}", if lock { " FOR UPDATE" } else { "" });
    sqlx::query_as(&sql).bind(id).fetch_optional(conn).await
}

pub async fn appeal_queue(conn: &mut PgConnection, after: After, limit: i64) -> Res<Vec<AppealRow>> {
    let sql = format!(
        "{APPEAL} WHERE status = 'open' AND ($1::timestamptz IS NULL OR (created_at, id) > ($1, $2))
         ORDER BY created_at, id LIMIT $3"
    );
    sqlx::query_as(&sql)
        .bind(ts(&after))
        .bind(aid(&after))
        .bind(limit)
        .fetch_all(conn)
        .await
}

pub async fn my_appeals(conn: &mut PgConnection, who: Uuid, after: After, limit: i64) -> Res<Vec<AppealRow>> {
    let sql = format!(
        "{APPEAL} WHERE appellant_id = $1 AND ($2::timestamptz IS NULL OR (created_at, id) < ($2, $3))
         ORDER BY created_at DESC, id DESC LIMIT $4"
    );
    sqlx::query_as(&sql)
        .bind(who)
        .bind(ts(&after))
        .bind(aid(&after))
        .bind(limit)
        .fetch_all(conn)
        .await
}

pub async fn decide_appeal(conn: &mut PgConnection, id: Uuid, by: Uuid, status: &str, note: Option<&str>) -> Res<()> {
    sqlx::query("UPDATE appeals SET status = $2, decided_by = $3, decision_note = $4, decided_at = now() WHERE id = $1")
        .bind(id)
        .bind(status)
        .bind(by)
        .bind(note)
        .execute(conn)
        .await
        .map(|_| ())
}
