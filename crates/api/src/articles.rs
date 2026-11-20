//! Articles: blog-style pages written by moderators, readable by everyone, optionally shown on the home page.

use crate::extract::{ApiJson, ApiPath, MaybeUser, Meta, Moderator, RequireRole};
use crate::problem::{ApiError, ApiResult, Problem};
use application::articles::{self as uc, Create, HomeSetting, Update};
use application::App;
use axum::extract::{Multipart, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Utc};
use infrastructure::articles::{ArticleImageRow, ArticleRow};
use serde::{Deserialize, Deserializer, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

/// An article. `body_md` is empty in lists.
#[derive(Serialize, ToSchema)]
pub struct Article {
    pub id: Uuid,
    pub slug: String,
    pub title: String,
    pub summary: Option<String>,
    /// Markdown. Empty in lists; fetch the article for the text.
    pub body_md: String,
    /// Show at `/api/v1/media/{cover_image_id}/{width}`.
    pub cover_image_id: Option<Uuid>,
    /// `draft` (moderators only) or `published`.
    pub status: String,
    pub published_at: Option<DateTime<Utc>>,
    pub on_home: bool,
    pub home_from: Option<DateTime<Utc>>,
    pub home_until: Option<DateTime<Utc>>,
    /// Whether the home page shows it right now.
    pub home_now: bool,
    pub updated_at: DateTime<Utc>,
    pub version: i32,
}

fn article(a: ArticleRow, with_body: bool) -> Article {
    let now = Utc::now();
    Article {
        home_now: a.status == "published"
            && a.on_home
            && a.home_from.is_none_or(|f| f <= now)
            && a.home_until.is_none_or(|u| u > now),
        id: a.id,
        slug: a.slug,
        title: a.title,
        summary: a.summary,
        body_md: if with_body { a.body_md } else { String::new() },
        cover_image_id: a.cover_image_id,
        status: a.status,
        published_at: a.published_at,
        on_home: a.on_home,
        home_from: a.home_from,
        home_until: a.home_until,
        updated_at: a.updated_at,
        version: a.version,
    }
}

#[derive(Serialize, ToSchema)]
pub struct ArticleList {
    pub items: Vec<Article>,
}

#[utoipa::path(get, path = "/api/v1/articles", operation_id = "articles_list", tag = "articles", responses(
    (status = 200, body = ArticleList, description = "Published articles, newest first (moderators also see drafts)")))]
pub async fn list(State(app): State<App>, MaybeUser(p): MaybeUser) -> ApiResult<Json<ArticleList>> {
    let rows = uc::list(&app, p.as_ref()).await?;
    Ok(Json(ArticleList {
        items: rows.into_iter().map(|a| article(a, false)).collect(),
    }))
}

#[utoipa::path(get, path = "/api/v1/articles/home", operation_id = "articles_home", tag = "articles", responses(
    (status = 200, body = ArticleList, description = "Articles whose home-page period is running now (at most three); empty most of the time")))]
pub async fn home(State(app): State<App>) -> ApiResult<Json<ArticleList>> {
    let rows = uc::on_home(&app).await?;
    Ok(Json(ArticleList {
        items: rows.into_iter().map(|a| article(a, false)).collect(),
    }))
}

#[utoipa::path(get, path = "/api/v1/articles/{key}", operation_id = "articles_read", tag = "articles", params(("key" = String, Path)), responses(
    (status = 200, body = Article),
    (status = 404, body = Problem, content_type = "application/problem+json")))]
pub async fn read(
    State(app): State<App>,
    MaybeUser(p): MaybeUser,
    ApiPath(slug): ApiPath<String>,
) -> ApiResult<Json<Article>> {
    Ok(Json(article(uc::read(&app, p.as_ref(), &slug).await?, true)))
}

