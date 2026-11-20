//! SQL behind song browsing (migration 0014): filtering songs by metadata, listing the values of
//! one metadata kind, and typed suggestions. Everything reads the existing catalogue tables; nothing
//! is copied. The one place that knows which metadata kinds exist is [`Facet`].
//!
//! A *filter* is a (facet, value). Different filters combine with AND, and every query here applies
//! the same song predicate, so a list, a count, a facet list and a suggestion can never disagree
//! about what "matches the active filters" means. All user input travels as bound parameters.

use crate::search::Ranking;
use crate::seo;
use sqlx::{PgConnection, Postgres, QueryBuilder};
use std::collections::HashMap;
use uuid::Uuid;

type Res<T> = Result<T, sqlx::Error>;

/// The metadata kinds a song can be browsed or filtered by. Add a kind here (and to the frontend
/// registry, which the generated API types force) and it works everywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Facet {
    /// Anyone credited on the song in any role.
    Artist,
    Singer,
    Lyricist,
    Composer,
    Album,
    Raga,
    Deity,
    Religion,
    Category,
    /// The kind of devotional piece: bhajan, stotra, aarti…
    Form,
    /// A language the song's published lyrics exist in.
    Language,
    Tag,
}

/// How a facet's values are identified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A catalogue entity of this type, addressed by its browse slug.
    Entity(&'static str),
    /// A value stored on the song, addressed by its normalized text.
    Text,
}

pub const RELIGIONS: [&str; 7] = ["hindu", "sikh", "buddhist", "jain", "christian", "muslim", "other"];
pub const CATEGORIES: [&str; 2] = ["mainstream", "devotional"];

