//! The home page's lists. They are computed by the worker about once an hour (see
//! `infrastructure::home` for the formula) and stored, so every visitor sees the same fixed lists
//! between runs. On a fresh install, before the worker's first run, the first request computes them once.

use crate::authz;
use crate::browse::{cards, ensure_slugs, locale_for, Ref};
use crate::localization::{names, resolve_requested, Display};
use crate::seo::{pick_slug, Canonical};
use crate::{images, App, AppError};
use chrono::{DateTime, Utc};
use domain::localization::EntityType;
use domain::{Principal, Role};
use infrastructure::browse::SongRow;
use infrastructure::home as repo;
use infrastructure::seo as seo_repo;
use sha2::{Digest, Sha256};
use sqlx::PgConnection;
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Item {
    pub rank: i16,
    pub id: Uuid,
    pub title: Display,
    /// Public page (songs and artists). Albums have none; use `slug` with the browse page.
    pub page: Option<Canonical>,
    /// Browse slug (albums and artists): `/browse?album=<slug>`.
    pub slug: Option<String>,
    pub image: Option<Uuid>,
    /// Singers and albums of a song: each links to the browse page for that value.
    pub singers: Vec<Ref>,
    pub albums: Vec<Ref>,
    pub views: i64,
    pub rating_avg: Option<f64>,
    pub rating_count: i32,
    pub comments: i32,
}

/// Sections in their default order; `editors_picks` is curated, the rest come from the worker's run.
pub const SECTIONS: [&str; 6] = [
    "editors_picks",
    "popular_songs",
    "new_songs",
    "popular_albums",
    "new_albums",
    "popular_artists",
];

