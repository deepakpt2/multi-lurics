//! Admins create metadata values in advance.

use crate::extract::{Admin, ApiJson, Meta, RequireRole};
use crate::problem::{ApiResult, Problem};
use application::metadata as uc;
use application::App;
use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Deserialize, ToSchema)]
pub struct MetadataCreate {
    /// `artist`, `album`, `raga`, `deity` or `tag`.
    pub kind: String,
    /// One value per entry, at most 200. Blank entries are ignored.
    pub names: Vec<String>,
    /// Artists: `person` (default), `group`, `other`. Albums: `album` (default), `soundtrack`, `single`, `ep`, `compilation`, `other`.
    pub sub_kind: Option<String>,
    /// Albums: release year, for every album created.
    pub year: Option<i32>,
}

#[derive(Serialize, ToSchema)]
pub struct MetadataResult {
    pub name: String,
    pub id: Option<Uuid>,
    /// False when it was already in the catalogue (by name, alias or another script's spelling).
    pub created: bool,
    /// Why this name could not be made (`too_long`, `invalid_value`…).
    pub problem: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct MetadataResponse {
    pub results: Vec<MetadataResult>,
}

#[utoipa::path(post, path = "/api/v1/admin/metadata", operation_id = "create_metadata", tag = "admin",
    request_body = MetadataCreate, security(("session" = [])), responses(
    (status = 200, body = MetadataResponse),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn create_metadata(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireRole(p, ..): RequireRole<Admin>,
    ApiJson(b): ApiJson<MetadataCreate>,
) -> ApiResult<Json<MetadataResponse>> {
    let kind = uc::Kind::parse(&b.kind)
        .ok_or_else(|| application::AppError::from(domain::DomainError::invalid("kind", "invalid_choice")))?;
    let results = uc::create(&app, &p, &meta, kind, b.names, b.sub_kind, b.year).await?;
    Ok(Json(MetadataResponse {
        results: results
            .into_iter()
            .map(|c| MetadataResult {
                name: c.name,
                id: c.id,
                created: c.created,
                problem: c.problem,
            })
            .collect(),
    }))
}
