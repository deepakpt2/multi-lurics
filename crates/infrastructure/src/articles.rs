//! Articles (blog-style pages written by moderators) and their pictures.

use chrono::{DateTime, Utc};
use sqlx::PgConnection;
use uuid::Uuid;

type Res<T> = Result<T, sqlx::Error>;

const COLS: &str =
    "id, slug, title, summary, body_md, cover_image_id, status, published_at, on_home, home_from, home_until,
                    created_by, created_at, updated_at, version";

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ArticleRow {
    pub id: Uuid,
    pub slug: String,
    pub title: String,
    pub summary: Option<String>,
    pub body_md: String,
    pub cover_image_id: Option<Uuid>,
    pub status: String,
    pub published_at: Option<DateTime<Utc>>,
    pub on_home: bool,
    pub home_from: Option<DateTime<Utc>>,
    pub home_until: Option<DateTime<Utc>>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: i32,
}

pub struct NewArticle<'a> {
    pub slug: &'a str,
    pub title: &'a str,
    pub summary: Option<&'a str>,
    pub body_md: &'a str,
    pub publish: bool,
    pub created_by: Uuid,
}

pub async fn slug_taken(conn: &mut PgConnection, slug: &str) -> Res<bool> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM articles WHERE slug = $1)")
        .bind(slug)
        .fetch_one(conn)
        .await
}

pub async fn insert(conn: &mut PgConnection, n: &NewArticle<'_>) -> Res<ArticleRow> {
    sqlx::query_as(&format!(
        "INSERT INTO articles (slug, title, summary, body_md, status, published_at, created_by)
         VALUES ($1, $2, $3, $4, CASE WHEN $5 THEN 'published' ELSE 'draft' END, CASE WHEN $5 THEN now() END, $6)
         RETURNING {COLS}"
    ))
    .bind(n.slug)
    .bind(n.title)
    .bind(n.summary)
    .bind(n.body_md)
    .bind(n.publish)
    .bind(n.created_by)
    .fetch_one(conn)
    .await
}

pub async fn get(conn: &mut PgConnection, id: Uuid, lock: bool) -> Res<Option<ArticleRow>> {
    sqlx::query_as(&format!(
        "SELECT {COLS} FROM articles WHERE id = $1{}",
        if lock { " FOR UPDATE" } else { "" }
    ))
    .bind(id)
    .fetch_optional(conn)
    .await
}

pub async fn by_slug(conn: &mut PgConnection, slug: &str) -> Res<Option<ArticleRow>> {
    sqlx::query_as(&format!("SELECT {COLS} FROM articles WHERE slug = $1"))
        .bind(slug)
        .fetch_optional(conn)
        .await
}

/// Writes every editable field (the caller has merged the changes into `a`) and bumps the version.
pub async fn save(conn: &mut PgConnection, a: &ArticleRow) -> Res<ArticleRow> {
    sqlx::query_as(&format!(
        "UPDATE articles SET slug = $2, title = $3, summary = $4, body_md = $5, cover_image_id = $6, status = $7,
                published_at = $8, on_home = $9, home_from = $10, home_until = $11, updated_at = now(), version = version + 1
         WHERE id = $1 RETURNING {COLS}"
    ))
    .bind(a.id)
    .bind(&a.slug)
    .bind(&a.title)
    .bind(&a.summary)
    .bind(&a.body_md)
    .bind(a.cover_image_id)
    .bind(&a.status)
    .bind(a.published_at)
    .bind(a.on_home)
    .bind(a.home_from)
    .bind(a.home_until)
    .fetch_one(conn)
    .await
}

pub async fn delete(conn: &mut PgConnection, id: Uuid) -> Res<()> {
    sqlx::query("DELETE FROM articles WHERE id = $1")
        .bind(id)
        .execute(conn)
        .await?;
    Ok(())
}

/// Published articles, newest first; `all` adds drafts (for moderators).
pub async fn list(conn: &mut PgConnection, all: bool, limit: i64) -> Res<Vec<ArticleRow>> {
    sqlx::query_as(&format!(
        "SELECT {COLS} FROM articles WHERE ($1 OR status = 'published')
         ORDER BY COALESCE(published_at, created_at) DESC, id LIMIT $2"
    ))
    .bind(all)
    .bind(limit)
    .fetch_all(conn)
    .await
}

/// Articles whose home-page period is running now.
pub async fn on_home(conn: &mut PgConnection, limit: i64) -> Res<Vec<ArticleRow>> {
    sqlx::query_as(&format!(
        "SELECT {COLS} FROM articles
         WHERE status = 'published' AND on_home
           AND (home_from IS NULL OR home_from <= now()) AND (home_until IS NULL OR home_until > now())
         ORDER BY published_at DESC, id LIMIT $1"
    ))
    .bind(limit)
    .fetch_all(conn)
    .await
}

pub async fn count_images(conn: &mut PgConnection, article: Uuid) -> Res<i64> {
    sqlx::query_scalar("SELECT count(*) FROM article_images WHERE article_id = $1")
        .bind(article)
        .fetch_one(conn)
        .await
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ArticleImageRow {
    pub id: Uuid,
    pub article_id: Uuid,
    pub width: i32,
    pub height: i32,
    pub sizes: Vec<i32>,
    pub credit: Option<String>,
    pub created_at: DateTime<Utc>,
}

const IMG_COLS: &str = "id, article_id, width, height, sizes, credit, created_at";

#[allow(clippy::too_many_arguments)]
pub async fn insert_image(
    conn: &mut PgConnection,
    id: Uuid,
    article: Uuid,
    width: i32,
    height: i32,
    sizes: &[i32],
    credit: Option<&str>,
    by: Uuid,
) -> Res<ArticleImageRow> {
    sqlx::query_as(&format!(
        "INSERT INTO article_images (id, article_id, width, height, sizes, credit, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING {IMG_COLS}"
    ))
    .bind(id)
    .bind(article)
    .bind(width)
    .bind(height)
    .bind(sizes)
    .bind(credit)
    .bind(by)
    .fetch_one(conn)
    .await
}

pub async fn images(conn: &mut PgConnection, article: Uuid) -> Res<Vec<ArticleImageRow>> {
    sqlx::query_as(&format!(
        "SELECT {IMG_COLS} FROM article_images WHERE article_id = $1 ORDER BY created_at, id"
    ))
    .bind(article)
    .fetch_all(conn)
    .await
}

pub async fn image(conn: &mut PgConnection, id: Uuid) -> Res<Option<ArticleImageRow>> {
    sqlx::query_as(&format!("SELECT {IMG_COLS} FROM article_images WHERE id = $1"))
        .bind(id)
        .fetch_optional(conn)
        .await
}

pub async fn delete_image(conn: &mut PgConnection, id: Uuid) -> Res<()> {
    sqlx::query("DELETE FROM article_images WHERE id = $1")
        .bind(id)
        .execute(conn)
        .await?;
    Ok(())
}

/// Ids of an article's pictures (to remove their files when it is deleted).
pub async fn image_ids(conn: &mut PgConnection, article: Uuid) -> Res<Vec<Uuid>> {
    sqlx::query_scalar("SELECT id FROM article_images WHERE article_id = $1")
        .bind(article)
        .fetch_all(conn)
        .await
}
