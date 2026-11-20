//! Articles: blog-style pages (celebrations, special occasions) written by moderators as Markdown
//! elsewhere and pasted in. Anyone reads published ones; moderators write, publish and choose which
//! are shown on the home page, and for how long.

use crate::catalogue::{record, snapshot};
use crate::media::{self, MediaError};
use crate::{authz, App, AppError, RequestMeta};
use chrono::{DateTime, Utc};
use domain::{DomainError, Principal, Role};
use infrastructure::articles::{self as repo, ArticleImageRow, ArticleRow, NewArticle};
use infrastructure::AppTx;
use sqlx::PgConnection;
use uuid::Uuid;

pub const MAX_BODY: usize = 200_000;
pub const HOME_LIMIT: i64 = 3;
const LIST_LIMIT: i64 = 100;
const MAX_IMAGES: i64 = 60;
/// Widths of article pictures (kept in proportion).
const WIDTHS: [u32; 3] = [480, 960, 1600];

fn invalid(field: &'static str, code: &'static str) -> AppError {
    DomainError::invalid(field, code).into()
}

#[derive(Default)]
pub struct Create {
    pub title: String,
    pub slug: Option<String>,
    pub summary: Option<String>,
    pub body_md: String,
    pub publish: bool,
}

/// Fields left as `None` are unchanged. `summary` and the home dates use a double option so they can be cleared.
#[derive(Default)]
pub struct Update {
    pub expected_version: i32,
    pub title: Option<String>,
    pub slug: Option<String>,
    pub summary: Option<Option<String>>,
    pub body_md: Option<String>,
    pub cover_image_id: Option<Option<Uuid>>,
    pub publish: Option<bool>,
}

pub struct HomeSetting {
    pub on_home: bool,
    pub from: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
}

fn slugify(title: &str) -> String {
    let mut out = String::new();
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_matches('-');
    out.chars().take(60).collect::<String>().trim_matches('-').to_owned()
}

fn check_slug(s: &str) -> bool {
    // `new` and `home` are pages of their own.
    !matches!(s, "new" | "home")
        && !s.is_empty()
        && s.len() <= 80
        && !s.starts_with('-')
        && !s.ends_with('-')
        && !s.contains("--")
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn clean(s: Option<String>) -> Option<String> {
    s.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty())
}

fn check_text(title: &str, summary: &Option<String>, body: &str) -> Result<(), AppError> {
    if title.is_empty() {
        return Err(invalid("title", "required"));
    }
    if title.chars().count() > 200 {
        return Err(invalid("title", "too_long"));
    }
    if summary.as_deref().is_some_and(|s| s.chars().count() > 500) {
        return Err(invalid("summary", "too_long"));
    }
    if body.trim().is_empty() {
        return Err(invalid("body_md", "required"));
    }
    if body.chars().count() > MAX_BODY {
        return Err(invalid("body_md", "too_long"));
    }
    Ok(())
}

pub async fn create(app: &App, p: &Principal, meta: &RequestMeta, i: Create) -> Result<ArticleRow, AppError> {
    authz::require_role(p, Role::Moderator)?;
    let title = i.title.trim().to_owned();
    let summary = clean(i.summary);
    check_text(&title, &summary, &i.body_md)?;
    let mut tx = AppTx::begin(&app.pool).await?;
    let slug = match clean(i.slug) {
        Some(s) => {
            if !check_slug(&s) {
                return Err(invalid("slug", "invalid_value"));
            }
            if repo::slug_taken(&mut tx, &s).await? {
                return Err(AppError::Taken {
                    code: "slug_taken",
                    existing_id: None,
                });
            }
            s
        }
        None => free_slug(&mut tx, &title).await?,
    };
    let row = repo::insert(
        &mut tx,
        &NewArticle {
            slug: &slug,
            title: &title,
            summary: summary.as_deref(),
            body_md: &i.body_md,
            publish: i.publish,
            created_by: p.user_id,
        },
    )
    .await?;
    record(
        &mut tx,
        p,
        meta,
        "article.create",
        "article",
        row.id,
        None,
        snapshot(&summary_of(&row)),
    )
    .await?;
    tx.commit().await?;
    Ok(row)
}

fn summary_of(a: &ArticleRow) -> serde_json::Value {
    serde_json::json!({"slug": a.slug, "title": a.title, "status": a.status, "on_home": a.on_home})
}

async fn free_slug(conn: &mut PgConnection, title: &str) -> Result<String, AppError> {
    let base = match slugify(title) {
        s if s.is_empty() || !check_slug(&s) => format!("article-{}", &Uuid::new_v4().simple().to_string()[..8]),
        s => s,
    };
    let mut slug = base.clone();
    let mut n = 2;
    while repo::slug_taken(conn, &slug).await? {
        slug = format!("{base}-{n}");
        n += 1;
    }
    Ok(slug)
}