#[derive(Deserialize, ToSchema)]
pub struct ArticleCreate {
    pub title: String,
    /// Address of the page; made from the title when left out.
    pub slug: Option<String>,
    pub summary: Option<String>,
    /// Markdown.
    pub body_md: String,
    /// Publish at once (default: save as a draft).
    #[serde(default)]
    pub publish: bool,
}

#[utoipa::path(post, path = "/api/v1/articles", operation_id = "articles_create", tag = "articles", request_body = ArticleCreate, security(("session" = [])), responses(
    (status = 201, body = Article),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json"),
    (status = 409, body = Problem, content_type = "application/problem+json", description = "`slug_taken`"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn create(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireRole(p, ..): RequireRole<Moderator>,
    ApiJson(b): ApiJson<ArticleCreate>,
) -> ApiResult<(StatusCode, Json<Article>)> {
    let row = uc::create(
        &app,
        &p,
        &meta,
        Create {
            title: b.title,
            slug: b.slug,
            summary: b.summary,
            body_md: b.body_md,
            publish: b.publish,
        },
    )
    .await?;
    Ok((StatusCode::CREATED, Json(article(row, true))))
}

/// A field that may be left out (unchanged), `null` (cleared) or set.
fn set<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(d).map(Some)
}

#[derive(Deserialize, ToSchema)]
pub struct ArticleUpdate {
    pub expected_version: i32,
    pub title: Option<String>,
    pub slug: Option<String>,
    #[serde(default, deserialize_with = "set")]
    pub summary: Option<Option<String>>,
    pub body_md: Option<String>,
    #[serde(default, deserialize_with = "set")]
    pub cover_image_id: Option<Option<Uuid>>,
    /// `true` publishes, `false` returns it to a draft (which also takes it off the home page).
    pub publish: Option<bool>,
}

#[utoipa::path(patch, path = "/api/v1/articles/{key}", operation_id = "articles_update", tag = "articles", params(("key" = Uuid, Path)), request_body = ArticleUpdate,
    security(("session" = [])), responses(
    (status = 200, body = Article),
    (status = 404, body = Problem, content_type = "application/problem+json"),
    (status = 409, body = Problem, content_type = "application/problem+json", description = "`slug_taken` or a stale version"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn update(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireRole(p, ..): RequireRole<Moderator>,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(b): ApiJson<ArticleUpdate>,
) -> ApiResult<Json<Article>> {
    let row = uc::update(
        &app,
        &p,
        &meta,
        id,
        Update {
            expected_version: b.expected_version,
            title: b.title,
            slug: b.slug,
            summary: b.summary,
            body_md: b.body_md,
            cover_image_id: b.cover_image_id,
            publish: b.publish,
        },
    )
    .await?;
    Ok(Json(article(row, true)))
}

#[derive(Deserialize, ToSchema)]
pub struct HomeBody {
    pub on_home: bool,
    /// Show from this time (default: now).
    pub home_from: Option<DateTime<Utc>>,
    /// Hide again at this time (default: until switched off).
    pub home_until: Option<DateTime<Utc>>,
}

#[utoipa::path(put, path = "/api/v1/articles/{key}/home", operation_id = "articles_set_home", tag = "articles", params(("key" = Uuid, Path)), request_body = HomeBody,
    security(("session" = [])), responses(
    (status = 200, body = Article),
    (status = 404, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json", description = "`not_published`, `in_the_past`, `must_be_after_start`")))]
pub async fn set_home(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireRole(p, ..): RequireRole<Moderator>,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(b): ApiJson<HomeBody>,
) -> ApiResult<Json<Article>> {
    let row = uc::set_home(
        &app,
        &p,
        &meta,
        id,
        HomeSetting {
            on_home: b.on_home,
            from: b.home_from,
            until: b.home_until,
        },
    )
    .await?;
    Ok(Json(article(row, true)))
}

#[utoipa::path(delete, path = "/api/v1/articles/{key}", operation_id = "articles_delete", tag = "articles", params(("key" = Uuid, Path)), security(("session" = [])), responses(
    (status = 204), (status = 404, body = Problem, content_type = "application/problem+json")))]
pub async fn delete(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireRole(p, ..): RequireRole<Moderator>,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<StatusCode> {
    uc::delete(&app, &p, &meta, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, ToSchema)]
pub struct ArticleImage {
    pub id: Uuid,
    pub width: i32,
    pub height: i32,
    pub credit: Option<String>,
    /// Ready to paste into the article: `![](/api/v1/media/<id>/960)`.
    pub markdown: String,
}

fn image(r: ArticleImageRow) -> ArticleImage {
    ArticleImage {
        markdown: format!("![](/api/v1/media/{}/960)", r.id),
        id: r.id,
        width: r.width,
        height: r.height,
        credit: r.credit,
    }
}

#[derive(Serialize, ToSchema)]
pub struct ArticleImages {
    pub items: Vec<ArticleImage>,
}

#[utoipa::path(get, path = "/api/v1/articles/{key}/images", operation_id = "articles_images", tag = "articles", params(("key" = Uuid, Path)), security(("session" = [])), responses(
    (status = 200, body = ArticleImages)))]
