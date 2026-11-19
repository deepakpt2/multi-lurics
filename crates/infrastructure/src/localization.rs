//! Localization persistence: reference data, localized names + immutable revisions, aliases,
//! default languages and the minimal taxonomy entities. Callers own the transaction.

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::PgConnection;
use uuid::Uuid;

type Res<T> = Result<T, sqlx::Error>;
type After = Option<(DateTime<Utc>, Uuid)>;

// ---------- reference data ----------

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LanguageRow {
    pub id: Uuid,
    pub iso639_3: String,
    pub iso639_1: Option<String>,
    pub iso639_2: Option<String>,
    pub name_en: String,
    pub endonym: String,
    pub is_rtl: bool,
    pub default_script: Option<String>,
    pub is_extension: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ScriptRow {
    pub id: Uuid,
    pub iso15924: String,
    pub numeric_code: Option<i32>,
    pub name_en: String,
    pub is_rtl: bool,
    pub is_extension: bool,
    pub created_at: DateTime<Utc>,
}

const LANGUAGE: &str = "SELECT l.id, l.iso639_3, l.iso639_1, l.iso639_2, l.name_en, l.endonym, l.is_rtl,
        s.iso15924 AS default_script, l.is_extension, l.created_at
    FROM languages l LEFT JOIN scripts s ON s.id = l.default_script_id";

pub async fn list_languages(conn: &mut PgConnection, after: After, limit: i64) -> Res<Vec<LanguageRow>> {
    let (ts, id) = after.unzip();
    sqlx::query_as(&format!(
        "{LANGUAGE} WHERE ($1::timestamptz IS NULL OR (l.created_at, l.id) < ($1, $2))
         ORDER BY l.created_at DESC, l.id DESC LIMIT $3"
    ))
    .bind(ts)
    .bind(id)
    .bind(limit)
    .fetch_all(conn)
    .await
}

pub async fn list_scripts(conn: &mut PgConnection, after: After, limit: i64) -> Res<Vec<ScriptRow>> {
    let (ts, id) = after.unzip();
    sqlx::query_as(
        "SELECT id, iso15924, numeric_code, name_en, is_rtl, is_extension, created_at FROM scripts
         WHERE ($1::timestamptz IS NULL OR (created_at, id) < ($1, $2))
         ORDER BY created_at DESC, id DESC LIMIT $3",
    )
    .bind(ts)
    .bind(id)
    .bind(limit)
    .fetch_all(conn)
    .await
}

/// Language by ISO 639-1 or 639-3 code (case-insensitive).
pub async fn find_language(conn: &mut PgConnection, code: &str) -> Res<Option<LanguageRow>> {
    sqlx::query_as(&format!(
        "{LANGUAGE} WHERE l.iso639_1 = lower($1) OR l.iso639_3 = lower($1)"
    ))
    .bind(code)
    .fetch_optional(conn)
    .await
}

pub async fn find_script(conn: &mut PgConnection, code: &str) -> Res<Option<ScriptRow>> {
    sqlx::query_as(
        "SELECT id, iso15924, numeric_code, name_en, is_rtl, is_extension, created_at FROM scripts
         WHERE lower(iso15924) = lower($1)",
    )
    .bind(code)
    .fetch_optional(conn)
    .await
}

/// (language id, tag, is_rtl) for display attributes.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LangInfo {
    pub id: Uuid,
    pub tag: String,
    pub is_rtl: bool,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ScriptInfo {
    pub id: Uuid,
    pub code: String,
    pub is_rtl: bool,
}

pub async fn language_infos(conn: &mut PgConnection, ids: &[Uuid]) -> Res<Vec<LangInfo>> {
    sqlx::query_as("SELECT id, coalesce(iso639_1, iso639_3) AS tag, is_rtl FROM languages WHERE id = ANY($1)")
        .bind(ids)
        .fetch_all(conn)
        .await
}

pub async fn script_infos(conn: &mut PgConnection, ids: &[Uuid]) -> Res<Vec<ScriptInfo>> {
    sqlx::query_as("SELECT id, iso15924 AS code, is_rtl FROM scripts WHERE id = ANY($1)")
        .bind(ids)
        .fetch_all(conn)
        .await
}

// ---------- display candidates (verified only) ----------

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CandidateRow {
    pub entity_id: Uuid,
    pub language_id: Uuid,
    pub script_id: Option<Uuid>,
    pub name: String,
    pub description: Option<String>,
    pub is_preferred: bool,
}

/// The only query that feeds public display: pending and rejected rows can never appear here.
pub async fn verified_candidates(conn: &mut PgConnection, entity_type: &str, ids: &[Uuid]) -> Res<Vec<CandidateRow>> {
    sqlx::query_as(
        "SELECT entity_id, language_id, script_id, name, description, is_preferred
         FROM entity_localizations
         WHERE entity_type = $1 AND entity_id = ANY($2) AND verification_state = 'verified'
         ORDER BY entity_id, is_preferred DESC, created_at, id",
    )
    .bind(entity_type)
    .bind(ids)
    .fetch_all(conn)
    .await
}

/// `table` comes from `EntityType::table()`, a fixed set.
pub async fn default_languages(
    conn: &mut PgConnection,
    table: &'static str,
    ids: &[Uuid],
) -> Res<Vec<(Uuid, Option<Uuid>)>> {
    sqlx::query_as(&format!(
        "SELECT id, default_language_id FROM {table} WHERE id = ANY($1)"
    ))
    .bind(ids)
    .fetch_all(conn)
    .await
}

/// Version and creator of an entity, optionally locking its row. `created_by` is NULL for roles.
pub async fn entity_meta(
    conn: &mut PgConnection,
    table: &'static str,
    id: Uuid,
    lock: bool,
) -> Res<Option<(i32, Option<Uuid>)>> {
    let by = if table == "credit_roles" {
        "NULL::uuid"
    } else {
        "created_by"
    };
    let lock = if lock { " FOR UPDATE" } else { "" };
    sqlx::query_as(&format!("SELECT version, {by} FROM {table} WHERE id = $1{lock}"))
        .bind(id)
        .fetch_optional(conn)
        .await
}

pub async fn set_default_language(conn: &mut PgConnection, table: &'static str, id: Uuid, lang: Uuid) -> Res<i32> {
    let touch = if table == "credit_roles" {
        ""
    } else {
        ", updated_at = now()"
    };
    sqlx::query_scalar(&format!(
        "UPDATE {table} SET default_language_id = $2, version = version + 1{touch} WHERE id = $1 RETURNING version"
    ))
    .bind(id)
    .bind(lang)
    .fetch_one(conn)
    .await
}

pub async fn current_default_language(conn: &mut PgConnection, table: &'static str, id: Uuid) -> Res<Option<Uuid>> {
    sqlx::query_scalar(&format!("SELECT default_language_id FROM {table} WHERE id = $1"))
        .bind(id)
        .fetch_one(conn)
        .await
}

// ---------- localizations ----------

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LocalizationData {
    pub language_id: Uuid,
    pub script_id: Option<Uuid>,
    pub name: String,
    pub description: Option<String>,
    pub is_preferred: bool,
    pub provenance: String,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct LocalizationRow {
    pub id: Uuid,
    pub entity_type: String,
    pub entity_id: Uuid,
    pub language_id: Uuid,
    pub script_id: Option<Uuid>,
    pub language_code: String,
    pub script_code: Option<String>,
    pub is_rtl: bool,
    pub name: String,
    pub description: Option<String>,
    pub is_preferred: bool,
    pub provenance: String,
    pub source: Option<String>,
    pub verification_state: String,
    pub created_by: Uuid,
    pub verified_by: Option<Uuid>,
    pub verified_at: Option<DateTime<Utc>>,
    pub current_revision_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl LocalizationRow {
    pub fn data(&self) -> LocalizationData {
        LocalizationData {
            language_id: self.language_id,
            script_id: self.script_id,
            name: self.name.clone(),
            description: self.description.clone(),
            is_preferred: self.is_preferred,
            provenance: self.provenance.clone(),
            source: self.source.clone(),
        }
    }
}

const LOC: &str = "SELECT e.id, e.entity_type, e.entity_id, e.language_id, e.script_id,
        coalesce(l.iso639_1, l.iso639_3) AS language_code, s.iso15924 AS script_code,
        coalesce(s.is_rtl, l.is_rtl) AS is_rtl, e.name, e.description, e.is_preferred, e.provenance, e.source,
        e.verification_state, e.created_by, e.verified_by, e.verified_at, e.current_revision_id,
        e.created_at, e.updated_at
    FROM entity_localizations e JOIN languages l ON l.id = e.language_id LEFT JOIN scripts s ON s.id = e.script_id";

pub async fn get_localization(conn: &mut PgConnection, id: Uuid, lock: bool) -> Res<Option<LocalizationRow>> {
    let lock = if lock { " FOR UPDATE OF e" } else { "" };
    sqlx::query_as(&format!("{LOC} WHERE e.id = $1{lock}"))
        .bind(id)
        .fetch_optional(conn)
        .await
}

pub async fn insert_localization(
    conn: &mut PgConnection,
    entity_type: &str,
    entity_id: Uuid,
    d: &LocalizationData,
    state: &str,
    by: Uuid,
    verified_by: Option<Uuid>,
) -> Res<Uuid> {
    sqlx::query_scalar(
        "INSERT INTO entity_localizations
            (entity_type, entity_id, language_id, script_id, name, description, is_preferred, provenance, source,
             verification_state, created_by, verified_by, verified_at)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12, CASE WHEN $12::uuid IS NULL THEN NULL ELSE now() END)
         RETURNING id",
    )
    .bind(entity_type)
    .bind(entity_id)
    .bind(d.language_id)
    .bind(d.script_id)
    .bind(&d.name)
    .bind(&d.description)
    .bind(d.is_preferred)
    .bind(&d.provenance)
    .bind(&d.source)
    .bind(state)
    .bind(by)
    .bind(verified_by)
    .fetch_one(conn)
    .await
}

/// Rewrites the content and moderation state together (an edit and its resulting state are one change).
pub async fn update_localization(
    conn: &mut PgConnection,
    id: Uuid,
    d: &LocalizationData,
    state: &str,
    verified_by: Option<Uuid>,
) -> Res<()> {
    sqlx::query(
        "UPDATE entity_localizations SET language_id=$2, script_id=$3, name=$4, description=$5, is_preferred=$6,
            provenance=$7, source=$8, verification_state=$9, verified_by=$10,
            verified_at = CASE WHEN $10::uuid IS NULL THEN NULL ELSE now() END, updated_at = now()
         WHERE id = $1",
    )
    .bind(id)
    .bind(d.language_id)
    .bind(d.script_id)
    .bind(&d.name)
    .bind(&d.description)
    .bind(d.is_preferred)
    .bind(&d.provenance)
    .bind(&d.source)
    .bind(state)
    .bind(verified_by)
    .execute(conn)
    .await
    .map(|_| ())
}

pub async fn set_current_revision(conn: &mut PgConnection, id: Uuid, rev: Uuid) -> Res<()> {
    sqlx::query("UPDATE entity_localizations SET current_revision_id = $2 WHERE id = $1")
        .bind(id)
        .bind(rev)
        .execute(conn)
        .await
        .map(|_| ())
}

/// Another non-rejected row already holding the preferred slot for this entity+language.
pub async fn preferred_holder(
    conn: &mut PgConnection,
    entity_type: &str,
    entity_id: Uuid,
    language_id: Uuid,
    except: Option<Uuid>,
) -> Res<Option<Uuid>> {
    sqlx::query_scalar(
        "SELECT id FROM entity_localizations
         WHERE entity_type = $1 AND entity_id = $2 AND language_id = $3 AND is_preferred AND id IS DISTINCT FROM $4",
    )
    .bind(entity_type)
    .bind(entity_id)
    .bind(language_id)
    .bind(except)
    .fetch_optional(conn)
    .await
}

/// Another row already holding the (language, script) slot of this entity.
pub async fn slot_holder(
    conn: &mut PgConnection,
    entity_type: &str,
    entity_id: Uuid,
    language_id: Uuid,
    script_id: Option<Uuid>,
    except: Option<Uuid>,
) -> Res<Option<Uuid>> {
    sqlx::query_scalar(
        "SELECT id FROM entity_localizations
         WHERE entity_type = $1 AND entity_id = $2 AND language_id = $3
           AND script_id IS NOT DISTINCT FROM $4 AND id IS DISTINCT FROM $5",
    )
    .bind(entity_type)
    .bind(entity_id)
    .bind(language_id)
    .bind(script_id)
    .bind(except)
    .fetch_optional(conn)
    .await
}

pub async fn has_preferred(
    conn: &mut PgConnection,
    entity_type: &str,
    entity_id: Uuid,
    language_id: Uuid,
) -> Res<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM entity_localizations
         WHERE entity_type = $1 AND entity_id = $2 AND language_id = $3 AND is_preferred)",
    )
    .bind(entity_type)
    .bind(entity_id)
    .bind(language_id)
    .fetch_one(conn)
    .await
}

