//! Comparison and community endpoints. Reads of public content are open; writes need a session.
//! Moderation endpoints take any signed-in user and let the use case decide, so a refused
//! attempt is recorded in the audit log (an extractor-level 403 would not be).
//! Responses depend on the caller or must reflect unpublishing at once: never cached.

use crate::extract::{Admin, ApiJson, ApiPath, ApiQuery, MaybeUser, Meta, RequireRole, RequireUser};
use crate::localization::Dir;
use crate::problem::{ApiResult, Problem};
use crate::routes::PageQuery;
use application::community::{self as uc, ReportInput, Resolution};
use application::comparison::{self as cmp, Panel};
use application::App;
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Utc};
use domain::community::CommunitySettings;
use infrastructure::community::{AppealRow, CommentRow, NoteRow, ReportRow, ReputationEventRow};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

// ---------- comparison ----------

/// The published text of one contribution, with enough context to label a panel.
#[derive(Serialize, ToSchema)]
pub struct PanelContent {
    pub recording_id: Uuid,
    pub recording_title: String,
    pub composition_id: Uuid,
    pub composition_title: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub language: String,
    pub script: String,
    pub dir: Dir,
    /// The slot's primary; any other contribution in the slot is an alternative.
    pub is_primary: bool,
    /// How many published contributions share this slot (more than one means alternatives exist).
    pub slot_size: i64,
    pub revision_id: Uuid,
    pub revision_no: i32,
    pub body: String,
    pub revised_at: DateTime<Utc>,
}

#[derive(Serialize, ToSchema)]
pub struct ResolvedPanel {
    /// The id that was asked for, echoed so panels line up with the request.
    pub id: Uuid,
    /// `false` when the contribution is not public (unknown, pending, rejected or withdrawn).
    pub available: bool,
    pub content: Option<PanelContent>,
}

#[derive(Serialize, ToSchema)]
pub struct ResolvedComparison {
    pub panels: Vec<ResolvedPanel>,
}

fn panel(p: Panel) -> ResolvedPanel {
    ResolvedPanel {
        id: p.id,
        available: p.content.is_some(),
        content: p.content.map(|r| PanelContent {
            recording_id: r.recording_id,
            recording_title: r.recording_title,
            composition_id: r.composition_id,
            composition_title: r.composition_title,
            kind: r.kind,
            language: r.language,
            script: r.script,
            dir: if r.script_rtl { Dir::Rtl } else { Dir::Ltr },
            is_primary: r.is_primary,
            slot_size: r.slot_size,
            revision_id: r.revision_id,
            revision_no: r.revision_no,
            body: r.body,
            revised_at: r.revised_at,
        }),
    }
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ResolveQuery {
    /// One to four contribution ids, comma-separated. The URL carries ids only, never lyric text.
    pub ids: String,
}

#[utoipa::path(get, path = "/api/v1/comparisons/resolve", tag = "comparison", params(ResolveQuery), responses(
    (status = 200, body = ResolvedComparison, description = "The current published text per id, in request order"),
    (status = 400, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn resolve(
    State(app): State<App>,
    ApiQuery(q): ApiQuery<ResolveQuery>,
) -> ApiResult<Json<ResolvedComparison>> {
    let panels = cmp::resolve(&app, &q.ids).await?;
    Ok(Json(ResolvedComparison {
        panels: panels.into_iter().map(panel).collect(),
    }))
}

#[derive(Deserialize, ToSchema)]
pub struct SaveComparison {
    pub name: String,
    /// One to four contribution ids that are public now.
    pub contribution_ids: Vec<Uuid>,
}

#[derive(Serialize, ToSchema)]
pub struct SavedComparison {
    pub id: Uuid,
    pub name: String,
    pub contribution_ids: Vec<Uuid>,
    pub created_at: DateTime<Utc>,
    /// Resolved from the current published revisions; omitted in listings.
    pub panels: Option<Vec<ResolvedPanel>>,
}

#[derive(Serialize, ToSchema)]
pub struct SavedComparisonPage {
    pub items: Vec<SavedComparison>,
    pub next_cursor: Option<String>,
}

fn saved(s: cmp::Saved) -> SavedComparison {
    SavedComparison {
        id: s.row.id,
        name: s.row.name,
        contribution_ids: s.row.contribution_ids,
        created_at: s.row.created_at,
        panels: Some(s.panels.into_iter().map(panel).collect()),
    }
}

#[utoipa::path(post, path = "/api/v1/me/comparisons", tag = "comparison", request_body = SaveComparison,
    security(("session" = [])), responses(
    (status = 201, body = SavedComparison),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json"),
    (status = 429, body = Problem, content_type = "application/problem+json")))]
