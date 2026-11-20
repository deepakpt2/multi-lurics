//! Entity suggestion, proposals and merges, all on the normalized `search_keys` index
//! (maintained by database triggers; see migration 0006).

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::PgConnection;
use uuid::Uuid;

type Res<T> = Result<T, sqlx::Error>;

/// Table and display column of each entity type. Fixed strings, never user input.
pub fn table_of(kind: &str) -> Option<(&'static str, &'static str)> {
    Some(match kind {
        "artist" => ("artists", "name"),
        "album" => ("albums", "title"),
        "label" => ("labels", "name"),
        "publisher" => ("publishers", "name"),
        "genre" => ("genres", "name"),
        "raga" => ("ragas", "name"),
        "deity" => ("deities", "name"),
        "tag" => ("tags", "name"),
        _ => return None,
    })
}

// ---------- suggest ----------

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Hit {
    pub entity_id: Uuid,
    /// Where the best match came from: `name`, `localization` or `alias`.
    pub origin: String,
    /// 1.0 exact (ignoring case, spacing, punctuation, diacritics), 0.9 prefix, else trigram similarity.
    pub score: f32,
}

pub async fn suggest(conn: &mut PgConnection, kind: &str, q: &str, limit: i64) -> Res<Vec<Hit>> {
    sqlx::query_as(
        "WITH q AS (SELECT lyrics_search_key($2) AS k, replace(lyrics_search_key($2), ' ', '') AS c, lyrics_phonetic_key($2) AS p, lyrics_phonetic_key($2, false) AS pp)
         SELECT entity_id, origin, score FROM (
             SELECT DISTINCT ON (s.entity_id) s.entity_id, s.origin,
                    (CASE WHEN s.compact = q.c THEN 1.0
                          WHEN s.key LIKE q.k || '%' THEN 0.9
                          WHEN length(q.p) >= 3 AND s.phon = q.p THEN 0.85
                          WHEN length(q.pp) >= 3 AND s.phon LIKE q.pp || '%' THEN 0.8
                          ELSE similarity(s.key, q.k) END)::float4 AS score
             FROM search_keys s, q
             WHERE s.entity_type = $1 AND q.c <> ''
               AND (s.compact = q.c OR s.key LIKE q.k || '%' OR s.key % q.k OR (length(q.p) >= 3 AND s.phon = q.p) OR (length(q.pp) >= 3 AND s.phon LIKE q.pp || '%'))
             ORDER BY s.entity_id, 3 DESC
         ) h ORDER BY score DESC, entity_id LIMIT $3",
    )
    .bind(kind)
    .bind(q)
    .bind(limit)
    .fetch_all(conn)
    .await
}