pub async fn update(app: &App, p: &Principal, meta: &RequestMeta, id: Uuid, i: Update) -> Result<ArticleRow, AppError> {
    authz::require_role(p, Role::Moderator)?;
    let mut tx = AppTx::begin(&app.pool).await?;
    let before = repo::get(&mut tx, id, true)
        .await?
        .ok_or(AppError::NotFound { entity: "article" })?;
    if before.version != i.expected_version {
        return Err(AppError::StaleVersion {
            current_version: before.version,
        });
    }
    let mut a = before.clone();
    if let Some(t) = i.title {
        a.title = t.trim().to_owned();
    }
    if let Some(s) = i.summary {
        a.summary = clean(s);
    }
    if let Some(b) = i.body_md {
        a.body_md = b;
    }
    check_text(&a.title, &a.summary, &a.body_md)?;
    if let Some(s) = clean(i.slug) {
        if s != a.slug {
            if !check_slug(&s) {
                return Err(invalid("slug", "invalid_value"));
            }
            if repo::slug_taken(&mut tx, &s).await? {
                return Err(AppError::Taken {
                    code: "slug_taken",
                    existing_id: None,
                });
            }
            a.slug = s;
        }
    }
    if let Some(c) = i.cover_image_id {
        if let Some(img) = c {
            match repo::image(&mut tx, img).await? {
                Some(r) if r.article_id == id => {}
                _ => return Err(invalid("cover_image_id", "unknown_image")),
            }
        }
        a.cover_image_id = c;
    }
    match i.publish {
        Some(true) if a.status != "published" => {
            a.status = "published".into();
            a.published_at = Some(Utc::now());
        }
        Some(false) => {
            a.status = "draft".into();
            a.published_at = None;
            // A draft is never on the home page.
            a.on_home = false;
        }
        _ => {}
    }
    let row = repo::save(&mut tx, &a).await?;
    record(
        &mut tx,
        p,
        meta,
        "article.update",
        "article",
        id,
        snapshot(&summary_of(&before)),
        snapshot(&summary_of(&row)),
    )
    .await?;
    tx.commit().await?;
    Ok(row)
}

/// Puts the article on the home page (now, or for a period) or takes it off.
pub async fn set_home(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    id: Uuid,
    s: HomeSetting,
) -> Result<ArticleRow, AppError> {
    authz::require_role(p, Role::Moderator)?;
    if let (Some(f), Some(u)) = (s.from, s.until) {
        if f >= u {
            return Err(invalid("home_until", "must_be_after_start"));
        }
    }
    let mut tx = AppTx::begin(&app.pool).await?;
    let before = repo::get(&mut tx, id, true)
        .await?
        .ok_or(AppError::NotFound { entity: "article" })?;
    if s.on_home && before.status != "published" {
        return Err(invalid("on_home", "not_published"));
    }
    if s.on_home && s.until.is_some_and(|u| u <= Utc::now()) {
        return Err(invalid("home_until", "in_the_past"));
    }
    let mut a = before.clone();
    a.on_home = s.on_home;
    (a.home_from, a.home_until) = if s.on_home { (s.from, s.until) } else { (None, None) };
    let row = repo::save(&mut tx, &a).await?;
    record(
        &mut tx,
        p,
        meta,
        "article.home",
        "article",
        id,
        snapshot(&summary_of(&before)),
        snapshot(&summary_of(&row)),
    )
    .await?;
    tx.commit().await?;
    Ok(row)
}

pub async fn delete(app: &App, p: &Principal, meta: &RequestMeta, id: Uuid) -> Result<(), AppError> {
    authz::require_role(p, Role::Moderator)?;
    let mut tx = AppTx::begin(&app.pool).await?;
    let before = repo::get(&mut tx, id, true)
        .await?
        .ok_or(AppError::NotFound { entity: "article" })?;
    let images = repo::image_ids(&mut tx, id).await?;
    repo::delete(&mut tx, id).await?;
    record(
        &mut tx,
        p,
        meta,
        "article.delete",
        "article",
        id,
        snapshot(&summary_of(&before)),
        None,
    )
    .await?;
    tx.commit().await?;
    for i in images {
        let _ = app.media.delete_prefix(&i.to_string()).await;
    }
    Ok(())
}

/// A published article by address; moderators also see drafts.
pub async fn read(app: &App, p: Option<&Principal>, slug: &str) -> Result<ArticleRow, AppError> {
    let mut conn = app.pool.acquire().await?;
    let a = repo::by_slug(&mut conn, slug)
        .await?
        .ok_or(AppError::NotFound { entity: "article" })?;
    if a.status != "published" && !p.is_some_and(|p| p.has_role(Role::Moderator)) {
        return Err(AppError::NotFound { entity: "article" });
    }
    Ok(a)
}

