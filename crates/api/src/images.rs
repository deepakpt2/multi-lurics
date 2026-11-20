//! Image upload, review and delivery. Files are served from `/api/v1/media/{id}/{width}`.

use crate::extract::{ApiJson, ApiPath, ApiQuery, MaybeUser, Meta, Moderator, RequireRole, RequireUser};
use crate::problem::{ApiError, ApiResult, Problem};
use crate::routes::PageQuery;
use application::images::{self as uc, EntityImages, QueueItem, UploadInput};
use application::App;
use axum::extract::{Multipart, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use infrastructure::images::ImageRow;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ImageStatus {
    /// Uploaded but not yet attached to anything (used while proposing a new entity).
    Draft,
    /// Waiting for a moderator.
    Pending,
    /// The public image of its entity.
    Approved,
    Rejected,
    /// Replaced by a newer approved image.
    Superseded,
}

#[derive(Serialize, ToSchema)]
pub struct Image {
    pub id: Uuid,
    pub status: ImageStatus,
    pub entity_type: Option<String>,
    pub entity_id: Option<Uuid>,
    pub width: i32,
    pub height: i32,
    pub credit: Option<String>,
    pub created_at: DateTime<Utc>,
    pub decision_note: Option<String>,
}

impl From<ImageRow> for Image {
    fn from(r: ImageRow) -> Self {
        Self {
            status: match r.status.as_str() {
                "draft" => ImageStatus::Draft,
                "approved" => ImageStatus::Approved,
                "rejected" => ImageStatus::Rejected,
                "superseded" => ImageStatus::Superseded,
                _ => ImageStatus::Pending,
            },
            id: r.id,
            entity_type: r.entity_type,
            entity_id: r.entity_id,
            width: r.width,
            height: r.height,
            credit: r.credit,
            created_at: r.created_at,
            decision_note: r.decision_note,
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct ImageLimits {
    pub max_bytes: u64,
    pub min_side: u32,
    pub max_side: u32,
    /// Square sizes generated for every image.
    pub sizes: Vec<u32>,
    pub types: Vec<String>,
}

#[utoipa::path(get, path = "/api/v1/images/limits", tag = "images", responses(
    (status = 200, body = ImageLimits, description = "What an upload may be")))]
pub async fn limits(State(app): State<App>) -> Json<ImageLimits> {
    let l = uc::limits(&app);
    Json(ImageLimits {
        max_bytes: l.max_bytes,
        min_side: l.min_side,
        max_side: l.max_side,
        sizes: l.sizes,
        types: ["image/jpeg", "image/png", "image/webp", "image/gif"]
            .map(String::from)
            .to_vec(),
    })
}

/// Multipart form: `file` (required), `rights` (`true`, required), `credit`, and `entity_type` with `entity_id`.
#[derive(ToSchema)]
#[allow(dead_code)]
pub struct ImageUpload {
    #[schema(format = Binary)]
    pub file: String,
    pub rights: bool,
    pub credit: Option<String>,
    pub entity_type: Option<String>,
    pub entity_id: Option<Uuid>,
}

#[utoipa::path(post, path = "/api/v1/images", tag = "images", security(("session" = [])),
    request_body(content = ImageUpload, content_type = "multipart/form-data"), responses(
    (status = 201, body = Image, description = "`approved` when the uploader may publish directly (moderators, trusted contributors), otherwise `pending`; `draft` when no entity was given"),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 404, body = Problem, content_type = "application/problem+json"),
    (status = 409, body = Problem, content_type = "application/problem+json", description = "`image_pending` or `image_already_current`"),
    (status = 413, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json",
        description = "`file`: `required`, `too_large`, `unsupported_type`, `corrupt`, `dimensions_too_small`, `dimensions_too_large`; `rights`: `must_confirm`")))]
pub async fn upload(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireUser(p): RequireUser,
    mut form: Multipart,
) -> ApiResult<Response> {
    let mut input = UploadInput {
        bytes: Vec::new(),
        credit: None,
        rights: false,
        entity_type: None,
        entity_id: None,
    };
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
    let malformed = || {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "malformed_request",
            "The upload form could not be read",
        )
    };
    while let Some(field) = form.next_field().await.map_err(bad)? {
        let name = field.name().unwrap_or_default().to_owned();
        match name.as_str() {
            "file" => input.bytes = field.bytes().await.map_err(bad)?.to_vec(),
            "credit" => input.credit = Some(field.text().await.map_err(bad)?),
            "rights" => input.rights = matches!(field.text().await.map_err(bad)?.trim(), "true" | "on" | "1"),
            "entity_type" => input.entity_type = Some(field.text().await.map_err(bad)?).filter(|s| !s.is_empty()),
            "entity_id" => {
                let t = field.text().await.map_err(bad)?;
                if !t.trim().is_empty() {
                    input.entity_id = Some(t.trim().parse().map_err(|_| malformed())?);
                }
            }
            _ => {}
        }
    }
    let row = uc::upload(&app, &p, &meta, input).await?;
    let id = row.id;
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, format!("/api/v1/media/{id}/640"))],
        Json(Image::from(row)),
    )
        .into_response())
}

#[utoipa::path(delete, path = "/api/v1/images/{id}", tag = "images", params(("id" = Uuid, Path)),
    security(("session" = [])), responses(
    (status = 204, description = "Withdrawn (only your own draft or pending images)"),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 404, body = Problem, content_type = "application/problem+json"),
    (status = 409, body = Problem, content_type = "application/problem+json")))]