/// True when the text has at least one searchable character.
pub async fn has_key(conn: &mut PgConnection, q: &str) -> Res<bool> {
    sqlx::query_scalar("SELECT lyrics_search_key($1) <> ''")
        .bind(q)
        .fetch_one(conn)
        .await
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Facts {
    pub id: Uuid,
    pub name: String,
    pub kind: Option<String>,
    /// Artist disambiguation, language endonym.
    pub detail: Option<String>,
    pub year: Option<i16>,
    /// ISO code for languages and scripts.
    pub code: Option<String>,
}

pub async fn facts(conn: &mut PgConnection, kind: &str, ids: &[Uuid]) -> Res<Vec<Facts>> {
    let sql = match kind {
        "artist" => "SELECT id, name, kind, disambiguation AS detail, NULL::smallint AS year, NULL::text AS code FROM artists WHERE id = ANY($1)",
        "album" => "SELECT id, title AS name, kind, NULL::text AS detail, release_year AS year, NULL::text AS code FROM albums WHERE id = ANY($1)",
        "label" => "SELECT id, name, NULL::text AS kind, NULL::text AS detail, NULL::smallint AS year, NULL::text AS code FROM labels WHERE id = ANY($1)",
        "publisher" => "SELECT id, name, NULL::text AS kind, NULL::text AS detail, NULL::smallint AS year, NULL::text AS code FROM publishers WHERE id = ANY($1)",
        "genre" => "SELECT id, name, NULL::text AS kind, NULL::text AS detail, NULL::smallint AS year, NULL::text AS code FROM genres WHERE id = ANY($1)",
        "raga" => "SELECT id, name, NULL::text AS kind, NULL::text AS detail, NULL::smallint AS year, NULL::text AS code FROM ragas WHERE id = ANY($1)",
        "deity" => "SELECT id, name, NULL::text AS kind, NULL::text AS detail, NULL::smallint AS year, NULL::text AS code FROM deities WHERE id = ANY($1)",
        "tag" => "SELECT id, name, NULL::text AS kind, NULL::text AS detail, NULL::smallint AS year, NULL::text AS code FROM tags WHERE id = ANY($1)",
        "language" => "SELECT id, name_en AS name, NULL::text AS kind, endonym AS detail, NULL::smallint AS year, iso639_3 AS code FROM languages WHERE id = ANY($1)",
        "script" => "SELECT id, name_en AS name, NULL::text AS kind, NULL::text AS detail, NULL::smallint AS year, iso15924 AS code FROM scripts WHERE id = ANY($1)",
        _ => return Ok(Vec::new()),
    };
    sqlx::query_as(sql).bind(ids).fetch_all(conn).await
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Related {
    pub entity_id: Uuid,
    pub kind: String,
    pub target_id: Uuid,
    pub title: String,
}

/// Up to three related works per entity (artists: what they are credited on; albums: who made them).
pub async fn related(conn: &mut PgConnection, kind: &str, ids: &[Uuid]) -> Res<Vec<Related>> {
    let sql = match kind {
        "artist" => {
            "SELECT entity_id, kind, target_id, title FROM (
                SELECT c.artist_id AS entity_id, 'composition' AS kind, p.id AS target_id, p.title,
                       row_number() OVER (PARTITION BY c.artist_id ORDER BY c.created_at DESC, c.id) AS rn
                FROM credits c JOIN compositions p ON p.id = c.composition_id WHERE c.artist_id = ANY($1)
                UNION ALL
                SELECT c.artist_id, 'recording', p.id, p.title,
                       row_number() OVER (PARTITION BY c.artist_id ORDER BY c.created_at DESC, c.id)
                FROM credits c JOIN recordings p ON p.id = c.recording_id WHERE c.artist_id = ANY($1)
                UNION ALL
                SELECT c.artist_id, 'album', p.id, p.title,
                       row_number() OVER (PARTITION BY c.artist_id ORDER BY c.created_at DESC, c.id)
                FROM credits c JOIN albums p ON p.id = c.album_id WHERE c.artist_id = ANY($1)
             ) w WHERE rn <= 3 ORDER BY entity_id, rn, kind, target_id"
        }
        "album" => {
            "SELECT entity_id, kind, target_id, title FROM (
                SELECT c.album_id AS entity_id, 'artist' AS kind, a.id AS target_id, a.name AS title,
                       row_number() OVER (PARTITION BY c.album_id ORDER BY c.position, c.id) AS rn
                FROM credits c JOIN artists a ON a.id = c.artist_id WHERE c.album_id = ANY($1)
             ) w WHERE rn <= 3 ORDER BY entity_id, rn"
        }
        _ => return Ok(Vec::new()),
    };
    sqlx::query_as(sql).bind(ids).fetch_all(conn).await
}

// ---------- proposals ----------

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct ProposalRow {
    pub id: Uuid,
    pub entity_type: String,
    pub name: String,
    pub kind: Option<String>,
    pub disambiguation: Option<String>,
    pub aliases: Vec<String>,
    pub note: Option<String>,
    pub shown_matches: Vec<Uuid>,
    pub status: String,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub decided_by: Option<Uuid>,
    pub decided_at: Option<DateTime<Utc>>,
    pub decision_note: Option<String>,
    pub entity_id: Option<Uuid>,
    pub duplicate_of_id: Option<Uuid>,
}

const PROPOSAL: &str =
    "id, entity_type, name, kind, disambiguation, aliases, note, shown_matches, status, created_by, \
    created_at, decided_by, decided_at, decision_note, entity_id, duplicate_of_id";

pub struct NewProposal<'a> {
    pub entity_type: &'a str,
    pub name: &'a str,
    pub kind: Option<&'a str>,
    pub disambiguation: Option<&'a str>,
    pub aliases: &'a [String],
    pub note: Option<&'a str>,
    pub shown_matches: &'a [Uuid],
    pub created_by: Uuid,
}