pub async fn images(
    State(app): State<App>,
    RequireRole(p, ..): RequireRole<Moderator>,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<Json<ArticleImages>> {
    let rows = uc::images(&app, &p, id).await?;
    Ok(Json(ArticleImages {
        items: rows.into_iter().map(image).collect(),
    }))
}

#[derive(ToSchema)]
#[allow(dead_code)]
pub struct ArticleImageUpload {
    #[schema(format = Binary)]
    pub file: String,
    pub credit: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/articles/{key}/images", operation_id = "articles_upload_image", tag = "articles", params(("key" = Uuid, Path)),
    request_body(content = ArticleImageUpload, content_type = "multipart/form-data"), security(("session" = [])), responses(
    (status = 201, body = ArticleImage, description = "A picture of the article, public at once, in proportion (not cropped)"),
    (status = 404, body = Problem, content_type = "application/problem+json"),
    (status = 413, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json",
        description = "`file`: `required`, `too_large`, `unsupported_type`, `corrupt`, `dimensions_too_small`, `dimensions_too_large`")))]
pub async fn upload_image(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireRole(p, ..): RequireRole<Moderator>,
    ApiPath(id): ApiPath<Uuid>,
    mut form: Multipart,
) -> ApiResult<(StatusCode, Json<ArticleImage>)> {
    let bad = |e: axum::extract::multipart::MultipartError| {
        if e.status() == StatusCode::PAYLOAD_TOO_LARGE {
            ApiError::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload_too_large",
                "The upload is larger than allowed",
            )
        } else {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                "malformed_request",
                "The upload form could not be read",
            )
        }
    };
    let (mut bytes, mut credit) = (Vec::new(), None);
    while let Some(field) = form.next_field().await.map_err(bad)? {
        match field.name().unwrap_or_default() {
            "file" => bytes = field.bytes().await.map_err(bad)?.to_vec(),
            "credit" => credit = Some(field.text().await.map_err(bad)?),
            _ => {}
        }
    }
    let row = uc::upload_image(&app, &p, &meta, id, bytes, credit).await?;
    Ok((StatusCode::CREATED, Json(image(row))))
}

#[utoipa::path(delete, path = "/api/v1/articles/{key}/images/{image_id}", operation_id = "articles_delete_image", tag = "articles",
    params(("key" = Uuid, Path), ("image_id" = Uuid, Path)), security(("session" = [])), responses(
    (status = 204), (status = 404, body = Problem, content_type = "application/problem+json")))]
pub async fn delete_image(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireRole(p, ..): RequireRole<Moderator>,
    ApiPath((id, image_id)): ApiPath<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    uc::delete_image(&app, &p, &meta, id, image_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