pub async fn save_comparison(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireUser(p): RequireUser,
    ApiJson(b): ApiJson<SaveComparison>,
) -> ApiResult<(StatusCode, Json<SavedComparison>)> {
    Ok((
        StatusCode::CREATED,
        Json(saved(cmp::save(&app, &p, &meta, &b.name, &b.contribution_ids).await?)),
    ))
}

#[utoipa::path(get, path = "/api/v1/me/comparisons", tag = "comparison", params(PageQuery),
    security(("session" = [])), responses(
    (status = 200, body = SavedComparisonPage),
    (status = 401, body = Problem, content_type = "application/problem+json")))]
pub async fn list_saved(
    State(app): State<App>,
    RequireUser(p): RequireUser,
    ApiQuery(q): ApiQuery<PageQuery>,
) -> ApiResult<Json<SavedComparisonPage>> {
    let page = cmp::list_saved(&app, &p, q.cursor.as_deref(), q.limit).await?;
    Ok(Json(SavedComparisonPage {
        items: page
            .items
            .into_iter()
            .map(|r| SavedComparison {
                id: r.id,
                name: r.name,
                contribution_ids: r.contribution_ids,
                created_at: r.created_at,
                panels: None,
            })
            .collect(),
        next_cursor: page.next_cursor,
    }))
}

#[utoipa::path(get, path = "/api/v1/me/comparisons/{id}", tag = "comparison", params(("id" = Uuid, Path)),
    security(("session" = [])), responses(
    (status = 200, body = SavedComparison, description = "Resolved against what is published now"),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 404, body = Problem, content_type = "application/problem+json")))]
pub async fn get_saved(
    State(app): State<App>,
    RequireUser(p): RequireUser,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<Json<SavedComparison>> {
    Ok(Json(saved(cmp::get_saved(&app, &p, id).await?)))
}

#[utoipa::path(delete, path = "/api/v1/me/comparisons/{id}", tag = "comparison", params(("id" = Uuid, Path)),
    security(("session" = [])), responses(
    (status = 204),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 404, body = Problem, content_type = "application/problem+json")))]
pub async fn delete_saved(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireUser(p): RequireUser,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<StatusCode> {
    cmp::delete_saved(&app, &p, &meta, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- engagement ----------

#[derive(Serialize, ToSchema)]
pub struct ReactionCount {
    pub kind: String,
    pub count: i64,
}

#[derive(Serialize, ToSchema)]
pub struct MyEngagement {
    pub rating: Option<i16>,
    pub reactions: Vec<String>,
    pub bookmarked: bool,
}

#[derive(Serialize, ToSchema)]
pub struct Engagement {
    pub rating_count: i64,
    pub rating_average: Option<f64>,
    pub comment_count: i64,
    pub reactions: Vec<ReactionCount>,
    /// Present for signed-in callers only.
    pub mine: Option<MyEngagement>,
}

#[utoipa::path(get, path = "/api/v1/contributions/{id}/engagement", tag = "community", params(("id" = Uuid, Path)),
    responses((status = 200, body = Engagement), (status = 404, body = Problem, content_type = "application/problem+json")))]
pub async fn engagement(
    State(app): State<App>,
    MaybeUser(viewer): MaybeUser,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<Json<Engagement>> {
    let e = uc::engagement(&app, viewer.as_ref(), id).await?;
    Ok(Json(Engagement {
        rating_count: e.summary.rating_count,
        rating_average: e.summary.rating_avg,
        comment_count: e.summary.comment_count,
        reactions: e
            .reactions
            .into_iter()
            .map(|(kind, count)| ReactionCount { kind, count })
            .collect(),
        mine: e.mine.map(|m| MyEngagement {
            rating: m.rating,
            reactions: m.reactions,
            bookmarked: m.bookmarked,
        }),
    }))
}

#[derive(Deserialize, ToSchema)]
pub struct RatingBody {
    /// 1–5. You cannot rate a contribution you wrote or revised.
    pub value: i64,
}

#[utoipa::path(put, path = "/api/v1/contributions/{id}/rating", tag = "community", params(("id" = Uuid, Path)),
    request_body = RatingBody, security(("session" = [])), responses(
    (status = 204),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json"),
    (status = 404, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json", description = "`self_rating` or out of range"),
    (status = 429, body = Problem, content_type = "application/problem+json")))]
pub async fn put_rating(
    State(app): State<App>,
    RequireUser(p): RequireUser,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(b): ApiJson<RatingBody>,
) -> ApiResult<StatusCode> {
    uc::rate(&app, &p, id, b.value).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(delete, path = "/api/v1/contributions/{id}/rating", tag = "community", params(("id" = Uuid, Path)),
    security(("session" = [])), responses(
    (status = 204),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 404, body = Problem, content_type = "application/problem+json")))]
