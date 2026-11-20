//! Legal endpoints. Filing a notice is public (rate limited per client); everything else is staff
//! or the affected contributor. Staff endpoints take any signed-in user and let the use case
//! decide, so a refused attempt lands in the audit log. Never cached.

use crate::extract::{ApiJson, ApiPath, ApiQuery, Meta, RequireUser};
use crate::problem::{ApiResult, Problem};
use application::legal::{self as uc, NoticeAction, NoticeInput};
use application::App;
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Utc};
use infrastructure::legal::{AppealRow, NoticeRow};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

#[derive(Deserialize, ToSchema)]
pub struct NoticeCreate {
    /// `copyright`, `content` or `privacy`.
    pub kind: String,
    /// Required unless `kind` is `privacy`. Must be a published contribution.
    pub contribution_id: Option<Uuid>,
    pub contact_name: String,
    pub contact_email: String,
    pub statement: String,
    /// The filer states in good faith that the notice is accurate. Must be `true`.
    pub good_faith: bool,
    /// Bot-check answer; required when `GET /api/v1/captcha` says it is enabled.
    pub captcha_token: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct NoticeReceipt {
    /// Quote this when writing to us. The notice itself is not readable by the public.
    pub reference: String,
    pub status: String,
}

#[utoipa::path(post, path = "/api/v1/legal/notices", tag = "legal", request_body = NoticeCreate, responses(
    (status = 201, body = NoticeReceipt),
    (status = 404, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json"),
    (status = 429, body = Problem, content_type = "application/problem+json")))]
pub async fn file_legal_notice(
    State(app): State<App>,
    Meta(meta): Meta,
    headers: axum::http::HeaderMap,
    ApiJson(b): ApiJson<NoticeCreate>,
) -> ApiResult<(StatusCode, Json<NoticeReceipt>)> {
    crate::captcha::check(&app, &headers, b.captcha_token.as_deref()).await?;
    let r = uc::file_notice(
        &app,
        &meta,
        NoticeInput {
            kind: b.kind,
            contribution_id: b.contribution_id,
            contact_name: b.contact_name,
            contact_email: b.contact_email,
            statement: b.statement,
            good_faith: b.good_faith,
        },
    )
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(NoticeReceipt {
            reference: r.reference,
            status: r.status,
        }),
    ))
}

/// Staff view. Includes the filer's contact details, which are never returned elsewhere.
#[derive(Serialize, ToSchema)]
pub struct Notice {
    pub id: Uuid,
    pub reference: String,
    pub kind: String,
    pub contribution_id: Option<Uuid>,
    pub contact_name: String,
    pub contact_email: String,
    pub statement: String,
    /// `open`, `taken_down`, `rejected`, `closed` or `reinstated`.
    pub status: String,
    pub decided_by: Option<Uuid>,
    pub decided_at: Option<DateTime<Utc>>,
    pub decision_note: Option<String>,
    pub created_at: DateTime<Utc>,
}

fn notice(r: NoticeRow) -> Notice {
    Notice {
        id: r.id,
        reference: r.reference,
        kind: r.kind,
        contribution_id: r.contribution_id,
        contact_name: r.contact_name,
        contact_email: r.contact_email,
        statement: r.statement,
        status: r.status,
        decided_by: r.decided_by,
        decided_at: r.decided_at,
        decision_note: r.decision_note,
        created_at: r.created_at,
    }
}

