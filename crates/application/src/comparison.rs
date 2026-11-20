//! Side-by-side comparison of up to four published contributions.
//!
//! A comparison is a list of contribution ids and nothing else: the URL and the saved record
//! hold ids, never lyric text. Content is resolved on every read from the *current published*
//! revision, so a new publish shows up at once and an unpublished one disappears at once.
//! Each panel is its own contribution, so an alternative in a slot is never mistaken for the
//! slot's primary.

use crate::catalogue::record;
use crate::community::{self, throttle};
use crate::pagination::{self, Cursor, Page};
use crate::{App, AppError, RequestMeta};
use domain::community::{self as dc, MAX_PANELS};
use domain::Principal;
use infrastructure::community::{self as repo, ResolvedRow, SavedRow};
use infrastructure::AppTx;
use serde_json::json;
use uuid::Uuid;

/// One requested panel. `content` is `None` when the contribution is not public (unknown,
/// pending, rejected or withdrawn): the caller cannot tell which, by design.
#[derive(Debug, Clone)]
pub struct Panel {
    pub id: Uuid,
    pub content: Option<ResolvedRow>,
}

#[derive(Debug, Clone)]
pub struct Saved {
    pub row: SavedRow,
    pub panels: Vec<Panel>,
}

pub fn parse_ids(raw: &str) -> Result<Vec<Uuid>, AppError> {
    let parts: Vec<&str> = raw.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
    if parts.is_empty() {
        return Err(domain::DomainError::invalid("ids", "required").into());
    }
    if parts.len() > MAX_PANELS {
        return Err(domain::DomainError::invalid("ids", "too_many").into());
    }
    parts
        .iter()
        .map(|s| Uuid::parse_str(s).map_err(|_| domain::DomainError::invalid("ids", "invalid_id").into()))
        .collect()
}

async fn panels(conn: &mut sqlx::PgConnection, ids: &[Uuid]) -> Result<Vec<Panel>, AppError> {
    let rows = repo::resolve_published(conn, ids).await?;
    Ok(ids
        .iter()
        .map(|id| Panel {
            id: *id,
            content: rows.iter().find(|r| r.id == *id).cloned(),
        })
        .collect())
}

pub async fn resolve(app: &App, raw_ids: &str) -> Result<Vec<Panel>, AppError> {
    let ids = parse_ids(raw_ids)?;
    let mut conn = app.pool.acquire().await?;
    panels(&mut conn, &ids).await
}

pub async fn save(app: &App, p: &Principal, meta: &RequestMeta, name: &str, ids: &[Uuid]) -> Result<Saved, AppError> {
    let mut v = domain::Violations::default();
    let name = v.check(dc::text("name", name, 100));
    if ids.is_empty() {
        v.push("contribution_ids", "required");
    } else if ids.len() > MAX_PANELS {
        v.push("contribution_ids", "too_many");
    }
    v.finish()?;
    let name = name.unwrap_or_default();
    let mut tx = AppTx::begin(&app.pool).await?;
    let settings = community::load_settings(&mut tx).await?;
    throttle(&mut tx, p, "comparison", &settings).await?;
    // Only what the public can see may be saved: a save cannot be used to probe pending work.
    let public = repo::published_ids(&mut tx, ids).await?;
    if ids.iter().any(|i| !public.contains(i)) {
        return Err(domain::DomainError::invalid("contribution_ids", "not_published").into());
    }
    let row = repo::insert_saved(&mut tx, p.user_id, &name, ids).await?;
    record(
        &mut tx,
        p,
        meta,
        "comparison.saved",
        "comparison",
        row.id,
        None,
        Some(json!({ "contribution_ids": ids })),
    )
    .await?;
    let panels = panels(&mut tx, ids).await?;
    tx.commit().await?;
    Ok(Saved { row, panels })
}

pub async fn list_saved(
    app: &App,
    p: &Principal,
    cursor: Option<&str>,
    limit: Option<i64>,
) -> Result<Page<SavedRow>, AppError> {
    let limit = pagination::limit(limit)?;
    let after = cursor.map(Cursor::decode).transpose()?.map(|c| (c.ts, c.id));
    let mut conn = app.pool.acquire().await?;
    let rows = repo::list_saved(&mut conn, p.user_id, after, limit + 1).await?;
    Ok(Page::from_overfetch(rows, limit, |r| Cursor {
        ts: r.created_at,
        id: r.id,
    }))
}

pub async fn get_saved(app: &App, p: &Principal, id: Uuid) -> Result<Saved, AppError> {
    let mut conn = app.pool.acquire().await?;
    let row = repo::get_saved(&mut conn, p.user_id, id)
        .await?
        .ok_or(AppError::NotFound { entity: "comparison" })?;
    let panels = panels(&mut conn, &row.contribution_ids).await?;
    Ok(Saved { row, panels })
}

pub async fn delete_saved(app: &App, p: &Principal, meta: &RequestMeta, id: Uuid) -> Result<(), AppError> {
    let mut tx = AppTx::begin(&app.pool).await?;
    if !repo::delete_saved(&mut tx, p.user_id, id).await? {
        return Err(AppError::NotFound { entity: "comparison" });
    }
    record(&mut tx, p, meta, "comparison.deleted", "comparison", id, None, None).await?;
    tx.commit().await?;
    Ok(())
}