pub async fn delete_rating(
    State(app): State<App>,
    RequireUser(p): RequireUser,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<StatusCode> {
    uc::unrate(&app, &p, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(put, path = "/api/v1/contributions/{id}/reactions/{kind}", tag = "community",
    params(("id" = Uuid, Path), ("kind" = String, Path, description = "like, moving or insightful")),
    security(("session" = [])), responses(
    (status = 204),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json"),
    (status = 404, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json"),
    (status = 429, body = Problem, content_type = "application/problem+json")))]
pub async fn put_reaction(
    State(app): State<App>,
    RequireUser(p): RequireUser,
    ApiPath((id, kind)): ApiPath<(Uuid, String)>,
) -> ApiResult<StatusCode> {
    uc::react(&app, &p, id, &kind, true).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(delete, path = "/api/v1/contributions/{id}/reactions/{kind}", tag = "community",
    params(("id" = Uuid, Path), ("kind" = String, Path)), security(("session" = [])), responses(
    (status = 204),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 404, body = Problem, content_type = "application/problem+json")))]
pub async fn delete_reaction(
    State(app): State<App>,
    RequireUser(p): RequireUser,
    ApiPath((id, kind)): ApiPath<(Uuid, String)>,
) -> ApiResult<StatusCode> {
    uc::react(&app, &p, id, &kind, false).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(put, path = "/api/v1/contributions/{id}/bookmark", tag = "community", params(("id" = Uuid, Path)),
    security(("session" = [])), responses(
    (status = 204),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 404, body = Problem, content_type = "application/problem+json"),
    (status = 429, body = Problem, content_type = "application/problem+json")))]
pub async fn put_bookmark(
    State(app): State<App>,
    RequireUser(p): RequireUser,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<StatusCode> {
    uc::bookmark(&app, &p, id, true).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(delete, path = "/api/v1/contributions/{id}/bookmark", tag = "community", params(("id" = Uuid, Path)),
    security(("session" = [])), responses(
    (status = 204),
    (status = 401, body = Problem, content_type = "application/problem+json")))]
pub async fn delete_bookmark(
    State(app): State<App>,
    RequireUser(p): RequireUser,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<StatusCode> {
    uc::bookmark(&app, &p, id, false).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, ToSchema)]
pub struct Bookmark {
    pub contribution_id: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize, ToSchema)]
pub struct BookmarkPage {
    pub items: Vec<Bookmark>,
    pub next_cursor: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/me/bookmarks", tag = "community", params(PageQuery), security(("session" = [])), responses(
    (status = 200, body = BookmarkPage, description = "Only contributions that are still public"),
    (status = 401, body = Problem, content_type = "application/problem+json")))]
pub async fn my_bookmarks(
    State(app): State<App>,
    RequireUser(p): RequireUser,
    ApiQuery(q): ApiQuery<PageQuery>,
) -> ApiResult<Json<BookmarkPage>> {
    let page = uc::list_bookmarks(&app, &p, q.cursor.as_deref(), q.limit).await?;
    Ok(Json(BookmarkPage {
        items: page
            .items
            .into_iter()
            .map(|(contribution_id, created_at)| Bookmark {
                contribution_id,
                created_at,
            })
            .collect(),
        next_cursor: page.next_cursor,
    }))
}

// ---------- comments ----------

#[derive(Serialize, ToSchema)]
pub struct Comment {
    pub id: Uuid,
    pub contribution_id: Uuid,
    pub author_id: Uuid,
    pub author_name: String,
    pub body: String,
    /// `visible` in public lists; moderators and authors see `hidden` / `removed` on direct responses.
    pub status: String,
    pub version: i32,
    pub created_at: DateTime<Utc>,
    pub edited_at: Option<DateTime<Utc>>,
}