pub async fn list_localizations(
    conn: &mut PgConnection,
    entity: Option<(&str, Uuid)>,
    state: Option<&str>,
    after: After,
    limit: i64,
) -> Res<Vec<LocalizationRow>> {
    let (ts, id) = after.unzip();
    let (et, eid) = entity.map_or((None, None), |(t, i)| (Some(t.to_owned()), Some(i)));
    sqlx::query_as(&format!(
        "{LOC} WHERE ($1::text IS NULL OR e.entity_type = $1) AND ($2::uuid IS NULL OR e.entity_id = $2)
           AND ($3::text IS NULL OR e.verification_state = $3)
           AND ($4::timestamptz IS NULL OR (e.created_at, e.id) < ($4, $5))
         ORDER BY e.created_at DESC, e.id DESC LIMIT $6"
    ))
    .bind(et)
    .bind(eid)
    .bind(state)
    .bind(ts)
    .bind(id)
    .bind(limit)
    .fetch_all(conn)
    .await
}

// ---------- revisions ----------

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct RevisionRow {
    pub id: Uuid,
    pub localization_id: Uuid,
    pub revision_no: i32,
    pub parent_revision_id: Option<Uuid>,
    pub action: String,
    pub data: Value,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

/// Appends the next revision. Callers hold the localization's row lock; the unique
/// `(localization_id, parent_revision_id)` index backs that up.
pub async fn append_revision(
    conn: &mut PgConnection,
    localization_id: Uuid,
    parent: Option<Uuid>,
    action: &str,
    data: Value,
    by: Uuid,
) -> Res<Uuid> {
    sqlx::query_scalar(
        "INSERT INTO localization_revisions (localization_id, revision_no, parent_revision_id, action, data, created_by)
         VALUES ($1, (SELECT coalesce(max(revision_no), 0) + 1 FROM localization_revisions WHERE localization_id = $1),
                 $2, $3, $4, $5) RETURNING id",
    )
    .bind(localization_id)
    .bind(parent)
    .bind(action)
    .bind(data)
    .bind(by)
    .fetch_one(conn)
    .await
}

pub async fn list_revisions(
    conn: &mut PgConnection,
    localization_id: Uuid,
    after: After,
    limit: i64,
) -> Res<Vec<RevisionRow>> {
    let (ts, id) = after.unzip();
    sqlx::query_as(
        "SELECT id, localization_id, revision_no, parent_revision_id, action, data, created_by, created_at
         FROM localization_revisions
         WHERE localization_id = $1 AND ($2::timestamptz IS NULL OR (created_at, id) < ($2, $3))
         ORDER BY created_at DESC, id DESC LIMIT $4",
    )
    .bind(localization_id)
    .bind(ts)
    .bind(id)
    .bind(limit)
    .fetch_all(conn)
    .await
}

// ---------- aliases ----------

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct AliasRow {
    pub id: Uuid,
    pub entity_type: String,
    pub entity_id: Uuid,
    pub language_code: Option<String>,
    pub script_code: Option<String>,
    pub is_rtl: Option<bool>,
    pub alias: String,
    pub source: String,
    pub verification_state: String,
    pub created_by: Uuid,
    pub verified_by: Option<Uuid>,
    pub verified_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

const ALIAS: &str = "SELECT a.id, a.entity_type, a.entity_id, coalesce(l.iso639_1, l.iso639_3) AS language_code,
        s.iso15924 AS script_code, coalesce(s.is_rtl, l.is_rtl) AS is_rtl, a.alias, a.source,
        a.verification_state, a.created_by, a.verified_by, a.verified_at, a.created_at
    FROM entity_aliases a LEFT JOIN languages l ON l.id = a.language_id LEFT JOIN scripts s ON s.id = a.script_id";

/// A new alias row; `verified_by` set means it is published immediately (moderator submission).
pub struct NewAlias<'a> {
    pub entity_type: &'a str,
    pub entity_id: Uuid,
    pub language_id: Option<Uuid>,
    pub script_id: Option<Uuid>,
    pub alias: &'a str,
    pub source: &'a str,
    pub state: &'a str,
    pub created_by: Uuid,
    pub verified_by: Option<Uuid>,
}

pub async fn insert_alias(conn: &mut PgConnection, a: &NewAlias<'_>) -> Res<Uuid> {
    sqlx::query_scalar(
        "INSERT INTO entity_aliases (entity_type, entity_id, language_id, script_id, alias, source,
            verification_state, created_by, verified_by, verified_at)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9, CASE WHEN $9::uuid IS NULL THEN NULL ELSE now() END) RETURNING id",
    )
    .bind(a.entity_type)
    .bind(a.entity_id)
    .bind(a.language_id)
    .bind(a.script_id)
    .bind(a.alias)
    .bind(a.source)
    .bind(a.state)
    .bind(a.created_by)
    .bind(a.verified_by)
    .fetch_one(conn)
    .await
}