impl Facet {
    pub const ALL: [Facet; 12] = [
        Self::Artist,
        Self::Singer,
        Self::Lyricist,
        Self::Composer,
        Self::Album,
        Self::Raga,
        Self::Deity,
        Self::Religion,
        Self::Category,
        Self::Form,
        Self::Language,
        Self::Tag,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Artist => "artist",
            Self::Singer => "singer",
            Self::Lyricist => "lyricist",
            Self::Composer => "composer",
            Self::Album => "album",
            Self::Raga => "raga",
            Self::Deity => "deity",
            Self::Religion => "religion",
            Self::Category => "category",
            Self::Form => "form",
            Self::Language => "language",
            Self::Tag => "tag",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.as_str() == s)
    }

    pub fn kind(self) -> Kind {
        match self {
            Self::Artist | Self::Singer | Self::Lyricist | Self::Composer => Kind::Entity("artist"),
            Self::Album => Kind::Entity("album"),
            Self::Raga => Kind::Entity("raga"),
            Self::Deity => Kind::Entity("deity"),
            Self::Tag => Kind::Entity("tag"),
            _ => Kind::Text,
        }
    }

    /// Rows of (song, value[, label]) for this facet, before any filtering.
    fn links(self) -> &'static str {
        match self {
            Self::Singer => {
                "SELECT r.composition_id AS song, k.artist_id AS v FROM credits k JOIN recordings r ON r.id = k.recording_id
                 WHERE k.role IN ('performer', 'featured_performer')"
            }
            Self::Lyricist => "SELECT k.composition_id AS song, k.artist_id AS v FROM credits k WHERE k.role = 'lyricist'",
            Self::Composer => "SELECT k.composition_id AS song, k.artist_id AS v FROM credits k WHERE k.role = 'composer'",
            Self::Artist => {
                "SELECT k.composition_id AS song, k.artist_id AS v FROM credits k WHERE k.composition_id IS NOT NULL
                 UNION ALL SELECT r.composition_id, k.artist_id FROM credits k JOIN recordings r ON r.id = k.recording_id"
            }
            Self::Album => {
                "SELECT r.composition_id AS song, t.album_id AS v FROM album_tracks t JOIN recordings r ON r.id = t.recording_id"
            }
            Self::Raga => "SELECT c.id AS song, c.raga_id AS v FROM compositions c WHERE c.raga_id IS NOT NULL",
            Self::Deity => "SELECT c.id AS song, c.deity_id AS v FROM compositions c WHERE c.deity_id IS NOT NULL",
            Self::Religion => "SELECT c.id AS song, c.religion AS v, c.religion AS label FROM compositions c WHERE c.religion IS NOT NULL",
            Self::Category => "SELECT c.id AS song, c.category AS v, c.category AS label FROM compositions c",
            Self::Form => {
                "SELECT c.id AS song, c.devotional_form AS v, c.devotional_form AS label FROM compositions c
                 WHERE c.devotional_form IS NOT NULL"
            }
            Self::Tag => "SELECT composition_id AS song, tag_id AS v FROM composition_tags",
            Self::Language => {
                "SELECT r.composition_id AS song, lower(l.iso639_3) AS v, coalesce(l.endonym, l.name_en) AS label
                 FROM contributions x JOIN recordings r ON r.id = x.recording_id JOIN languages l ON l.id = x.language_id
                 WHERE x.published_revision_id IS NOT NULL"
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Entity(Uuid),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filter {
    pub facet: Facet,
    pub value: Value,
}

/// A song is public when it has a published lyric: the same rule as `seo_is_public('composition', …)`.
pub(crate) const PUBLISHED: &str =
    "EXISTS (SELECT 1 FROM recordings pr JOIN contributions px ON px.recording_id = pr.id
                          WHERE pr.composition_id = c.id AND px.published_revision_id IS NOT NULL)";

fn push_value(qb: &mut QueryBuilder<'_, Postgres>, v: &Value) {
    match v {
        Value::Entity(id) => qb.push_bind(*id),
        Value::Text(s) => qb.push_bind(s.clone()),
    };
}

/// ` AND (…)` for each filter, over a `compositions c` row.
fn push_filters(qb: &mut QueryBuilder<'_, Postgres>, filters: &[Filter]) {
    for f in filters {
        qb.push(" AND ");
        match f.facet {
            Facet::Raga => {
                qb.push("c.raga_id = ");
            }
            Facet::Deity => {
                qb.push("c.deity_id = ");
            }
            Facet::Religion => {
                qb.push("c.religion = ");
            }
            Facet::Category => {
                qb.push("c.category = ");
            }
            Facet::Form => {
                qb.push("c.devotional_form = ");
            }
            // Entity and language facets: the song has a link row with this value.
            _ => {
                qb.push("EXISTS (SELECT 1 FROM (")
                    .push(f.facet.links())
                    .push(") fl WHERE fl.song = c.id AND fl.v = ");
                push_value(qb, &f.value);
                qb.push(")");
                continue;
            }
        }
        push_value(qb, &f.value);
    }
}

// ---------- slugs ----------

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SlugRow {
    pub entity_id: Uuid,
    pub slug: String,
}

/// Entities a slug can be asked for. The slug may be current or retired; a merged entity resolves to
/// the one it was merged into (followed to the end of the chain).
pub async fn resolve_slug(conn: &mut PgConnection, entity_type: &str, slug: &str) -> Res<Option<Uuid>> {
    sqlx::query_scalar(
        "WITH RECURSIVE hop(id, depth) AS (
             SELECT entity_id, 0 FROM browse_slugs WHERE entity_type = $1 AND slug = $2
             UNION ALL
             SELECT m.into_id, hop.depth + 1 FROM hop JOIN entity_merges m ON m.from_id = hop.id AND m.entity_type = $1
             WHERE hop.depth < 10)
         SELECT id FROM hop ORDER BY depth DESC LIMIT 1",
    )
    .bind(entity_type)
    .bind(slug)
    .fetch_optional(conn)
    .await
}