#[derive(Debug, Clone)]
pub struct Section {
    pub key: &'static str,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone)]
pub struct Home {
    pub generated_at: Option<DateTime<Utc>>,
    pub sections: Vec<Section>,
}

/// Rebuilds the lists and clears expired view-dedupe rows. The worker calls this on a timer.
pub async fn refresh(app: &App) -> Result<DateTime<Utc>, AppError> {
    let mut tx = app.pool.begin().await?;
    let at = repo::refresh(&mut tx).await?;
    repo::purge_seen(&mut tx).await?;
    tx.commit().await?;
    Ok(at)
}

pub async fn home(app: &App, lang: Option<&str>) -> Result<Home, AppError> {
    let mut conn = app.pool.acquire().await?;
    let mut run = repo::last_run(&mut conn).await?;
    drop(conn);
    if run.is_none() {
        run = Some(refresh(app).await?);
    }
    let mut conn = app.pool.acquire().await?;
    let rows = repo::read(&mut conn).await?;
    let req = resolve_requested(&mut conn, lang).await?;

    let ids_of = |list: &str| -> Vec<Uuid> { rows.iter().filter(|r| r.list == list).map(|r| r.entity_id).collect() };
    let picks = repo::picks(&mut conn).await?;
    let song_ids: Vec<Uuid> = ids_of("popular_songs")
        .into_iter()
        .chain(ids_of("new_songs"))
        .chain(picks.iter().copied())
        .collect();
    let album_ids: Vec<Uuid> = ids_of("popular_albums")
        .into_iter()
        .chain(ids_of("new_albums"))
        .collect();
    let artist_ids = ids_of("popular_artists");

    // Songs: the same cards the browse page shows (title, singers, public page).
    let song_rows: Vec<SongRow> =
        sqlx::query_as("SELECT id, title, year, 0::bigint AS total FROM compositions WHERE id = ANY($1)")
            .bind(&song_ids)
            .fetch_all(&mut *conn)
            .await?;
    drop(conn);
    let song_cards = cards(app, song_rows.clone(), lang).await?;
    let cards_by_id: HashMap<Uuid, _> = song_rows.iter().map(|r| r.id).zip(song_cards).collect();

    let mut conn = app.pool.acquire().await?;
    let album_names = plain_names(&mut conn, "albums", "title", &album_ids).await?;
    let artist_names = plain_names(&mut conn, "artists", "name", &artist_ids).await?;
    let mut refs: Vec<(EntityType, Uuid, String)> = Vec::new();
    refs.extend(album_names.iter().map(|(id, n)| (EntityType::Album, *id, n.clone())));
    refs.extend(artist_names.iter().map(|(id, n)| (EntityType::Artist, *id, n.clone())));
    let shown = names(&mut conn, &req, &refs).await?;
    let album_images = images::current_for(&mut conn, "album", &album_ids).await?;
    let artist_images = images::current_for(&mut conn, "artist", &artist_ids).await?;
    let cfg = seo_repo::load_config(&mut conn).await?;
    let langs = seo_repo::locale_languages(&mut conn, &cfg).await?;
    let locale = locale_for(&cfg, lang);
    let artist_pages = seo_repo::current_slugs(&mut conn, "artist", &artist_ids).await?;
    drop(conn);
    let album_slugs = ensure_slugs(app, "album", &album_ids).await?;
    let artist_slugs = ensure_slugs(app, "artist", &artist_ids).await?;

    let mut all: Vec<repo::ListRow> = rows;
    all.extend(picks.iter().enumerate().map(|(i, id)| repo::ListRow {
        list: "editors_picks".into(),
        rank: i as i16 + 1,
        entity_id: *id,
        views: 0,
        rating_avg: None,
        rating_count: 0,
        comments: 0,
    }));
    let rows = all;
    let sections = SECTIONS
        .iter()
        .map(|&key| {
            let items = rows
                .iter()
                .filter(|r| r.list == key)
                .filter_map(|r| {
                    let base = |title: Display| Item {
                        rank: r.rank,
                        id: r.entity_id,
                        title,
                        page: None,
                        slug: None,
                        image: None,
                        singers: Vec::new(),
                        albums: Vec::new(),
                        views: r.views,
                        rating_avg: r.rating_avg,
                        rating_count: r.rating_count,
                        comments: r.comments,
                    };
                    Some(match key {
                        "popular_songs" | "new_songs" | "editors_picks" => {
                            let c = cards_by_id.get(&r.entity_id)?;
                            Item {
                                page: c.page.clone(),
                                singers: c.singers.clone(),
                                albums: c.albums.clone(),
                                ..base(c.title.clone())
                            }
                        }
                        "popular_albums" | "new_albums" => {
                            let name = album_names.get(&r.entity_id)?;
                            Item {
                                slug: album_slugs.get(&r.entity_id).cloned(),
                                image: album_images.get(&r.entity_id).copied(),
                                ..base(shown.get(EntityType::Album, r.entity_id, name))
                            }
                        }
                        _ => {
                            let name = artist_names.get(&r.entity_id)?;
                            Item {
                                page: pick_slug(&artist_pages, r.entity_id, &langs, &locale, &cfg),
                                slug: artist_slugs.get(&r.entity_id).cloned(),
                                image: artist_images.get(&r.entity_id).copied(),
                                ..base(shown.get(EntityType::Artist, r.entity_id, name))
                            }
                        }
                    })
                })
                .collect();
            Section { key, items }
        })
        .collect();
    Ok(Home {
        generated_at: run,
        sections,
    })
}

async fn plain_names(
    conn: &mut PgConnection,
    table: &str,
    col: &str,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, String>, AppError> {
    let rows: Vec<(Uuid, String)> = sqlx::query_as(&format!("SELECT id, {col} FROM {table} WHERE id = ANY($1)"))
        .bind(ids)
        .fetch_all(conn)
        .await?;
    Ok(rows.into_iter().collect())
}

/// Counts a page view (once per viewer, target and day) and returns the target's total views.
pub async fn record_view(app: &App, viewer: &str, kind: &str, id: Uuid) -> Result<i64, AppError> {
    let day = Utc::now().date_naive();
    let hash = format!("{:x}", Sha256::digest(format!("{day}|{viewer}|{kind}|{id}")));
    let mut conn = app.pool.acquire().await?;
    repo::record_view(&mut conn, kind, id, &hash)
        .await?
        .ok_or(AppError::NotFound { entity: "page" })
}

/// Moderators feature a song on the home page. Idempotent.
pub async fn pick(app: &App, p: &Principal, id: Uuid) -> Result<(), AppError> {
    authz::require_role(p, Role::Moderator)?;
    let mut conn = app.pool.acquire().await?;
    if repo::add_pick(&mut conn, id, p.user_id).await? {
        Ok(())
    } else {
        Err(AppError::NotFound { entity: "song" })
    }
}

pub async fn unpick(app: &App, p: &Principal, id: Uuid) -> Result<(), AppError> {
    authz::require_role(p, Role::Moderator)?;
    let mut conn = app.pool.acquire().await?;
    repo::remove_pick(&mut conn, id).await?;
    Ok(())
}

pub async fn is_picked(app: &App, id: Uuid) -> Result<bool, AppError> {
    let mut conn = app.pool.acquire().await?;
    Ok(repo::is_picked(&mut conn, id).await?)
}