fn comment(c: CommentRow) -> Comment {
    Comment {
        id: c.id,
        contribution_id: c.contribution_id,
        author_id: c.author_id,
        author_name: c.author_name,
        body: c.body,
        status: c.status,
        version: c.version,
        created_at: c.created_at,
        edited_at: c.edited_at,
    }
}

#[derive(Serialize, ToSchema)]
pub struct CommentPage {
    pub items: Vec<Comment>,
    pub next_cursor: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct CommentBody {
    pub body: String,
}

#[derive(Deserialize, ToSchema)]
pub struct CommentEdit {
    pub expected_version: i32,
    pub body: String,
}

#[utoipa::path(get, path = "/api/v1/contributions/{id}/comments", tag = "community", params(("id" = Uuid, Path), PageQuery),
    responses((status = 200, body = CommentPage), (status = 404, body = Problem, content_type = "application/problem+json")))]
pub async fn list_comments(
    State(app): State<App>,
    ApiPath(id): ApiPath<Uuid>,
    ApiQuery(q): ApiQuery<PageQuery>,
) -> ApiResult<Json<CommentPage>> {
    let page = uc::list_comments(&app, id, q.cursor.as_deref(), q.limit).await?;
    Ok(Json(CommentPage {
        items: page.items.into_iter().map(comment).collect(),
        next_cursor: page.next_cursor,
    }))
}

#[utoipa::path(post, path = "/api/v1/contributions/{id}/comments", tag = "community", params(("id" = Uuid, Path)),
    request_body = CommentBody, security(("session" = [])), responses(
    (status = 201, body = Comment),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json"),
    (status = 404, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json", description = "Too long, empty, or flagged as spam"),
    (status = 429, body = Problem, content_type = "application/problem+json")))]
pub async fn create_comment(
    State(app): State<App>,
    RequireUser(p): RequireUser,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(b): ApiJson<CommentBody>,
) -> ApiResult<(StatusCode, Json<Comment>)> {
    Ok((
        StatusCode::CREATED,
        Json(comment(uc::create_comment(&app, &p, id, &b.body).await?)),
    ))
}

#[utoipa::path(get, path = "/api/v1/me/comments", tag = "community", params(PageQuery), security(("session" = [])), responses(
    (status = 200, body = CommentPage, description = "Your comments, including hidden ones (so they can be appealed)"),
    (status = 401, body = Problem, content_type = "application/problem+json")))]
pub async fn my_comments(
    State(app): State<App>,
    RequireUser(p): RequireUser,
    ApiQuery(q): ApiQuery<PageQuery>,
) -> ApiResult<Json<CommentPage>> {
    let page = uc::my_comments(&app, &p, q.cursor.as_deref(), q.limit).await?;
    Ok(Json(CommentPage {
        items: page.items.into_iter().map(comment).collect(),
        next_cursor: page.next_cursor,
    }))
}

#[utoipa::path(patch, path = "/api/v1/comments/{id}", tag = "community", params(("id" = Uuid, Path)),
    request_body = CommentEdit, security(("session" = [])), responses(
    (status = 200, body = Comment),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json"),
    (status = 404, body = Problem, content_type = "application/problem+json"),
    (status = 409, body = Problem, content_type = "application/problem+json", description = "`version_conflict`"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn edit_comment(
    State(app): State<App>,
    RequireUser(p): RequireUser,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(b): ApiJson<CommentEdit>,
) -> ApiResult<Json<Comment>> {
    Ok(Json(comment(
        uc::edit_comment(&app, &p, id, b.expected_version, &b.body).await?,
    )))
}

#[utoipa::path(delete, path = "/api/v1/comments/{id}", tag = "community", params(("id" = Uuid, Path)),
    security(("session" = [])), responses(
    (status = 204),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 404, body = Problem, content_type = "application/problem+json")))]