pub async fn current_slugs(conn: &mut PgConnection, entity_type: &str, ids: &[Uuid]) -> Res<HashMap<Uuid, String>> {
    let rows: Vec<SlugRow> = sqlx::query_as(
        "SELECT entity_id, slug FROM browse_slugs WHERE entity_type = $1 AND entity_id = ANY($2) AND is_current",
    )
    .bind(entity_type)
    .bind(ids)
    .fetch_all(conn)
    .await?;
    Ok(rows.into_iter().map(|r| (r.entity_id, r.slug)).collect())
}

/// Brings one entity's browse slug up to date and returns the current one (`None` when the entity is
/// gone). Idempotent. A renamed entity keeps its old slug as history. A taken slug gets `-2`, `-3`…
/// Must run inside a transaction (an advisory lock serializes writers).
pub async fn sync_slug(conn: &mut PgConnection, entity_type: &str, id: Uuid) -> Res<Option<String>> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("browse-slug:{entity_type}"))
        .execute(&mut *conn)
        .await?;
    let sql = match entity_type {
        "artist" => "SELECT name FROM artists WHERE id = $1",
        "album" => "SELECT title FROM albums WHERE id = $1",
        "raga" => "SELECT name FROM ragas WHERE id = $1",
        "deity" => "SELECT name FROM deities WHERE id = $1",
        "tag" => "SELECT name FROM tags WHERE id = $1",
        _ => return Ok(None),
    };
    let Some(name): Option<String> = sqlx::query_scalar(sql).bind(id).fetch_optional(&mut *conn).await? else {
        return Ok(None);
    };
    let policy = seo::load_config(conn).await?.slug;
    let base = policy.slugify(&name).unwrap_or_else(|| entity_type.to_owned());
    let mut n = 1u32;
    let chosen = loop {
        let candidate = if n == 1 { base.clone() } else { format!("{base}-{n}") };
        let owner: Option<Uuid> =
            sqlx::query_scalar("SELECT entity_id FROM browse_slugs WHERE entity_type = $1 AND slug = $2")
                .bind(entity_type)
                .bind(&candidate)
                .fetch_optional(&mut *conn)
                .await?;
        if owner.is_none() || owner == Some(id) {
            break candidate;
        }
        n += 1;
    };
    let current: Option<String> =
        sqlx::query_scalar("SELECT slug FROM browse_slugs WHERE entity_type = $1 AND entity_id = $2 AND is_current")
            .bind(entity_type)
            .bind(id)
            .fetch_optional(&mut *conn)
            .await?;
    // A name that merely changed case or punctuation keeps its address: only a different slug moves it,
    // and an entity that already has one keeps it while the plain form is merely taken by someone else.
    if current.as_deref() == Some(chosen.as_str()) {
        return Ok(current);
    }
    sqlx::query("UPDATE browse_slugs SET is_current = false WHERE entity_type = $1 AND entity_id = $2 AND is_current")
        .bind(entity_type)
        .bind(id)
        .execute(&mut *conn)
        .await?;
    let reactivated = sqlx::query(
        "UPDATE browse_slugs SET is_current = true WHERE entity_type = $1 AND entity_id = $2 AND slug = $3",
    )
    .bind(entity_type)
    .bind(id)
    .bind(&chosen)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    if reactivated == 0 {
        sqlx::query("INSERT INTO browse_slugs (entity_type, entity_id, slug, is_current) VALUES ($1, $2, $3, true)")
            .bind(entity_type)
            .bind(id)
            .bind(&chosen)
            .execute(&mut *conn)
            .await?;
    }
    Ok(Some(chosen))
}

// ---------- labels for resolved filters ----------

