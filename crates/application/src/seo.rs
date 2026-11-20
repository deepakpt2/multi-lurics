//! Public pages: resolve a (locale, slug) to a stable entity, decide the one canonical URL, and
//! assemble what an indexable page needs. Everything here reads published content only.
//!
//! Rules that keep search engines from seeing duplicates:
//! * one current slug per (entity, language); every older slug redirects to it;
//! * a slug asked for in a locale with no content for the entity redirects to a locale that has some;
//! * spelling variants of a slug (case, Unicode normalization form) redirect to the exact slug;
//! * alternates (`hreflang`) list only locales that really have content.

use crate::localization::{names, Display};
use crate::{App, AppError};
use chrono::{DateTime, Utc};
use domain::localization::{EntityType, Requested};
use domain::slug::normalize_incoming;
use infrastructure::seo::{self as repo, LocaleLang, SeoConfig, SlugRow};
use sqlx::PgConnection;
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Canonical {
    pub locale: String,
    pub slug: String,
}

#[derive(Debug, Clone)]
pub struct Link {
    pub id: Uuid,
    pub label: Display,
    /// The public page for it, when it has one.
    pub target: Option<Canonical>,
    pub scope: &'static str,
}

#[derive(Debug, Clone)]
pub struct Credit {
    pub artist: Link,
    pub role: String,
}