pub async fn get_alias(conn: &mut PgConnection, id: Uuid, lock: bool) -> Res<Option<AliasRow>> {
    let lock = if lock { " FOR UPDATE OF a" } else { "" };
    sqlx::query_as(&format!("{ALIAS} WHERE a.id = $1{lock}"))
        .bind(id)
        .fetch_optional(conn)
        .await
}

pub async fn set_alias_state(conn: &mut PgConnection, id: Uuid, state: &str, by: Uuid) -> Res<()> {
    sqlx::query(
        "UPDATE entity_aliases SET verification_state = $2,
            verified_by = CASE WHEN $2 = 'verified' THEN $3 END,
            verified_at = CASE WHEN $2 = 'verified' THEN now() END WHERE id = $1",
    )
    .bind(id)
    .bind(state)
    .bind(by)
    .execute(conn)
    .await
    .map(|_| ())
}

pub async fn list_aliases(
    conn: &mut PgConnection,
    entity: Option<(&str, Uuid)>,
    state: Option<&str>,
    after: After,
    limit: i64,
) -> Res<Vec<AliasRow>> {
    let (ts, id) = after.unzip();
    let (et, eid) = entity.map_or((None, None), |(t, i)| (Some(t.to_owned()), Some(i)));
    sqlx::query_as(&format!(
        "{ALIAS} WHERE ($1::text IS NULL OR a.entity_type = $1) AND ($2::uuid IS NULL OR a.entity_id = $2)
           AND ($3::text IS NULL OR a.verification_state = $3)
           AND ($4::timestamptz IS NULL OR (a.created_at, a.id) < ($4, $5))
         ORDER BY a.created_at DESC, a.id DESC LIMIT $6"
    ))
    .bind(et)
    .bind(eid)
    .bind(state)
    .bind(ts)
    .bind(id)
    .bind(limit)
    .fetch_all(conn)
    .await
}