/// Display label of a text value, or `None` when no song carries it.
pub async fn text_label(conn: &mut PgConnection, facet: Facet, v: &str) -> Res<Option<String>> {
    let sql = match facet {
        Facet::Religion | Facet::Category | Facet::Form => {
            return Ok((RELIGIONS.contains(&v) && facet == Facet::Religion
                || CATEGORIES.contains(&v) && facet == Facet::Category
                || domain::catalogue::DEVOTIONAL_FORMS.contains(&v) && facet == Facet::Form)
                .then(|| v.to_owned()));
        }
        Facet::Language => "SELECT coalesce(endonym, name_en) FROM languages WHERE lower(iso639_3) = $1",
        _ => return Ok(None),
    };
    sqlx::query_scalar(sql).bind(v).fetch_optional(conn).await
}

// ---------- the song list ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    Newest,
    Title,
    /// Grouped by album (the first album the song is on, as the cards show), in track order, then by title.
    Album,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SongRow {
    pub id: Uuid,
    pub title: String,
    pub year: Option<i16>,
    pub total: i64,
}

pub async fn list_songs(
    conn: &mut PgConnection,
    filters: &[Filter],
    sort: Sort,
    limit: i64,
    offset: i64,
) -> Res<Vec<SongRow>> {
    let mut qb = QueryBuilder::new("SELECT c.id, c.title, c.year, count(*) OVER() AS total FROM compositions c WHERE ");
    qb.push(PUBLISHED);
    push_filters(&mut qb, filters);
    qb.push(match sort {
        Sort::Newest => " ORDER BY c.created_at DESC, c.id DESC",
        Sort::Title => " ORDER BY lower(c.title), c.id",
        Sort::Album => {
            " ORDER BY (SELECT a.title FROM album_tracks t JOIN recordings r ON r.id = t.recording_id
                                 JOIN albums a ON a.id = t.album_id WHERE r.composition_id = c.id
                                 ORDER BY a.title, a.id LIMIT 1) NULLS LAST,
                       (SELECT a.id FROM album_tracks t JOIN recordings r ON r.id = t.recording_id
                                 JOIN albums a ON a.id = t.album_id WHERE r.composition_id = c.id
                                 ORDER BY a.title, a.id LIMIT 1),
                       (SELECT t.position FROM album_tracks t JOIN recordings r ON r.id = t.recording_id
                                 JOIN albums a ON a.id = t.album_id WHERE r.composition_id = c.id
                                 ORDER BY a.title, a.id LIMIT 1),
                       lower(c.title), c.id"
        }
    });
    qb.push(" LIMIT ").push_bind(limit).push(" OFFSET ").push_bind(offset);
    qb.build_query_as().fetch_all(conn).await
}

