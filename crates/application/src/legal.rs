//! Legal workflows. Anyone may file a notice (no account: rights holders are rarely members).
//! A moderator decides it; a takedown withdraws the public pointer only, so revisions stay intact.
//! The contributor may appeal; an admin who did not decide the notice decides the appeal, and an
//! upheld appeal republishes exactly the revision that was withdrawn. Every step is audited and
//! every refused staff action is recorded.

use crate::catalogue::{record, settle};
use crate::community::{audit_outside, moderator};
use crate::pagination::{self, Cursor, Page};
use crate::{App, AppError, RequestMeta};
use domain::community as dc;
use domain::{Principal, Role};
use infrastructure::audit::{self, AuditEntry};
use infrastructure::legal::{self as repo, AppealRow, NoticeRow};
use infrastructure::{lyrics as lyr_repo, AppTx};
use serde_json::json;
use uuid::Uuid;

fn violation(field: &'static str, code: &'static str) -> AppError {
    domain::DomainError::invalid(field, code).into()
}

fn after(cursor: Option<&str>) -> Result<repo::After, AppError> {
    Ok(cursor.map(Cursor::decode).transpose()?.map(|c| (c.ts, c.id)))
}

fn conflict(code: &'static str) -> AppError {
    AppError::Conflict {
        code,
        current_revision_id: None,
    }
}

pub struct NoticeInput {
    pub kind: String,
    pub contribution_id: Option<Uuid>,
    pub contact_name: String,
    pub contact_email: String,
    pub statement: String,
    pub good_faith: bool,
}

/// What the filer gets back: a reference to quote, never the stored contact details.
pub struct Receipt {
    pub reference: String,
    pub status: String,
}

fn email_ok(e: &str) -> bool {
    e.len() <= 254
        && !e.chars().any(|c| c.is_whitespace() || c.is_control())
        && e.split_once('@')
            .is_some_and(|(l, d)| !l.is_empty() && d.contains('.') && !d.starts_with('.') && !d.ends_with('.'))
}

pub async fn file_notice(app: &App, meta: &RequestMeta, i: NoticeInput) -> Result<Receipt, AppError> {
    let mut v = domain::Violations::default();
    if !matches!(i.kind.as_str(), "copyright" | "privacy" | "content") {
        v.push("kind", "invalid_choice");
    }
    if i.kind != "privacy" && i.contribution_id.is_none() {
        v.push("contribution_id", "required");
    }
    let name = v.check(dc::text("contact_name", &i.contact_name, 200));
    let email = i.contact_email.trim().to_owned();
    if !email_ok(&email) {
        v.push("contact_email", "invalid_email");
    }
    let statement = v.check(dc::text("statement", &i.statement, 5000));
    if statement.as_ref().is_some_and(|s| s.chars().count() < 20) {
        v.push("statement", "too_short");
    }
    if !i.good_faith {
        v.push("good_faith", "must_accept");
    }
    v.finish()?;

    let mut tx = AppTx::begin(&app.pool).await?;
    // Only public work can be the subject; unknown and unpublished ids look the same, so a notice
    // cannot be used to probe for pending submissions.
    if let Some(id) = i.contribution_id {
        if lyr_repo::published_ids(&mut tx, &[id]).await?.is_empty() {
            return Err(AppError::NotFound { entity: "contribution" });
        }
    }
    let reference = format!("LN-{}", Uuid::new_v4().simple().to_string()[..10].to_uppercase());
    let row = repo::insert_notice(
        &mut tx,
        &reference,
        &i.kind,
        i.contribution_id,
        &name.unwrap_or_default(),
        &email,
        &statement.unwrap_or_default(),
    )
    .await?;
    audit::record(
        &mut tx,
        AuditEntry {
            request_id: meta.request_id.as_deref(),
            after: Some(
                json!({ "reference": row.reference, "kind": row.kind, "contribution_id": row.contribution_id }),
            ),
            ..AuditEntry::new("legal.notice_filed", "legal_notice", Some(row.id))
        },
    )
    .await?;
    tx.commit().await?;
    Ok(Receipt {
        reference: row.reference,
        status: row.status,
    })
}

