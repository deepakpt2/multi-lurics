//! SQL for public pages: localized slugs (generation, history, resolution), page data and the
//! sitemap. A page exists only for published content (`seo_is_public`, migration 0008).

use domain::slug::SlugPolicy;
use serde::Deserialize;
use sqlx::PgConnection;
use uuid::Uuid;

type Res<T> = Result<T, sqlx::Error>;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SeoConfig {
    pub locales: Vec<String>,
    pub default_locale: String,
    pub slug: SlugPolicy,
    pub sitemap_page_size: i64,
}

impl Default for SeoConfig {
    fn default() -> Self {
        Self {
            locales: vec!["en".into()],
            default_locale: "en".into(),
            slug: SlugPolicy::default(),
            sitemap_page_size: 5000,
        }
    }
}

pub async fn load_config(conn: &mut PgConnection) -> Res<SeoConfig> {
    let v: Option<serde_json::Value> = sqlx::query_scalar("SELECT value FROM settings WHERE key = 'seo'")
        .fetch_optional(conn)
        .await?;
    Ok(match v.map(serde_json::from_value::<SeoConfig>) {
        Some(Ok(mut c)) if !c.locales.is_empty() => {
            if !c.locales.contains(&c.default_locale) {
                c.default_locale = c.locales[0].clone();
            }
            c.sitemap_page_size = c.sitemap_page_size.clamp(1, 50_000);
            c
        }
        Some(Err(e)) => {
            tracing::warn!(error = %e, "seo setting is invalid; using defaults");
            SeoConfig::default()
        }
        _ => SeoConfig::default(),
    })
}

/// URL scope of each entity type, and back.
pub fn scope_of(entity_type: &str) -> &'static str {
    match entity_type {
        "composition" => "song",
        "recording" => "recording",
        _ => "artist",
    }
}

pub fn entity_type_of(scope: &str) -> Option<&'static str> {
    Some(match scope {
        "song" => "composition",
        "recording" => "recording",
        "artist" => "artist",
        _ => return None,
    })
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LocaleLang {
    pub locale: String,
    pub language_id: Uuid,
}

/// Configured UI locales that map to a known language, in configured order.
pub async fn locale_languages(conn: &mut PgConnection, cfg: &SeoConfig) -> Res<Vec<LocaleLang>> {
    let mut found: Vec<LocaleLang> =
        sqlx::query_as("SELECT iso639_1 AS locale, id AS language_id FROM languages WHERE iso639_1 = ANY($1)")
            .bind(&cfg.locales)
            .fetch_all(conn)
            .await?;
    found.sort_by_key(|l| cfg.locales.iter().position(|x| *x == l.locale));
    Ok(found)
}

pub async fn is_public(conn: &mut PgConnection, entity_type: &str, id: Uuid) -> Res<bool> {
    sqlx::query_scalar("SELECT seo_is_public($1, $2)")
        .bind(entity_type)
        .bind(id)
        .fetch_one(conn)
        .await
}

fn table_title(entity_type: &str) -> (&'static str, &'static str) {
    match entity_type {
        "composition" => ("compositions", "title"),
        "recording" => ("recordings", "title"),
        _ => ("artists", "name"),
    }
}

/// Canonical title and default language; `None` when the entity no longer exists.
pub async fn base(conn: &mut PgConnection, entity_type: &str, id: Uuid) -> Res<Option<(String, Option<Uuid>)>> {
    let (table, col) = table_title(entity_type);
    sqlx::query_as(&format!("SELECT {col}, default_language_id FROM {table} WHERE id = $1"))
        .bind(id)
        .fetch_optional(conn)
        .await
}

/// Locales that really have content: a verified localization in the locale's language, or that
/// language is the entity's default (or, when none is declared, the default locale). Never empty.
pub async fn content_locales(
    conn: &mut PgConnection,
    cfg: &SeoConfig,
    langs: &[LocaleLang],
    entity_type: &str,
    id: Uuid,
    default_language: Option<Uuid>,
) -> Res<Vec<LocaleLang>> {
    let have: Vec<Uuid> = sqlx::query_scalar(
        "SELECT DISTINCT language_id FROM entity_localizations
         WHERE entity_type = $1 AND entity_id = $2 AND verification_state = 'verified'",
    )
    .bind(entity_type)
    .bind(id)
    .fetch_all(conn)
    .await?;
    let mut out: Vec<LocaleLang> = langs
        .iter()
        .filter(|l| {
            have.contains(&l.language_id)
                || default_language == Some(l.language_id)
                // Without a declared default language the title is taken to be in the default locale.
                || (default_language.is_none() && l.locale == cfg.default_locale)
        })
        .cloned()
        .collect();
    if out.is_empty() {
        out.extend(langs.iter().find(|l| l.locale == cfg.default_locale).cloned());
    }
    Ok(out)
}