pub async fn count_songs(conn: &mut PgConnection, filters: &[Filter]) -> Res<i64> {
    let mut qb = QueryBuilder::new("SELECT count(*) FROM compositions c WHERE ");
    qb.push(PUBLISHED);
    push_filters(&mut qb, filters);
    qb.build_query_scalar().fetch_one(conn).await
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PersonRow {
    pub song: Uuid,
    pub id: Uuid,
    pub name: String,
}

/// Singers of each song (performer credits on any of its recordings), in credit order.
pub async fn singers(conn: &mut PgConnection, songs: &[Uuid]) -> Res<Vec<PersonRow>> {
    sqlx::query_as(
        "SELECT song, id, name FROM (
             SELECT DISTINCT ON (r.composition_id, a.id) r.composition_id AS song, a.id, a.name, k.position, k.created_at
             FROM credits k JOIN recordings r ON r.id = k.recording_id JOIN artists a ON a.id = k.artist_id
             WHERE r.composition_id = ANY($1) AND k.role IN ('performer', 'featured_performer')
             ORDER BY r.composition_id, a.id, k.position) s
         ORDER BY song, position, created_at",
    )
    .bind(songs)
    .fetch_all(conn)
    .await
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AlbumRow {
    pub song: Uuid,
    pub id: Uuid,
    pub title: String,
}

pub async fn albums(conn: &mut PgConnection, songs: &[Uuid]) -> Res<Vec<AlbumRow>> {
    sqlx::query_as(
        "SELECT DISTINCT r.composition_id AS song, a.id, a.title FROM album_tracks t
         JOIN recordings r ON r.id = t.recording_id JOIN albums a ON a.id = t.album_id
         WHERE r.composition_id = ANY($1) ORDER BY 1, 3, 2",
    )
    .bind(songs)
    .fetch_all(conn)
    .await
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LyricLanguage {
    pub song: Uuid,
    pub kind: String,
    pub is_primary: bool,
    pub code: String,
    pub name: String,
}

/// Languages of each song's published lyrics, by kind (`original`, `translation`, `transliteration`).
pub async fn lyric_languages(conn: &mut PgConnection, songs: &[Uuid]) -> Res<Vec<LyricLanguage>> {
    sqlx::query_as(
        "SELECT DISTINCT r.composition_id AS song, x.type AS kind, x.is_primary, lower(coalesce(l.iso639_1, l.iso639_3)) AS code,
                coalesce(l.endonym, l.name_en) AS name
         FROM contributions x JOIN recordings r ON r.id = x.recording_id JOIN languages l ON l.id = x.language_id
         WHERE r.composition_id = ANY($1) AND x.published_revision_id IS NOT NULL
         ORDER BY 1, 2, 3 DESC, 5",
    )
    .bind(songs)
    .fetch_all(conn)
    .await
}

// ---------- listing the values of one facet ----------

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct EntityValue {
    pub id: Uuid,
    pub name: String,
    pub songs: i64,
    pub total: i64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TextValue {
    pub value: String,
    pub label: String,
    pub songs: i64,
    pub total: i64,
}

/// Word-prefix, prefix, compact-prefix or typo match of the query against the indexed names of
/// an entity (canonical, localized, transliterated, aliases): the condition used by every name lookup.
fn push_name_match(qb: &mut QueryBuilder<'_, Postgres>, entity_type: &str, id_expr: &str, q: &str) {
    qb.push(format!(
        "{id_expr} IN (SELECT d.entity_id FROM search_documents d WHERE d.entity_type = "
    ))
    .push_bind(entity_type.to_owned())
    .push(" AND d.origin <> 'metadata' AND (d.key LIKE lyrics_search_key(")
    .push_bind(q.to_owned())
    .push(") || '%' OR d.key LIKE '% ' || lyrics_search_key(")
    .push_bind(q.to_owned())
    .push(") || '%' OR d.compact LIKE replace(lyrics_search_key(")
    .push_bind(q.to_owned())
    .push("), ' ', '') || '%' OR d.key % lyrics_search_key(")
    .push_bind(q.to_owned())
    .push(")))");
}

/// Artists or albums credited on songs that match the filters, with how many such songs each has.
pub async fn list_entities(
    conn: &mut PgConnection,
    facet: Facet,
    q: Option<&str>,
    filters: &[Filter],
    limit: i64,
    offset: i64,
) -> Res<Vec<EntityValue>> {
    let Kind::Entity(entity_type) = facet.kind() else {
        return Ok(Vec::new());
    };
    let (table, name) = match entity_type {
        "album" => ("albums", "e.title"),
        "raga" => ("ragas", "e.name"),
        "deity" => ("deities", "e.name"),
        "tag" => ("tags", "e.name"),
        _ => ("artists", "e.name"),
    };
    // Counting happens inside (one row per entity), names and paging outside, so `count(*) OVER()`
    // is the number of entities, not of songs.
    let mut qb = QueryBuilder::new(format!(
        "SELECT e.id, {name} AS name, g.songs, count(*) OVER() AS total FROM (
             SELECT l.v AS id, count(DISTINCT c.id)::bigint AS songs FROM ({}) l
             JOIN compositions c ON c.id = l.song WHERE ",
        facet.links()
    ));
    qb.push(PUBLISHED);
    push_filters(&mut qb, filters);
    qb.push(format!(" GROUP BY l.v) g JOIN {table} e ON e.id = g.id WHERE TRUE"));
    if let Some(q) = q.filter(|q| !q.trim().is_empty()) {
        qb.push(" AND ");
        push_name_match(&mut qb, entity_type, "e.id", q);
    }
    qb.push(format!(" ORDER BY lower({name}), e.id LIMIT "))
        .push_bind(limit)
        .push(" OFFSET ")
        .push_bind(offset);
    qb.build_query_as().fetch_all(conn).await
}

/// Values of a text facet (raga, deity, religion, category, language, tag) on songs that match the filters.
pub async fn list_values(
    conn: &mut PgConnection,
    facet: Facet,
    q: Option<&str>,
    filters: &[Filter],
    limit: i64,
    offset: i64,
) -> Res<Vec<TextValue>> {
    let mut qb = QueryBuilder::new(
        "SELECT v AS value, label, songs, count(*) OVER() AS total FROM (SELECT l.v, min(l.label) AS label, count(DISTINCT c.id)::bigint AS songs FROM (",
    );
    qb.push(facet.links())
        .push(") l JOIN compositions c ON c.id = l.song WHERE ")
        .push(PUBLISHED);
    push_filters(&mut qb, filters);
    if let Some(q) = q.filter(|q| !q.trim().is_empty()) {
        qb.push(" AND (lyrics_search_key(l.label) LIKE lyrics_search_key(")
            .push_bind(q.to_owned())
            .push(") || '%' OR lyrics_search_key(l.label) LIKE '% ' || lyrics_search_key(")
            .push_bind(q.to_owned())
            .push(") || '%' OR l.v LIKE ")
            .push_bind(format!("{}%", q.trim().to_lowercase().replace(['%', '_', '\\'], "")))
            .push(")");
    }
    qb.push(" GROUP BY l.v) t ORDER BY lower(label), v LIMIT ")
        .push_bind(limit)
        .push(" OFFSET ")
        .push_bind(offset);
    qb.build_query_as().fetch_all(conn).await
}

// ---------- suggestions ----------

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct NameHit {
    pub entity_id: Uuid,
    pub origin: String,
    pub class: String,
    pub display: String,
    pub score: f64,
    pub singer: bool,
    pub lyricist: bool,
    pub composer: bool,
}

fn role_exists(role: &str, id: &str) -> String {
    match role {
        "singer" => format!(
            "EXISTS (SELECT 1 FROM credits k JOIN recordings r ON r.id = k.recording_id JOIN compositions c ON c.id = r.composition_id
              WHERE k.artist_id = {id} AND k.role IN ('performer', 'featured_performer')"
        ),
        other => format!(
            "EXISTS (SELECT 1 FROM credits k JOIN compositions c ON c.id = k.composition_id
              WHERE k.artist_id = {id} AND k.role = '{other}'"
        ),
    }
}

/// Names of `doc_type` (`artist`, `album` or `composition`) matching `q`, ranked, and restricted to
/// those that have a published song matching the active filters. For artists each row also says
/// which roles apply in that context (so "Yesudas" can be offered as a singer and as a composer).
pub async fn suggest_names(
    conn: &mut PgConnection,
    ranking: &Ranking,
    doc_type: &str,
    q: &str,
    filters: &[Filter],
    limit: i64,
) -> Res<Vec<NameHit>> {
    let cfg = serde_json::to_value(ranking).expect("ranking serializes");
    let mut qb = QueryBuilder::new("WITH q AS (SELECT lyrics_search_key(");
    qb.push_bind(q.to_owned())
        .push(") AS k, replace(lyrics_search_key(")
        .push_bind(q.to_owned())
        .push("), ' ', '') AS c, lower(btrim(")
        .push_bind(q.to_owned())
        .push(")) AS raw, lyrics_phonetic_key(")
        .push_bind(q.to_owned())
        .push(") AS p, lyrics_phonetic_key(")
        .push_bind(q.to_owned())
        .push(", false) AS pp),
         cand AS (
           SELECT d.entity_id, d.origin, d.display, d.popularity,
                  CASE WHEN lower(d.display) = q.raw THEN 'exact'
                       WHEN d.key = q.k OR d.compact = q.c THEN 'normalized'
                       WHEN d.key LIKE q.k || '%' OR d.compact LIKE q.c || '%' THEN 'prefix'
                       WHEN d.key LIKE '% ' || q.k || '%' THEN 'word'
                       WHEN length(q.pp) >= 3 AND (d.phon LIKE q.p || '%' OR d.phon LIKE q.pp || '%') THEN 'phonetic'
                       ELSE 'fuzzy' END AS class,
                  similarity(d.key, q.k)::float8 AS sim
           FROM search_documents d, q
           WHERE d.entity_type = ")
        .push_bind(doc_type.to_owned())
        .push(
            " AND q.c <> '' AND d.origin <> 'metadata'
              AND (d.key LIKE q.k || '%' OR d.key LIKE '% ' || q.k || '%' OR d.compact LIKE q.c || '%' OR d.key % q.k
                   OR (length(q.pp) >= 3 AND (d.phon LIKE q.p || '%' OR d.phon LIKE q.pp || '%')))
              AND (d.origin = 'name'
                   OR (d.origin = 'alias' AND EXISTS (SELECT 1 FROM entity_aliases a WHERE a.id = d.source_id AND a.verification_state = 'verified'))
                   OR (d.origin IN ('localization', 'transliteration') AND EXISTS (
                        SELECT 1 FROM entity_localizations l WHERE l.id = d.source_id AND l.verification_state = 'verified')))
              AND ",
        );
    // Context: the entity must be on at least one published song that matches every active filter.
    match doc_type {
        "composition" => {
            qb.push("EXISTS (SELECT 1 FROM compositions c WHERE c.id = d.entity_id AND ")
                .push(PUBLISHED);
            push_filters(&mut qb, filters);
            qb.push(")");
        }
        "album" => {
            qb.push(
                "EXISTS (SELECT 1 FROM album_tracks t JOIN recordings r ON r.id = t.recording_id
                  JOIN compositions c ON c.id = r.composition_id WHERE t.album_id = d.entity_id AND ",
            )
            .push(PUBLISHED);
            push_filters(&mut qb, filters);
            qb.push(")");
        }
        "raga" | "deity" | "tag" => {
            let (from, cond) = match doc_type {
                "raga" => ("compositions c", "c.raga_id = d.entity_id"),
                "deity" => ("compositions c", "c.deity_id = d.entity_id"),
                _ => (
                    "composition_tags ct JOIN compositions c ON c.id = ct.composition_id",
                    "ct.tag_id = d.entity_id",
                ),
            };
            qb.push(format!("EXISTS (SELECT 1 FROM {from} WHERE {cond} AND "))
                .push(PUBLISHED);
            push_filters(&mut qb, filters);
            qb.push(")");
        }
        _ => {
            qb.push("(");
            for (i, role) in ["singer", "lyricist", "composer"].into_iter().enumerate() {
                if i > 0 {
                    qb.push(" OR ");
                }
                qb.push(role_exists(role, "d.entity_id")).push(" AND ").push(PUBLISHED);
                push_filters(&mut qb, filters);
                qb.push(")");
            }
            qb.push(")");
        }
    }
    qb.push(
        "),
         scored AS (
           SELECT cand.*, (w.j->'class_weights'->>(CASE WHEN cand.class IN ('word', 'phonetic') THEN 'prefix' ELSE cand.class END))::float8
                    * CASE WHEN cand.class = 'word' THEN 0.9 WHEN cand.class = 'phonetic' THEN 0.8
                           WHEN cand.class = 'fuzzy' THEN cand.sim ELSE 1 END
                    * (w.j->'origin_multipliers'->>cand.origin)::float8
                  + LEAST((w.j->>'popularity_cap')::float8, (w.j->>'popularity_weight')::float8 * ln(1 + cand.popularity::float8)) AS score
           FROM cand, (SELECT ",
    );
    qb.push_bind(cfg).push(
        "::jsonb AS j) w),
         best AS (SELECT DISTINCT ON (entity_id) * FROM scored ORDER BY entity_id, score DESC, origin),
         top AS (SELECT * FROM best ORDER BY score DESC, entity_id LIMIT ",
    );
    qb.push_bind(limit).push(
        ")
         SELECT b.entity_id, b.origin, b.class, b.display, b.score, ",
    );
    if doc_type == "artist" {
        for (i, role) in ["singer", "lyricist", "composer"].into_iter().enumerate() {
            if i > 0 {
                qb.push(", ");
            }
            qb.push(role_exists(role, "b.entity_id")).push(" AND ").push(PUBLISHED);
            push_filters(&mut qb, filters);
            qb.push(format!(") AS {role}"));
        }
    } else {
        qb.push("false AS singer, false AS lyricist, false AS composer");
    }
    qb.push(" FROM top b ORDER BY b.score DESC, b.entity_id");
    qb.build_query_as().fetch_all(conn).await
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ValueHit {
    pub facet: String,
    pub value: String,
    pub label: String,
    pub songs: i64,
}

/// Text-valued metadata (raga, deity, language, religion, category) whose name starts a word of `q`,
/// that occurs on a published song matching the filters.
pub async fn suggest_values(conn: &mut PgConnection, q: &str, filters: &[Filter], per_kind: i64) -> Res<Vec<ValueHit>> {
    let lower = q.trim().to_lowercase();
    let codes = |list: &[&str]| -> Vec<String> {
        list.iter()
            .filter(|c| c.starts_with(&lower))
            .map(|c| (*c).to_owned())
            .collect()
    };
    let mut qb = QueryBuilder::new("(SELECT 'x' AS facet, '' AS value, '' AS label, 0::bigint AS songs WHERE FALSE)");
    for (facet, col, list) in [
        ("religion", "c.religion", codes(&RELIGIONS)),
        ("category", "c.category", codes(&CATEGORIES)),
        ("form", "c.devotional_form", codes(&domain::catalogue::DEVOTIONAL_FORMS)),
    ] {
        if list.is_empty() {
            continue;
        }
        qb.push(format!(
            " UNION ALL (SELECT '{facet}', {col}, {col}, count(*)::bigint FROM compositions c WHERE {col} = ANY("
        ))
        .push_bind(list)
        .push(") AND ")
        .push(PUBLISHED);
        push_filters(&mut qb, filters);
        qb.push(format!(" GROUP BY 2 ORDER BY 4 DESC LIMIT {per_kind})"));
    }
    qb.push(
        " UNION ALL (SELECT 'language', lower(l.iso639_3), min(coalesce(l.endonym, l.name_en)), count(DISTINCT c.id)::bigint
          FROM languages l JOIN contributions x ON x.language_id = l.id AND x.published_revision_id IS NOT NULL
          JOIN recordings r ON r.id = x.recording_id JOIN compositions c ON c.id = r.composition_id
          WHERE TRUE",
    );
    qb.push(" AND (lyrics_search_key(l.name_en) LIKE lyrics_search_key(")
        .push_bind(q.to_owned())
        .push(") || '%' OR lyrics_search_key(coalesce(l.endonym, '')) LIKE lyrics_search_key(")
        .push_bind(q.to_owned())
        .push(") || '%' OR lower(l.iso639_3) = ")
        .push_bind(lower)
        .push(")");
    push_filters(&mut qb, filters);
    qb.push(format!(" GROUP BY 2 ORDER BY 4 DESC LIMIT {per_kind})"));
    qb.build_query_as().fetch_all(conn).await
}
