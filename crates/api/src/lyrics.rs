//! Lyric contribution endpoints. The public reads published revisions only; the author and
//! moderators also see the head revision and its review state. Responses are never cached
//! (`Cache-Control: private, no-store`) because their content depends on who is asking.

use crate::extract::{Admin, ApiJson, ApiPath, ApiQuery, MaybeUser, Meta, Moderator, RequireRole, RequireUser};
use crate::localization::Dir;
use crate::problem::{ApiResult, Problem};
use crate::routes::PageQuery;
use application::lyrics::{self as uc, ContributionView, Outcome, Verdict};
use application::{App, AppError};
use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use domain::lyrics::{Hunk, Op, TrustPolicy};
use infrastructure::lyrics::{ContributionRow, RevisionRow};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

#[derive(Serialize, ToSchema)]
#[allow(dead_code)]
#[serde(rename_all = "snake_case")]
pub enum ContributionType {
    Original,
    Translation,
    Transliteration,
}

#[derive(Serialize, ToSchema)]
#[allow(dead_code)]
#[serde(rename_all = "snake_case")]
pub enum ContributionStatus {
    Pending,
    Approved,
    Published,
    Rejected,
}

/// One lyric version in a slot (recording + type + language + script). Several contributions
/// may share a slot as alternatives; at most one is the primary.
#[derive(Serialize, ToSchema)]
pub struct ContributionSummary {
    pub id: Uuid,
    pub recording_id: Uuid,
    #[serde(rename = "type")]
    pub kind: ContributionType,
    /// Language tag of the lyrics (`ml`, `ar`).
    pub language: String,
    /// ISO 15924 script code (`Mlym`, `Arab`). Language and script are separate on purpose.
    pub script: String,
    /// Text direction of the script; set it on the element that renders the body.
    pub dir: Dir,
    pub is_primary: bool,
    /// Review state of the newest revision. The public always sees `published`.
    pub status: ContributionStatus,
    /// The translated or transliterated original, when it is public.
    pub source_contribution_id: Option<Uuid>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

fn status_of(s: &str) -> ContributionStatus {
    match s {
        "approved" => ContributionStatus::Approved,
        "published" => ContributionStatus::Published,
        "rejected" => ContributionStatus::Rejected,
        _ => ContributionStatus::Pending,
    }
}

fn kind_of(s: &str) -> ContributionType {
    match s {
        "translation" => ContributionType::Translation,
        "transliteration" => ContributionType::Transliteration,
        _ => ContributionType::Original,
    }
}

/// `public` hides the review state of heads the public cannot see.
fn summary(r: ContributionRow, public: bool) -> ContributionSummary {
    ContributionSummary {
        id: r.id,
        recording_id: r.recording_id,
        kind: kind_of(&r.kind),
        language: r.language,
        script: r.script,
        dir: if r.script_rtl { Dir::Rtl } else { Dir::Ltr },
        is_primary: r.is_primary,
        status: if public {
            ContributionStatus::Published
        } else {
            status_of(&r.status)
        },
        source_contribution_id: r.source_contribution_id,
        created_by: r.created_by,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }
}

#[derive(Serialize, ToSchema)]
pub struct Revision {
    pub id: Uuid,
    pub contribution_id: Uuid,
    pub revision_no: i32,
    pub parent_revision_id: Option<Uuid>,
    pub body: String,
    pub note: Option<String>,
    /// Set when this revision restored an older one.
    pub restored_from_revision_id: Option<Uuid>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    /// Was this revision ever public?
    pub published: bool,
}

impl From<RevisionRow> for Revision {
    fn from(r: RevisionRow) -> Self {
        Self {
            id: r.id,
            contribution_id: r.contribution_id,
            revision_no: r.revision_no,
            parent_revision_id: r.parent_revision_id,
            body: r.body,
            note: r.note,
            restored_from_revision_id: r.restored_from_revision_id,
            created_by: r.created_by,
            created_at: r.created_at,
            published: r.published,
        }
    }
}

/// A revision without its body, for history lists.
#[derive(Serialize, ToSchema)]
pub struct RevisionSummary {
    pub id: Uuid,
    pub contribution_id: Uuid,
    pub revision_no: i32,
    pub parent_revision_id: Option<Uuid>,
    pub note: Option<String>,
    pub restored_from_revision_id: Option<Uuid>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub published: bool,
    /// Length of the body in characters.
    pub size: usize,
}

impl From<RevisionRow> for RevisionSummary {
    fn from(r: RevisionRow) -> Self {
        Self {
            size: r.body.chars().count(),
            id: r.id,
            contribution_id: r.contribution_id,
            revision_no: r.revision_no,
            parent_revision_id: r.parent_revision_id,
            note: r.note,
            restored_from_revision_id: r.restored_from_revision_id,
            created_by: r.created_by,
            created_at: r.created_at,
            published: r.published,
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct ContributionDetail {
    #[serde(flatten)]
    pub contribution: ContributionSummary,
    /// The public revision: what everyone reads. Null until something is published.
    pub published: Option<Revision>,
    /// Newest revision, which may be unpublished. Author and moderators only.
    pub head: Option<Revision>,
    /// Send this as `expected_parent_revision_id` when editing or deciding. Author and moderators only.
    pub current_revision_id: Option<Uuid>,
    /// The head differs from what is public. Author and moderators only.
    pub has_unpublished_changes: bool,
    pub review_note: Option<String>,
}

impl From<ContributionView> for ContributionDetail {
    fn from(v: ContributionView) -> Self {
        let private = v.privileged;
        let changes = private && v.row.current_revision_id != v.row.published_revision_id;
        Self {
            current_revision_id: v.row.current_revision_id.filter(|_| private),
            review_note: v.row.review_note.clone().filter(|_| private),
            has_unpublished_changes: changes,
            published: v.published.map(Into::into),
            head: v.head.map(Into::into),
            contribution: summary(v.row, !private),
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct ContributionPage {
    pub items: Vec<ContributionSummary>,
    pub next_cursor: Option<String>,
}

fn contribution_page(p: application::pagination::Page<ContributionRow>, public: bool) -> ContributionPage {
    ContributionPage {
        items: p.items.into_iter().map(|r| summary(r, public)).collect(),
        next_cursor: p.next_cursor,
    }
}

#[derive(Serialize, ToSchema)]
pub struct RevisionPage {
    pub items: Vec<RevisionSummary>,
    pub next_cursor: Option<String>,
}

#[derive(Serialize, ToSchema)]
#[allow(dead_code)]
#[serde(rename_all = "snake_case")]
pub enum DiffOp {
    Equal,
    Insert,
    Delete,
}

#[derive(Serialize, ToSchema)]
pub struct DiffHunk {
    pub op: DiffOp,
    pub lines: Vec<String>,
}

#[derive(Serialize, ToSchema)]
pub struct RevisionDiff {
    pub from: RevisionSummary,
    pub to: RevisionSummary,
    pub hunks: Vec<DiffHunk>,
}

fn hunk(h: Hunk) -> DiffHunk {
    DiffHunk {
        op: match h.op {
            Op::Equal => DiffOp::Equal,
            Op::Insert => DiffOp::Insert,
            Op::Delete => DiffOp::Delete,
        },
        lines: h.lines,
    }
}

// ---------- bodies and queries ----------

#[derive(Deserialize, ToSchema)]
pub struct ContributionCreate {
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub script: String,
    #[serde(default)]
    pub body: String,
    pub note: Option<String>,
    /// Optional pointer to the original (or other version) this one was made from.
    pub source_contribution_id: Option<Uuid>,
}

#[derive(Deserialize, ToSchema)]
pub struct RevisionCreate {
    /// The head revision you based this on. Required; a mismatch returns 409 with `current_revision_id`.
    pub expected_parent_revision_id: Option<Uuid>,
    /// New text. Give either `body` or `restore_from_revision_id`.
    pub body: Option<String>,
    /// Restore an older revision of this contribution as a new revision.
    pub restore_from_revision_id: Option<Uuid>,
    pub note: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct ContributionDecision {
    /// The head revision that was reviewed. A mismatch returns 409 with `current_revision_id`.
    pub expected_parent_revision_id: Option<Uuid>,
    pub note: Option<String>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListQuery {
    /// `original`, `translation` or `transliteration`.
    #[serde(rename = "type")]
    #[param(rename = "type")]
    pub kind: Option<String>,
    pub language: Option<String>,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct TrustPolicyBody {
    /// Role names (`moderator`, `admin`) whose submissions publish without review.
    pub auto_publish_roles: Vec<String>,
    /// Members with at least this many published contributions publish without review; null disables it.
    pub auto_publish_min_published: Option<i64>,
}

impl From<TrustPolicy> for TrustPolicyBody {
    fn from(p: TrustPolicy) -> Self {
        Self {
            auto_publish_roles: p.auto_publish_roles,
            auto_publish_min_published: p.auto_publish_min_published,
        }
    }
}

// ---------- plumbing ----------

static REPLAYED: HeaderName = HeaderName::from_static("idempotent-replayed");

/// Optional `Idempotency-Key` header. Retrying with the same key returns the first result.
fn idem_key(h: &HeaderMap) -> Result<Option<String>, AppError> {
    match h.get("idempotency-key") {
        None => Ok(None),
        Some(v) => v
            .to_str()
            .map(|s| Some(s.to_owned()))
            .map_err(|_| domain::DomainError::invalid("idempotency_key", "invalid").into()),
    }
}

fn respond(status: StatusCode, location: Option<String>, o: Outcome<ContributionView>) -> Response {
    let mut res = (status, Json(ContributionDetail::from(o.value))).into_response();
    if let Some(l) = location.and_then(|l| HeaderValue::from_str(&l).ok()) {
        res.headers_mut().insert(header::LOCATION, l);
    }
    if o.replayed {
        res.headers_mut()
            .insert(REPLAYED.clone(), HeaderValue::from_static("true"));
    }
    res
}

// ---------- handlers ----------

#[utoipa::path(post, path = "/api/v1/recordings/{id}/contributions", tag = "lyrics", request_body = ContributionCreate,
    params(("id" = Uuid, Path), ("Idempotency-Key" = Option<String>, Header, description = "8–128 chars; a retry returns the first result")),
    security(("session" = [])), responses(
    (status = 201, body = ContributionDetail, description = "Published at once when the trust policy allows, otherwise queued for review"),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 404, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn create_contribution(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireUser(p): RequireUser,
    ApiPath(recording): ApiPath<Uuid>,
    headers: HeaderMap,
    ApiJson(b): ApiJson<ContributionCreate>,
) -> ApiResult<Response> {
    let key = idem_key(&headers)?;
    let o = uc::create_contribution(
        &app,
        &p,
        &meta,
        recording,
        key.as_deref(),
        uc::ContributionInput {
            kind: b.kind,
            language: b.language,
            script: b.script,
            body: b.body,
            note: b.note,
            source_contribution_id: b.source_contribution_id,
        },
    )
    .await?;
    let location = format!("/api/v1/contributions/{}", o.value.row.id);
    Ok(respond(StatusCode::CREATED, Some(location), o))
}

#[utoipa::path(get, path = "/api/v1/recordings/{id}/contributions", tag = "lyrics",
    params(("id" = Uuid, Path), PageQuery, ListQuery), responses(
    (status = 200, body = ContributionPage, description = "Published contributions only"),
    (status = 404, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn list_contributions(
    State(app): State<App>,
    ApiPath(recording): ApiPath<Uuid>,
    ApiQuery(q): ApiQuery<PageQuery>,
    ApiQuery(f): ApiQuery<ListQuery>,
) -> ApiResult<Json<ContributionPage>> {
    let page = uc::list_for_recording(
        &app,
        recording,
        f.kind.as_deref(),
        f.language.as_deref(),
        q.cursor.as_deref(),
        q.limit,
    )
    .await?;
    Ok(Json(contribution_page(page, true)))
}

#[utoipa::path(get, path = "/api/v1/contributions/{id}", tag = "lyrics", params(("id" = Uuid, Path)), responses(
    (status = 200, body = ContributionDetail, description = "Public: the published revision. Author and moderators: also the head and review state"),
    (status = 404, body = Problem, content_type = "application/problem+json")))]
pub async fn get_contribution(
    State(app): State<App>,
    MaybeUser(viewer): MaybeUser,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<Json<ContributionDetail>> {
    Ok(Json(uc::get_contribution(&app, viewer.as_ref(), id).await?.into()))
}

#[utoipa::path(post, path = "/api/v1/contributions/{id}/revisions", tag = "lyrics", request_body = RevisionCreate,
    params(("id" = Uuid, Path), ("Idempotency-Key" = Option<String>, Header, description = "8–128 chars; a retry returns the first result")),
    security(("session" = [])), responses(
    (status = 201, body = ContributionDetail),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json"),
    (status = 404, body = Problem, content_type = "application/problem+json"),
    (status = 409, body = Problem, content_type = "application/problem+json", description = "stale_revision, with current_revision_id"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn create_revision(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireUser(p): RequireUser,
    ApiPath(id): ApiPath<Uuid>,
    headers: HeaderMap,
    ApiJson(b): ApiJson<RevisionCreate>,
) -> ApiResult<Response> {
    let key = idem_key(&headers)?;
    let o = uc::add_revision(
        &app,
        &p,
        &meta,
        id,
        key.as_deref(),
        uc::RevisionInput {
            expected_parent_revision_id: b.expected_parent_revision_id,
            body: b.body,
            restore_from_revision_id: b.restore_from_revision_id,
            note: b.note,
        },
    )
    .await?;
    Ok(respond(StatusCode::CREATED, None, o))
}

#[utoipa::path(get, path = "/api/v1/contributions/{id}/revisions", tag = "lyrics", params(("id" = Uuid, Path), PageQuery), responses(
    (status = 200, body = RevisionPage, description = "Public: revisions that were published. Author and moderators: all"),
    (status = 404, body = Problem, content_type = "application/problem+json")))]
pub async fn list_revisions(
    State(app): State<App>,
    MaybeUser(viewer): MaybeUser,
    ApiPath(id): ApiPath<Uuid>,
    ApiQuery(q): ApiQuery<PageQuery>,
) -> ApiResult<Json<RevisionPage>> {
    let page = uc::list_revisions(&app, viewer.as_ref(), id, q.cursor.as_deref(), q.limit).await?;
    Ok(Json(RevisionPage {
        items: page.items.into_iter().map(Into::into).collect(),
        next_cursor: page.next_cursor,
    }))
}

#[utoipa::path(get, path = "/api/v1/revisions/{id}", tag = "lyrics", params(("id" = Uuid, Path)), responses(
    (status = 200, body = Revision),
    (status = 404, body = Problem, content_type = "application/problem+json")))]
pub async fn get_revision(
    State(app): State<App>,
    MaybeUser(viewer): MaybeUser,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<Json<Revision>> {
    Ok(Json(uc::get_revision(&app, viewer.as_ref(), id).await?.into()))
}

#[utoipa::path(get, path = "/api/v1/revisions/{a}/diff/{b}", tag = "lyrics",
    params(("a" = Uuid, Path), ("b" = Uuid, Path)), responses(
    (status = 200, body = RevisionDiff),
    (status = 404, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json", description = "Revisions of different contributions")))]
pub async fn diff_revisions(
    State(app): State<App>,
    MaybeUser(viewer): MaybeUser,
    ApiPath((a, b)): ApiPath<(Uuid, Uuid)>,
) -> ApiResult<Json<RevisionDiff>> {
    let d = uc::diff(&app, viewer.as_ref(), a, b).await?;
    Ok(Json(RevisionDiff {
        from: d.from.into(),
        to: d.to.into(),
        hunks: d.hunks.into_iter().map(hunk).collect(),
    }))
}

async fn decide(
    app: App,
    meta: application::RequestMeta,
    p: domain::Principal,
    id: Uuid,
    headers: HeaderMap,
    verdict: Verdict,
    b: ContributionDecision,
) -> ApiResult<Response> {
    let key = idem_key(&headers)?;
    let o = uc::decide(
        &app,
        &p,
        &meta,
        id,
        key.as_deref(),
        verdict,
        uc::Decision {
            expected_parent_revision_id: b.expected_parent_revision_id,
            note: b.note,
        },
    )
    .await?;
    Ok(respond(StatusCode::OK, None, o))
}

macro_rules! decision_endpoint {
    ($name:ident, $path:literal, $verdict:expr, $doc:literal) => {
        #[utoipa::path(post, path = $path, tag = "moderation", request_body = ContributionDecision,
            params(("id" = Uuid, Path), ("Idempotency-Key" = Option<String>, Header, description = "8–128 chars; a retry returns the first result")),
            security(("session" = [])), responses(
            (status = 200, body = ContributionDetail, description = $doc),
            (status = 401, body = Problem, content_type = "application/problem+json"),
            (status = 403, body = Problem, content_type = "application/problem+json"),
            (status = 409, body = Problem, content_type = "application/problem+json")))]
        pub async fn $name(
            State(app): State<App>,
            Meta(meta): Meta,
            RequireRole(p, ..): RequireRole<Moderator>,
            ApiPath(id): ApiPath<Uuid>,
            headers: HeaderMap,
            ApiJson(b): ApiJson<ContributionDecision>,
        ) -> ApiResult<Response> {
            decide(app, meta, p, id, headers, $verdict, b).await
        }
    };
}

decision_endpoint!(
    approve_contribution,
    "/api/v1/contributions/{id}/approve",
    Verdict::Approve,
    "Accepted for publication; not yet public"
);
decision_endpoint!(
    publish_contribution,
    "/api/v1/contributions/{id}/publish",
    Verdict::Publish,
    "The reviewed revision is now public; becomes the primary if the slot has none"
);
decision_endpoint!(
    reject_contribution,
    "/api/v1/contributions/{id}/reject",
    Verdict::Reject,
    "Rejected; whatever was public stays public"
);

#[utoipa::path(get, path = "/api/v1/moderation/contributions", tag = "moderation", params(PageQuery),
    security(("session" = [])), responses(
    (status = 200, body = ContributionPage),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json")))]
pub async fn contribution_queue(
    State(app): State<App>,
    RequireRole(p, ..): RequireRole<Moderator>,
    ApiQuery(q): ApiQuery<PageQuery>,
) -> ApiResult<Json<ContributionPage>> {
    Ok(Json(contribution_page(
        uc::moderation_queue(&app, &p, q.cursor.as_deref(), q.limit).await?,
        false,
    )))
}

#[utoipa::path(get, path = "/api/v1/me/contributions", tag = "lyrics", params(PageQuery),
    security(("session" = [])), responses(
    (status = 200, body = ContributionPage, description = "Your own contributions in any state"),
    (status = 401, body = Problem, content_type = "application/problem+json")))]
pub async fn my_contributions(
    State(app): State<App>,
    RequireUser(p): RequireUser,
    ApiQuery(q): ApiQuery<PageQuery>,
) -> ApiResult<Json<ContributionPage>> {
    Ok(Json(contribution_page(
        uc::list_mine(&app, &p, q.cursor.as_deref(), q.limit).await?,
        false,
    )))
}

#[utoipa::path(get, path = "/api/v1/admin/trust-policy", tag = "admin", security(("session" = [])), responses(
    (status = 200, body = TrustPolicyBody),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json")))]
pub async fn get_trust_policy(
    State(app): State<App>,
    RequireRole(p, ..): RequireRole<Admin>,
) -> ApiResult<Json<TrustPolicyBody>> {
    Ok(Json(uc::get_trust_policy(&app, &p).await?.into()))
}

#[utoipa::path(put, path = "/api/v1/admin/trust-policy", tag = "admin", request_body = TrustPolicyBody,
    security(("session" = [])), responses(
    (status = 200, body = TrustPolicyBody),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn set_trust_policy(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireRole(p, ..): RequireRole<Admin>,
    ApiJson(b): ApiJson<TrustPolicyBody>,
) -> ApiResult<Json<TrustPolicyBody>> {
    let policy = TrustPolicy {
        auto_publish_roles: b.auto_publish_roles,
        auto_publish_min_published: b.auto_publish_min_published,
    };
    Ok(Json(uc::set_trust_policy(&app, &p, &meta, policy).await?.into()))
}