async fn localized_title(conn: &mut PgConnection, entity_type: &str, id: Uuid, lang: Uuid) -> Res<Option<String>> {
    sqlx::query_scalar(
        "SELECT name FROM entity_localizations
         WHERE entity_type = $1 AND entity_id = $2 AND language_id = $3 AND verification_state = 'verified'
         ORDER BY is_preferred DESC, created_at, id LIMIT 1",
    )
    .bind(entity_type)
    .bind(id)
    .bind(lang)
    .fetch_optional(conn)
    .await
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SlugRow {
    pub entity_type: String,
    pub entity_id: Uuid,
    pub language_id: Uuid,
    pub slug: String,
    pub is_current: bool,
}

const SLUG_COLS: &str = "entity_type, entity_id, language_id, slug, is_current";

/// Brings the entity's slugs up to date and returns every slug it has ever had (for cache tags).
/// Idempotent. Creates nothing for unpublished content, so a draft never gets a URL. A changed
/// title retires the old slug (kept as history) and makes a new one current. Collisions are
/// resolved the same way every time: the plain slug if free, otherwise the slug plus the first
/// eight hex digits of the id. Must run inside a transaction.
pub async fn sync(conn: &mut PgConnection, entity_type: &str, id: Uuid) -> Res<Vec<SlugRow>> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("slug:{entity_type}:{id}"))
        .execute(&mut *conn)
        .await?;
    let cfg = load_config(conn).await?;
    let langs = locale_languages(conn, &cfg).await?;
    if let Some((title, default_language)) = base(conn, entity_type, id).await? {
        if is_public(conn, entity_type, id).await? {
            let id8 = id.simple().to_string()[..8].to_owned();
            for loc in content_locales(&mut *conn, &cfg, &langs, entity_type, id, default_language).await? {
                let text = localized_title(&mut *conn, entity_type, id, loc.language_id)
                    .await?
                    .unwrap_or_else(|| title.clone());
                let desired = cfg.slug.slugify(&text);
                // A reserved word is never used bare: it would shadow a route or a word we own.
                let reserved = desired.as_deref().is_some_and(|s| cfg.slug.is_reserved(s));
                let plain = desired.unwrap_or_else(|| format!("id-{id8}"));
                let suffixed = format!("{plain}-{id8}");
                let current: Option<String> = sqlx::query_scalar(
                    "SELECT slug FROM localized_slugs WHERE entity_type = $1 AND entity_id = $2 AND language_id = $3 AND is_current",
                )
                .bind(entity_type)
                .bind(id)
                .bind(loc.language_id)
                .fetch_optional(&mut *conn)
                .await?;
                let plain_owner: Option<Uuid> = sqlx::query_scalar(
                    "SELECT entity_id FROM localized_slugs WHERE entity_type = $1 AND language_id = $2 AND slug = $3",
                )
                .bind(entity_type)
                .bind(loc.language_id)
                .bind(&plain)
                .fetch_optional(&mut *conn)
                .await?;
                let free = |o: Option<Uuid>| o.is_none() || o == Some(id);
                let chosen = if free(plain_owner) && !reserved {
                    plain
                } else {
                    suffixed
                };
                if current.as_deref() == Some(chosen.as_str()) {
                    continue;
                }
                sqlx::query("UPDATE localized_slugs SET is_current = false WHERE entity_type = $1 AND entity_id = $2 AND language_id = $3 AND is_current")
                    .bind(entity_type)
                    .bind(id)
                    .bind(loc.language_id)
                    .execute(&mut *conn)
                    .await?;
                // Reactivate our own retired slug, otherwise add a new one.
                let reactivated = sqlx::query("UPDATE localized_slugs SET is_current = true WHERE entity_type = $1 AND entity_id = $2 AND language_id = $3 AND slug = $4")
                    .bind(entity_type)
                    .bind(id)
                    .bind(loc.language_id)
                    .bind(&chosen)
                    .execute(&mut *conn)
                    .await?
                    .rows_affected();
                if reactivated == 0 {
                    sqlx::query("INSERT INTO localized_slugs (entity_type, entity_id, language_id, slug, is_current) VALUES ($1, $2, $3, $4, true)")
                        .bind(entity_type)
                        .bind(id)
                        .bind(loc.language_id)
                        .bind(&chosen)
                        .execute(&mut *conn)
                        .await?;
                }
            }
        }
    }
    all_slugs(conn, entity_type, &[id]).await
}