pub async fn list(app: &App, p: Option<&Principal>) -> Result<Vec<ArticleRow>, AppError> {
    let mut conn = app.pool.acquire().await?;
    let all = p.is_some_and(|p| p.has_role(Role::Moderator));
    Ok(repo::list(&mut conn, all, LIST_LIMIT).await?)
}

pub async fn on_home(app: &App) -> Result<Vec<ArticleRow>, AppError> {
    let mut conn = app.pool.acquire().await?;
    Ok(repo::on_home(&mut conn, HOME_LIMIT).await?)
}

pub async fn images(app: &App, p: &Principal, article: Uuid) -> Result<Vec<ArticleImageRow>, AppError> {
    authz::require_role(p, Role::Moderator)?;
    let mut conn = app.pool.acquire().await?;
    Ok(repo::images(&mut conn, article).await?)
}

pub async fn upload_image(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    article: Uuid,
    bytes: Vec<u8>,
    credit: Option<String>,
) -> Result<ArticleImageRow, AppError> {
    authz::require_role(p, Role::Moderator)?;
    if bytes.is_empty() {
        return Err(invalid("file", "required"));
    }
    if bytes.len() as u64 > app.config.media_max_bytes {
        return Err(invalid("file", "too_large"));
    }
    let credit = clean(credit);
    if credit.as_deref().is_some_and(|c| c.chars().count() > 300) {
        return Err(invalid("credit", "too_long"));
    }
    {
        let mut conn = app.pool.acquire().await?;
        if repo::get(&mut conn, article, false).await?.is_none() {
            return Err(AppError::NotFound { entity: "article" });
        }
        if repo::count_images(&mut conn, article).await? >= MAX_IMAGES {
            return Err(AppError::Restricted {
                code: "too_many_images",
            });
        }
    }
    let max_side = app.config.media_max_side;
    let processed = tokio::task::spawn_blocking(move || media::process_wide(&bytes, &WIDTHS, max_side))
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .map_err(|e| match e {
            MediaError::UnsupportedType => invalid("file", "unsupported_type"),
            MediaError::Corrupt => invalid("file", "corrupt"),
            MediaError::TooLarge => invalid("file", "dimensions_too_large"),
            MediaError::TooSmall => invalid("file", "dimensions_too_small"),
        })?;
    let id = Uuid::new_v4();
    for (w, b) in &processed.variants {
        app.media
            .put(&media::key(id, *w), b.clone())
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
    }
    let sizes: Vec<i32> = processed.variants.iter().map(|v| v.0 as i32).collect();
    let result = async {
        let mut tx = AppTx::begin(&app.pool).await?;
        let row = repo::insert_image(
            &mut tx,
            id,
            article,
            processed.width as i32,
            processed.height as i32,
            &sizes,
            credit.as_deref(),
            p.user_id,
        )
        .await?;
        record(
            &mut tx,
            p,
            meta,
            "article.image",
            "article",
            article,
            None,
            snapshot(&serde_json::json!({"image": id})),
        )
        .await?;
        tx.commit().await?;
        Ok::<_, AppError>(row)
    }
    .await;
    if result.is_err() {
        let _ = app.media.delete_prefix(&id.to_string()).await;
    }
    result
}

pub async fn delete_image(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    article: Uuid,
    image: Uuid,
) -> Result<(), AppError> {
    authz::require_role(p, Role::Moderator)?;
    let mut tx = AppTx::begin(&app.pool).await?;
    match repo::image(&mut tx, image).await? {
        Some(r) if r.article_id == article => {}
        _ => return Err(AppError::NotFound { entity: "image" }),
    }
    repo::delete_image(&mut tx, image).await?;
    record(
        &mut tx,
        p,
        meta,
        "article.image_delete",
        "article",
        article,
        snapshot(&serde_json::json!({"image": image})),
        None,
    )
    .await?;
    tx.commit().await?;
    let _ = app.media.delete_prefix(&image.to_string()).await;
    Ok(())
}

/// The stored file of an article picture at the smallest width of at least `width`; `None` if it is not one.
pub async fn image_file(app: &App, id: Uuid, width: u32) -> Result<Option<Vec<u8>>, AppError> {
    let mut conn = app.pool.acquire().await?;
    let Some(row) = repo::image(&mut conn, id).await? else {
        return Ok(None);
    };
    drop(conn);
    let size = row
        .sizes
        .iter()
        .copied()
        .filter(|s| *s as u32 >= width)
        .min()
        .or_else(|| row.sizes.iter().copied().max())
        .ok_or(AppError::NotFound { entity: "image" })?;
    app.media
        .get(&media::key(id, size as u32))
        .await
        .map_err(|e| AppError::Internal(e.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_come_from_titles() {
        assert_eq!(slugify("Onam 2026: Celebrating!"), "onam-2026-celebrating");
        assert_eq!(slugify("ഓണം"), "");
        assert!(check_slug("onam-2026"));
        assert!(!check_slug("Onam"));
        assert!(!check_slug("a--b"));
    }
}