pub async fn remove_comment(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireUser(p): RequireUser,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<StatusCode> {
    uc::remove_own_comment(&app, &p, &meta, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- profiles and reputation ----------

#[derive(Serialize, ToSchema)]
pub struct Profile {
    pub user_id: Uuid,
    pub display_name: String,
    pub bio: String,
    pub joined_at: DateTime<Utc>,
    pub published_contributions: i64,
    pub comments: i64,
    /// Sum of the reputation event log.
    pub reputation: i64,
    /// A label for the score; it grants nothing by itself.
    pub level: String,
    /// Roles held, listed separately and only ever granted explicitly (`moderator`, `admin`).
    pub roles: Vec<String>,
}

fn profile(p: uc::Profile) -> Profile {
    Profile {
        user_id: p.row.user_id,
        display_name: p.row.display_name,
        bio: p.row.bio,
        joined_at: p.row.joined_at,
        published_contributions: p.row.published_contributions,
        comments: p.row.comments,
        reputation: p.row.reputation,
        level: p.level,
        roles: p.roles.iter().map(|r| r.as_str().to_owned()).collect(),
    }
}

#[derive(Deserialize, ToSchema)]
pub struct ProfileUpdate {
    pub bio: String,
}

#[utoipa::path(get, path = "/api/v1/users/{id}/profile", tag = "community", params(("id" = Uuid, Path)),
    responses((status = 200, body = Profile), (status = 404, body = Problem, content_type = "application/problem+json")))]
pub async fn get_profile(State(app): State<App>, ApiPath(id): ApiPath<Uuid>) -> ApiResult<Json<Profile>> {
    Ok(Json(profile(uc::get_profile(&app, id).await?)))
}

#[utoipa::path(put, path = "/api/v1/me/profile", tag = "community", request_body = ProfileUpdate, security(("session" = [])), responses(
    (status = 200, body = Profile),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn put_profile(
    State(app): State<App>,
    RequireUser(p): RequireUser,
    ApiJson(b): ApiJson<ProfileUpdate>,
) -> ApiResult<Json<Profile>> {
    Ok(Json(profile(uc::set_bio(&app, &p, &b.bio).await?)))
}

#[derive(Serialize, ToSchema)]
pub struct ReputationEvent {
    pub id: Uuid,
    pub kind: String,
    pub points: i32,
    pub subject_type: String,
    pub subject_id: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize, ToSchema)]
pub struct ReputationPage {
    pub score: i64,
    pub level: String,
    pub items: Vec<ReputationEvent>,
    pub next_cursor: Option<String>,
}

fn rep_event(e: ReputationEventRow) -> ReputationEvent {
    ReputationEvent {
        id: e.id,
        kind: e.kind,
        points: e.points,
        subject_type: e.subject_type,
        subject_id: e.subject_id,
        created_at: e.created_at,
    }
}

#[utoipa::path(get, path = "/api/v1/me/reputation", tag = "community", params(PageQuery), security(("session" = [])), responses(
    (status = 200, body = ReputationPage),
    (status = 401, body = Problem, content_type = "application/problem+json")))]
pub async fn my_reputation(
    State(app): State<App>,
    RequireUser(p): RequireUser,
    ApiQuery(q): ApiQuery<PageQuery>,
) -> ApiResult<Json<ReputationPage>> {
    let r = uc::my_reputation(&app, &p).await?;
    let page = uc::my_reputation_events(&app, &p, q.cursor.as_deref(), q.limit).await?;
    Ok(Json(ReputationPage {
        score: r.score,
        level: r.level,
        items: page.items.into_iter().map(rep_event).collect(),
        next_cursor: page.next_cursor,
    }))
}

// ---------- reports ----------

#[derive(Serialize, ToSchema)]
pub struct Report {
    pub id: Uuid,
    pub reporter_id: Uuid,
    /// `contribution` or `comment`.
    pub target_type: String,
    pub target_id: Uuid,
    pub reason: String,
    pub detail: Option<String>,
    /// `open`, `upheld` or `dismissed`.
    pub status: String,
    pub resolved_by: Option<Uuid>,
    pub resolution_note: Option<String>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

fn report(r: ReportRow) -> Report {
    Report {
        id: r.id,
        reporter_id: r.reporter_id,
        target_type: r.target_type,
        target_id: r.target_id,
        reason: r.reason,
        detail: r.detail,
        status: r.status,
        resolved_by: r.resolved_by,
        resolution_note: r.resolution_note,
        resolved_at: r.resolved_at,
        created_at: r.created_at,
    }
}

#[derive(Serialize, ToSchema)]
pub struct ReportPage {
    pub items: Vec<Report>,
    pub next_cursor: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct ReportCreate {
    /// `contribution` or `comment`.
    pub target_type: String,
    pub target_id: Uuid,
    /// `spam`, `abuse`, `copyright`, `inaccurate` or `other`.
    pub reason: String,
    pub detail: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/reports", tag = "moderation", request_body = ReportCreate, security(("session" = [])), responses(
    (status = 201, body = Report),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json"),
    (status = 404, body = Problem, content_type = "application/problem+json"),
    (status = 409, body = Problem, content_type = "application/problem+json", description = "`report_exists`"),
    (status = 422, body = Problem, content_type = "application/problem+json"),
    (status = 429, body = Problem, content_type = "application/problem+json")))]
