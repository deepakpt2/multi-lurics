//! SQL behind the Postgres search implementation: the published-only `search_documents` read
//! model (migration 0007), ranking weights from `settings`, display facts and usage metrics.
//! Nothing here is exposed to API clients; the application layer hides it behind `SearchService`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgConnection;
use uuid::Uuid;

type Res<T> = Result<T, sqlx::Error>;

/// Ranking weights. Defaults mirror the seeded `search_ranking` setting; any key missing from the
/// stored JSON falls back to them, so a partial override is safe.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Ranking {
    pub class_weights: ClassWeights,
    pub origin_multipliers: OriginMultipliers,
    pub type_bonus: TypeBonus,
    pub popularity_weight: f64,
    /// Popularity can reorder results within a class but never lift one over the next class.
    pub popularity_cap: f64,
    pub language_boost: f64,
    pub min_suggest_chars: usize,
    pub cache_max_age_secs: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClassWeights {
    pub exact: f64,
    pub normalized: f64,
    pub prefix: f64,
    pub fuzzy: f64,
    /// Lyrics: the text typed opens the song, starts one of its lines, or is somewhere inside a line.
    /// Below a title match, in that order, so a song that opens with the words comes before one that
    /// merely contains them.
    pub lyric_opening: f64,
    pub lyric_line: f64,
    pub lyric_inside: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OriginMultipliers {
    pub name: f64,
    pub localization: f64,
    pub transliteration: f64,
    pub alias: f64,
    /// Raga, deity or tag of a song.
    pub metadata: f64,
    /// The words of a published lyric.
    pub lyrics: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TypeBonus {
    pub composition: f64,
    pub recording: f64,
    pub album: f64,
    pub artist: f64,
}

impl Default for ClassWeights {
    fn default() -> Self {
        Self {
            exact: 100.0,
            normalized: 90.0,
            prefix: 70.0,
            fuzzy: 40.0,
            lyric_opening: 60.0,
            lyric_line: 50.0,
            lyric_inside: 30.0,
        }
    }
}
impl Default for OriginMultipliers {
    fn default() -> Self {
        Self {
            name: 1.0,
            localization: 1.0,
            transliteration: 0.95,
            alias: 0.9,
            metadata: 0.8,
            lyrics: 1.0,
        }
    }
}
impl Default for TypeBonus {
    fn default() -> Self {
        Self {
            composition: 4.0,
            recording: 3.0,
            album: 2.0,
            artist: 1.0,
        }
    }
}
impl Default for Ranking {
    fn default() -> Self {
        Self {
            class_weights: ClassWeights::default(),
            origin_multipliers: OriginMultipliers::default(),
            type_bonus: TypeBonus::default(),
            popularity_weight: 2.0,
            popularity_cap: 10.0,
            language_boost: 5.0,
            min_suggest_chars: 2,
            cache_max_age_secs: 30,
        }
    }
}

pub async fn load_ranking(conn: &mut PgConnection) -> Res<Ranking> {
    let value: Option<Value> = sqlx::query_scalar("SELECT value FROM settings WHERE key = 'search_ranking'")
        .fetch_optional(conn)
        .await?;
    Ok(match value.map(serde_json::from_value) {
        Some(Ok(r)) => r,
        Some(Err(e)) => {
            tracing::warn!(error = %e, "search_ranking setting is invalid; using defaults");
            Ranking::default()
        }
        None => Ranking::default(),
    })
}

pub struct Query<'a> {
    pub q: &'a str,
    pub types: &'a [&'a str],
    pub contributors_only: bool,
    pub language_id: Option<Uuid>,
    pub limit: i64,
    /// Keyset position of the last row of the previous page: (score, entity id).
    pub after: Option<(i64, Uuid)>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Row {
    pub entity_type: String,
    pub entity_id: Uuid,
    pub origin: String,
    pub class: String,
    pub display: String,
    pub score: i64,
}

/// Exact → normalized → prefix → fuzzy classes, weighted by where the name came from, plus a bounded
/// popularity and language boost. One row per entity (its best name). Hidden names are re-checked
/// against their source rows so an unverified name never matches while the worker catches up.
pub async fn search(conn: &mut PgConnection, ranking: &Ranking, q: &Query<'_>) -> Res<Vec<Row>> {
    let cfg = serde_json::to_value(ranking).expect("ranking serializes");
    let types: Vec<String> = q.types.iter().map(|t| (*t).to_owned()).collect();
    sqlx::query_as(
        "WITH q AS (SELECT lyrics_search_key($1) AS k, replace(lyrics_search_key($1), ' ', '') AS c, lower(btrim($1)) AS raw),
              w AS (SELECT $5::jsonb AS j),
              cand AS (
                SELECT d.entity_type, d.entity_id, d.origin, d.display, d.popularity,
                       CASE WHEN lower(d.display) = q.raw THEN 'exact'
                            WHEN d.key = q.k OR d.compact = q.c THEN 'normalized'
                            WHEN d.key LIKE q.k || '%' OR d.compact LIKE q.c || '%' THEN 'prefix'
                            ELSE 'fuzzy' END AS class,
                       similarity(d.key, q.k)::float8 AS sim,
                       (d.language_id IS NOT NULL AND d.language_id = $4) AS lang_hit
                FROM search_documents d, q
                WHERE d.entity_type = ANY($2) AND q.c <> ''
                  AND (NOT $3 OR d.contributor)
                  AND (d.compact = q.c OR d.key LIKE q.k || '%' OR d.compact LIKE q.c || '%' OR d.key % q.k)
                  AND (d.origin IN ('name', 'metadata')
                       OR (d.origin = 'alias' AND EXISTS (
                            SELECT 1 FROM entity_aliases a WHERE a.id = d.source_id AND a.verification_state = 'verified'))
                       OR (d.origin IN ('localization', 'transliteration') AND EXISTS (
                            SELECT 1 FROM entity_localizations l WHERE l.id = d.source_id AND l.verification_state = 'verified')))
                UNION ALL
                -- The words of published lyrics, anywhere in the text (trigram index on `key`; three or more
                -- characters). How early the words come decides the class, and the line that holds them is shown.
                SELECT 'composition', m.composition_id, 'lyrics', m.snippet, 0, m.class, 1.0::float8, m.lang_hit
                FROM (
                  SELECT s.composition_id, s.lang_hit, s.rank,
                         CASE s.rank WHEN 1 THEN 'lyric_opening' WHEN 2 THEN 'lyric_line' ELSE 'lyric_inside' END AS class,
                         split_part(v.body, E'\n', s.line) AS snippet
                  FROM (
                    SELECT ls.composition_id, ls.contribution_id, (ls.language_id = $4) AS lang_hit,
                           CASE WHEN (SELECT l FROM unnest(ls.lines) l WHERE l <> '' LIMIT 1) LIKE q.k || '%' THEN 1
                                WHEN EXISTS (SELECT 1 FROM unnest(ls.lines) l WHERE l LIKE q.k || '%') THEN 2
                                ELSE 3 END AS rank,
                           COALESCE((SELECT i FROM generate_subscripts(ls.lines, 1) i WHERE ls.lines[i] LIKE q.k || '%' ORDER BY i LIMIT 1),
                                    (SELECT i FROM generate_subscripts(ls.lines, 1) i WHERE ls.lines[i] LIKE '%' || q.k || '%' ORDER BY i LIMIT 1),
                                    1) AS line
                    FROM lyric_search ls, q
                    WHERE 'composition' = ANY($2) AND NOT $3 AND char_length(q.c) >= 3 AND ls.key LIKE '%' || q.k || '%'
                    ORDER BY 4, 1 LIMIT 600
                  ) s
                  JOIN contributions c ON c.id = s.contribution_id
                  JOIN lyric_revisions v ON v.id = c.published_revision_id
                ) m
              ),
              scored AS (
                SELECT cand.*,
                       (w.j->'class_weights'->>cand.class)::float8 * CASE WHEN cand.class = 'fuzzy' THEN cand.sim ELSE 1 END
                         * (w.j->'origin_multipliers'->>cand.origin)::float8
                       + LEAST((w.j->>'popularity_cap')::float8,
                               (w.j->>'popularity_weight')::float8 * ln(1 + cand.popularity::float8))
                       + CASE WHEN cand.lang_hit THEN (w.j->>'language_boost')::float8 ELSE 0 END
                       + COALESCE((w.j->'type_bonus'->>cand.entity_type)::float8, 0) AS score
                FROM cand, w
              ),
              best AS (
                SELECT DISTINCT ON (entity_type, entity_id) entity_type, entity_id, origin, class, display,
                       round(score * 1000)::bigint AS score
                FROM scored ORDER BY entity_type, entity_id, score DESC, origin
              )
         SELECT entity_type, entity_id, origin, class, display, score FROM best
         WHERE ($6::bigint IS NULL OR (score, entity_id) < ($6, $7))
         ORDER BY score DESC, entity_id DESC LIMIT $8",
    )
    .bind(q.q)
    .bind(types)
    .bind(q.contributors_only)
    .bind(q.language_id)
    .bind(cfg)
    .bind(q.after.map(|a| a.0))
    .bind(q.after.map(|a| a.1))
    .bind(q.limit)
    .fetch_all(conn)
    .await
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Fact {
    pub id: Uuid,
    pub title: String,
    pub sub_kind: Option<String>,
    pub detail: Option<String>,
    pub year: Option<i16>,
    /// A recording's composition.
    pub parent_id: Option<Uuid>,
    pub parent_title: Option<String>,
}

/// Display facts for hits. An id that no longer exists (merged away, deleted) is simply absent.
pub async fn facts(conn: &mut PgConnection, entity_type: &str, ids: &[Uuid]) -> Res<Vec<Fact>> {
    let sql = match entity_type {
        "composition" => "SELECT id, title, NULL::text AS sub_kind, iswc AS detail, year, NULL::uuid AS parent_id, NULL::text AS parent_title FROM compositions WHERE id = ANY($1)",
        "recording" => "SELECT r.id, r.title, NULL::text AS sub_kind, NULL::text AS detail, r.year, c.id AS parent_id, c.title AS parent_title FROM recordings r JOIN compositions c ON c.id = r.composition_id WHERE r.id = ANY($1)",
        "artist" => "SELECT id, name AS title, kind AS sub_kind, disambiguation AS detail, NULL::smallint AS year, NULL::uuid AS parent_id, NULL::text AS parent_title FROM artists WHERE id = ANY($1)",
        "album" => "SELECT id, title, kind AS sub_kind, NULL::text AS detail, release_year AS year, NULL::uuid AS parent_id, NULL::text AS parent_title FROM albums WHERE id = ANY($1)",
        _ => return Ok(Vec::new()),
    };
    sqlx::query_as(sql).bind(ids).fetch_all(conn).await
}

/// Aggregate counters only: no query text, user or address is stored.
pub async fn record_metric(
    conn: &mut PgConnection,
    endpoint: &str,
    shard: i16,
    empty: bool,
    latency_ms: i64,
    accepted: bool,
) -> Res<()> {
    sqlx::query(
        "INSERT INTO search_metrics (day, endpoint, shard, requests, empty, latency_ms, accepted)
         VALUES (current_date, $1, $2, $3, $4, $5, $6)
         ON CONFLICT (day, endpoint, shard) DO UPDATE SET
             requests = search_metrics.requests + EXCLUDED.requests,
             empty = search_metrics.empty + EXCLUDED.empty,
             latency_ms = search_metrics.latency_ms + EXCLUDED.latency_ms,
             accepted = search_metrics.accepted + EXCLUDED.accepted",
    )
    .bind(endpoint)
    .bind(shard)
    .bind(i64::from(!accepted))
    .bind(i64::from(empty))
    .bind(latency_ms)
    .bind(i64::from(accepted))
    .execute(conn)
    .await
    .map(|_| ())
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MetricsDay {
    pub day: chrono::NaiveDate,
    pub endpoint: String,
    pub requests: i64,
    pub empty: i64,
    pub latency_ms: i64,
    pub accepted: i64,
}

pub async fn metrics(conn: &mut PgConnection, days: i32) -> Res<Vec<MetricsDay>> {
    sqlx::query_as(
        "SELECT day, endpoint, sum(requests)::bigint AS requests, sum(empty)::bigint AS empty,
                sum(latency_ms)::bigint AS latency_ms, sum(accepted)::bigint AS accepted
         FROM search_metrics WHERE day > current_date - $1::int
         GROUP BY day, endpoint ORDER BY day DESC, endpoint",
    )
    .bind(days)
    .fetch_all(conn)
    .await
}

/// Rebuilds one entity's documents from the source tables (idempotent; used by the worker).
pub async fn reindex(conn: &mut PgConnection, entity_type: &str, id: Uuid) -> Res<()> {
    sqlx::query("SELECT search_documents_refresh($1, $2)")
        .bind(entity_type)
        .bind(id)
        .execute(conn)
        .await
        .map(|_| ())
}
