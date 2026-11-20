//! Bulk import of songs, for moderators. The web app reads a spreadsheet and sends it in batches.

use crate::extract::{ApiJson, Meta, Moderator, RequireRole};
use crate::problem::{ApiResult, Problem};
use application::import as uc;
use application::App;
use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

/// One song. Everything but `title` is optional; a missing column is the same as an empty one.
#[derive(Deserialize, ToSchema, Default)]
#[serde(default)]
pub struct ImportRow {
    pub title: String,
    /// Language of the lyrics: `ml`, `mal` or `Malayalam`. Needed when there are lyrics.
    pub language: String,
    /// Script of the lyrics (`Mlym`, `Deva`…); the language's usual script when blank.
    pub script: String,
    /// The original lyrics. Leave blank to import the song without lyrics (it then has no public page).
    pub lyrics: String,
    /// The album, or the film. Found by title, otherwise created.
    pub album: String,
    /// `album` (default), `soundtrack` (a film), `single`, `ep` or `compilation`. Only used when the album is created.
    pub album_kind: String,
    pub year: Option<i32>,
    /// People, each found by name or alias, otherwise created.
    pub singers: Vec<String>,
    pub composers: Vec<String>,
    pub lyricists: Vec<String>,
    /// `mainstream` (default) or `devotional`.
    pub category: String,
    pub religion: String,
    pub deity: String,
    pub raga: String,
    pub devotional_form: String,
    pub source_work: String,
    pub occasions: Vec<String>,
    pub tags: Vec<String>,
    /// A YouTube link in any common form, or a video id.
    pub youtube: String,
}

impl From<ImportRow> for uc::Row {
    fn from(r: ImportRow) -> Self {
        Self {
            title: r.title,
            language: r.language,
            script: r.script,
            lyrics: r.lyrics,
            album: r.album,
            album_kind: r.album_kind,
            year: r.year,
            singers: r.singers,
            composers: r.composers,
            lyricists: r.lyricists,
            category: r.category,
            religion: r.religion,
            deity: r.deity,
            raga: r.raga,
            devotional_form: r.devotional_form,
            source_work: r.source_work,
            occasions: r.occasions,
            tags: r.tags,
            youtube: r.youtube,
        }
    }
}

#[derive(Deserialize, ToSchema)]
pub struct ImportRequest {
    /// At most 100 rows per request.
    pub rows: Vec<ImportRow>,
    /// Check everything and report, but write nothing.
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Serialize, ToSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImportStatus {
    /// Dry run: it would be imported.
    WouldCreate,
    Created,
    /// Already in the catalogue; nothing was written.
    Skipped,
    Failed,
}

#[derive(Serialize, ToSchema)]
pub struct ImportProblem {
    /// The column, or `row` for a problem with the row as a whole.
    pub field: String,
    /// A stable code the web app turns into a message.
    pub code: String,
}

#[derive(Serialize, ToSchema)]
pub struct ImportRowResult {
    /// Position in the request, from 1.
    pub row: usize,
    pub title: String,
    pub status: ImportStatus,
    /// For a skipped row: `already_exists`.
    pub reason: Option<String>,
    pub problems: Vec<ImportProblem>,
    pub composition_id: Option<Uuid>,
}

#[derive(Serialize, ToSchema)]
pub struct ImportResponse {
    pub dry_run: bool,
    pub results: Vec<ImportRowResult>,
    pub created: usize,
    pub skipped: usize,
    pub failed: usize,
}

#[utoipa::path(post, path = "/api/v1/import/songs", operation_id = "import_songs", tag = "import",
    request_body = ImportRequest, security(("session" = [])), responses(
    (status = 200, body = ImportResponse, description = "One result per row, in order. A failed row does not stop the others."),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json", description = "Moderators and admins only."),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn import_songs(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireRole(p, ..): RequireRole<Moderator>,
    ApiJson(b): ApiJson<ImportRequest>,
) -> ApiResult<Json<ImportResponse>> {
    let report = uc::songs(&app, &p, &meta, b.rows.into_iter().map(Into::into).collect(), b.dry_run).await?;
    let results: Vec<ImportRowResult> = report
        .results
        .into_iter()
        .map(|r| ImportRowResult {
            row: r.row,
            title: r.title,
            status: match r.status {
                uc::Status::WouldCreate => ImportStatus::WouldCreate,
                uc::Status::Created => ImportStatus::Created,
                uc::Status::Skipped => ImportStatus::Skipped,
                uc::Status::Failed => ImportStatus::Failed,
            },
            reason: r.reason,
            problems: r
                .problems
                .into_iter()
                .map(|(field, code)| ImportProblem { field, code })
                .collect(),
            composition_id: r.composition_id,
        })
        .collect();
    let count = |s: ImportStatus| results.iter().filter(|r| r.status == s).count();
    Ok(Json(ImportResponse {
        dry_run: report.dry_run,
        created: count(ImportStatus::Created) + count(ImportStatus::WouldCreate),
        skipped: count(ImportStatus::Skipped),
        failed: count(ImportStatus::Failed),
        results,
    }))
}
