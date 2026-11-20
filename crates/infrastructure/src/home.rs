//! View counting and the hourly home-page lists.
//!
//! Song score = ln(1 + views in 30 days)
//!            + 2   * ((bayes_rating - 1) / 4) * ln(1 + ratings)
//!            + 1.5 * ln(1 + visible comments in 30 days)
//! where bayes_rating = (5 * 3.0 + sum of ratings) / (5 + ratings): a song with few ratings is pulled
//! towards a neutral 3, and no ratings means no rating contribution. Albums score their own views plus
//! the average score of their songs; artists are ranked by views of their page plus views of their songs.

use super::browse::PUBLISHED;
use chrono::{DateTime, Utc};
use sqlx::PgConnection;
use uuid::Uuid;

type Res<T> = Result<T, sqlx::Error>;

/// Items per list.
pub const LIST_SIZE: i64 = 10;
/// Days of views that count towards "popular".
pub const WINDOW_DAYS: i32 = 30;
pub const LISTS: [&str; 5] = [
    "popular_songs",
    "new_songs",
    "popular_albums",
    "new_albums",
    "popular_artists",
];

/// Records one view unless this viewer already counted today. Returns the target's total views, or
/// `None` when the target does not exist (or is not public).
pub async fn record_view(conn: &mut PgConnection, kind: &str, id: Uuid, seen_hash: &str) -> Res<Option<i64>> {
    let exists: bool = match kind {
        "song" => {
            sqlx::query_scalar(&format!(
                "SELECT EXISTS (SELECT 1 FROM compositions c WHERE c.id = $1 AND {PUBLISHED})"
            ))
            .bind(id)
            .fetch_one(&mut *conn)
            .await?
        }
        "artist" => {
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM artists WHERE id = $1)")
                .bind(id)
                .fetch_one(&mut *conn)
                .await?
        }
        "album" => {
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM albums WHERE id = $1)")
                .bind(id)
                .fetch_one(&mut *conn)
                .await?
        }
        _ => false,
    };
    if !exists {
        return Ok(None);
    }
    let fresh = sqlx::query("INSERT INTO view_seen (seen_hash, day) VALUES ($1, current_date) ON CONFLICT DO NOTHING")
        .bind(seen_hash)
        .execute(&mut *conn)
        .await?
        .rows_affected()
        > 0;
    if fresh {
        sqlx::query(
            "INSERT INTO view_counts (kind, entity_id, day, views) VALUES ($1, $2, current_date, 1)
             ON CONFLICT (kind, entity_id, day) DO UPDATE SET views = view_counts.views + 1",
        )
        .bind(kind)
        .bind(id)
        .execute(&mut *conn)
        .await?;
    }
    total_views(conn, kind, id).await.map(Some)
}

pub async fn total_views(conn: &mut PgConnection, kind: &str, id: Uuid) -> Res<i64> {
    sqlx::query_scalar("SELECT coalesce(sum(views), 0)::bigint FROM view_counts WHERE kind = $1 AND entity_id = $2")
        .bind(kind)
        .bind(id)
        .fetch_one(conn)
        .await
}

pub async fn purge_seen(conn: &mut PgConnection) -> Res<u64> {
    Ok(sqlx::query("DELETE FROM view_seen WHERE day < current_date - 1")
        .execute(conn)
        .await?
        .rows_affected())
}

pub async fn last_run(conn: &mut PgConnection) -> Res<Option<DateTime<Utc>>> {
    sqlx::query_scalar("SELECT computed_at FROM home_list_runs WHERE id = 1")
        .fetch_optional(conn)
        .await
}

