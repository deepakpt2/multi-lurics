//! Suggested corrections to published lyrics: anyone signed in suggests, a moderator decides.

use crate::extract::{ApiJson, ApiPath, Meta, Moderator, RequireRole, RequireUser};
use crate::problem::{ApiResult, Problem};
use application::App;
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Deserialize, ToSchema)]
pub struct CorrectionCreate {
    /// The whole lyric as it should read.
    pub body: String,
    /// What was wrong, in a few words.
    pub note: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct CorrectionCreated {
    pub id: Uuid,
}

#[derive(Deserialize, ToSchema)]
pub struct CorrectionDecision {
    pub note: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct CorrectionItem {
    pub id: Uuid,
    pub contribution_id: Uuid,
    pub language: String,
    /// The suggested text.
    pub body: String,
    /// The text that is public now.
    pub current_body: String,
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize, ToSchema)]
pub struct CorrectionList {
    pub items: Vec<CorrectionItem>,
}

#[utoipa::path(post, path = "/api/v1/contributions/{id}/corrections", tag = "lyrics", operation_id = "corrections_suggest", request_body = CorrectionCreate,
    params(("id" = Uuid, Path)), security(("session" = [])), responses(
    (status = 201, body = CorrectionCreated),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 404, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json"),
    (status = 429, body = Problem, content_type = "application/problem+json")))]
pub async fn suggest(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireUser(p): RequireUser,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(b): ApiJson<CorrectionCreate>,
) -> ApiResult<(StatusCode, Json<CorrectionCreated>)> {
    let id = application::corrections::suggest(&app, &p, &meta, id, &b.body, b.note.as_deref()).await?;
    Ok((StatusCode::CREATED, Json(CorrectionCreated { id })))
}

#[utoipa::path(get, path = "/api/v1/moderation/corrections", tag = "moderation", operation_id = "corrections_queue", security(("session" = [])), responses(
    (status = 200, body = CorrectionList),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json")))]
pub async fn queue(State(app): State<App>, RequireRole(p, ..): RequireRole<Moderator>) -> ApiResult<Json<CorrectionList>> {
    let items = application::corrections::queue(&app, &p)
        .await?
        .into_iter()
        .map(|k| CorrectionItem {
            id: k.id,
            contribution_id: k.contribution_id,
            language: k.language,
            body: k.body,
            current_body: k.current_body,
            note: k.note,
            created_at: k.created_at,
        })
        .collect();
    Ok(Json(CorrectionList { items }))
}

#[utoipa::path(post, path = "/api/v1/moderation/corrections/{id}/apply", tag = "moderation", operation_id = "corrections_apply", request_body = CorrectionDecision,
    params(("id" = Uuid, Path)), security(("session" = [])), responses(
    (status = 204, description = "Published as a new revision"),
    (status = 403, body = Problem, content_type = "application/problem+json"),
    (status = 404, body = Problem, content_type = "application/problem+json")))]
pub async fn apply(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireRole(p, ..): RequireRole<Moderator>,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(b): ApiJson<CorrectionDecision>,
) -> ApiResult<StatusCode> {
    application::corrections::apply(&app, &p, &meta, id, b.note.as_deref()).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(post, path = "/api/v1/moderation/corrections/{id}/reject", tag = "moderation", operation_id = "corrections_reject", request_body = CorrectionDecision,
    params(("id" = Uuid, Path)), security(("session" = [])), responses(
    (status = 204, description = "Rejected; the lyric is unchanged"),
    (status = 403, body = Problem, content_type = "application/problem+json"),
    (status = 404, body = Problem, content_type = "application/problem+json")))]
pub async fn reject(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireRole(p, ..): RequireRole<Moderator>,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(b): ApiJson<CorrectionDecision>,
) -> ApiResult<StatusCode> {
    application::corrections::reject(&app, &p, &meta, id, b.note.as_deref()).await?;
    Ok(StatusCode::NO_CONTENT)
}