#[derive(Debug, Clone)]
pub struct Lyric {
    pub id: Uuid,
    pub recording_id: Uuid,
    pub kind: String,
    pub language: String,
    pub script: String,
    pub rtl: bool,
    pub is_primary: bool,
    pub body: String,
    pub revised_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct Page {
    pub scope: &'static str,
    pub id: Uuid,
    pub locale: String,
    pub canonical: Canonical,
    /// Locales that have content, each with its own current slug.
    pub alternates: Vec<Canonical>,
    pub x_default: Canonical,
    pub title: Display,
    pub year: Option<i16>,
    pub code: Option<String>,
    pub kind: Option<String>,
    pub detail: Option<String>,
    pub duration_ms: Option<i32>,
    pub parent: Option<Link>,
    pub credits: Vec<Credit>,
    pub songs: Vec<Link>,
    pub recordings: Vec<Link>,
    pub lyrics: Vec<Lyric>,
    pub updated_at: DateTime<Utc>,
}

pub enum Resolved {
    Page(Box<Page>),
    /// This URL is not the canonical one; send the visitor (and crawlers) there for good.
    Moved(Canonical),
}

/// Picks the canonical URL: the requested locale when it has content, else the default locale,
/// else the first locale that has any. Returns it with all alternates and the `x-default`.
fn choose(
    cfg: &SeoConfig,
    content: &[LocaleLang],
    slugs: &HashMap<Uuid, String>,
    requested: Option<&str>,
) -> Option<(Canonical, Vec<Canonical>, Canonical)> {
    let alternates: Vec<Canonical> = content
        .iter()
        .filter_map(|l| {
            slugs.get(&l.language_id).map(|s| Canonical {
                locale: l.locale.clone(),
                slug: s.clone(),
            })
        })
        .collect();
    let by_locale = |loc: &str| alternates.iter().find(|a| a.locale == loc).cloned();
    let x_default = by_locale(&cfg.default_locale).or_else(|| alternates.first().cloned())?;
    let canonical = requested.and_then(by_locale).unwrap_or_else(|| x_default.clone());
    Some((canonical, alternates, x_default))
}

async fn locales(conn: &mut PgConnection, cfg: &SeoConfig) -> Result<Vec<LocaleLang>, AppError> {
    Ok(repo::locale_languages(conn, cfg).await?)
}

fn pick_slug(rows: &[SlugRow], id: Uuid, langs: &[LocaleLang], locale: &str, cfg: &SeoConfig) -> Option<Canonical> {
    let find = |loc: &str| {
        let lang = langs.iter().find(|l| l.locale == loc)?.language_id;
        rows.iter()
            .find(|r| r.entity_id == id && r.language_id == lang)
            .map(|r| Canonical {
                locale: loc.to_owned(),
                slug: r.slug.clone(),
            })
    };
    find(locale)
        .or_else(|| find(&cfg.default_locale))
        .or_else(|| langs.iter().find_map(|l| find(&l.locale)))
}

async fn links(
    conn: &mut PgConnection,
    cfg: &SeoConfig,
    langs: &[LocaleLang],
    entity_type: &'static str,
    items: &[(Uuid, String)],
    req: &Requested,
    locale: &str,
) -> Result<Vec<Link>, AppError> {
    if items.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<Uuid> = items.iter().map(|i| i.0).collect();
    let public = repo::is_public_batch(conn, entity_type, &ids).await?;
    let slugs = repo::current_slugs(conn, entity_type, &ids).await?;
    let et = match entity_type {
        "composition" => EntityType::Composition,
        "recording" => EntityType::Recording,
        _ => EntityType::Artist,
    };
    let refs: Vec<_> = items.iter().map(|(id, t)| (et, *id, t.clone())).collect();
    let shown = names(conn, req, &refs).await?;
    Ok(items
        .iter()
        .map(|(id, title)| Link {
            id: *id,
            label: shown.get(et, *id, title),
            target: public
                .contains(id)
                .then(|| pick_slug(&slugs, *id, langs, locale, cfg))
                .flatten(),
            scope: repo::scope_of(entity_type),
        })
        .collect())
}

/// `Ok(None)` is a 404: unknown slug, unknown locale, or content that is not published.
pub async fn page(app: &App, scope: &str, locale: &str, slug: &str) -> Result<Option<Resolved>, AppError> {
    let Some(entity_type) = repo::entity_type_of(scope) else {
        return Ok(None);
    };
    if slug.is_empty() || slug.chars().count() > 200 {
        return Ok(None);
    }
    let mut conn = app.pool.acquire().await?;
    let cfg = repo::load_config(&mut conn).await?;
    let langs = locales(&mut conn, &cfg).await?;
    let Some(here) = langs.iter().find(|l| l.locale == locale).cloned() else {
        return Ok(None);
    };

    let wanted = normalize_incoming(slug);
    let row = match repo::find_slug(&mut conn, entity_type, here.language_id, &wanted).await? {
        Some(r) => Some(r),
        None => repo::find_slug_any(&mut conn, entity_type, &wanted).await?,
    };
    let Some(row) = row else { return Ok(None) };

    // An artist merged into another keeps its URLs: they lead to the survivor.
    let mut id = row.entity_id;
    if repo::base(&mut conn, entity_type, id).await?.is_none() {
        let survivor = if entity_type == "artist" {
            infrastructure::entities::resolve_redirect(&mut conn, "artist", id).await?
        } else {
            None
        };
        match survivor {
            Some(s) => id = s,
            None => return Ok(None),
        }
    }
    let Some((_, default_language)) = repo::base(&mut conn, entity_type, id).await? else {
        return Ok(None);
    };
    if !repo::is_public(&mut conn, entity_type, id).await? {
        return Ok(None);
    }

    let content = repo::content_locales(&mut conn, &cfg, &langs, entity_type, id, default_language).await?;
    let current: HashMap<Uuid, String> = repo::current_slugs(&mut conn, entity_type, &[id])
        .await?
        .into_iter()
        .map(|r| (r.language_id, r.slug))
        .collect();
    let Some((canonical, alternates, x_default)) = choose(&cfg, &content, &current, Some(locale)) else {
        return Ok(None);
    };
    if id != row.entity_id || canonical.locale != locale || canonical.slug != slug {
        return Ok(Some(Resolved::Moved(canonical)));
    }

    let core = repo::core(&mut conn, entity_type, id)
        .await?
        .ok_or(AppError::NotFound { entity: "page" })?;
    let req = Requested {
        language_id: Some(here.language_id),
        script_id: None,
    };
    let et = match entity_type {
        "composition" => EntityType::Composition,
        "recording" => EntityType::Recording,
        _ => EntityType::Artist,
    };
    let title = names(&mut conn, &req, &[(et, id, core.title.clone())])
        .await?
        .get(et, id, &core.title);

    let mut credits = Vec::new();
    if entity_type != "artist" {
        let rows = repo::credits(&mut conn, entity_type, id).await?;
        let artists: Vec<(Uuid, String)> = rows.iter().map(|c| (c.artist_id, c.name.clone())).collect();
        let linked = links(&mut conn, &cfg, &langs, "artist", &artists, &req, locale).await?;
        for (row, link) in rows.into_iter().zip(linked) {
            credits.push(Credit {
                artist: link,
                role: row.role,
            });
        }
    }
    let as_pairs = |v: Vec<repo::Linked>| v.into_iter().map(|l| (l.id, l.title)).collect::<Vec<_>>();
    let (songs, recordings) = match entity_type {
        "composition" => {
            let r = as_pairs(repo::linked(&mut conn, "recordings_of_song", id).await?);
            (
                Vec::new(),
                links(&mut conn, &cfg, &langs, "recording", &r, &req, locale).await?,
            )
        }
        "artist" => {
            let s = as_pairs(repo::linked(&mut conn, "songs_of_artist", id).await?);
            let r = as_pairs(repo::linked(&mut conn, "recordings_of_artist", id).await?);
            (
                links(&mut conn, &cfg, &langs, "composition", &s, &req, locale).await?,
                links(&mut conn, &cfg, &langs, "recording", &r, &req, locale).await?,
            )
        }
        _ => (Vec::new(), Vec::new()),
    };
    let parent = match core.parent_id {
        Some(pid) => {
            let (title, _) = repo::base(&mut conn, "composition", pid).await?.unwrap_or_default();
            links(&mut conn, &cfg, &langs, "composition", &[(pid, title)], &req, locale)
                .await?
                .pop()
        }
        None => None,
    };
    let lyric_targets: Vec<Uuid> = match entity_type {
        "recording" => vec![id],
        "composition" => recordings.iter().map(|r| r.id).collect(),
        _ => Vec::new(),
    };
    let lyrics = if lyric_targets.is_empty() {
        Vec::new()
    } else {
        repo::lyrics(&mut conn, &lyric_targets)
            .await?
            .into_iter()
            .map(|l| Lyric {
                id: l.contribution_id,
                recording_id: l.recording_id,
                kind: l.kind,
                language: l.language,
                script: l.script,
                rtl: l.rtl,
                is_primary: l.is_primary,
                body: l.body,
                revised_at: l.revised_at,
            })
            .collect()
    };
    Ok(Some(Resolved::Page(Box::new(Page {
        scope: repo::scope_of(entity_type),
        id,
        locale: locale.to_owned(),
        canonical,
        alternates,
        x_default,
        title,
        year: core.year,
        code: core.code,
        kind: core.kind,
        detail: core.detail,
        duration_ms: core.duration_ms,
        parent,
        credits,
        songs,
        recordings,
        lyrics,
        updated_at: core.updated_at,
    }))))
}

// ---------- sitemap ----------

#[derive(Debug, Clone)]
pub struct SitemapEntry {
    pub scope: &'static str,
    pub id: Uuid,
    pub lastmod: DateTime<Utc>,
    pub canonical: Canonical,
    pub alternates: Vec<Canonical>,
}

#[derive(Debug, Clone)]
pub struct Sitemap {
    pub total: i64,
    pub page_size: i64,
    pub pages: i64,
    pub items: Vec<SitemapEntry>,
}

/// One page of public entities with their canonical URLs. Entities that do not have a slug yet
/// (the worker has not run) are left out rather than listed with a guessed URL.
pub async fn sitemap(app: &App, page: i64) -> Result<Sitemap, AppError> {
    if page < 0 {
        return Err(domain::DomainError::invalid("page", "out_of_range").into());
    }
    let mut conn = app.pool.acquire().await?;
    let cfg = repo::load_config(&mut conn).await?;
    let langs = locales(&mut conn, &cfg).await?;
    let total = repo::sitemap_count(&mut conn).await?;
    let rows = repo::sitemap_page(
        &mut conn,
        page.saturating_mul(cfg.sitemap_page_size),
        cfg.sitemap_page_size,
    )
    .await?;
    let mut items = Vec::new();
    for kind in ["artist", "composition", "recording"] {
        let part: Vec<_> = rows.iter().filter(|r| r.entity_type == kind).collect();
        if part.is_empty() {
            continue;
        }
        let ids: Vec<Uuid> = part.iter().map(|r| r.entity_id).collect();
        let slugs = repo::current_slugs(&mut conn, kind, &ids).await?;
        let have = repo::content_languages_batch(&mut conn, kind, &ids).await?;
        for r in part {
            let own: HashMap<Uuid, String> = slugs
                .iter()
                .filter(|s| s.entity_id == r.entity_id)
                .map(|s| (s.language_id, s.slug.clone()))
                .collect();
            let undeclared = have.iter().any(|(e, lang)| *e == r.entity_id && lang.is_none());
            let mut content: Vec<LocaleLang> = langs
                .iter()
                .filter(|l| {
                    have.iter()
                        .any(|(e, lang)| *e == r.entity_id && *lang == Some(l.language_id))
                        || (undeclared && l.locale == cfg.default_locale)
                })
                .cloned()
                .collect();
            if content.is_empty() {
                content.extend(langs.iter().find(|l| l.locale == cfg.default_locale).cloned());
            }
            if let Some((canonical, alternates, _)) = choose(&cfg, &content, &own, None) {
                items.push(SitemapEntry {
                    scope: repo::scope_of(kind),
                    id: r.entity_id,
                    lastmod: r.lastmod,
                    canonical,
                    alternates,
                });
            }
        }
    }
    Ok(Sitemap {
        total,
        page_size: cfg.sitemap_page_size,
        pages: (total + cfg.sitemap_page_size - 1) / cfg.sitemap_page_size,
        items,
    })
}