pub async fn create_report(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireUser(p): RequireUser,
    ApiJson(b): ApiJson<ReportCreate>,
) -> ApiResult<(StatusCode, Json<Report>)> {
    let r = uc::report(
        &app,
        &p,
        &meta,
        ReportInput {
            target_type: b.target_type,
            target_id: b.target_id,
            reason: b.reason,
            detail: b.detail,
        },
    )
    .await?;
    Ok((StatusCode::CREATED, Json(report(r))))
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ReportQueueQuery {
    pub cursor: Option<String>,
    pub limit: Option<i64>,
    /// `open` (default), `upheld` or `dismissed`.
    pub status: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/moderation/reports", tag = "moderation", params(ReportQueueQuery), security(("session" = [])), responses(
    (status = 200, body = ReportPage),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json", description = "Refusals are audited")))]
pub async fn report_queue(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireUser(p): RequireUser,
    ApiQuery(q): ApiQuery<ReportQueueQuery>,
) -> ApiResult<Json<ReportPage>> {
    let page = uc::report_queue(&app, &p, &meta, q.status.as_deref(), q.cursor.as_deref(), q.limit).await?;
    Ok(Json(ReportPage {
        items: page.items.into_iter().map(report).collect(),
        next_cursor: page.next_cursor,
    }))
}

#[derive(Deserialize, ToSchema)]
pub struct ResolveReport {
    /// `upheld` or `dismissed`.
    pub resolution: String,
    /// `none` (default) or `hide_comment` (only with `upheld`, on a comment report).
    pub action: Option<String>,
    pub note: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/moderation/reports/{id}/resolve", tag = "moderation", params(("id" = Uuid, Path)),
    request_body = ResolveReport, security(("session" = [])), responses(
    (status = 200, body = Report),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json", description = "Refusals are audited"),
    (status = 404, body = Problem, content_type = "application/problem+json"),
    (status = 409, body = Problem, content_type = "application/problem+json", description = "`already_resolved`"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn resolve_report(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireUser(p): RequireUser,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(b): ApiJson<ResolveReport>,
) -> ApiResult<Json<Report>> {
    let upheld = match b.resolution.as_str() {
        "upheld" => true,
        "dismissed" => false,
        _ => {
            return Err(
                application::AppError::from(domain::DomainError::invalid("resolution", "invalid_choice")).into(),
            )
        }
    };
    let hide_comment = match b.action.as_deref().unwrap_or("none") {
        "none" => false,
        "hide_comment" => true,
        _ => return Err(application::AppError::from(domain::DomainError::invalid("action", "invalid_choice")).into()),
    };
    Ok(Json(report(
        uc::resolve_report(
            &app,
            &p,
            &meta,
            id,
            Resolution {
                upheld,
                hide_comment,
                note: b.note,
            },
        )
        .await?,
    )))
}

#[utoipa::path(post, path = "/api/v1/moderation/comments/{id}/hide", tag = "moderation", params(("id" = Uuid, Path)),
    request_body = ModNote, security(("session" = [])), responses(
    (status = 200, body = Comment),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json", description = "Refusals are audited"),
    (status = 404, body = Problem, content_type = "application/problem+json"),
    (status = 409, body = Problem, content_type = "application/problem+json")))]
pub async fn hide_comment(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireUser(p): RequireUser,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(b): ApiJson<ModNote>,
) -> ApiResult<Json<Comment>> {
    Ok(Json(comment(
        uc::hide_comment(&app, &p, &meta, id, b.note.as_deref()).await?,
    )))
}

#[derive(Deserialize, ToSchema)]
pub struct ModNote {
    pub note: Option<String>,
}

// ---------- moderator notes ----------

#[derive(Serialize, ToSchema)]
pub struct Note {
    pub id: Uuid,
    pub subject_type: String,
    pub subject_id: Uuid,
    pub author_id: Uuid,
    pub body: String,
    pub created_at: DateTime<Utc>,
}

fn note(n: NoteRow) -> Note {
    Note {
        id: n.id,
        subject_type: n.subject_type,
        subject_id: n.subject_id,
        author_id: n.author_id,
        body: n.body,
        created_at: n.created_at,
    }
}

#[derive(Serialize, ToSchema)]
pub struct NotePage {
    pub items: Vec<Note>,
    pub next_cursor: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct NoteCreate {
    /// `contribution`, `comment`, `user` or `report`.
    pub subject_type: String,
    pub subject_id: Uuid,
    pub body: String,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct NoteQuery {
    pub subject_type: String,
    pub subject_id: Uuid,
    pub cursor: Option<String>,
    pub limit: Option<i64>,
}

#[utoipa::path(post, path = "/api/v1/moderation/notes", tag = "moderation", request_body = NoteCreate, security(("session" = [])), responses(
    (status = 201, body = Note),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json", description = "Refusals are audited"),
    (status = 404, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn create_note(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireUser(p): RequireUser,
    ApiJson(b): ApiJson<NoteCreate>,
) -> ApiResult<(StatusCode, Json<Note>)> {
    Ok((
        StatusCode::CREATED,
        Json(note(
            uc::add_note(&app, &p, &meta, &b.subject_type, b.subject_id, &b.body).await?,
        )),
    ))
}

#[utoipa::path(get, path = "/api/v1/moderation/notes", tag = "moderation", params(NoteQuery), security(("session" = [])), responses(
    (status = 200, body = NotePage),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json", description = "Refusals are audited")))]
pub async fn list_notes(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireUser(p): RequireUser,
    ApiQuery(q): ApiQuery<NoteQuery>,
) -> ApiResult<Json<NotePage>> {
    let page = uc::list_notes(
        &app,
        &p,
        &meta,
        &q.subject_type,
        q.subject_id,
        q.cursor.as_deref(),
        q.limit,
    )
    .await?;
    Ok(Json(NotePage {
        items: page.items.into_iter().map(note).collect(),
        next_cursor: page.next_cursor,
    }))
}

// ---------- appeals ----------

#[derive(Serialize, ToSchema)]
pub struct Appeal {
    pub id: Uuid,
    pub appellant_id: Uuid,
    pub target_type: String,
    pub target_id: Uuid,
    pub message: String,
    /// `open`, `upheld` or `denied`.
    pub status: String,
    pub decided_by: Option<Uuid>,
    pub decision_note: Option<String>,
    pub decided_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

fn appeal(a: AppealRow) -> Appeal {
    Appeal {
        id: a.id,
        appellant_id: a.appellant_id,
        target_type: a.target_type,
        target_id: a.target_id,
        message: a.message,
        status: a.status,
        decided_by: a.decided_by,
        decision_note: a.decision_note,
        decided_at: a.decided_at,
        created_at: a.created_at,
    }
}

#[derive(Serialize, ToSchema)]
pub struct AppealPage {
    pub items: Vec<Appeal>,
    pub next_cursor: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct AppealCreate {
    /// The hidden comment being appealed; only its author may appeal.
    pub comment_id: Uuid,
    pub message: String,
}

#[derive(Deserialize, ToSchema)]
pub struct AppealDecision {
    /// `upheld` restores the comment; `denied` leaves it hidden.
    pub decision: String,
    pub note: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/appeals", tag = "moderation", request_body = AppealCreate, security(("session" = [])), responses(
    (status = 201, body = Appeal),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 404, body = Problem, content_type = "application/problem+json"),
    (status = 409, body = Problem, content_type = "application/problem+json", description = "`appeal_pending` or `nothing_to_appeal`"),
    (status = 422, body = Problem, content_type = "application/problem+json"),
    (status = 429, body = Problem, content_type = "application/problem+json")))]
pub async fn create_appeal(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireUser(p): RequireUser,
    ApiJson(b): ApiJson<AppealCreate>,
) -> ApiResult<(StatusCode, Json<Appeal>)> {
    Ok((
        StatusCode::CREATED,
        Json(appeal(uc::appeal(&app, &p, &meta, b.comment_id, &b.message).await?)),
    ))
}

#[utoipa::path(get, path = "/api/v1/me/appeals", tag = "moderation", params(PageQuery), security(("session" = [])), responses(
    (status = 200, body = AppealPage),
    (status = 401, body = Problem, content_type = "application/problem+json")))]
pub async fn my_appeals(
    State(app): State<App>,
    RequireUser(p): RequireUser,
    ApiQuery(q): ApiQuery<PageQuery>,
) -> ApiResult<Json<AppealPage>> {
    let page = uc::my_appeals(&app, &p, q.cursor.as_deref(), q.limit).await?;
    Ok(Json(AppealPage {
        items: page.items.into_iter().map(appeal).collect(),
        next_cursor: page.next_cursor,
    }))
}

#[utoipa::path(get, path = "/api/v1/moderation/appeals", tag = "moderation", params(PageQuery), security(("session" = [])), responses(
    (status = 200, body = AppealPage),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json", description = "Refusals are audited")))]
pub async fn appeal_queue(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireUser(p): RequireUser,
    ApiQuery(q): ApiQuery<PageQuery>,
) -> ApiResult<Json<AppealPage>> {
    let page = uc::appeal_queue(&app, &p, &meta, q.cursor.as_deref(), q.limit).await?;
    Ok(Json(AppealPage {
        items: page.items.into_iter().map(appeal).collect(),
        next_cursor: page.next_cursor,
    }))
}

#[utoipa::path(post, path = "/api/v1/moderation/appeals/{id}/decide", tag = "moderation", params(("id" = Uuid, Path)),
    request_body = AppealDecision, security(("session" = [])), responses(
    (status = 200, body = Appeal),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json", description = "Not a moderator (audited) or `own_decision`"),
    (status = 404, body = Problem, content_type = "application/problem+json"),
    (status = 409, body = Problem, content_type = "application/problem+json", description = "`already_resolved`"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn decide_appeal(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireUser(p): RequireUser,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(b): ApiJson<AppealDecision>,
) -> ApiResult<Json<Appeal>> {
    let uphold = match b.decision.as_str() {
        "upheld" => true,
        "denied" => false,
        _ => {
            return Err(application::AppError::from(domain::DomainError::invalid("decision", "invalid_choice")).into())
        }
    };
    Ok(Json(appeal(
        uc::decide_appeal(&app, &p, &meta, id, uphold, b.note.as_deref()).await?,
    )))
}

// ---------- admin settings ----------

#[utoipa::path(get, path = "/api/v1/admin/community-settings", tag = "admin", security(("session" = [])), responses(
    (status = 200, body = CommunitySettingsDoc),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json")))]
pub async fn get_settings(
    State(app): State<App>,
    RequireRole(p, ..): RequireRole<Admin>,
) -> ApiResult<Json<CommunitySettings>> {
    Ok(Json(uc::get_settings(&app, &p).await?))
}

#[utoipa::path(put, path = "/api/v1/admin/community-settings", tag = "admin", request_body = CommunitySettingsDoc, security(("session" = [])), responses(
    (status = 200, body = CommunitySettingsDoc),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn put_settings(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireRole(p, ..): RequireRole<Admin>,
    ApiJson(b): ApiJson<CommunitySettings>,
) -> ApiResult<Json<CommunitySettings>> {
    Ok(Json(uc::set_settings(&app, &p, &meta, b).await?))
}

/// Tunable community rules: reputation points and thresholds, text limits, rate limits, spam heuristics.
#[derive(Serialize, Deserialize, ToSchema)]
#[allow(dead_code)]
pub struct CommunitySettingsDoc {
    /// Event kind → points.
    pub points: std::collections::BTreeMap<String, i64>,
    /// Action (`rate`, `react`, `comment`, `report`, `appeal`) → minimum reputation.
    pub min_reputation: std::collections::BTreeMap<String, i64>,
    pub levels: Vec<LevelDoc>,
    pub limits: LimitsDoc,
    /// Action → `{max, window_secs}`.
    pub rate: std::collections::BTreeMap<String, RateDoc>,
    pub spam: SpamDoc,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[allow(dead_code)]
pub struct LevelDoc {
    pub name: String,
    pub min: i64,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[allow(dead_code)]
pub struct LimitsDoc {
    pub comment_chars: usize,
    pub bio_chars: usize,
    pub report_detail_chars: usize,
    pub appeal_chars: usize,
    pub note_chars: usize,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[allow(dead_code)]
pub struct RateDoc {
    pub max: u32,
    pub window_secs: u32,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[allow(dead_code)]
pub struct SpamDoc {
    pub max_links: usize,
    pub max_repeat_run: usize,
    pub duplicate_window_secs: u32,
    pub blocked_terms: Vec<String>,
}