pub async fn notice_queue(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    status: Option<&str>,
    cursor: Option<&str>,
    limit: Option<i64>,
) -> Result<Page<NoticeRow>, AppError> {
    moderator(app, p, meta, "legal.queue", "legal_notice", None).await?;
    let status = match status.unwrap_or("open") {
        s @ ("open" | "taken_down" | "rejected" | "closed" | "reinstated") => s,
        _ => return Err(violation("status", "invalid_choice")),
    };
    let limit = pagination::limit(limit)?;
    let mut conn = app.pool.acquire().await?;
    let rows = repo::notice_queue(&mut conn, status, after(cursor)?, limit + 1).await?;
    Ok(Page::from_overfetch(rows, limit, |r| Cursor {
        ts: r.created_at,
        id: r.id,
    }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeAction {
    Takedown,
    Reject,
    Close,
}

pub async fn decide_notice(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    id: Uuid,
    action: NoticeAction,
    note: Option<&str>,
) -> Result<NoticeRow, AppError> {
    moderator(app, p, meta, "legal.decide", "legal_notice", Some(id)).await?;
    let note = domain::lyrics::note("note", note, 1000)?;
    let mut tx = AppTx::begin(&app.pool).await?;
    let n = repo::get_notice(&mut tx, id, true)
        .await?
        .ok_or(AppError::NotFound { entity: "legal_notice" })?;
    if n.status != "open" {
        return Err(conflict("already_resolved"));
    }
    let (status, mut revision, mut was_primary) = match action {
        NoticeAction::Reject => ("rejected", None, false),
        NoticeAction::Close => ("closed", None, false),
        NoticeAction::Takedown => ("taken_down", None, false),
    };
    if action == NoticeAction::Takedown {
        // Privacy requests are handled outside the catalogue and cannot take a contribution down.
        let target = n.contribution_id.ok_or_else(|| violation("action", "no_target"))?;
        let c = lyr_repo::get_contribution(&mut tx, target, true)
            .await?
            .ok_or(AppError::NotFound { entity: "contribution" })?;
        lyr_repo::lock_slot(&mut tx, c.recording_id, &c.kind, c.language_id, c.script_id).await?;
        let published = c.published_revision_id.ok_or_else(|| conflict("not_published"))?;
        revision = Some(published);
        was_primary = c.is_primary;
        repo::withdraw(&mut tx, target, &format!("taken down: {}", n.reference)).await?;
        record(
            &mut tx,
            p,
            meta,
            "contribution.taken_down",
            "contribution",
            target,
            Some(json!({ "status": c.status, "published_revision_id": published, "is_primary": c.is_primary })),
            Some(json!({ "status": "rejected", "notice": n.reference })),
        )
        .await?;
        infrastructure::outbox::enqueue(
            &mut tx,
            "lyrics.unpublished",
            "contribution",
            target,
            json!({ "contribution_id": target, "recording_id": c.recording_id }),
        )
        .await?;
    }
    repo::decide_notice(&mut tx, id, status, p.user_id, note.as_deref(), revision, was_primary).await?;
    record(
        &mut tx,
        p,
        meta,
        "legal.notice_decided",
        "legal_notice",
        id,
        Some(json!({ "status": "open" })),
        Some(json!({ "status": status, "reference": n.reference, "note": note })),
    )
    .await?;
    let row = repo::get_notice(&mut tx, id, false).await?.expect("just updated");
    tx.commit().await?;
    Ok(row)
}

/// The contributor of a taken-down work asks for a second look. One open appeal per notice.
pub async fn appeal(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    notice_id: Uuid,
    statement: &str,
) -> Result<AppealRow, AppError> {
    let statement = dc::text("statement", statement, 5000)?;
    if statement.chars().count() < 20 {
        return Err(violation("statement", "too_short"));
    }
    let mut tx = AppTx::begin(&app.pool).await?;
    let n = repo::get_notice(&mut tx, notice_id, true)
        .await?
        .ok_or(AppError::NotFound { entity: "legal_notice" })?;
    // Anyone but the contributor sees a missing notice, not a forbidden one.
    let author = match n.contribution_id {
        Some(c) => lyr_repo::get_contribution(&mut tx, c, false)
            .await?
            .map(|c| c.created_by),
        None => None,
    };
    if author != Some(p.user_id) {
        return Err(AppError::NotFound { entity: "legal_notice" });
    }
    if n.status != "taken_down" {
        return Err(conflict("nothing_to_appeal"));
    }
    let row = settle(
        repo::insert_appeal(&mut tx, notice_id, p.user_id, &statement)
            .await
            .map_err(Into::into),
    )?;
    record(
        &mut tx,
        p,
        meta,
        "legal.appeal_filed",
        "legal_notice",
        notice_id,
        None,
        Some(json!({ "appeal_id": row.id, "reference": n.reference })),
    )
    .await?;
    tx.commit().await?;
    Ok(row)
}

async fn admin(app: &App, p: &Principal, meta: &RequestMeta, action: &str, id: Option<Uuid>) -> Result<(), AppError> {
    if p.has_role(Role::Admin) {
        return Ok(());
    }
    audit_outside(
        app,
        p,
        meta,
        "moderation.denied",
        "takedown_appeal",
        id,
        json!({ "attempted": action }),
    )
    .await;
    Err(AppError::Forbidden)
}

pub async fn appeal_queue(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    status: Option<&str>,
    cursor: Option<&str>,
    limit: Option<i64>,
) -> Result<Page<AppealRow>, AppError> {
    admin(app, p, meta, "legal.appeal_queue", None).await?;
    let status = match status.unwrap_or("open") {
        s @ ("open" | "upheld" | "denied") => s,
        _ => return Err(violation("status", "invalid_choice")),
    };
    let limit = pagination::limit(limit)?;
    let mut conn = app.pool.acquire().await?;
    let rows = repo::appeal_queue(&mut conn, status, after(cursor)?, limit + 1).await?;
    Ok(Page::from_overfetch(rows, limit, |r| Cursor {
        ts: r.created_at,
        id: r.id,
    }))
}

pub async fn decide_appeal(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    id: Uuid,
    uphold: bool,
    note: Option<&str>,
) -> Result<AppealRow, AppError> {
    admin(app, p, meta, "legal.appeal_decide", Some(id)).await?;
    let note = domain::lyrics::note("note", note, 1000)?;
    let mut tx = AppTx::begin(&app.pool).await?;
    let a = repo::get_appeal(&mut tx, id, true)
        .await?
        .ok_or(AppError::NotFound { entity: "appeal" })?;
    if a.status != "open" {
        return Err(conflict("already_resolved"));
    }
    let n = repo::get_notice(&mut tx, a.notice_id, true)
        .await?
        .ok_or(AppError::NotFound { entity: "legal_notice" })?;
    // Nobody reviews their own takedown.
    if n.decided_by == Some(p.user_id) {
        return Err(AppError::Restricted { code: "own_decision" });
    }
    if uphold {
        let (target, revision) = n
            .contribution_id
            .zip(n.taken_down_revision_id)
            .ok_or_else(|| conflict("nothing_to_appeal"))?;
        let c = lyr_repo::get_contribution(&mut tx, target, true)
            .await?
            .ok_or(AppError::NotFound { entity: "contribution" })?;
        lyr_repo::lock_slot(&mut tx, c.recording_id, &c.kind, c.language_id, c.script_id).await?;
        let primary = repo::reinstate(
            &mut tx,
            target,
            revision,
            &format!("reinstated on appeal: {}", n.reference),
        )
        .await?;
        record(
            &mut tx,
            p,
            meta,
            "contribution.reinstated",
            "contribution",
            target,
            Some(json!({ "status": c.status })),
            Some(json!({ "status": "published", "published_revision_id": revision, "is_primary": primary })),
        )
        .await?;
        infrastructure::outbox::enqueue(
            &mut tx,
            "lyrics.published",
            "contribution",
            target,
            json!({ "contribution_id": target, "revision_id": revision, "recording_id": c.recording_id, "primary": primary }),
        )
        .await?;
        repo::set_notice_status(&mut tx, n.id, "reinstated").await?;
    }
    repo::decide_appeal(
        &mut tx,
        id,
        if uphold { "upheld" } else { "denied" },
        p.user_id,
        note.as_deref(),
    )
    .await?;
    record(
        &mut tx,
        p,
        meta,
        "legal.appeal_decided",
        "legal_notice",
        n.id,
        Some(json!({ "status": "open" })),
        Some(json!({ "appeal_id": id, "outcome": if uphold { "upheld" } else { "denied" }, "reference": n.reference })),
    )
    .await?;
    let row = repo::get_appeal(&mut tx, id, false).await?.expect("just updated");
    tx.commit().await?;
    Ok(row)
}