pub async fn insert_proposal(conn: &mut PgConnection, p: &NewProposal<'_>) -> Res<ProposalRow> {
    sqlx::query_as(&format!(
        "INSERT INTO entity_proposals (entity_type, name, compact, kind, disambiguation, aliases, note, shown_matches, created_by)
         VALUES ($1, $2, replace(lyrics_search_key($2), ' ', ''), $3, $4, $5, $6, $7, $8) RETURNING {PROPOSAL}"
    ))
    .bind(p.entity_type)
    .bind(p.name)
    .bind(p.kind)
    .bind(p.disambiguation)
    .bind(p.aliases)
    .bind(p.note)
    .bind(p.shown_matches)
    .bind(p.created_by)
    .fetch_one(conn)
    .await
}

pub async fn pending_proposal_like(conn: &mut PgConnection, kind: &str, name: &str) -> Res<Option<Uuid>> {
    sqlx::query_scalar(
        "SELECT id FROM entity_proposals WHERE entity_type = $1 AND status = 'pending'
           AND compact = replace(lyrics_search_key($2), ' ', '')",
    )
    .bind(kind)
    .bind(name)
    .fetch_optional(conn)
    .await
}

pub async fn get_proposal(conn: &mut PgConnection, id: Uuid, lock: bool) -> Res<Option<ProposalRow>> {
    let lock = if lock { " FOR UPDATE" } else { "" };
    sqlx::query_as(&format!("SELECT {PROPOSAL} FROM entity_proposals WHERE id = $1{lock}"))
        .bind(id)
        .fetch_optional(conn)
        .await
}

pub async fn list_pending_proposals(
    conn: &mut PgConnection,
    after: Option<(DateTime<Utc>, Uuid)>,
    limit: i64,
) -> Res<Vec<ProposalRow>> {
    sqlx::query_as(&format!(
        "SELECT {PROPOSAL} FROM entity_proposals
         WHERE status = 'pending' AND ($1::timestamptz IS NULL OR (created_at, id) > ($1, $2))
         ORDER BY created_at, id LIMIT $3"
    ))
    .bind(after.map(|a| a.0))
    .bind(after.map(|a| a.1))
    .bind(limit + 1)
    .fetch_all(conn)
    .await
}

pub async fn decide_proposal(
    conn: &mut PgConnection,
    id: Uuid,
    status: &str,
    by: Uuid,
    note: Option<&str>,
    entity_id: Option<Uuid>,
    duplicate_of_id: Option<Uuid>,
) -> Res<ProposalRow> {
    sqlx::query_as(&format!(
        "UPDATE entity_proposals SET status = $2, decided_by = $3, decided_at = now(), decision_note = $4,
                entity_id = $5, duplicate_of_id = $6
         WHERE id = $1 RETURNING {PROPOSAL}"
    ))
    .bind(id)
    .bind(status)
    .bind(by)
    .bind(note)
    .bind(entity_id)
    .bind(duplicate_of_id)
    .fetch_one(conn)
    .await
}

pub async fn entity_exists(conn: &mut PgConnection, kind: &str, id: Uuid) -> Res<bool> {
    let Some((table, _)) = table_of(kind) else {
        return Ok(false);
    };
    sqlx::query_scalar(&format!("SELECT EXISTS (SELECT 1 FROM {table} WHERE id = $1)"))
        .bind(id)
        .fetch_one(conn)
        .await
}

// ---------- merge ----------

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Locked {
    pub id: Uuid,
    pub version: i32,
    pub kind: Option<String>,
    pub strong_id: Option<String>,
    pub name: String,
    pub snapshot: Value,
}