/// Rebuilds every list in one transaction (the caller's). Concurrent refreshes queue on the lock.
pub async fn refresh(conn: &mut PgConnection) -> Res<DateTime<Utc>> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('home-lists', 0))")
        .execute(&mut *conn)
        .await?;
    sqlx::query("DELETE FROM home_lists").execute(&mut *conn).await?;

    // One row per public song with everything its score needs.
    sqlx::query(&format!(
        "CREATE TEMP TABLE song_scores ON COMMIT DROP AS
         WITH v AS (SELECT entity_id AS id, sum(views)::bigint AS views FROM view_counts
                    WHERE kind = 'song' AND day > current_date - {WINDOW_DAYS} GROUP BY 1),
              r AS (SELECT rec.composition_id AS id, count(*)::int AS n, avg(rt.value)::float8 AS avg, sum(rt.value)::float8 AS total
                    FROM ratings rt JOIN contributions x ON x.id = rt.contribution_id AND x.published_revision_id IS NOT NULL
                    JOIN recordings rec ON rec.id = x.recording_id GROUP BY 1),
              m AS (SELECT rec.composition_id AS id, count(*)::int AS n
                    FROM comments cm JOIN contributions x ON x.id = cm.contribution_id AND x.published_revision_id IS NOT NULL
                    JOIN recordings rec ON rec.id = x.recording_id
                    WHERE cm.status = 'visible' AND cm.created_at > now() - interval '{WINDOW_DAYS} days' GROUP BY 1)
         SELECT c.id, coalesce(v.views, 0) AS views, coalesce(r.n, 0) AS rating_count, r.avg AS rating_avg, coalesce(m.n, 0) AS comments,
                ln(1 + coalesce(v.views, 0))
                + 2.0 * (((15.0 + coalesce(r.total, 0)) / (5 + coalesce(r.n, 0)) - 1) / 4.0) * ln(1 + coalesce(r.n, 0))
                + 1.5 * ln(1 + coalesce(m.n, 0)) AS score
         FROM compositions c LEFT JOIN v ON v.id = c.id LEFT JOIN r ON r.id = c.id LEFT JOIN m ON m.id = c.id
         WHERE {PUBLISHED}"
    ))
    .execute(&mut *conn)
    .await?;

    sqlx::query(&format!(
        "INSERT INTO home_lists (list, rank, entity_id, score, views, rating_avg, rating_count, comments)
         SELECT 'popular_songs', row_number() OVER (ORDER BY score DESC, id), id, score, views, rating_avg, rating_count, comments
         FROM song_scores WHERE score > 0 ORDER BY score DESC, id LIMIT {LIST_SIZE}"
    ))
    .execute(&mut *conn)
    .await?;

    // New songs: by when their first lyric was published.
    sqlx::query(&format!(
        "INSERT INTO home_lists (list, rank, entity_id, score, views, rating_avg, rating_count, comments)
         SELECT 'new_songs', row_number() OVER (ORDER BY f.first DESC, s.id), s.id, extract(epoch FROM f.first),
                s.views, s.rating_avg, s.rating_count, s.comments
         FROM song_scores s JOIN (
             SELECT rec.composition_id AS id, min(pe.created_at) AS first
             FROM publication_events pe JOIN contributions x ON x.id = pe.contribution_id
             JOIN recordings rec ON rec.id = x.recording_id GROUP BY 1) f ON f.id = s.id
         ORDER BY f.first DESC, s.id LIMIT {LIST_SIZE}"
    ))
    .execute(&mut *conn)
    .await?;

    // Albums that have at least one public song.
    sqlx::query(
        "CREATE TEMP TABLE album_scores ON COMMIT DROP AS
         WITH av AS (SELECT entity_id AS id, sum(views)::bigint AS views FROM view_counts
                     WHERE kind = 'album' AND day > current_date - 30 GROUP BY 1),
              t AS (SELECT t.album_id AS id, avg(s.score) AS avg_score, count(*) AS songs
                    FROM album_tracks t JOIN recordings rec ON rec.id = t.recording_id
                    JOIN song_scores s ON s.id = rec.composition_id GROUP BY 1)
         SELECT a.id, a.created_at, coalesce(av.views, 0) AS views, ln(1 + coalesce(av.views, 0)) + t.avg_score AS score
         FROM albums a JOIN t ON t.id = a.id LEFT JOIN av ON av.id = a.id",
    )
    .execute(&mut *conn)
    .await?;
    sqlx::query(&format!(
        "INSERT INTO home_lists (list, rank, entity_id, score, views)
         SELECT 'popular_albums', row_number() OVER (ORDER BY score DESC, id), id, score, views
         FROM album_scores WHERE score > 0 ORDER BY score DESC, id LIMIT {LIST_SIZE}"
    ))
    .execute(&mut *conn)
    .await?;
    sqlx::query(&format!(
        "INSERT INTO home_lists (list, rank, entity_id, score, views)
         SELECT 'new_albums', row_number() OVER (ORDER BY created_at DESC, id), id, extract(epoch FROM created_at), views
         FROM album_scores ORDER BY created_at DESC, id LIMIT {LIST_SIZE}"
    ))
    .execute(&mut *conn)
    .await?;

    // Artists: views of their page plus views of the songs they are credited on, in any role.
    sqlx::query(&format!(
        "INSERT INTO home_lists (list, rank, entity_id, score, views)
         SELECT 'popular_artists', row_number() OVER (ORDER BY views DESC, id), id, views, views
         FROM (
             SELECT a.id, coalesce(pv.views, 0) + coalesce(sv.views, 0) AS views
             FROM artists a
             LEFT JOIN (SELECT entity_id AS id, sum(views)::bigint AS views FROM view_counts
                        WHERE kind = 'artist' AND day > current_date - {WINDOW_DAYS} GROUP BY 1) pv ON pv.id = a.id
             LEFT JOIN (SELECT u.artist_id AS id, sum(s.views)::bigint AS views FROM (
                            SELECT DISTINCT k.artist_id, coalesce(k.composition_id, rec.composition_id) AS song
                            FROM credits k LEFT JOIN recordings rec ON rec.id = k.recording_id
                            WHERE k.composition_id IS NOT NULL OR k.recording_id IS NOT NULL) u
                        JOIN song_scores s ON s.id = u.song GROUP BY 1) sv ON sv.id = a.id
         ) x WHERE views > 0 ORDER BY views DESC, id LIMIT {LIST_SIZE}"
    ))
    .execute(&mut *conn)
    .await?;

    sqlx::query_scalar(
        "INSERT INTO home_list_runs (id, computed_at) VALUES (1, now())
         ON CONFLICT (id) DO UPDATE SET computed_at = now() RETURNING computed_at",
    )
    .fetch_one(conn)
    .await
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ListRow {
    pub list: String,
    pub rank: i16,
    pub entity_id: Uuid,
    pub views: i64,
    pub rating_avg: Option<f64>,
    pub rating_count: i32,
    pub comments: i32,
}

