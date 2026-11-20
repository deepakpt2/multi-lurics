//! Catalogue persistence. Every function takes a connection so the caller owns the transaction.
//! Values arrive already validated and normalized by `domain::catalogue`.

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::error::ErrorKind;
use sqlx::PgConnection;
use uuid::Uuid;

type Res<T> = Result<T, sqlx::Error>;
type After = Option<(DateTime<Utc>, Uuid)>;

/// A constraint violation the application can turn into a domain response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DbViolation {
    Unique(String),
    ForeignKey(String),
    /// Deadlock or serialization failure: the request lost a race and may be retried.
    Concurrency,
}

pub fn violation(e: &sqlx::Error) -> Option<DbViolation> {
    let sqlx::Error::Database(d) = e else { return None };
    if matches!(d.code().as_deref(), Some("40P01" | "40001")) {
        return Some(DbViolation::Concurrency);
    }
    let c = d.constraint().unwrap_or_default().to_owned();
    match d.kind() {
        ErrorKind::UniqueViolation => Some(DbViolation::Unique(c)),
        ErrorKind::ForeignKeyViolation => Some(DbViolation::ForeignKey(c)),
        _ => None,
    }
}

/// Case-insensitive prefix pattern for `lower(col) LIKE $n` with LIKE metacharacters escaped.
pub fn like_prefix(raw: Option<&str>) -> Option<String> {
    let s = raw.map(str::trim).filter(|s| !s.is_empty())?;
    let mut out = String::with_capacity(s.len() + 1);
    for c in s.to_lowercase().chars() {
        if matches!(c, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('%');
    Some(out)
}

/// Unique identifier columns, used to name the existing record on a duplicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Identifier {
    ArtistIsni,
    CompositionIswc,
    RecordingIsrc,
    AlbumBarcode,
}

impl Identifier {
    pub fn from_constraint(c: &str) -> Option<Self> {
        Some(match c {
            "artists_isni_key" => Self::ArtistIsni,
            "compositions_iswc_key" => Self::CompositionIswc,
            "recordings_isrc_key" => Self::RecordingIsrc,
            "albums_barcode_key" => Self::AlbumBarcode,
            _ => return None,
        })
    }

    fn sql(self) -> &'static str {
        match self {
            Self::ArtistIsni => "SELECT id FROM artists WHERE isni = $1",
            Self::CompositionIswc => "SELECT id FROM compositions WHERE iswc = $1",
            Self::RecordingIsrc => "SELECT id FROM recordings WHERE isrc = $1",
            Self::AlbumBarcode => "SELECT id FROM albums WHERE barcode = $1",
        }
    }
}

/// The record already holding `value`, other than `except`.
pub async fn find_by_identifier(
    conn: &mut PgConnection,
    ident: Identifier,
    value: &str,
    except: Option<Uuid>,
) -> Res<Option<Uuid>> {
    let id: Option<Uuid> = sqlx::query_scalar(ident.sql()).bind(value).fetch_optional(conn).await?;
    Ok(id.filter(|id| Some(*id) != except))
}

/// Ids among `ids` that do not exist in `table` (a fixed, trusted table name).
async fn missing(conn: &mut PgConnection, table: &'static str, ids: &[Uuid]) -> Res<Vec<Uuid>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    sqlx::query_scalar(&format!(
        "SELECT x FROM unnest($1::uuid[]) AS x WHERE NOT EXISTS (SELECT 1 FROM {table} t WHERE t.id = x)"
    ))
    .bind(ids)
    .fetch_all(conn)
    .await
}

pub async fn missing_artists(conn: &mut PgConnection, ids: &[Uuid]) -> Res<Vec<Uuid>> {
    missing(conn, "artists", ids).await
}

pub async fn missing_recordings(conn: &mut PgConnection, ids: &[Uuid]) -> Res<Vec<Uuid>> {
    missing(conn, "recordings", ids).await
}

pub async fn composition_exists(conn: &mut PgConnection, id: Uuid) -> Res<bool> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM compositions WHERE id = $1)")
        .bind(id)
        .fetch_one(conn)
        .await
}

