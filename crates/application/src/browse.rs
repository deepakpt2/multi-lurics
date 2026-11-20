//! Browsing songs by metadata. Public, read-only, published content only.
//!
//! A filter in a request is a (facet, slug) pair. Slugs are resolved to catalogue entities here, so
//! a localized name, an alias or a retired slug all end at the one underlying entity, and the answer
//! always carries the *current* slug so a client can correct its URL. An unknown slug is reported as
//! unresolved and matches nothing (AND semantics: no song has a thing that does not exist).

use crate::localization::{names, resolve_requested, Display};
use crate::seo::{pick_slug, Canonical};
use crate::{images, App, AppError};
use domain::localization::EntityType;
use domain::slug::normalize_incoming;
use domain::Violations;
use infrastructure::browse::{self as repo, Facet, Filter, Kind, Value};
use infrastructure::search as search_repo;
use infrastructure::seo as seo_repo;
use sqlx::PgConnection;
use std::collections::HashMap;
use uuid::Uuid;

pub use infrastructure::browse::{Facet as FacetKind, Sort};

pub const DEFAULT_PAGE_SIZE: i64 = 20;
pub const MAX_PAGE_SIZE: i64 = 50;
const MAX_FILTERS: usize = 12;
const MAX_SLUG_CHARS: usize = 200;
const MAX_QUERY_CHARS: usize = 200;
pub const SUGGEST_DEFAULT: i64 = 10;
pub const SUGGEST_MAX: i64 = 20;
const PER_KIND: i64 = 4;
const SINGERS_SHOWN: usize = 4;
const ALBUMS_SHOWN: usize = 3;

/// What the client sent: filters as (facet, slug) in the order given.
#[derive(Debug, Clone, Default)]
pub struct Selection(pub Vec<(Facet, String)>);