pub async fn read(conn: &mut PgConnection) -> Res<Vec<ListRow>> {
    sqlx::query_as(
        "SELECT list, rank, entity_id, views, rating_avg, rating_count, comments FROM home_lists ORDER BY list, rank",
    )
    .fetch_all(conn)
    .await
}

/// Featured songs, newest pick first, only those still public.
pub async fn picks(conn: &mut PgConnection) -> Res<Vec<Uuid>> {
    sqlx::query_scalar(&format!(
        "SELECT c.id FROM editors_picks e JOIN compositions c ON c.id = e.composition_id
         WHERE {PUBLISHED} ORDER BY e.picked_at DESC, c.id LIMIT {LIST_SIZE}"
    ))
    .fetch_all(conn)
    .await
}

pub async fn is_picked(conn: &mut PgConnection, id: Uuid) -> Res<bool> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM editors_picks WHERE composition_id = $1)")
        .bind(id)
        .fetch_one(conn)
        .await
}

/// `false` when the song is not public (or does not exist).
pub async fn add_pick(conn: &mut PgConnection, id: Uuid, by: Uuid) -> Res<bool> {
    let public: bool = sqlx::query_scalar(&format!(
        "SELECT EXISTS (SELECT 1 FROM compositions c WHERE c.id = $1 AND {PUBLISHED})"
    ))
    .bind(id)
    .fetch_one(&mut *conn)
    .await?;
    if public {
        sqlx::query("INSERT INTO editors_picks (composition_id, picked_by) VALUES ($1, $2) ON CONFLICT DO NOTHING")
            .bind(id)
            .bind(by)
            .execute(conn)
            .await?;
    }
    Ok(public)
}

pub async fn remove_pick(conn: &mut PgConnection, id: Uuid) -> Res<()> {
    sqlx::query("DELETE FROM editors_picks WHERE composition_id = $1")
        .bind(id)
        .execute(conn)
        .await?;
    Ok(())
}