pub async fn withdraw(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireUser(p): RequireUser,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<StatusCode> {
    uc::withdraw(&app, &p, &meta, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, ToSchema)]
pub struct EntityImageState {
    /// The public image, if any: show it at `/api/v1/media/{current}/{width}`.
    pub current: Option<Uuid>,
    /// The signed-in person's own image waiting for review.
    pub mine_pending: Option<Image>,
}

#[utoipa::path(get, path = "/api/v1/entities/{entity_type}/{entity_id}/images", tag = "images",
    params(("entity_type" = String, Path), ("entity_id" = Uuid, Path)), responses(
    (status = 200, body = EntityImageState),
    (status = 404, body = Problem, content_type = "application/problem+json")))]
pub async fn entity_images(
    State(app): State<App>,
    MaybeUser(p): MaybeUser,
    ApiPath((kind, id)): ApiPath<(String, Uuid)>,
) -> ApiResult<Response> {
    let EntityImages { current, mine } = uc::for_entity(&app, p.as_ref(), &kind, id).await?;
    let mut res = Json(EntityImageState {
        current,
        mine_pending: mine.map(Into::into),
    })
    .into_response();
    // Depends on who is asking.
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    Ok(res)
}

#[utoipa::path(get, path = "/api/v1/media/{id}/{width}", tag = "images", params(("id" = Uuid, Path),
    ("width" = u32, Path, description = "Wanted width in pixels; the smallest generated size at least this wide is returned")), responses(
    (status = 200, content_type = "image/jpeg", description = "A square JPEG. Approved images are immutable and cacheable for a year."),
    (status = 404, body = Problem, content_type = "application/problem+json")))]
pub async fn media(
    State(app): State<App>,
    MaybeUser(p): MaybeUser,
    ApiPath((id, width)): ApiPath<(Uuid, u32)>,
) -> ApiResult<Response> {
    let f = uc::file(&app, p.as_ref(), id, width).await?;
    let cache = if f.public {
        "public, max-age=31536000, immutable"
    } else {
        "private, no-store"
    };
    Ok((
        [
            (header::CONTENT_TYPE, "image/jpeg"),
            (header::CACHE_CONTROL, cache),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            // The bytes are inert, but nothing should ever render them as a page.
            (header::CONTENT_SECURITY_POLICY, "default-src 'none'; sandbox"),
        ],
        f.bytes,
    )
        .into_response())
}

// ---------- moderation ----------

#[derive(Serialize, ToSchema)]
pub struct ImageReview {
    pub image: Image,
    pub uploader: String,
    /// Name of the entity the image is for (or of the proposed entity).
    pub subject: String,
    /// Set when the image arrived with a new-entity proposal; it is decided with that proposal.
    pub proposal_id: Option<Uuid>,
    /// The image people see today, to compare with.
    pub current_id: Option<Uuid>,
}

#[derive(Serialize, ToSchema)]
pub struct ImageReviewPage {
    pub items: Vec<ImageReview>,
    pub next_cursor: Option<String>,
}

impl From<QueueItem> for ImageReview {
    fn from(q: QueueItem) -> Self {
        Self {
            proposal_id: q.row.proposal_id,
            subject: q.subject,
            uploader: q.uploader,
            current_id: q.current_id,
            image: q.row.into(),
        }
    }
}

#[utoipa::path(get, path = "/api/v1/moderation/images", tag = "moderation", params(PageQuery),
    security(("session" = [])), responses(
    (status = 200, body = ImageReviewPage, description = "Images waiting for review, oldest first"),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json")))]
pub async fn queue(
    State(app): State<App>,
    RequireRole(p, ..): RequireRole<Moderator>,
    ApiQuery(q): ApiQuery<PageQuery>,
) -> ApiResult<Json<ImageReviewPage>> {
    let page = uc::queue(&app, &p, q.cursor.as_deref(), q.limit).await?;
    Ok(Json(ImageReviewPage {
        items: page.items.into_iter().map(Into::into).collect(),
        next_cursor: page.next_cursor,
    }))
}

#[derive(Deserialize, ToSchema)]
pub struct ImageDecision {
    /// Required when rejecting.
    pub note: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/images/{id}/approve", tag = "moderation", request_body = ImageDecision,
    params(("id" = Uuid, Path)), security(("session" = [])), responses(
    (status = 200, body = Image, description = "Now the entity's public image; the previous one is retired"),
    (status = 403, body = Problem, content_type = "application/problem+json"),
    (status = 404, body = Problem, content_type = "application/problem+json"),
    (status = 409, body = Problem, content_type = "application/problem+json", description = "Not pending, or decided with its proposal")))]
pub async fn approve(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireRole(p, ..): RequireRole<Moderator>,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(b): ApiJson<ImageDecision>,
) -> ApiResult<Json<Image>> {
    Ok(Json(uc::approve(&app, &p, &meta, id, b.note.as_deref()).await?.into()))
}

#[utoipa::path(post, path = "/api/v1/images/{id}/reject", tag = "moderation", request_body = ImageDecision,
    params(("id" = Uuid, Path)), security(("session" = [])), responses(
    (status = 200, body = Image, description = "Rejected; the file is deleted. Also removes a public image."),
    (status = 403, body = Problem, content_type = "application/problem+json"),
    (status = 404, body = Problem, content_type = "application/problem+json"),
    (status = 409, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn reject(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireRole(p, ..): RequireRole<Moderator>,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(b): ApiJson<ImageDecision>,
) -> ApiResult<Json<Image>> {
    Ok(Json(
        uc::reject(&app, &p, &meta, id, b.note.as_deref().unwrap_or(""))
            .await?
            .into(),
    ))
}