impl Selection {
    fn check(&self, v: &mut Violations) {
        if self.0.len() > MAX_FILTERS {
            v.push("filters", "too_many");
        }
        if self
            .0
            .iter()
            .any(|(_, s)| s.is_empty() || s.chars().count() > MAX_SLUG_CHARS)
        {
            v.push("filters", "invalid_value");
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedFilter {
    pub facet: Facet,
    /// The entity's id and approved image, for facets that are entities.
    pub id: Option<Uuid>,
    pub image: Option<Uuid>,
    /// Basic facts: an artist's or album's kind (a code), and a short line (disambiguation, release year).
    pub kind: Option<String>,
    pub detail: Option<String>,
    /// The current slug (what the URL should say).
    pub slug: String,
    pub label: Display,
    pub resolved: bool,
}

struct Resolved {
    filters: Vec<Filter>,
    shown: Vec<ResolvedFilter>,
    all: bool,
}

#[derive(Debug, Clone)]
pub struct Ref {
    pub id: Uuid,
    pub slug: String,
    pub name: Display,
    /// The entity's approved image, if it has one.
    pub image: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct Language {
    pub code: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct Song {
    /// Where the song's public page is; `None` only while its page does not exist yet.
    pub page: Option<Canonical>,
    pub title: Display,
    pub year: Option<i16>,
    pub singers: Vec<Ref>,
    pub albums: Vec<Ref>,
    pub original_language: Option<Language>,
    pub translations: Vec<Language>,
}

#[derive(Debug, Clone)]
pub struct Paged<T> {
    pub items: Vec<T>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
    pub filters: Vec<ResolvedFilter>,
}

fn paging(v: &mut Violations, page: Option<i64>, size: Option<i64>) -> (i64, i64) {
    let page = match page {
        None => 1,
        Some(p) if (1..=100_000).contains(&p) => p,
        Some(_) => {
            v.push("page", "out_of_range");
            1
        }
    };
    let size = match size {
        None => DEFAULT_PAGE_SIZE,
        Some(n) if (1..=MAX_PAGE_SIZE).contains(&n) => n,
        Some(_) => {
            v.push("page_size", "out_of_range");
            DEFAULT_PAGE_SIZE
        }
    };
    (page, size)
}

fn clean_q(v: &mut Violations, q: Option<&str>) -> Option<String> {
    let q = q.map(str::trim).filter(|q| !q.is_empty())?;
    if q.chars().count() > MAX_QUERY_CHARS {
        v.push("q", "too_long");
    }
    Some(q.to_owned())
}

/// The locale a slug should be in for this request: the language tag's base when the site has that
/// locale, otherwise the default locale.
pub(crate) fn locale_for(cfg: &seo_repo::SeoConfig, lang: Option<&str>) -> String {
    let base = lang
        .and_then(|l| l.split(['-', '_']).next())
        .unwrap_or("")
        .to_lowercase();
    if cfg.locales.contains(&base) {
        base
    } else {
        cfg.default_locale.clone()
    }
}

/// Current browse slugs for the entities, creating any that do not exist yet.
pub(crate) async fn ensure_slugs(
    app: &App,
    entity_type: &'static str,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, String>, AppError> {
    let mut conn = app.pool.acquire().await?;
    let mut found = repo::current_slugs(&mut conn, entity_type, ids).await?;
    drop(conn);
    let missing: Vec<Uuid> = ids.iter().copied().filter(|id| !found.contains_key(id)).collect();
    if !missing.is_empty() {
        let mut tx = app.pool.begin().await?;
        for id in missing {
            if let Some(slug) = repo::sync_slug(&mut tx, entity_type, id).await? {
                found.insert(id, slug);
            }
        }
        tx.commit().await?;
    }
    Ok(found)
}

async fn resolve(
    app: &App,
    conn: &mut PgConnection,
    sel: &Selection,
    lang: Option<&str>,
) -> Result<Resolved, AppError> {
    let req = resolve_requested(conn, lang).await?;
    let mut filters = Vec::new();
    let mut pending: Vec<(Facet, String, Option<Uuid>, Option<String>)> = Vec::new();
    let mut all = true;
    for (facet, raw) in &sel.0 {
        let slug = normalize_incoming(raw);
        match facet.kind() {
            Kind::Entity(entity_type) => {
                let id = repo::resolve_slug(conn, entity_type, &slug).await?;
                if let Some(id) = id {
                    let f = Filter {
                        facet: *facet,
                        value: Value::Entity(id),
                    };
                    if !filters.contains(&f) {
                        filters.push(f);
                    }
                } else {
                    all = false;
                }
                pending.push((*facet, slug, id, None));
            }
            Kind::Text => {
                let label = repo::text_label(conn, *facet, &slug).await?;
                if label.is_some() {
                    let f = Filter {
                        facet: *facet,
                        value: Value::Text(slug.clone()),
                    };
                    if !filters.contains(&f) {
                        filters.push(f);
                    }
                } else {
                    all = false;
                }
                pending.push((*facet, slug, None, label));
            }
        }
    }
    // Current slugs and display names of the resolved entities.
    let mut current: HashMap<(&'static str, Uuid), String> = HashMap::new();
    for entity_type in ["artist", "album", "raga", "deity", "tag"] {
        let ids: Vec<Uuid> = pending
            .iter()
            .filter(|(f, ..)| f.kind() == Kind::Entity(entity_type))
            .filter_map(|(_, _, id, _)| *id)
            .collect();
        if !ids.is_empty() {
            for (id, slug) in ensure_slugs(app, entity_type, &ids).await? {
                current.insert((entity_type, id), slug);
            }
        }
    }
    let mut refs = Vec::new();
    for (facet, _, id, _) in &pending {
        if let (Some(id), Kind::Entity(t)) = (id, facet.kind()) {
            let base = entity_name(conn, t, *id).await?;
            refs.push((entity_of(t), *id, base));
        }
    }
    let shown_names = names(conn, &req, &refs).await?;
    let mut pictures: HashMap<(&'static str, Uuid), Uuid> = HashMap::new();
    type Facts = (Option<String>, Option<String>);
    let mut facts: HashMap<(&'static str, Uuid), Facts> = HashMap::new();
    for entity_type in ["artist", "album", "raga", "deity", "tag"] {
        let ids: Vec<Uuid> = pending
            .iter()
            .filter(|(f, ..)| f.kind() == Kind::Entity(entity_type))
            .filter_map(|(_, _, id, _)| *id)
            .collect();
        if ids.is_empty() {
            continue;
        }
        for (id, image) in images::current_for(conn, entity_type, &ids).await? {
            pictures.insert((entity_type, id), image);
        }
        for id in ids {
            let row: Option<(Option<String>, Option<String>)> = match entity_type {
                "artist" => {
                    sqlx::query_as("SELECT kind, disambiguation FROM artists WHERE id = $1")
                        .bind(id)
                        .fetch_optional(&mut *conn)
                        .await?
                }
                "album" => {
                    sqlx::query_as("SELECT kind, release_year::text FROM albums WHERE id = $1")
                        .bind(id)
                        .fetch_optional(&mut *conn)
                        .await?
                }
                _ => None,
            };
            if let Some(r) = row {
                facts.insert((entity_type, id), r);
            }
        }
    }
    let shown = pending
        .into_iter()
        .map(|(facet, slug, id, label)| match (facet.kind(), id, label) {
            (Kind::Entity(t), Some(id), _) => {
                let base = refs.iter().find(|r| r.1 == id).map(|r| r.2.clone()).unwrap_or_default();
                let (kind, detail) = facts.get(&(t, id)).cloned().unwrap_or_default();
                ResolvedFilter {
                    facet,
                    slug: current.get(&(t, id)).cloned().unwrap_or(slug),
                    label: shown_names.get(entity_of(t), id, &base),
                    resolved: true,
                    id: Some(id),
                    image: pictures.get(&(t, id)).copied(),
                    kind,
                    detail,
                }
            }
            (Kind::Text, _, Some(label)) => ResolvedFilter {
                facet,
                slug,
                label: Display::base(&label),
                resolved: true,
                id: None,
                image: None,
                kind: None,
                detail: None,
            },
            (_, _, _) => ResolvedFilter {
                facet,
                label: Display::base(&slug),
                slug,
                resolved: false,
                id: None,
                image: None,
                kind: None,
                detail: None,
            },
        })
        .collect();
    Ok(Resolved { filters, shown, all })
}

fn entity_of(entity_type: &str) -> EntityType {
    match entity_type {
        "album" => EntityType::Album,
        "raga" => EntityType::Raga,
        "deity" => EntityType::Deity,
        "tag" => EntityType::Tag,
        _ => EntityType::Artist,
    }
}

async fn entity_name(conn: &mut PgConnection, entity_type: &str, id: Uuid) -> Result<String, AppError> {
    let sql = match entity_type {
        "album" => "SELECT title FROM albums WHERE id = $1",
        "raga" => "SELECT name FROM ragas WHERE id = $1",
        "deity" => "SELECT name FROM deities WHERE id = $1",
        "tag" => "SELECT name FROM tags WHERE id = $1",
        _ => "SELECT name FROM artists WHERE id = $1",
    };
    Ok(sqlx::query_scalar(sql)
        .bind(id)
        .fetch_optional(conn)
        .await?
        .unwrap_or_default())
}

/// Song cards (titles, singers, albums, languages, public page) for already-chosen songs, in order.
pub(crate) async fn cards(app: &App, rows: Vec<repo::SongRow>, lang: Option<&str>) -> Result<Vec<Song>, AppError> {
    let mut conn = app.pool.acquire().await?;
    let ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let req = resolve_requested(&mut conn, lang).await?;
    let cfg = seo_repo::load_config(&mut conn).await?;
    let langs = seo_repo::locale_languages(&mut conn, &cfg).await?;
    let locale = locale_for(&cfg, lang);

    // Public pages: any song missing its slug gets one now (the worker normally has).
    let mut slugs = seo_repo::current_slugs(&mut conn, "composition", &ids).await?;
    let missing: Vec<Uuid> = ids
        .iter()
        .copied()
        .filter(|id| !slugs.iter().any(|s| s.entity_id == *id))
        .collect();
    if !missing.is_empty() {
        let mut tx = app.pool.begin().await?;
        for id in missing {
            seo_repo::sync(&mut tx, "composition", id).await?;
        }
        tx.commit().await?;
        slugs = seo_repo::current_slugs(&mut conn, "composition", &ids).await?;
    }

    let singers = repo::singers(&mut conn, &ids).await?;
    let albums = repo::albums(&mut conn, &ids).await?;
    let languages = repo::lyric_languages(&mut conn, &ids).await?;
    drop(conn);
    let singer_slugs = ensure_slugs(app, "artist", &singers.iter().map(|s| s.id).collect::<Vec<_>>()).await?;
    let album_slugs = ensure_slugs(app, "album", &albums.iter().map(|a| a.id).collect::<Vec<_>>()).await?;

    let mut conn = app.pool.acquire().await?;
    let singer_images =
        images::current_for(&mut conn, "artist", &singers.iter().map(|s| s.id).collect::<Vec<_>>()).await?;
    let album_images =
        images::current_for(&mut conn, "album", &albums.iter().map(|a| a.id).collect::<Vec<_>>()).await?;
    let mut refs: Vec<(EntityType, Uuid, String)> = rows
        .iter()
        .map(|r| (EntityType::Composition, r.id, r.title.clone()))
        .collect();
    refs.extend(singers.iter().map(|s| (EntityType::Artist, s.id, s.name.clone())));
    refs.extend(albums.iter().map(|a| (EntityType::Album, a.id, a.title.clone())));
    let shown = names(&mut conn, &req, &refs).await?;

    Ok(rows
        .into_iter()
        .map(|r| {
            let lang_of = |kind: &'static str| languages.iter().filter(move |l| l.song == r.id && l.kind == kind);
            let to_lang = |l: &repo::LyricLanguage| Language {
                code: l.code.clone(),
                name: l.name.clone(),
            };
            let original = lang_of("original").next().map(to_lang);
            let mut translations: Vec<Language> = Vec::new();
            for l in lang_of("translation").map(to_lang) {
                if original.as_ref().map(|o| &o.code) != Some(&l.code) && !translations.iter().any(|t| t.code == l.code)
                {
                    translations.push(l);
                }
            }
            Song {
                page: pick_slug(&slugs, r.id, &langs, &locale, &cfg),
                title: shown.get(EntityType::Composition, r.id, &r.title),
                year: r.year,
                singers: singers
                    .iter()
                    .filter(|s| s.song == r.id)
                    .filter_map(|s| {
                        Some(Ref {
                            id: s.id,
                            slug: singer_slugs.get(&s.id)?.clone(),
                            name: shown.get(EntityType::Artist, s.id, &s.name),
                            image: singer_images.get(&s.id).copied(),
                        })
                    })
                    .take(SINGERS_SHOWN)
                    .collect(),
                albums: albums
                    .iter()
                    .filter(|a| a.song == r.id)
                    .filter_map(|a| {
                        Some(Ref {
                            id: a.id,
                            slug: album_slugs.get(&a.id)?.clone(),
                            name: shown.get(EntityType::Album, a.id, &a.title),
                            image: album_images.get(&a.id).copied(),
                        })
                    })
                    .take(ALBUMS_SHOWN)
                    .collect(),
                original_language: original,
                translations,
            }
        })
        .collect())
}

/// `GET /browse/songs`
pub async fn songs(
    app: &App,
    sel: &Selection,
    page: Option<i64>,
    page_size: Option<i64>,
    sort: Option<&str>,
    lang: Option<&str>,
) -> Result<Paged<Song>, AppError> {
    let mut v = Violations::default();
    sel.check(&mut v);
    let (page, size) = paging(&mut v, page, page_size);
    let sort = match sort {
        None | Some("newest") => Sort::Newest,
        Some("title") => Sort::Title,
        Some("album") => Sort::Album,
        Some(_) => {
            v.push("sort", "invalid_value");
            Sort::Newest
        }
    };
    v.finish()?;
    let mut conn = app.pool.acquire().await?;
    let resolved = resolve(app, &mut conn, sel, lang).await?;
    if !resolved.all {
        return Ok(Paged {
            items: Vec::new(),
            page,
            page_size: size,
            total: 0,
            filters: resolved.shown,
        });
    }
    let offset = (page - 1) * size;
    let rows = repo::list_songs(&mut conn, &resolved.filters, sort, size, offset).await?;
    let total = match rows.first() {
        Some(r) => r.total,
        None if offset > 0 => repo::count_songs(&mut conn, &resolved.filters).await?,
        None => 0,
    };
    drop(conn);
    let items = cards(app, rows, lang).await?;
    Ok(Paged {
        items,
        page,
        page_size: size,
        total,
        filters: resolved.shown,
    })
}

#[derive(Debug, Clone)]
pub struct Value_ {
    /// The entity's id, for entity facets (needed to attach an image); `None` for plain values.
    pub id: Option<Uuid>,
    pub slug: String,
    pub label: Display,
    pub songs: i64,
    pub image: Option<Uuid>,
}

/// `GET /browse/{facet}`: the values of one kind of metadata (among songs matching the filters), paged.
pub async fn values(
    app: &App,
    facet: Facet,
    q: Option<&str>,
    sel: &Selection,
    page: Option<i64>,
    page_size: Option<i64>,
    lang: Option<&str>,
) -> Result<Paged<Value_>, AppError> {
    let mut v = Violations::default();
    sel.check(&mut v);
    let (page, size) = paging(&mut v, page, page_size);
    let q = clean_q(&mut v, q);
    v.finish()?;
    let mut conn = app.pool.acquire().await?;
    let resolved = resolve(app, &mut conn, sel, lang).await?;
    if !resolved.all {
        return Ok(Paged {
            items: Vec::new(),
            page,
            page_size: size,
            total: 0,
            filters: resolved.shown,
        });
    }
    let offset = (page - 1) * size;
    let (items, total) = match facet.kind() {
        Kind::Entity(entity_type) => {
            let rows = repo::list_entities(&mut conn, facet, q.as_deref(), &resolved.filters, size, offset).await?;
            let total = rows.first().map_or(0, |r| r.total);
            let req = resolve_requested(&mut conn, lang).await?;
            let refs: Vec<_> = rows
                .iter()
                .map(|r| (entity_of(entity_type), r.id, r.name.clone()))
                .collect();
            let shown = names(&mut conn, &req, &refs).await?;
            let pictures =
                images::current_for(&mut conn, entity_type, &rows.iter().map(|r| r.id).collect::<Vec<_>>()).await?;
            drop(conn);
            let slugs = ensure_slugs(app, entity_type, &rows.iter().map(|r| r.id).collect::<Vec<_>>()).await?;
            let items = rows
                .into_iter()
                .filter_map(|r| {
                    Some(Value_ {
                        id: Some(r.id),
                        slug: slugs.get(&r.id)?.clone(),
                        label: shown.get(entity_of(entity_type), r.id, &r.name),
                        songs: r.songs,
                        image: pictures.get(&r.id).copied(),
                    })
                })
                .collect();
            (items, total)
        }
        Kind::Text => {
            let rows = repo::list_values(&mut conn, facet, q.as_deref(), &resolved.filters, size, offset).await?;
            let total = rows.first().map_or(0, |r| r.total);
            (
                rows.into_iter()
                    .map(|r| Value_ {
                        id: None,
                        slug: r.value,
                        label: Display::base(&r.label),
                        songs: r.songs,
                        image: None,
                    })
                    .collect(),
                total,
            )
        }
    };
    Ok(Paged {
        items,
        page,
        page_size: size,
        total,
        filters: resolved.shown,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    Name,
    Localization,
    Transliteration,
    Alias,
}

#[derive(Debug, Clone)]
pub struct Suggestion {
    /// `song`, or a facet key.
    pub kind: &'static str,
    /// A song's page slug, or the facet value's slug.
    pub slug: String,
    /// The locale of a song's page slug.
    pub locale: Option<String>,
    pub label: Display,
    /// The name that matched, as written, and how it relates to the label.
    pub matched: String,
    pub via: Via,
    score: f64,
}

/// `GET /browse/suggest`: typed suggestions for what the visitor is typing, limited to what exists
/// together with the filters already chosen. Songs, albums, singers, lyricists, composers, ragas,
/// deities, languages, religions and categories.
pub async fn suggest(
    app: &App,
    q: &str,
    sel: &Selection,
    lang: Option<&str>,
    limit: Option<i64>,
) -> Result<(Vec<Suggestion>, u32), AppError> {
    let mut v = Violations::default();
    sel.check(&mut v);
    let q = q.trim().to_owned();
    if q.chars().count() > MAX_QUERY_CHARS {
        v.push("q", "too_long");
    }
    let settings = app.search.settings().await?;
    if q.is_empty() {
        v.push("q", "required");
    } else if q.chars().count() < settings.min_suggest_chars {
        v.push("q", "too_short");
    }
    let limit = match limit {
        None => SUGGEST_DEFAULT,
        Some(n) if (1..=SUGGEST_MAX).contains(&n) => n,
        Some(_) => {
            v.push("limit", "out_of_range");
            SUGGEST_DEFAULT
        }
    };
    v.finish()?;
    let max_age = settings.cache_max_age_secs;
    let mut conn = app.pool.acquire().await?;
    let resolved = resolve(app, &mut conn, sel, lang).await?;
    if !resolved.all {
        return Ok((Vec::new(), max_age));
    }
    let ranking = search_repo::load_ranking(&mut conn).await?;
    let req = resolve_requested(&mut conn, lang).await?;
    let cfg = seo_repo::load_config(&mut conn).await?;
    let langs = seo_repo::locale_languages(&mut conn, &cfg).await?;
    let locale = locale_for(&cfg, lang);
    drop(conn);

    // Independent lookups, each on its own connection: they run side by side.
    let fetch = |doc: &'static str| {
        let pool = app.pool.clone();
        let ranking = ranking.clone();
        let filters = resolved.filters.clone();
        let q = q.clone();
        async move {
            let mut c = pool.acquire().await?;
            Ok::<_, AppError>(repo::suggest_names(&mut c, &ranking, doc, &q, &filters, PER_KIND * 2).await?)
        }
    };
    let values = {
        let pool = app.pool.clone();
        let filters = resolved.filters.clone();
        let q = q.clone();
        async move {
            let mut c = pool.acquire().await?;
            Ok::<_, AppError>(repo::suggest_values(&mut c, &q, &filters, PER_KIND).await?)
        }
    };
    let (songs, albums, artists, ragas, deities, tags, values) = tokio::try_join!(
        fetch("composition"),
        fetch("album"),
        fetch("artist"),
        fetch("raga"),
        fetch("deity"),
        fetch("tag"),
        values
    )?;

    let mut conn = app.pool.acquire().await?;
    let mut refs: Vec<(EntityType, Uuid, String)> = Vec::new();
    let mut bases: HashMap<Uuid, String> = HashMap::new();
    for (t, hits) in [
        (EntityType::Composition, &songs),
        (EntityType::Album, &albums),
        (EntityType::Artist, &artists),
        (EntityType::Raga, &ragas),
        (EntityType::Deity, &deities),
        (EntityType::Tag, &tags),
    ] {
        let table = match t {
            EntityType::Composition => "SELECT id, title FROM compositions WHERE id = ANY($1)",
            EntityType::Album => "SELECT id, title FROM albums WHERE id = ANY($1)",
            EntityType::Raga => "SELECT id, name FROM ragas WHERE id = ANY($1)",
            EntityType::Deity => "SELECT id, name FROM deities WHERE id = ANY($1)",
            EntityType::Tag => "SELECT id, name FROM tags WHERE id = ANY($1)",
            _ => "SELECT id, name FROM artists WHERE id = ANY($1)",
        };
        let ids: Vec<Uuid> = hits.iter().map(|h| h.entity_id).collect();
        let rows: Vec<(Uuid, String)> = sqlx::query_as(table).bind(&ids).fetch_all(&mut *conn).await?;
        for (id, base) in rows {
            refs.push((t, id, base.clone()));
            bases.insert(id, base);
        }
    }
    let shown = names(&mut conn, &req, &refs).await?;
    let mut song_slugs = seo_repo::current_slugs(
        &mut conn,
        "composition",
        &songs.iter().map(|h| h.entity_id).collect::<Vec<_>>(),
    )
    .await?;
    drop(conn);
    let missing: Vec<Uuid> = songs
        .iter()
        .map(|h| h.entity_id)
        .filter(|id| !song_slugs.iter().any(|s| s.entity_id == *id))
        .collect();
    if !missing.is_empty() {
        let mut tx = app.pool.begin().await?;
        for id in missing.iter().copied() {
            seo_repo::sync(&mut tx, "composition", id).await?;
        }
        tx.commit().await?;
        let mut conn = app.pool.acquire().await?;
        song_slugs = seo_repo::current_slugs(
            &mut conn,
            "composition",
            &songs.iter().map(|h| h.entity_id).collect::<Vec<_>>(),
        )
        .await?;
    }
    let album_slugs = ensure_slugs(app, "album", &albums.iter().map(|h| h.entity_id).collect::<Vec<_>>()).await?;
    let artist_slugs = ensure_slugs(app, "artist", &artists.iter().map(|h| h.entity_id).collect::<Vec<_>>()).await?;

    let via = |o: &str| match o {
        "localization" => Via::Localization,
        "transliteration" => Via::Transliteration,
        "alias" => Via::Alias,
        _ => Via::Name,
    };
    let taken = |facet: Facet, slug: &str| resolved.shown.iter().any(|f| f.facet == facet && f.slug == slug);
    let mut out: Vec<Suggestion> = Vec::new();
    for h in &songs {
        let base = bases.get(&h.entity_id).cloned().unwrap_or_default();
        // A song only appears while its page exists: the suggestion opens it.
        if let Some(page) = pick_slug(&song_slugs, h.entity_id, &langs, &locale, &cfg) {
            out.push(Suggestion {
                kind: "song",
                slug: page.slug,
                locale: Some(page.locale),
                label: shown.get(EntityType::Composition, h.entity_id, &base),
                matched: h.display.clone(),
                via: via(&h.origin),
                score: h.score,
            });
        }
    }
    for h in &albums {
        let (Some(slug), base) = (
            album_slugs.get(&h.entity_id),
            bases.get(&h.entity_id).cloned().unwrap_or_default(),
        ) else {
            continue;
        };
        if !taken(Facet::Album, slug) {
            out.push(Suggestion {
                kind: "album",
                slug: slug.clone(),
                locale: None,
                label: shown.get(EntityType::Album, h.entity_id, &base),
                matched: h.display.clone(),
                via: via(&h.origin),
                score: h.score,
            });
        }
    }
    for (hits, facet, kind, et, slug_kind) in [
        (&ragas, Facet::Raga, "raga", EntityType::Raga, "raga"),
        (&deities, Facet::Deity, "deity", EntityType::Deity, "deity"),
        (&tags, Facet::Tag, "tag", EntityType::Tag, "tag"),
    ] {
        let slugs = ensure_slugs(app, slug_kind, &hits.iter().map(|h| h.entity_id).collect::<Vec<_>>()).await?;
        for h in hits {
            let (Some(slug), base) = (
                slugs.get(&h.entity_id),
                bases.get(&h.entity_id).cloned().unwrap_or_default(),
            ) else {
                continue;
            };
            if !taken(facet, slug) {
                out.push(Suggestion {
                    kind,
                    slug: slug.clone(),
                    locale: None,
                    label: shown.get(et, h.entity_id, &base),
                    matched: h.display.clone(),
                    via: via(&h.origin),
                    score: h.score,
                });
            }
        }
    }
    for h in &artists {
        let (Some(slug), base) = (
            artist_slugs.get(&h.entity_id),
            bases.get(&h.entity_id).cloned().unwrap_or_default(),
        ) else {
            continue;
        };
        for (facet, kind, on) in [
            (Facet::Singer, "singer", h.singer),
            (Facet::Lyricist, "lyricist", h.lyricist),
            (Facet::Composer, "composer", h.composer),
        ] {
            if on && !taken(facet, slug) {
                out.push(Suggestion {
                    kind,
                    slug: slug.clone(),
                    locale: None,
                    label: shown.get(EntityType::Artist, h.entity_id, &base),
                    matched: h.display.clone(),
                    via: via(&h.origin),
                    score: h.score,
                });
            }
        }
    }
    for h in values {
        let Some(facet) = Facet::parse(&h.facet) else { continue };
        if !taken(facet, &h.value) {
            out.push(Suggestion {
                kind: facet.as_str(),
                slug: h.value,
                locale: None,
                label: Display::base(&h.label),
                matched: h.label,
                via: Via::Name,
                // Plain values rank just under a good name match, by how many songs carry them.
                score: 60.0 + (h.songs as f64).ln_1p(),
            });
        }
    }
    // Best first, but no one kind may crowd out the rest.
    out.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.label.text.cmp(&b.label.text))
    });
    let mut count: HashMap<&'static str, i64> = HashMap::new();
    out.retain(|s| {
        let n = count.entry(s.kind).or_default();
        *n += 1;
        *n <= PER_KIND
    });
    out.truncate(limit as usize);
    Ok((out, max_age))
}