pub async fn all_slugs(conn: &mut PgConnection, entity_type: &str, ids: &[Uuid]) -> Res<Vec<SlugRow>> {
    sqlx::query_as(&format!("SELECT {SLUG_COLS} FROM localized_slugs WHERE entity_type = $1 AND entity_id = ANY($2) ORDER BY created_at, id"))
        .bind(entity_type)
        .bind(ids)
        .fetch_all(conn)
        .await
}

pub async fn current_slugs(conn: &mut PgConnection, entity_type: &str, ids: &[Uuid]) -> Res<Vec<SlugRow>> {
    sqlx::query_as(&format!(
        "SELECT {SLUG_COLS} FROM localized_slugs WHERE entity_type = $1 AND entity_id = ANY($2) AND is_current"
    ))
    .bind(entity_type)
    .bind(ids)
    .fetch_all(conn)
    .await
}

pub async fn find_slug(
    conn: &mut PgConnection,
    entity_type: &str,
    language_id: Uuid,
    slug: &str,
) -> Res<Option<SlugRow>> {
    sqlx::query_as(&format!(
        "SELECT {SLUG_COLS} FROM localized_slugs WHERE entity_type = $1 AND language_id = $2 AND slug = $3"
    ))
    .bind(entity_type)
    .bind(language_id)
    .bind(slug)
    .fetch_optional(conn)
    .await
}

/// A slug asked for under a locale that never used it: the same words in another language.
/// Deterministic: current slugs first, then by language code.
pub async fn find_slug_any(conn: &mut PgConnection, entity_type: &str, slug: &str) -> Res<Option<SlugRow>> {
    sqlx::query_as(
        "SELECT s.entity_type, s.entity_id, s.language_id, s.slug, s.is_current
         FROM localized_slugs s JOIN languages l ON l.id = s.language_id
         WHERE s.entity_type = $1 AND s.slug = $2 ORDER BY s.is_current DESC, l.iso639_3 LIMIT 1",
    )
    .bind(entity_type)
    .bind(slug)
    .fetch_optional(conn)
    .await
}