/// Locks both rows in id order (no deadlock between opposite merges) and returns what exists.
pub async fn lock_pair(conn: &mut PgConnection, kind: &str, a: Uuid, b: Uuid) -> Res<Vec<Locked>> {
    let sql = match kind {
        "artist" => "SELECT id, version, kind, isni AS strong_id, name, to_jsonb(t) AS snapshot FROM artists t WHERE id = ANY($1) ORDER BY id FOR UPDATE",
        "album" => "SELECT id, version, kind, barcode AS strong_id, title AS name, to_jsonb(t) AS snapshot FROM albums t WHERE id = ANY($1) ORDER BY id FOR UPDATE",
        "label" => "SELECT id, version, NULL::text AS kind, NULL::text AS strong_id, name, to_jsonb(t) AS snapshot FROM labels t WHERE id = ANY($1) ORDER BY id FOR UPDATE",
        "publisher" => "SELECT id, version, NULL::text AS kind, NULL::text AS strong_id, name, to_jsonb(t) AS snapshot FROM publishers t WHERE id = ANY($1) ORDER BY id FOR UPDATE",
        "genre" => "SELECT id, version, NULL::text AS kind, NULL::text AS strong_id, name, to_jsonb(t) AS snapshot FROM genres t WHERE id = ANY($1) ORDER BY id FOR UPDATE",
        "raga" => "SELECT id, version, NULL::text AS kind, NULL::text AS strong_id, name, to_jsonb(t) AS snapshot FROM ragas t WHERE id = ANY($1) ORDER BY id FOR UPDATE",
        "deity" => "SELECT id, version, NULL::text AS kind, NULL::text AS strong_id, name, to_jsonb(t) AS snapshot FROM deities t WHERE id = ANY($1) ORDER BY id FOR UPDATE",
        "tag" => "SELECT id, version, NULL::text AS kind, NULL::text AS strong_id, name, to_jsonb(t) AS snapshot FROM tags t WHERE id = ANY($1) ORDER BY id FOR UPDATE",
        _ => return Ok(Vec::new()),
    };
    sqlx::query_as(sql).bind(vec![a, b]).fetch_all(conn).await
}

pub async fn already_merged_into(conn: &mut PgConnection, from: Uuid) -> Res<Option<Uuid>> {
    sqlx::query_scalar("SELECT into_id FROM entity_merges WHERE from_id = $1")
        .bind(from)
        .fetch_optional(conn)
        .await
}

/// Follows merge history to the surviving entity.
pub async fn resolve_redirect(conn: &mut PgConnection, kind: &str, from: Uuid) -> Res<Option<Uuid>> {
    sqlx::query_scalar(
        "WITH RECURSIVE r(id, depth) AS (
             SELECT into_id, 1 FROM entity_merges WHERE from_id = $2 AND entity_type = $1
             UNION ALL
             SELECT m.into_id, r.depth + 1 FROM entity_merges m JOIN r ON m.from_id = r.id
             WHERE r.depth < 20 AND m.entity_type = $1)
         SELECT id FROM r ORDER BY depth DESC LIMIT 1",
    )
    .bind(kind)
    .bind(from)
    .fetch_optional(conn)
    .await
}

/// Album tracks that cannot be carried over: same position, or the same recording, on both albums.
pub async fn track_conflicts(conn: &mut PgConnection, from: Uuid, into: Uuid) -> Res<i64> {
    sqlx::query_scalar(
        "SELECT count(*) FROM album_tracks f JOIN album_tracks t
           ON t.album_id = $2 AND (t.position = f.position OR t.recording_id = f.recording_id)
         WHERE f.album_id = $1",
    )
    .bind(from)
    .bind(into)
    .fetch_one(conn)
    .await
}

#[derive(Debug, Default, Serialize)]
pub struct Moved {
    pub credits: u64,
    pub duplicate_credits_dropped: u64,
    pub tracks: u64,
    pub localizations: u64,
    pub localizations_retired: u64,
    pub aliases: u64,
    pub duplicate_aliases_dropped: u64,
    pub names_kept_as_aliases: u64,
}