#[derive(Serialize, ToSchema)]
pub struct NoticePage {
    pub items: Vec<Notice>,
    pub next_cursor: Option<String>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct QueueQuery {
    pub cursor: Option<String>,
    pub limit: Option<i64>,
    /// Notices: `open` (default), `taken_down`, `rejected`, `closed`, `reinstated`. Appeals: `open`, `upheld`, `denied`.
    pub status: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/moderation/legal/notices", tag = "legal", params(QueueQuery), security(("session" = [])), responses(
    (status = 200, body = NoticePage),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json", description = "Refusals are audited")))]
pub async fn legal_notice_queue(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireUser(p): RequireUser,
    ApiQuery(q): ApiQuery<QueueQuery>,
) -> ApiResult<Json<NoticePage>> {
    let page = uc::notice_queue(&app, &p, &meta, q.status.as_deref(), q.cursor.as_deref(), q.limit).await?;
    Ok(Json(NoticePage {
        items: page.items.into_iter().map(notice).collect(),
        next_cursor: page.next_cursor,
    }))
}

#[derive(Deserialize, ToSchema)]
pub struct NoticeDecision {
    /// `takedown` (withdraws the public contribution), `reject` or `close`.
    pub action: String,
    pub note: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/moderation/legal/notices/{id}/decide", tag = "legal", params(("id" = Uuid, Path)), request_body = NoticeDecision, security(("session" = [])), responses(
    (status = 200, body = Notice),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json", description = "Refusals are audited"),
    (status = 404, body = Problem, content_type = "application/problem+json"),
    (status = 409, body = Problem, content_type = "application/problem+json", description = "`already_resolved` or `not_published`"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn decide_legal_notice(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireUser(p): RequireUser,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(b): ApiJson<NoticeDecision>,
) -> ApiResult<Json<Notice>> {
    let action = match b.action.as_str() {
        "takedown" => NoticeAction::Takedown,
        "reject" => NoticeAction::Reject,
        "close" => NoticeAction::Close,
        _ => return Err(application::AppError::from(domain::DomainError::invalid("action", "invalid_choice")).into()),
    };
    Ok(Json(notice(
        uc::decide_notice(&app, &p, &meta, id, action, b.note.as_deref()).await?,
    )))
}

#[derive(Deserialize, ToSchema)]
pub struct TakedownAppealCreate {
    pub statement: String,
}

#[derive(Serialize, ToSchema)]
pub struct TakedownAppeal {
    pub id: Uuid,
    pub notice_id: Uuid,
    pub appellant_id: Uuid,
    pub statement: String,
    /// `open`, `upheld` or `denied`.
    pub status: String,
    pub decision_note: Option<String>,
    pub decided_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

fn appeal(r: AppealRow) -> TakedownAppeal {
    TakedownAppeal {
        id: r.id,
        notice_id: r.notice_id,
        appellant_id: r.appellant_id,
        statement: r.statement,
        status: r.status,
        decision_note: r.decision_note,
        decided_at: r.decided_at,
        created_at: r.created_at,
    }
}

#[utoipa::path(post, path = "/api/v1/legal/notices/{id}/appeal", tag = "legal", params(("id" = Uuid, Path)), request_body = TakedownAppealCreate, security(("session" = [])), responses(
    (status = 201, body = TakedownAppeal),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 404, body = Problem, content_type = "application/problem+json", description = "Also for anyone but the contributor"),
    (status = 409, body = Problem, content_type = "application/problem+json", description = "`nothing_to_appeal` or `appeal_pending`"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn file_takedown_appeal(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireUser(p): RequireUser,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(b): ApiJson<TakedownAppealCreate>,
) -> ApiResult<(StatusCode, Json<TakedownAppeal>)> {
    let a = uc::appeal(&app, &p, &meta, id, &b.statement).await?;
    Ok((StatusCode::CREATED, Json(appeal(a))))
}

#[derive(Serialize, ToSchema)]
pub struct TakedownAppealPage {
    pub items: Vec<TakedownAppeal>,
    pub next_cursor: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/admin/legal/appeals", tag = "legal", params(QueueQuery), security(("session" = [])), responses(
    (status = 200, body = TakedownAppealPage),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json", description = "Refusals are audited")))]
pub async fn takedown_appeal_queue(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireUser(p): RequireUser,
    ApiQuery(q): ApiQuery<QueueQuery>,
) -> ApiResult<Json<TakedownAppealPage>> {
    let page = uc::appeal_queue(&app, &p, &meta, q.status.as_deref(), q.cursor.as_deref(), q.limit).await?;
    Ok(Json(TakedownAppealPage {
        items: page.items.into_iter().map(appeal).collect(),
        next_cursor: page.next_cursor,
    }))
}

#[derive(Deserialize, ToSchema)]
pub struct TakedownAppealDecision {
    /// `true` reinstates the contribution.
    pub uphold: bool,
    pub note: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/admin/legal/appeals/{id}/decide", tag = "legal", params(("id" = Uuid, Path)), request_body = TakedownAppealDecision, security(("session" = [])), responses(
    (status = 200, body = TakedownAppeal),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json", description = "Refusals are audited; `own_decision` when the admin took it down"),
    (status = 404, body = Problem, content_type = "application/problem+json"),
    (status = 409, body = Problem, content_type = "application/problem+json", description = "`already_resolved`")))]
pub async fn decide_takedown_appeal(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireUser(p): RequireUser,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(b): ApiJson<TakedownAppealDecision>,
) -> ApiResult<Json<TakedownAppeal>> {
    Ok(Json(appeal(
        uc::decide_appeal(&app, &p, &meta, id, b.uphold, b.note.as_deref()).await?,
    )))
}