// ---------- taxonomy (labels, publishers, genres) ----------

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct TaxonomyRow {
    pub id: Uuid,
    pub name: String,
    pub default_language_id: Option<Uuid>,
    pub version: i32,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

const TAX: &str = "id, name, default_language_id, version, created_by, created_at, updated_at";

pub async fn insert_taxonomy(conn: &mut PgConnection, table: &'static str, name: &str, by: Uuid) -> Res<TaxonomyRow> {
    sqlx::query_as(&format!(
        "INSERT INTO {table} (name, created_by) VALUES ($1, $2) RETURNING {TAX}"
    ))
    .bind(name)
    .bind(by)
    .fetch_one(conn)
    .await
}

pub async fn get_taxonomy(conn: &mut PgConnection, table: &'static str, id: Uuid) -> Res<Option<TaxonomyRow>> {
    sqlx::query_as(&format!("SELECT {TAX} FROM {table} WHERE id = $1"))
        .bind(id)
        .fetch_optional(conn)
        .await
}

pub async fn list_taxonomy(
    conn: &mut PgConnection,
    table: &'static str,
    after: After,
    limit: i64,
) -> Res<Vec<TaxonomyRow>> {
    let (ts, id) = after.unzip();
    sqlx::query_as(&format!(
        "SELECT {TAX} FROM {table} WHERE ($1::timestamptz IS NULL OR (created_at, id) < ($1, $2))
         ORDER BY created_at DESC, id DESC LIMIT $3"
    ))
    .bind(ts)
    .bind(id)
    .bind(limit)
    .fetch_all(conn)
    .await
}