// ---------- artists ----------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtistData {
    pub name: String,
    pub kind: String,
    pub disambiguation: Option<String>,
    pub isni: Option<String>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct ArtistRow {
    pub id: Uuid,
    pub name: String,
    pub kind: String,
    pub disambiguation: Option<String>,
    pub isni: Option<String>,
    pub version: i32,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ArtistRow {
    pub fn data(&self) -> ArtistData {
        ArtistData {
            name: self.name.clone(),
            kind: self.kind.clone(),
            disambiguation: self.disambiguation.clone(),
            isni: self.isni.clone(),
        }
    }
}

const ARTIST: &str = "id, name, kind, disambiguation, isni, version, created_by, created_at, updated_at";

pub async fn insert_artist(conn: &mut PgConnection, d: &ArtistData, by: Uuid) -> Res<ArtistRow> {
    sqlx::query_as(&format!(
        "INSERT INTO artists (name, kind, disambiguation, isni, created_by) VALUES ($1, $2, $3, $4, $5) RETURNING {ARTIST}"
    ))
    .bind(&d.name)
    .bind(&d.kind)
    .bind(&d.disambiguation)
    .bind(&d.isni)
    .bind(by)
    .fetch_one(conn)
    .await
}

pub async fn update_artist(conn: &mut PgConnection, id: Uuid, d: &ArtistData) -> Res<ArtistRow> {
    sqlx::query_as(&format!(
        "UPDATE artists SET name = $2, kind = $3, disambiguation = $4, isni = $5,
             version = version + 1, updated_at = now()
         WHERE id = $1 RETURNING {ARTIST}"
    ))
    .bind(id)
    .bind(&d.name)
    .bind(&d.kind)
    .bind(&d.disambiguation)
    .bind(&d.isni)
    .fetch_one(conn)
    .await
}

/// `lock` takes the row lock that serializes writers of this aggregate.
pub async fn get_artist(conn: &mut PgConnection, id: Uuid, lock: bool) -> Res<Option<ArtistRow>> {
    let lock = if lock { " FOR UPDATE" } else { "" };
    sqlx::query_as(&format!("SELECT {ARTIST} FROM artists WHERE id = $1{lock}"))
        .bind(id)
        .fetch_optional(conn)
        .await
}

#[derive(Debug, Clone, Default)]
pub struct ArtistFilter {
    pub kind: Option<String>,
    pub isni: Option<String>,
    pub name_prefix: Option<String>,
}

pub async fn list_artists(conn: &mut PgConnection, f: &ArtistFilter, after: After, limit: i64) -> Res<Vec<ArtistRow>> {
    let (ts, id) = after.unzip();
    sqlx::query_as(&format!(
        "SELECT {ARTIST} FROM artists
         WHERE ($1::text IS NULL OR kind = $1)
           AND ($2::text IS NULL OR isni = $2)
           AND ($3::text IS NULL OR lower(name) LIKE $3)
           AND ($4::timestamptz IS NULL OR (created_at, id) < ($4, $5))
         ORDER BY created_at DESC, id DESC LIMIT $6"
    ))
    .bind(&f.kind)
    .bind(&f.isni)
    .bind(like_prefix(f.name_prefix.as_deref()))
    .bind(ts)
    .bind(id)
    .bind(limit)
    .fetch_all(conn)
    .await
}

// ---------- compositions ----------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompositionData {
    pub title: String,
    pub iswc: Option<String>,
    pub year: Option<i16>,
    pub category: String,
    pub raga: Option<String>,
    pub religion: Option<String>,
    pub deity: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct CompositionRow {
    pub id: Uuid,
    pub title: String,
    pub iswc: Option<String>,
    pub year: Option<i16>,
    pub category: String,
    pub raga: Option<String>,
    pub religion: Option<String>,
    pub deity: Option<String>,
    pub tags: Vec<String>,
    pub version: i32,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl CompositionRow {
    pub fn data(&self) -> CompositionData {
        CompositionData {
            title: self.title.clone(),
            iswc: self.iswc.clone(),
            year: self.year,
            category: self.category.clone(),
            raga: self.raga.clone(),
            religion: self.religion.clone(),
            deity: self.deity.clone(),
            tags: self.tags.clone(),
        }
    }
}

/// A song's raga, deity and tags are catalogue entities; reads show their canonical names.
const COMPOSITION: &str =
    "compositions.id, compositions.title, compositions.iswc, compositions.year, compositions.category,
    (SELECT r.name FROM ragas r WHERE r.id = compositions.raga_id) AS raga, compositions.religion,
    (SELECT d.name FROM deities d WHERE d.id = compositions.deity_id) AS deity,
    ARRAY(SELECT g.name FROM composition_tags ct JOIN tags g ON g.id = ct.tag_id
          WHERE ct.composition_id = compositions.id ORDER BY ct.position, g.id) AS tags,
    compositions.version, compositions.created_by, compositions.created_at, compositions.updated_at";

/// Which entities a song's raga, deity and tags are.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Terms {
    pub raga_id: Option<Uuid>,
    pub deity_id: Option<Uuid>,
    pub tag_ids: Vec<Uuid>,
}

pub async fn insert_composition(
    conn: &mut PgConnection,
    d: &CompositionData,
    t: &Terms,
    by: Uuid,
) -> Res<CompositionRow> {
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO compositions (title, iswc, year, category, raga_id, religion, deity_id, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id",
    )
    .bind(&d.title)
    .bind(&d.iswc)
    .bind(d.year)
    .bind(&d.category)
    .bind(t.raga_id)
    .bind(&d.religion)
    .bind(t.deity_id)
    .bind(by)
    .fetch_one(&mut *conn)
    .await?;
    set_tags(conn, id, &t.tag_ids).await?;
    Ok(get_composition(conn, id, false).await?.expect("just inserted"))
}

pub async fn update_composition(
    conn: &mut PgConnection,
    id: Uuid,
    d: &CompositionData,
    t: &Terms,
) -> Res<CompositionRow> {
    sqlx::query(
        "UPDATE compositions SET title = $2, iswc = $3, year = $4, category = $5, raga_id = $6, religion = $7,
                deity_id = $8, version = version + 1, updated_at = now()
         WHERE id = $1",
    )
    .bind(id)
    .bind(&d.title)
    .bind(&d.iswc)
    .bind(d.year)
    .bind(&d.category)
    .bind(t.raga_id)
    .bind(&d.religion)
    .bind(t.deity_id)
    .execute(&mut *conn)
    .await?;
    set_tags(conn, id, &t.tag_ids).await?;
    Ok(get_composition(conn, id, false).await?.expect("just updated"))
}

/// Replaces a song's tags, keeping the order given.
async fn set_tags(conn: &mut PgConnection, id: Uuid, tags: &[Uuid]) -> Res<()> {
    sqlx::query("DELETE FROM composition_tags WHERE composition_id = $1 AND NOT (tag_id = ANY($2))")
        .bind(id)
        .bind(tags)
        .execute(&mut *conn)
        .await?;
    for (i, tag) in tags.iter().enumerate() {
        sqlx::query(
            "INSERT INTO composition_tags (composition_id, tag_id, position) VALUES ($1, $2, $3)
             ON CONFLICT (composition_id, tag_id) DO UPDATE SET position = EXCLUDED.position",
        )
        .bind(id)
        .bind(tag)
        .bind(i as i32)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

pub async fn get_composition(conn: &mut PgConnection, id: Uuid, lock: bool) -> Res<Option<CompositionRow>> {
    let lock = if lock { " FOR UPDATE OF compositions" } else { "" };
    sqlx::query_as(&format!("SELECT {COMPOSITION} FROM compositions WHERE id = $1{lock}"))
        .bind(id)
        .fetch_optional(conn)
        .await
}

/// The entity a typed raga, deity or tag stands for. Matches by normalized name, verified localization or
/// alias, and by phonetic key, so "Mohanam" and "മോഹനം" are the same one. Nothing matches: a new entity is
/// created from the text as written. `kind` is a fixed internal name, never user input.
pub async fn resolve_term(conn: &mut PgConnection, kind: &'static str, text: &str, by: Uuid) -> Res<(Uuid, String)> {
    debug_assert!(matches!(kind, "raga" | "deity" | "tag"));
    let table = match kind {
        "raga" => "ragas",
        "deity" => "deities",
        _ => "tags",
    };
    // One writer at a time per kind, so two songs saved together cannot create the same entity twice.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("term:{kind}"))
        .execute(&mut *conn)
        .await?;
    let found: Option<(Uuid, String)> = sqlx::query_as(&format!(
        "WITH q AS (SELECT replace(lyrics_search_key($2), ' ', '') AS c, lyrics_phonetic_key($2) AS p)
         SELECT e.id, e.name FROM search_keys s JOIN {table} e ON e.id = s.entity_id, q
         WHERE s.entity_type = $1 AND q.c <> '' AND (s.compact = q.c OR (q.p <> '' AND s.phon = q.p))
         ORDER BY (s.compact = q.c) DESC, (s.origin = 'name') DESC, e.created_at, e.id LIMIT 1"
    ))
    .bind(kind)
    .bind(text)
    .fetch_optional(&mut *conn)
    .await?;
    if let Some(hit) = found {
        return Ok(hit);
    }
    sqlx::query_as(&format!(
        "INSERT INTO {table} (name, created_by) VALUES ($1, $2) RETURNING id, name"
    ))
    .bind(text)
    .bind(by)
    .fetch_one(conn)
    .await
}

/// The entity ids a raga, deity or tag filter text stands for (same matching as `resolve_term`, never creates).
pub async fn find_terms(conn: &mut PgConnection, kind: &'static str, text: &str) -> Res<Vec<Uuid>> {
    sqlx::query_scalar(
        "WITH q AS (SELECT replace(lyrics_search_key($2), ' ', '') AS c, lyrics_phonetic_key($2) AS p)
         SELECT DISTINCT s.entity_id FROM search_keys s, q
         WHERE s.entity_type = $1 AND q.c <> '' AND (s.compact = q.c OR (q.p <> '' AND s.phon = q.p))",
    )
    .bind(kind)
    .bind(text)
    .fetch_all(conn)
    .await
}

#[derive(Debug, Clone, Default)]
pub struct CompositionFilter {
    pub artist_id: Option<Uuid>,
    pub iswc: Option<String>,
    pub title_prefix: Option<String>,
    pub category: Option<String>,
    pub raga: Option<String>,
    pub deity: Option<String>,
    pub tag: Option<String>,
}

pub async fn list_compositions(
    conn: &mut PgConnection,
    f: &CompositionFilter,
    after: After,
    limit: i64,
) -> Res<Vec<CompositionRow>> {
    let (ts, id) = after.unzip();
    // Filter text means the entities it names, whatever the spelling or script.
    let raga = match &f.raga {
        Some(t) => Some(find_terms(conn, "raga", t).await?),
        None => None,
    };
    let deity = match &f.deity {
        Some(t) => Some(find_terms(conn, "deity", t).await?),
        None => None,
    };
    let tag = match &f.tag {
        Some(t) => Some(find_terms(conn, "tag", t).await?),
        None => None,
    };
    sqlx::query_as(&format!(
        "SELECT {COMPOSITION} FROM compositions
         WHERE ($1::uuid IS NULL OR EXISTS (SELECT 1 FROM credits k WHERE k.composition_id = compositions.id AND k.artist_id = $1))
           AND ($2::text IS NULL OR iswc = $2)
           AND ($3::text IS NULL OR lower(title) LIKE $3)
           AND ($4::timestamptz IS NULL OR (created_at, id) < ($4, $5))
           AND ($7::text IS NULL OR category = $7)
           AND ($8::uuid[] IS NULL OR raga_id = ANY($8))
           AND ($9::uuid[] IS NULL OR deity_id = ANY($9))
           AND ($10::uuid[] IS NULL OR EXISTS (SELECT 1 FROM composition_tags ct WHERE ct.composition_id = compositions.id AND ct.tag_id = ANY($10)))
         ORDER BY created_at DESC, id DESC LIMIT $6"
    ))
    .bind(f.artist_id)
    .bind(&f.iswc)
    .bind(like_prefix(f.title_prefix.as_deref()))
    .bind(ts)
    .bind(id)
    .bind(limit)
    .bind(&f.category)
    .bind(raga)
    .bind(deity)
    .bind(tag)
    .fetch_all(conn)
    .await
}

// ---------- recordings ----------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordingData {
    pub composition_id: Uuid,
    pub title: String,
    pub isrc: Option<String>,
    pub duration_ms: Option<i32>,
    pub year: Option<i16>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct RecordingRow {
    pub id: Uuid,
    pub composition_id: Uuid,
    pub title: String,
    pub isrc: Option<String>,
    pub duration_ms: Option<i32>,
    pub year: Option<i16>,
    pub version: i32,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl RecordingRow {
    pub fn data(&self) -> RecordingData {
        RecordingData {
            composition_id: self.composition_id,
            title: self.title.clone(),
            isrc: self.isrc.clone(),
            duration_ms: self.duration_ms,
            year: self.year,
        }
    }
}

const RECORDING: &str =
    "id, composition_id, title, isrc, duration_ms, year, version, created_by, created_at, updated_at";

pub async fn insert_recording(conn: &mut PgConnection, d: &RecordingData, by: Uuid) -> Res<RecordingRow> {
    sqlx::query_as(&format!(
        "INSERT INTO recordings (composition_id, title, isrc, duration_ms, year, created_by)
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING {RECORDING}"
    ))
    .bind(d.composition_id)
    .bind(&d.title)
    .bind(&d.isrc)
    .bind(d.duration_ms)
    .bind(d.year)
    .bind(by)
    .fetch_one(conn)
    .await
}

pub async fn update_recording(conn: &mut PgConnection, id: Uuid, d: &RecordingData) -> Res<RecordingRow> {
    sqlx::query_as(&format!(
        "UPDATE recordings SET composition_id = $2, title = $3, isrc = $4, duration_ms = $5, year = $6,
             version = version + 1, updated_at = now()
         WHERE id = $1 RETURNING {RECORDING}"
    ))
    .bind(id)
    .bind(d.composition_id)
    .bind(&d.title)
    .bind(&d.isrc)
    .bind(d.duration_ms)
    .bind(d.year)
    .fetch_one(conn)
    .await
}

pub async fn get_recording(conn: &mut PgConnection, id: Uuid, lock: bool) -> Res<Option<RecordingRow>> {
    let lock = if lock { " FOR UPDATE" } else { "" };
    sqlx::query_as(&format!("SELECT {RECORDING} FROM recordings WHERE id = $1{lock}"))
        .bind(id)
        .fetch_optional(conn)
        .await
}

#[derive(Debug, Clone, Default)]
pub struct RecordingFilter {
    pub composition_id: Option<Uuid>,
    pub artist_id: Option<Uuid>,
    pub album_id: Option<Uuid>,
    pub isrc: Option<String>,
    pub title_prefix: Option<String>,
}

pub async fn list_recordings(
    conn: &mut PgConnection,
    f: &RecordingFilter,
    after: After,
    limit: i64,
) -> Res<Vec<RecordingRow>> {
    let (ts, id) = after.unzip();
    sqlx::query_as(&format!(
        "SELECT {RECORDING} FROM recordings r
         WHERE ($1::uuid IS NULL OR composition_id = $1)
           AND ($2::uuid IS NULL OR EXISTS (SELECT 1 FROM credits k WHERE k.recording_id = r.id AND k.artist_id = $2))
           AND ($3::uuid IS NULL OR EXISTS (SELECT 1 FROM album_tracks t WHERE t.recording_id = r.id AND t.album_id = $3))
           AND ($4::text IS NULL OR isrc = $4)
           AND ($5::text IS NULL OR lower(title) LIKE $5)
           AND ($6::timestamptz IS NULL OR (created_at, id) < ($6, $7))
         ORDER BY created_at DESC, id DESC LIMIT $8"
    ))
    .bind(f.composition_id)
    .bind(f.artist_id)
    .bind(f.album_id)
    .bind(&f.isrc)
    .bind(like_prefix(f.title_prefix.as_deref()))
    .bind(ts)
    .bind(id)
    .bind(limit)
    .fetch_all(conn)
    .await
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct RecordingSummaryRow {
    pub id: Uuid,
    pub title: String,
    pub composition_id: Uuid,
    pub isrc: Option<String>,
    pub duration_ms: Option<i32>,
}

/// Recordings of a composition (newest first), capped, plus the total count.
pub async fn composition_recordings(
    conn: &mut PgConnection,
    composition_id: Uuid,
    cap: i64,
) -> Res<(Vec<RecordingSummaryRow>, i64)> {
    let rows = sqlx::query_as(
        "SELECT id, title, composition_id, isrc, duration_ms FROM recordings
         WHERE composition_id = $1 ORDER BY created_at DESC, id DESC LIMIT $2",
    )
    .bind(composition_id)
    .bind(cap)
    .fetch_all(&mut *conn)
    .await?;
    let total = sqlx::query_scalar("SELECT count(*) FROM recordings WHERE composition_id = $1")
        .bind(composition_id)
        .fetch_one(conn)
        .await?;
    Ok((rows, total))
}

/// Compositions of the given recordings, read under a key-share lock so they cannot be
/// re-pointed to another composition until this transaction ends.
pub async fn recording_compositions(conn: &mut PgConnection, ids: &[Uuid]) -> Res<Vec<(Uuid, Uuid)>> {
    sqlx::query_as("SELECT id, composition_id FROM recordings WHERE id = ANY($1) ORDER BY id FOR KEY SHARE")
        .bind(ids)
        .fetch_all(conn)
        .await
}

// ---------- recording relationships ----------

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RelationshipData {
    pub kind: String,
    pub related_recording_id: Uuid,
    /// Set iff the kind requires both recordings to share a composition.
    pub composition_id: Option<Uuid>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RelationshipRow {
    pub kind: String,
    /// The recording on the other side of the relationship.
    pub other_id: Uuid,
    pub other_title: String,
    pub other_composition_id: Uuid,
}

pub async fn outgoing_relationships(conn: &mut PgConnection, recording_id: Uuid) -> Res<Vec<RelationshipRow>> {
    sqlx::query_as(
        "SELECT rel.kind, r.id AS other_id, r.title AS other_title, r.composition_id AS other_composition_id
         FROM recording_relationships rel JOIN recordings r ON r.id = rel.related_recording_id
         WHERE rel.recording_id = $1 ORDER BY rel.created_at, rel.id",
    )
    .bind(recording_id)
    .fetch_all(conn)
    .await
}

pub async fn incoming_relationships(conn: &mut PgConnection, recording_id: Uuid) -> Res<Vec<RelationshipRow>> {
    sqlx::query_as(
        "SELECT rel.kind, r.id AS other_id, r.title AS other_title, r.composition_id AS other_composition_id
         FROM recording_relationships rel JOIN recordings r ON r.id = rel.recording_id
         WHERE rel.related_recording_id = $1 ORDER BY rel.created_at, rel.id",
    )
    .bind(recording_id)
    .fetch_all(conn)
    .await
}

/// Number of relationships (either direction) that pin this recording to its composition.
pub async fn composition_bound_relationships(conn: &mut PgConnection, recording_id: Uuid) -> Res<i64> {
    sqlx::query_scalar(
        "SELECT count(*) FROM recording_relationships
         WHERE composition_id IS NOT NULL AND (recording_id = $1 OR related_recording_id = $1)",
    )
    .bind(recording_id)
    .fetch_one(conn)
    .await
}

pub async fn current_relationships(conn: &mut PgConnection, recording_id: Uuid) -> Res<Vec<RelationshipData>> {
    let rows: Vec<(String, Uuid, Option<Uuid>)> = sqlx::query_as(
        "SELECT kind, related_recording_id, composition_id FROM recording_relationships
         WHERE recording_id = $1 ORDER BY created_at, id",
    )
    .bind(recording_id)
    .fetch_all(conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(kind, related_recording_id, composition_id)| RelationshipData {
            kind,
            related_recording_id,
            composition_id,
        })
        .collect())
}

pub async fn delete_relationships(conn: &mut PgConnection, recording_id: Uuid) -> Res<()> {
    sqlx::query("DELETE FROM recording_relationships WHERE recording_id = $1")
        .bind(recording_id)
        .execute(conn)
        .await
        .map(|_| ())
}

pub async fn insert_relationships(
    conn: &mut PgConnection,
    recording_id: Uuid,
    rels: &[RelationshipData],
    by: Uuid,
) -> Res<()> {
    for r in rels {
        sqlx::query(
            "INSERT INTO recording_relationships (kind, recording_id, related_recording_id, composition_id, created_by)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(&r.kind)
        .bind(recording_id)
        .bind(r.related_recording_id)
        .bind(r.composition_id)
        .bind(by)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

// ---------- credits ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreditOwner {
    Composition(Uuid),
    Recording(Uuid),
    Album(Uuid),
}

impl CreditOwner {
    fn column(self) -> (&'static str, Uuid) {
        match self {
            Self::Composition(id) => ("composition_id", id),
            Self::Recording(id) => ("recording_id", id),
            Self::Album(id) => ("album_id", id),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CreditData {
    pub artist_id: Uuid,
    pub role: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CreditRow {
    pub artist_id: Uuid,
    pub artist_name: String,
    pub role: String,
    pub role_id: Uuid,
    pub role_category: String,
    pub role_label: String,
}

pub async fn list_credits(conn: &mut PgConnection, owner: CreditOwner) -> Res<Vec<CreditRow>> {
    let (col, id) = owner.column();
    sqlx::query_as(&format!(
        "SELECT k.artist_id, a.name AS artist_name, k.role, r.id AS role_id, r.category AS role_category,
                r.label_en AS role_label
         FROM credits k JOIN artists a ON a.id = k.artist_id JOIN credit_roles r ON r.code = k.role
         WHERE k.{col} = $1 ORDER BY k.position"
    ))
    .bind(id)
    .fetch_all(conn)
    .await
}

/// Replaces the whole credit list (order preserved).
pub async fn replace_credits(conn: &mut PgConnection, owner: CreditOwner, credits: &[CreditData]) -> Res<()> {
    let (col, id) = owner.column();
    sqlx::query(&format!("DELETE FROM credits WHERE {col} = $1"))
        .bind(id)
        .execute(&mut *conn)
        .await?;
    for (i, c) in credits.iter().enumerate() {
        sqlx::query(&format!(
            "INSERT INTO credits (artist_id, role, {col}, position) VALUES ($1, $2, $3, $4)"
        ))
        .bind(c.artist_id)
        .bind(&c.role)
        .bind(id)
        .bind(i as i32)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ArtistCreditRow {
    pub target_type: String,
    pub target_id: Uuid,
    pub title: String,
    pub role: String,
    pub role_id: Uuid,
    pub role_category: String,
    pub role_label: String,
}

/// What an artist is credited on, newest first, capped.
pub async fn artist_credits(conn: &mut PgConnection, artist_id: Uuid, cap: i64) -> Res<Vec<ArtistCreditRow>> {
    sqlx::query_as(
        "SELECT CASE WHEN k.composition_id IS NOT NULL THEN 'composition'
                     WHEN k.recording_id IS NOT NULL THEN 'recording' ELSE 'album' END AS target_type,
                coalesce(k.composition_id, k.recording_id, k.album_id) AS target_id,
                coalesce(c.title, r.title, al.title) AS title,
                k.role, cr.id AS role_id, cr.category AS role_category, cr.label_en AS role_label
         FROM credits k
         JOIN credit_roles cr ON cr.code = k.role
         LEFT JOIN compositions c ON c.id = k.composition_id
         LEFT JOIN recordings r ON r.id = k.recording_id
         LEFT JOIN albums al ON al.id = k.album_id
         WHERE k.artist_id = $1
         ORDER BY k.created_at DESC, k.id DESC LIMIT $2",
    )
    .bind(artist_id)
    .bind(cap)
    .fetch_all(conn)
    .await
}

// ---------- albums ----------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlbumData {
    pub title: String,
    pub kind: String,
    pub release_year: Option<i16>,
    pub barcode: Option<String>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct AlbumRow {
    pub id: Uuid,
    pub title: String,
    pub kind: String,
    pub release_year: Option<i16>,
    pub barcode: Option<String>,
    pub version: i32,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl AlbumRow {
    pub fn data(&self) -> AlbumData {
        AlbumData {
            title: self.title.clone(),
            kind: self.kind.clone(),
            release_year: self.release_year,
            barcode: self.barcode.clone(),
        }
    }
}

const ALBUM: &str = "id, title, kind, release_year, barcode, version, created_by, created_at, updated_at";

pub async fn insert_album(conn: &mut PgConnection, d: &AlbumData, by: Uuid) -> Res<AlbumRow> {
    sqlx::query_as(&format!(
        "INSERT INTO albums (title, kind, release_year, barcode, created_by) VALUES ($1, $2, $3, $4, $5) RETURNING {ALBUM}"
    ))
    .bind(&d.title)
    .bind(&d.kind)
    .bind(d.release_year)
    .bind(&d.barcode)
    .bind(by)
    .fetch_one(conn)
    .await
}

pub async fn update_album(conn: &mut PgConnection, id: Uuid, d: &AlbumData) -> Res<AlbumRow> {
    sqlx::query_as(&format!(
        "UPDATE albums SET title = $2, kind = $3, release_year = $4, barcode = $5,
             version = version + 1, updated_at = now()
         WHERE id = $1 RETURNING {ALBUM}"
    ))
    .bind(id)
    .bind(&d.title)
    .bind(&d.kind)
    .bind(d.release_year)
    .bind(&d.barcode)
    .fetch_one(conn)
    .await
}

pub async fn get_album(conn: &mut PgConnection, id: Uuid, lock: bool) -> Res<Option<AlbumRow>> {
    let lock = if lock { " FOR UPDATE" } else { "" };
    sqlx::query_as(&format!("SELECT {ALBUM} FROM albums WHERE id = $1{lock}"))
        .bind(id)
        .fetch_optional(conn)
        .await
}

#[derive(Debug, Clone, Default)]
pub struct AlbumFilter {
    pub artist_id: Option<Uuid>,
    pub kind: Option<String>,
    pub barcode: Option<String>,
    pub title_prefix: Option<String>,
}

pub async fn list_albums(conn: &mut PgConnection, f: &AlbumFilter, after: After, limit: i64) -> Res<Vec<AlbumRow>> {
    let (ts, id) = after.unzip();
    sqlx::query_as(&format!(
        "SELECT {ALBUM} FROM albums al
         WHERE ($1::uuid IS NULL OR EXISTS (SELECT 1 FROM credits k WHERE k.album_id = al.id AND k.artist_id = $1))
           AND ($2::text IS NULL OR kind = $2)
           AND ($3::text IS NULL OR barcode = $3)
           AND ($4::text IS NULL OR lower(title) LIKE $4)
           AND ($5::timestamptz IS NULL OR (created_at, id) < ($5, $6))
         ORDER BY created_at DESC, id DESC LIMIT $7"
    ))
    .bind(f.artist_id)
    .bind(&f.kind)
    .bind(&f.barcode)
    .bind(like_prefix(f.title_prefix.as_deref()))
    .bind(ts)
    .bind(id)
    .bind(limit)
    .fetch_all(conn)
    .await
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TrackRow {
    pub position: i32,
    pub recording_id: Uuid,
    pub title: String,
    pub composition_id: Uuid,
    pub duration_ms: Option<i32>,
}

pub async fn list_tracks(conn: &mut PgConnection, album_id: Uuid) -> Res<Vec<TrackRow>> {
    sqlx::query_as(
        "SELECT t.position, r.id AS recording_id, r.title, r.composition_id, r.duration_ms
         FROM album_tracks t JOIN recordings r ON r.id = t.recording_id
         WHERE t.album_id = $1 ORDER BY t.position",
    )
    .bind(album_id)
    .fetch_all(conn)
    .await
}

pub async fn track_ids(conn: &mut PgConnection, album_id: Uuid) -> Res<Vec<Uuid>> {
    sqlx::query_scalar("SELECT recording_id FROM album_tracks WHERE album_id = $1 ORDER BY position")
        .bind(album_id)
        .fetch_all(conn)
        .await
}

/// Replaces the track list; positions are 1-based in the given order.
pub async fn replace_tracks(conn: &mut PgConnection, album_id: Uuid, recordings: &[Uuid]) -> Res<()> {
    sqlx::query("DELETE FROM album_tracks WHERE album_id = $1")
        .bind(album_id)
        .execute(&mut *conn)
        .await?;
    sqlx::query(
        "INSERT INTO album_tracks (album_id, position, recording_id)
         SELECT $1, ord::int, rid FROM unnest($2::uuid[]) WITH ORDINALITY AS u(rid, ord)",
    )
    .bind(album_id)
    .bind(recordings)
    .execute(conn)
    .await
    .map(|_| ())
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AppearanceRow {
    pub album_id: Uuid,
    pub title: String,
    pub position: i32,
}

pub async fn recording_albums(conn: &mut PgConnection, recording_id: Uuid) -> Res<Vec<AppearanceRow>> {
    sqlx::query_as(
        "SELECT al.id AS album_id, al.title, t.position
         FROM album_tracks t JOIN albums al ON al.id = t.album_id
         WHERE t.recording_id = $1 ORDER BY al.created_at, al.id LIMIT 100",
    )
    .bind(recording_id)
    .fetch_all(conn)
    .await
}

#[cfg(test)]
mod tests {
    use super::like_prefix;

    #[test]
    fn like_prefix_escapes_metacharacters() {
        assert_eq!(like_prefix(Some(" Ab%_\\ ")).as_deref(), Some("ab\\%\\_\\\\%"));
        assert_eq!(like_prefix(Some("  ")), None);
        assert_eq!(like_prefix(None), None);
    }
}