/// Re-points every reference from `from` to `into` and removes `from`. The caller holds both row locks.
pub async fn merge_into(
    conn: &mut PgConnection,
    kind: &str,
    from: &Locked,
    into: &Locked,
    actor: Uuid,
    note: Option<&str>,
) -> Res<Moved> {
    let (table, _) = table_of(kind).expect("validated by the caller");
    let (f, i) = (from.id, into.id);
    let mut m = Moved::default();

    if kind == "artist" || kind == "album" {
        // Credits: the same fact twice collapses to one; the rest moves.
        let col = if kind == "artist" { "artist_id" } else { "album_id" };
        if kind == "artist" {
            m.duplicate_credits_dropped = sqlx::query(
                "DELETE FROM credits c USING credits k
                 WHERE c.artist_id = $1 AND k.artist_id = $2 AND k.role = c.role
                   AND k.composition_id IS NOT DISTINCT FROM c.composition_id
                   AND k.recording_id IS NOT DISTINCT FROM c.recording_id
                   AND k.album_id IS NOT DISTINCT FROM c.album_id",
            )
            .bind(f)
            .bind(i)
            .execute(&mut *conn)
            .await?
            .rows_affected();
        } else {
            m.duplicate_credits_dropped = sqlx::query(
                "DELETE FROM credits c USING credits k
                 WHERE c.album_id = $1 AND k.album_id = $2 AND k.role = c.role AND k.artist_id = c.artist_id",
            )
            .bind(f)
            .bind(i)
            .execute(&mut *conn)
            .await?
            .rows_affected();
        }
        m.credits = sqlx::query(&format!("UPDATE credits SET {col} = $2 WHERE {col} = $1"))
            .bind(f)
            .bind(i)
            .execute(&mut *conn)
            .await?
            .rows_affected();
        if kind == "album" {
            m.tracks = sqlx::query("UPDATE album_tracks SET album_id = $2 WHERE album_id = $1")
                .bind(f)
                .bind(i)
                .execute(&mut *conn)
                .await?
                .rows_affected();
        }
    }

    // Songs that used the merged raga, deity or tag now use the survivor (a tag a song already has is not doubled).
    match kind {
        "raga" | "deity" => {
            let col = if kind == "raga" { "raga_id" } else { "deity_id" };
            sqlx::query(&format!(
                "UPDATE compositions SET {col} = $2, updated_at = now() WHERE {col} = $1"
            ))
            .bind(f)
            .bind(i)
            .execute(&mut *conn)
            .await?;
        }
        "tag" => {
            sqlx::query(
                "DELETE FROM composition_tags d USING composition_tags k
                 WHERE d.tag_id = $1 AND k.tag_id = $2 AND k.composition_id = d.composition_id",
            )
            .bind(f)
            .bind(i)
            .execute(&mut *conn)
            .await?;
            sqlx::query("UPDATE composition_tags SET tag_id = $2 WHERE tag_id = $1")
                .bind(f)
                .bind(i)
                .execute(&mut *conn)
                .await?;
        }
        _ => {}
    }

    // Localizations. A slot the survivor already holds is kept as retired history on the survivor,
    // and its name survives as an alias; nothing is dropped.
    const CONFLICT: &str = "EXISTS (SELECT 1 FROM entity_localizations k WHERE k.entity_type = $1 AND k.entity_id = $3
                       AND k.merged_from_id IS NULL AND k.language_id = l.language_id
                       AND coalesce(k.script_id, '00000000-0000-0000-0000-000000000000'::uuid)
                         = coalesce(l.script_id, '00000000-0000-0000-0000-000000000000'::uuid))";
    // Only verified names become aliases; a pending name must not reach search through a merge.
    m.names_kept_as_aliases += sqlx::query(&format!(
        "INSERT INTO entity_aliases (entity_type, entity_id, language_id, script_id, alias, source,
                                     verification_state, created_by, verified_by, verified_at)
         SELECT $1, $3, l.language_id, l.script_id, l.name, 'Merged record ' || $2::text, 'verified', $4, $4, now()
         FROM entity_localizations l
         WHERE l.entity_type = $1 AND l.entity_id = $2 AND l.merged_from_id IS NULL
           AND l.verification_state = 'verified' AND {CONFLICT}
         ON CONFLICT DO NOTHING"
    ))
    .bind(kind)
    .bind(f)
    .bind(i)
    .bind(actor)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    m.localizations_retired = sqlx::query(&format!(
        "UPDATE entity_localizations l SET entity_id = $3, merged_from_id = $2, is_preferred = false,
                verification_state = 'rejected', updated_at = now()
         WHERE l.entity_type = $1 AND l.entity_id = $2 AND l.merged_from_id IS NULL AND {CONFLICT}"
    ))
    .bind(kind)
    .bind(f)
    .bind(i)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    m.localizations = sqlx::query(
        "UPDATE entity_localizations l SET entity_id = $3, updated_at = now(),
                is_preferred = l.is_preferred AND NOT EXISTS (
                    SELECT 1 FROM entity_localizations k WHERE k.entity_type = $1 AND k.entity_id = $3
                      AND k.language_id = l.language_id AND k.is_preferred)
         WHERE l.entity_type = $1 AND l.entity_id = $2",
    )
    .bind(kind)
    .bind(f)
    .bind(i)
    .execute(&mut *conn)
    .await?
    .rows_affected();

    // Aliases: keep them all; an identical one collapses.
    m.duplicate_aliases_dropped = sqlx::query(
        "DELETE FROM entity_aliases a USING entity_aliases k
         WHERE a.entity_type = $1 AND a.entity_id = $2 AND k.entity_type = $1 AND k.entity_id = $3
           AND lower(k.alias) = lower(a.alias)
           AND k.language_id IS NOT DISTINCT FROM a.language_id AND k.script_id IS NOT DISTINCT FROM a.script_id",
    )
    .bind(kind)
    .bind(f)
    .bind(i)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    m.aliases = sqlx::query("UPDATE entity_aliases SET entity_id = $3 WHERE entity_type = $1 AND entity_id = $2")
        .bind(kind)
        .bind(f)
        .bind(i)
        .execute(&mut *conn)
        .await?
        .rows_affected();

    // The merged record's own name stays findable.
    if sqlx::query_scalar::<_, bool>("SELECT lyrics_search_key($1) <> lyrics_search_key($2)")
        .bind(&from.name)
        .bind(&into.name)
        .fetch_one(&mut *conn)
        .await?
    {
        m.names_kept_as_aliases += insert_merge_alias(conn, kind, i, &from.name, None, None, actor, f).await?;
    }

    sqlx::query("UPDATE entity_proposals SET entity_id = $2 WHERE entity_id = $1")
        .bind(f)
        .bind(i)
        .execute(&mut *conn)
        .await?;
    sqlx::query(&format!(
        "UPDATE {table} SET version = version + 1, updated_at = now() WHERE id = $1"
    ))
    .bind(i)
    .execute(&mut *conn)
    .await?;
    sqlx::query(&format!("DELETE FROM {table} WHERE id = $1"))
        .bind(f)
        .execute(&mut *conn)
        .await?;
    sqlx::query(
        "INSERT INTO entity_merges (entity_type, from_id, into_id, actor_id, note, snapshot) VALUES ($1,$2,$3,$4,$5,$6)",
    )
    .bind(kind)
    .bind(f)
    .bind(i)
    .bind(actor)
    .bind(note)
    .bind(json!({ "entity": from.snapshot, "moved": m }))
    .execute(&mut *conn)
    .await?;
    Ok(m)
}