// ---------- page data ----------

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Core {
    pub title: String,
    pub year: Option<i16>,
    /// ISWC, ISRC or ISNI.
    pub code: Option<String>,
    pub kind: Option<String>,
    pub detail: Option<String>,
    pub duration_ms: Option<i32>,
    pub parent_id: Option<Uuid>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

pub async fn core(conn: &mut PgConnection, entity_type: &str, id: Uuid) -> Res<Option<Core>> {
    let sql = match entity_type {
        "composition" => "SELECT title, year, iswc AS code, NULL::text AS kind, NULL::text AS detail, NULL::int AS duration_ms, NULL::uuid AS parent_id, updated_at FROM compositions WHERE id = $1",
        "recording" => "SELECT title, year, isrc AS code, NULL::text AS kind, NULL::text AS detail, duration_ms, composition_id AS parent_id, updated_at FROM recordings WHERE id = $1",
        _ => "SELECT name AS title, NULL::smallint AS year, isni AS code, kind, disambiguation AS detail, NULL::int AS duration_ms, NULL::uuid AS parent_id, updated_at FROM artists WHERE id = $1",
    };
    sqlx::query_as(sql).bind(id).fetch_optional(conn).await
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CreditRow {
    pub artist_id: Uuid,
    pub name: String,
    pub role: String,
}

/// Credits on a composition or recording, in listed order.
pub async fn credits(conn: &mut PgConnection, entity_type: &str, id: Uuid) -> Res<Vec<CreditRow>> {
    let col = if entity_type == "composition" {
        "composition_id"
    } else {
        "recording_id"
    };
    sqlx::query_as(&format!("SELECT x.artist_id, a.name, x.role FROM credits x JOIN artists a ON a.id = x.artist_id WHERE x.{col} = $1 ORDER BY x.position, x.role, a.name"))
        .bind(id)
        .fetch_all(conn)
        .await
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Linked {
    pub id: Uuid,
    pub title: String,
}

/// Public related pages, by relation. Always limited.
pub async fn linked(conn: &mut PgConnection, relation: &str, id: Uuid) -> Res<Vec<Linked>> {
    let sql = match relation {
        "recordings_of_song" => "SELECT id, title FROM recordings r WHERE composition_id = $1 AND seo_is_public('recording', id) ORDER BY created_at, id LIMIT 50",
        "songs_of_artist" => "SELECT c.id, c.title FROM compositions c WHERE seo_is_public('composition', c.id) AND (
                EXISTS (SELECT 1 FROM credits x WHERE x.artist_id = $1 AND x.composition_id = c.id)
                OR EXISTS (SELECT 1 FROM credits x JOIN recordings r ON r.id = x.recording_id WHERE x.artist_id = $1 AND r.composition_id = c.id))
             ORDER BY c.title, c.id LIMIT 100",
        "recordings_of_artist" => "SELECT r.id, r.title FROM recordings r WHERE seo_is_public('recording', r.id)
             AND EXISTS (SELECT 1 FROM credits x WHERE x.artist_id = $1 AND x.recording_id = r.id)
             ORDER BY r.title, r.id LIMIT 100",
        _ => return Ok(Vec::new()),
    };
    sqlx::query_as(sql).bind(id).fetch_all(conn).await
}

pub async fn is_public_batch(conn: &mut PgConnection, entity_type: &str, ids: &[Uuid]) -> Res<Vec<Uuid>> {
    sqlx::query_scalar("SELECT id FROM unnest($2::uuid[]) AS id WHERE seo_is_public($1, id)")
        .bind(entity_type)
        .bind(ids)
        .fetch_all(conn)
        .await
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LyricRow {
    pub contribution_id: Uuid,
    pub recording_id: Uuid,
    pub kind: String,
    pub language: String,
    pub script: String,
    pub rtl: bool,
    pub is_primary: bool,
    pub body: String,
    pub revised_at: chrono::DateTime<chrono::Utc>,
}

/// Published lyrics only, via the single public read model.
pub async fn lyrics(conn: &mut PgConnection, recording_ids: &[Uuid]) -> Res<Vec<LyricRow>> {
    sqlx::query_as(
        "SELECT p.contribution_id, p.recording_id, p.type AS kind, coalesce(l.iso639_1, l.iso639_3) AS language,
                s.iso15924 AS script, s.is_rtl AS rtl, p.is_primary, p.body, p.revised_at
         FROM published_lyrics p JOIN languages l ON l.id = p.language_id JOIN scripts s ON s.id = p.script_id
         WHERE p.recording_id = ANY($1)
         ORDER BY p.is_primary DESC, p.type = 'original' DESC, p.revised_at DESC, p.contribution_id LIMIT 20",
    )
    .bind(recording_ids)
    .fetch_all(conn)
    .await
}

// ---------- merges and the sitemap ----------

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SitemapRow {
    pub entity_type: String,
    pub entity_id: Uuid,
    pub lastmod: chrono::DateTime<chrono::Utc>,
}

pub async fn sitemap_count(conn: &mut PgConnection) -> Res<i64> {
    sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM compositions WHERE seo_is_public('composition', id))
              + (SELECT count(*) FROM recordings WHERE seo_is_public('recording', id))
              + (SELECT count(*) FROM artists WHERE seo_is_public('artist', id))",
    )
    .fetch_one(conn)
    .await
}

/// A stable page of public entities: ordered by type then id, so pages never overlap.
pub async fn sitemap_page(conn: &mut PgConnection, offset: i64, limit: i64) -> Res<Vec<SitemapRow>> {
    sqlx::query_as(
        "SELECT entity_type, entity_id, lastmod FROM (
             SELECT 'artist' AS entity_type, a.id AS entity_id, a.updated_at AS lastmod FROM artists a WHERE seo_is_public('artist', a.id)
             UNION ALL
             SELECT 'composition', c.id, greatest(c.updated_at, (SELECT max(k.updated_at) FROM contributions k JOIN recordings r ON r.id = k.recording_id WHERE r.composition_id = c.id AND k.published_revision_id IS NOT NULL))
             FROM compositions c WHERE seo_is_public('composition', c.id)
             UNION ALL
             SELECT 'recording', r.id, greatest(r.updated_at, (SELECT max(k.updated_at) FROM contributions k WHERE k.recording_id = r.id AND k.published_revision_id IS NOT NULL))
             FROM recordings r WHERE seo_is_public('recording', r.id)
         ) e ORDER BY entity_type, entity_id OFFSET $1 LIMIT $2",
    )
    .bind(offset)
    .bind(limit)
    .fetch_all(conn)
    .await
}

/// Content locales for many entities at once, for sitemap alternates.
pub async fn content_languages_batch(
    conn: &mut PgConnection,
    entity_type: &str,
    ids: &[Uuid],
) -> Res<Vec<(Uuid, Option<Uuid>)>> {
    let (table, _) = table_title(entity_type);
    sqlx::query_as(&format!(
        "SELECT entity_id, language_id FROM entity_localizations
         WHERE entity_type = $1 AND entity_id = ANY($2) AND verification_state = 'verified'
         UNION SELECT id, default_language_id FROM {table} WHERE id = ANY($2)"
    ))
    .bind(entity_type)
    .bind(ids)
    .fetch_all(conn)
    .await
}