#[allow(clippy::too_many_arguments)]
async fn insert_merge_alias(
    conn: &mut PgConnection,
    kind: &str,
    entity_id: Uuid,
    alias: &str,
    language_id: Option<Uuid>,
    script_id: Option<Uuid>,
    actor: Uuid,
    from: Uuid,
) -> Res<u64> {
    Ok(sqlx::query(
        "INSERT INTO entity_aliases (entity_type, entity_id, language_id, script_id, alias, source,
                                     verification_state, created_by, verified_by, verified_at)
         VALUES ($1, $2, $3, $4, $5, $6, 'verified', $7, $7, now()) ON CONFLICT DO NOTHING",
    )
    .bind(kind)
    .bind(entity_id)
    .bind(language_id)
    .bind(script_id)
    .bind(alias)
    .bind(format!("Merged record {from}"))
    .bind(actor)
    .execute(conn)
    .await?
    .rows_affected())
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LocalName {
    pub entity_id: Uuid,
    pub language: String,
    pub name: String,
}

/// Verified localized names (and nothing pending) for disambiguation in pickers.
pub async fn local_names(conn: &mut PgConnection, kind: &str, ids: &[Uuid]) -> Res<Vec<LocalName>> {
    sqlx::query_as(
        "SELECT l.entity_id, coalesce(g.iso639_1, g.iso639_3) AS language, l.name
         FROM entity_localizations l JOIN languages g ON g.id = l.language_id
         WHERE l.entity_type = $1 AND l.entity_id = ANY($2) AND l.verification_state = 'verified'
         ORDER BY l.entity_id, l.is_preferred DESC, g.iso639_3, l.id",
    )
    .bind(kind)
    .bind(ids)
    .fetch_all(conn)
    .await
}
