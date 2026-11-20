//! Community use cases: ratings, reactions, bookmarks, comments, profiles, reputation, reports,
//! moderator notes and appeals.
//!
//! Rules every write follows, in order: authenticate (extractor) → reputation threshold → validate
//! → target must be public → rate limit → spam heuristics → write + audit in one transaction.
//! Reputation is a score from an append-only event log; it opens community actions but never
//! confers a role. Moderation checks the role itself and records a refusal in the audit log.

use crate::catalogue::{record, settle, snapshot};
use crate::pagination::{self, Cursor, Page};
use crate::{authz, App, AppError, RequestMeta};
use domain::community::{self as dc, CommunitySettings, ReactionKind};
use domain::{Principal, Role};
use infrastructure::audit::{self, AuditEntry};
use infrastructure::community::{
    self as repo, AppealRow, CommentRow, NoteRow, ProfileRow, ReportRow, ReputationEventRow, Summary,
};
use infrastructure::{lyrics as lyr_repo, repo as user_repo, AppTx};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::PgConnection;
use uuid::Uuid;

const SETTINGS_KEY: &str = "community";

fn after(cursor: Option<&str>) -> Result<repo::After, AppError> {
    Ok(cursor.map(Cursor::decode).transpose()?.map(|c| (c.ts, c.id)))
}

fn violation(field: &'static str, code: &'static str) -> AppError {
    domain::DomainError::invalid(field, code).into()
}

// ---------- settings ----------

pub async fn load_settings(conn: &mut PgConnection) -> Result<CommunitySettings, AppError> {
    let v = lyr_repo::get_setting(conn, SETTINGS_KEY)
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("setting {SETTINGS_KEY} is missing")))?;
    serde_json::from_value(v).map_err(|e| AppError::Internal(anyhow::anyhow!("setting {SETTINGS_KEY} is invalid: {e}")))
}

pub async fn get_settings(app: &App, p: &Principal) -> Result<CommunitySettings, AppError> {
    authz::require_role(p, Role::Admin)?;
    let mut conn = app.pool.acquire().await?;
    load_settings(&mut conn).await
}

pub async fn set_settings(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    new: CommunitySettings,
) -> Result<CommunitySettings, AppError> {
    authz::require_role(p, Role::Admin)?;
    new.validate()?;
    let mut tx = AppTx::begin(&app.pool).await?;
    let before = load_settings(&mut tx).await?;
    let value = serde_json::to_value(&new).map_err(|e| AppError::Internal(e.into()))?;
    lyr_repo::put_setting(&mut tx, SETTINGS_KEY, &value, p.user_id).await?;
    record(
        &mut tx,
        p,
        meta,
        "settings.community_changed",
        "settings",
        Uuid::nil(),
        snapshot(&before),
        snapshot(&new),
    )
    .await?;
    tx.commit().await?;
    Ok(new)
}

// ---------- abuse controls ----------

/// Reputation threshold for `action`. Moderators and admins are exempt: they hold a role, which
/// is a separate grant; everyone else needs the score.
pub async fn gate(conn: &mut PgConnection, p: &Principal, action: &str, s: &CommunitySettings) -> Result<(), AppError> {
    let min = s.min_reputation.get(action).copied().unwrap_or(0);
    if min > 0 && !p.has_role(Role::Moderator) && repo::reputation(conn, p.user_id).await? < min {
        return Err(AppError::Restricted {
            code: "reputation_too_low",
        });
    }
    Ok(())
}

/// Counts this action against the user's window. Serialised per user and action, so a burst of
/// concurrent requests cannot all pass the check.
pub async fn throttle(
    conn: &mut PgConnection,
    p: &Principal,
    action: &str,
    s: &CommunitySettings,
) -> Result<(), AppError> {
    let Some(rate) = s.rate.get(action) else { return Ok(()) };
    repo::lock_user_action(conn, p.user_id, action).await?;
    let (n, oldest) = repo::recent_actions(conn, p.user_id, action, i64::from(rate.window_secs)).await?;
    if n >= i64::from(rate.max) {
        let until = oldest.map_or(0, |o| {
            (o + chrono::Duration::seconds(i64::from(rate.window_secs)) - chrono::Utc::now()).num_seconds()
        });
        return Err(AppError::RateLimited {
            retry_after_secs: until.max(1) as u64,
        });
    }
    repo::note_action(conn, p.user_id, action).await?;
    Ok(())
}

/// Writes an audit row on its own connection, so it survives the refusal that follows.
pub(crate) async fn audit_outside(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    action: &str,
    target_type: &str,
    id: Option<Uuid>,
    extra: serde_json::Value,
) {
    let result = async {
        let mut c = app.pool.acquire().await?;
        audit::record(
            &mut c,
            AuditEntry {
                actor_id: Some(p.user_id),
                request_id: meta.request_id.as_deref(),
                metadata: extra,
                ..AuditEntry::new(action, target_type, id)
            },
        )
        .await?;
        Ok::<_, sqlx::Error>(())
    }
    .await;
    if let Err(e) = result {
        tracing::error!(error = ?e, action, "could not audit a refused action");
    }
}

/// Moderation is checked here, from the server-side roles; a refusal is audited, not just returned.
pub(crate) async fn moderator(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    action: &str,
    target_type: &str,
    id: Option<Uuid>,
) -> Result<(), AppError> {
    if p.has_role(Role::Moderator) {
        return Ok(());
    }
    audit_outside(
        app,
        p,
        meta,
        "moderation.denied",
        target_type,
        id,
        json!({ "attempted": action }),
    )
    .await;
    Err(AppError::Forbidden)
}

fn derived_id(parts: &[Uuid]) -> Uuid {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p.as_bytes());
    }
    let d = h.finalize();
    Uuid::from_slice(&d[..16]).expect("16 bytes")
}

// ---------- reputation ----------

/// Called inside the publishing transaction. Scores once per contribution, ever.
pub async fn award_published(conn: &mut PgConnection, creator: Uuid, contribution: Uuid) -> Result<(), AppError> {
    let s = load_settings(conn).await?;
    repo::add_reputation(
        conn,
        creator,
        "contribution_published",
        s.points_for("contribution_published"),
        "contribution",
        contribution,
    )
    .await?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct Reputation {
    pub score: i64,
    pub level: String,
}

pub async fn my_reputation(app: &App, p: &Principal) -> Result<Reputation, AppError> {
    let mut conn = app.pool.acquire().await?;
    let s = load_settings(&mut conn).await?;
    let score = repo::reputation(&mut conn, p.user_id).await?;
    Ok(Reputation {
        level: s.level_for(score).to_owned(),
        score,
    })
}

pub async fn my_reputation_events(
    app: &App,
    p: &Principal,
    cursor: Option<&str>,
    limit: Option<i64>,
) -> Result<Page<ReputationEventRow>, AppError> {
    let limit = pagination::limit(limit)?;
    let mut conn = app.pool.acquire().await?;
    let rows = repo::list_reputation(&mut conn, p.user_id, after(cursor)?, limit + 1).await?;
    Ok(Page::from_overfetch(rows, limit, |r| Cursor {
        ts: r.created_at,
        id: r.id,
    }))
}

// ---------- profiles ----------

#[derive(Debug, Clone)]
pub struct Profile {
    pub row: ProfileRow,
    pub level: String,
    /// Roles are listed on their own, never derived from the score.
    pub roles: Vec<Role>,
}

pub async fn get_profile(app: &App, id: Uuid) -> Result<Profile, AppError> {
    let mut conn = app.pool.acquire().await?;
    let row = repo::get_profile(&mut conn, id)
        .await?
        .ok_or(AppError::NotFound { entity: "profile" })?;
    let s = load_settings(&mut conn).await?;
    let roles = user_repo::roles(&mut conn, id).await?;
    Ok(Profile {
        level: s.level_for(row.reputation).to_owned(),
        row,
        roles,
    })
}

pub async fn set_bio(app: &App, p: &Principal, bio: &str) -> Result<Profile, AppError> {
    let mut conn = app.pool.acquire().await?;
    let s = load_settings(&mut conn).await?;
    let bio = if bio.trim().is_empty() {
        String::new()
    } else {
        dc::text("bio", bio, s.limits.bio_chars)?
    };
    if !bio.is_empty() {
        if let Some(code) = dc::spam_code(&bio, &s.spam, false) {
            return Err(violation("bio", code));
        }
    }
    repo::set_bio(&mut conn, p.user_id, &bio).await?;
    drop(conn);
    get_profile(app, p.user_id).await
}

// ---------- ratings, reactions, bookmarks ----------

async fn public_target(conn: &mut PgConnection, id: Uuid) -> Result<Uuid, AppError> {
    repo::published_creator(conn, id)
        .await?
        .ok_or(AppError::NotFound { entity: "contribution" })
}

#[derive(Debug, Clone)]
pub struct Engagement {
    pub summary: Summary,
    pub reactions: Vec<(String, i64)>,
    pub mine: Option<Mine>,
}

#[derive(Debug, Clone)]
pub struct Mine {
    pub rating: Option<i16>,
    pub reactions: Vec<String>,
    pub bookmarked: bool,
}

pub async fn engagement(app: &App, viewer: Option<&Principal>, id: Uuid) -> Result<Engagement, AppError> {
    let mut conn = app.pool.acquire().await?;
    public_target(&mut conn, id).await?;
    let summary = repo::summary(&mut conn, id).await?;
    let reactions = repo::reaction_counts(&mut conn, id).await?;
    let mine = match viewer {
        Some(p) => Some(Mine {
            rating: repo::my_rating(&mut conn, id, p.user_id).await?,
            reactions: repo::my_reactions(&mut conn, id, p.user_id).await?,
            bookmarked: repo::is_bookmarked(&mut conn, id, p.user_id).await?,
        }),
        None => None,
    };
    Ok(Engagement {
        summary,
        reactions,
        mine,
    })
}

pub async fn rate(app: &App, p: &Principal, id: Uuid, value: i64) -> Result<(), AppError> {
    if !(1..=5).contains(&value) {
        return Err(violation("value", "out_of_range"));
    }
    let mut tx = AppTx::begin(&app.pool).await?;
    let s = load_settings(&mut tx).await?;
    gate(&mut tx, p, "rate", &s).await?;
    let creator = public_target(&mut tx, id).await?;
    if repo::authored(&mut tx, id, p.user_id).await? {
        return Err(violation("contribution_id", "self_rating"));
    }
    throttle(&mut tx, p, "rating", &s).await?;
    let before = repo::upsert_rating(&mut tx, id, p.user_id, value as i16).await?;
    // A first high rating scores the author once per rater; changing it later does not farm points.
    if value >= 4 && before.is_none() {
        let subject = derived_id(&[id, p.user_id]);
        repo::add_reputation(
            &mut tx,
            creator,
            "rating_received_high",
            s.points_for("rating_received_high"),
            "rating",
            subject,
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn unrate(app: &App, p: &Principal, id: Uuid) -> Result<(), AppError> {
    let mut conn = app.pool.acquire().await?;
    public_target(&mut conn, id).await?;
    repo::delete_rating(&mut conn, id, p.user_id).await?;
    Ok(())
}

pub async fn react(app: &App, p: &Principal, id: Uuid, kind: &str, on: bool) -> Result<(), AppError> {
    let kind = ReactionKind::parse(kind).ok_or(violation("kind", "invalid_choice"))?;
    let mut tx = AppTx::begin(&app.pool).await?;
    let s = load_settings(&mut tx).await?;
    gate(&mut tx, p, "react", &s).await?;
    public_target(&mut tx, id).await?;
    throttle(&mut tx, p, "reaction", &s).await?;
    repo::set_reaction(&mut tx, id, p.user_id, kind.as_str(), on).await?;
    tx.commit().await?;
    Ok(())
}

pub async fn bookmark(app: &App, p: &Principal, id: Uuid, on: bool) -> Result<(), AppError> {
    let mut tx = AppTx::begin(&app.pool).await?;
    let s = load_settings(&mut tx).await?;
    if on {
        public_target(&mut tx, id).await?;
    }
    throttle(&mut tx, p, "bookmark", &s).await?;
    repo::set_bookmark(&mut tx, id, p.user_id, on).await?;
    tx.commit().await?;
    Ok(())
}

/// Contribution ids the member bookmarked that are still public.
pub async fn list_bookmarks(
    app: &App,
    p: &Principal,
    cursor: Option<&str>,
    limit: Option<i64>,
) -> Result<Page<(Uuid, chrono::DateTime<chrono::Utc>)>, AppError> {
    let limit = pagination::limit(limit)?;
    let mut conn = app.pool.acquire().await?;
    let rows = repo::list_bookmarks(&mut conn, p.user_id, after(cursor)?, limit + 1).await?;
    Ok(Page::from_overfetch(rows, limit, |r| Cursor { ts: r.1, id: r.0 }))
}

// ---------- comments ----------

pub async fn list_comments(
    app: &App,
    contribution: Uuid,
    cursor: Option<&str>,
    limit: Option<i64>,
) -> Result<Page<CommentRow>, AppError> {
    let limit = pagination::limit(limit)?;
    let after = after(cursor)?;
    let mut conn = app.pool.acquire().await?;
    public_target(&mut conn, contribution).await?;
    let rows = repo::list_comments(&mut conn, contribution, after, limit + 1).await?;
    Ok(Page::from_overfetch(rows, limit, |r| Cursor {
        ts: r.created_at,
        id: r.id,
    }))
}

pub async fn my_comments(
    app: &App,
    p: &Principal,
    cursor: Option<&str>,
    limit: Option<i64>,
) -> Result<Page<CommentRow>, AppError> {
    let limit = pagination::limit(limit)?;
    let mut conn = app.pool.acquire().await?;
    let rows = repo::list_my_comments(&mut conn, p.user_id, after(cursor)?, limit + 1).await?;
    Ok(Page::from_overfetch(rows, limit, |r| Cursor {
        ts: r.created_at,
        id: r.id,
    }))
}

pub async fn create_comment(app: &App, p: &Principal, contribution: Uuid, body: &str) -> Result<CommentRow, AppError> {
    let mut conn = app.pool.acquire().await?;
    let s = load_settings(&mut conn).await?;
    let body = dc::text("body", body, s.limits.comment_chars)?;
    let dup =
        repo::recent_duplicate_comment(&mut conn, p.user_id, &body, i64::from(s.spam.duplicate_window_secs)).await?;
    drop(conn);
    let mut tx = AppTx::begin(&app.pool).await?;
    gate(&mut tx, p, "comment", &s).await?;
    public_target(&mut tx, contribution).await?;
    throttle(&mut tx, p, "comment", &s).await?;
    // Heuristics run after the throttle is taken but their refusal is counted on its own.
    if let Some(code) = dc::spam_code(&body, &s.spam, dup) {
        drop(tx);
        if let Ok(mut c) = app.pool.acquire().await {
            let _ = repo::note_action(&mut c, p.user_id, "comment").await;
        }
        return Err(violation("body", code));
    }
    let id = repo::insert_comment(&mut tx, contribution, p.user_id, &body).await?;
    let row = repo::get_comment(&mut tx, id, false)
        .await?
        .ok_or(AppError::NotFound { entity: "comment" })?;
    tx.commit().await?;
    Ok(row)
}

/// Authors edit their own visible comments; `expected_version` makes a stale edit a 409.
pub async fn edit_comment(
    app: &App,
    p: &Principal,
    id: Uuid,
    expected_version: i32,
    body: &str,
) -> Result<CommentRow, AppError> {
    let mut tx = AppTx::begin(&app.pool).await?;
    let s = load_settings(&mut tx).await?;
    let body = dc::text("body", body, s.limits.comment_chars)?;
    let c = repo::get_comment(&mut tx, id, true)
        .await?
        .ok_or(AppError::NotFound { entity: "comment" })?;
    if c.author_id != p.user_id || c.status != "visible" {
        // A hidden or removed comment is not editable, and nobody learns that someone else's exists.
        return Err(if c.author_id == p.user_id {
            AppError::Forbidden
        } else {
            AppError::NotFound { entity: "comment" }
        });
    }
    if c.version != expected_version {
        return Err(AppError::StaleVersion {
            current_version: c.version,
        });
    }
    screen_in_tx(&body, &s)?;
    repo::edit_comment(&mut tx, id, &body).await?;
    let row = repo::get_comment(&mut tx, id, false)
        .await?
        .ok_or(AppError::NotFound { entity: "comment" })?;
    tx.commit().await?;
    Ok(row)
}

fn screen_in_tx(body: &str, s: &CommunitySettings) -> Result<(), AppError> {
    match dc::spam_code(body, &s.spam, false) {
        Some(code) => Err(violation("body", code)),
        None => Ok(()),
    }
}

pub async fn remove_own_comment(app: &App, p: &Principal, meta: &RequestMeta, id: Uuid) -> Result<(), AppError> {
    let mut tx = AppTx::begin(&app.pool).await?;
    let c = repo::get_comment(&mut tx, id, true)
        .await?
        .ok_or(AppError::NotFound { entity: "comment" })?;
    if c.author_id != p.user_id {
        return Err(AppError::NotFound { entity: "comment" });
    }
    if c.status == "visible" {
        repo::set_comment_status(&mut tx, id, "removed", None).await?;
        record(
            &mut tx,
            p,
            meta,
            "comment.removed",
            "comment",
            id,
            Some(json!({ "status": "visible" })),
            Some(json!({ "status": "removed" })),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Hides a visible comment. The moderator is recorded so an appeal is decided by someone else.
async fn hide(
    conn: &mut PgConnection,
    p: &Principal,
    meta: &RequestMeta,
    c: &CommentRow,
    penalty: Option<(&str, i64)>,
    note: Option<&str>,
) -> Result<(), AppError> {
    repo::set_comment_status(conn, c.id, "hidden", Some(p.user_id)).await?;
    record(
        conn,
        p,
        meta,
        "comment.hidden",
        "comment",
        c.id,
        Some(json!({ "status": c.status })),
        Some(json!({ "status": "hidden", "note": note })),
    )
    .await?;
    if let Some((kind, points)) = penalty {
        repo::add_reputation(conn, c.author_id, kind, points, "comment", c.id).await?;
    }
    Ok(())
}

pub async fn hide_comment(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    id: Uuid,
    note: Option<&str>,
) -> Result<CommentRow, AppError> {
    moderator(app, p, meta, "comment.hide", "comment", Some(id)).await?;
    let mut tx = AppTx::begin(&app.pool).await?;
    let s = load_settings(&mut tx).await?;
    let note = note.map(|n| dc::text("note", n, s.limits.note_chars)).transpose()?;
    let c = repo::get_comment(&mut tx, id, true)
        .await?
        .ok_or(AppError::NotFound { entity: "comment" })?;
    if c.status != "visible" {
        return Err(AppError::Conflict {
            code: "comment_not_visible",
            current_revision_id: None,
        });
    }
    hide(
        &mut tx,
        p,
        meta,
        &c,
        Some(("comment_removed", s.points_for("comment_removed"))),
        note.as_deref(),
    )
    .await?;
    let row = repo::get_comment(&mut tx, id, false)
        .await?
        .ok_or(AppError::NotFound { entity: "comment" })?;
    tx.commit().await?;
    Ok(row)
}

// ---------- reports ----------

#[derive(Debug, Clone)]
pub struct ReportInput {
    pub target_type: String,
    pub target_id: Uuid,
    pub reason: String,
    pub detail: Option<String>,
}

pub async fn report(app: &App, p: &Principal, meta: &RequestMeta, i: ReportInput) -> Result<ReportRow, AppError> {
    let mut tx = AppTx::begin(&app.pool).await?;
    let s = load_settings(&mut tx).await?;
    let mut v = domain::Violations::default();
    if !matches!(i.target_type.as_str(), "contribution" | "comment") {
        v.push("target_type", "invalid_choice");
    }
    if !matches!(
        i.reason.as_str(),
        "spam" | "abuse" | "copyright" | "inaccurate" | "other"
    ) {
        v.push("reason", "invalid_choice");
    }
    let detail = match i.detail.as_deref().filter(|d| !d.trim().is_empty()) {
        Some(d) => v.check(dc::text("detail", d, s.limits.report_detail_chars)),
        None => None,
    };
    v.finish()?;
    gate(&mut tx, p, "report", &s).await?;
    // Only public things can be reported, and a missing one looks the same as a hidden one.
    let author = match i.target_type.as_str() {
        "contribution" => public_target(&mut tx, i.target_id).await?,
        _ => {
            let c = repo::get_comment(&mut tx, i.target_id, false)
                .await?
                .filter(|c| c.status == "visible");
            let c = c.ok_or(AppError::NotFound { entity: "comment" })?;
            public_target(&mut tx, c.contribution_id).await?;
            c.author_id
        }
    };
    if author == p.user_id {
        return Err(violation("target_id", "own_content"));
    }
    throttle(&mut tx, p, "report", &s).await?;
    let row = settle(
        repo::insert_report(
            &mut tx,
            p.user_id,
            &i.target_type,
            i.target_id,
            &i.reason,
            detail.as_deref(),
        )
        .await
        .map_err(Into::into),
    )?;
    record(
        &mut tx,
        p,
        meta,
        "report.created",
        &i.target_type,
        i.target_id,
        None,
        Some(json!({ "report_id": row.id, "reason": i.reason })),
    )
    .await?;
    tx.commit().await?;
    Ok(row)
}

pub async fn report_queue(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    status: Option<&str>,
    cursor: Option<&str>,
    limit: Option<i64>,
) -> Result<Page<ReportRow>, AppError> {
    moderator(app, p, meta, "report.queue", "report", None).await?;
    let status = match status.unwrap_or("open") {
        s @ ("open" | "upheld" | "dismissed") => s,
        _ => return Err(violation("status", "invalid_choice")),
    };
    let limit = pagination::limit(limit)?;
    let mut conn = app.pool.acquire().await?;
    let rows = repo::report_queue(&mut conn, status, after(cursor)?, limit + 1).await?;
    Ok(Page::from_overfetch(rows, limit, |r| Cursor {
        ts: r.created_at,
        id: r.id,
    }))
}

#[derive(Debug, Clone)]
pub struct Resolution {
    pub upheld: bool,
    pub hide_comment: bool,
    pub note: Option<String>,
}

pub async fn resolve_report(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    id: Uuid,
    r: Resolution,
) -> Result<ReportRow, AppError> {
    moderator(app, p, meta, "report.resolve", "report", Some(id)).await?;
    let mut tx = AppTx::begin(&app.pool).await?;
    let s = load_settings(&mut tx).await?;
    let note = r
        .note
        .as_deref()
        .filter(|n| !n.trim().is_empty())
        .map(|n| dc::text("note", n, s.limits.note_chars))
        .transpose()?;
    let rep = repo::get_report(&mut tx, id, true)
        .await?
        .ok_or(AppError::NotFound { entity: "report" })?;
    // A report is decided once; a second moderator racing the first gets a conflict, not a second decision.
    if rep.status != "open" {
        return Err(AppError::Conflict {
            code: "already_resolved",
            current_revision_id: None,
        });
    }
    if r.hide_comment && (!r.upheld || rep.target_type != "comment") {
        return Err(violation("action", "invalid_choice"));
    }
    let status = if r.upheld { "upheld" } else { "dismissed" };
    repo::resolve_report(&mut tx, id, p.user_id, status, note.as_deref()).await?;
    if r.upheld {
        repo::add_reputation(
            &mut tx,
            rep.reporter_id,
            "report_upheld",
            s.points_for("report_upheld"),
            "report",
            id,
        )
        .await?;
        if let Some(author) = repo::target_author(&mut tx, &rep.target_type, rep.target_id).await? {
            repo::add_reputation(
                &mut tx,
                author,
                "report_upheld_against",
                s.points_for("report_upheld_against"),
                &rep.target_type,
                rep.target_id,
            )
            .await?;
        }
        if r.hide_comment {
            if let Some(c) = repo::get_comment(&mut tx, rep.target_id, true)
                .await?
                .filter(|c| c.status == "visible")
            {
                hide(&mut tx, p, meta, &c, None, note.as_deref()).await?;
            }
        }
    }
    record(
        &mut tx, p, meta, "report.resolved", "report", id,
        Some(json!({ "status": "open" })),
        Some(json!({ "status": status, "target_type": rep.target_type, "target_id": rep.target_id, "hide_comment": r.hide_comment })),
    )
    .await?;
    let row = repo::get_report(&mut tx, id, false)
        .await?
        .ok_or(AppError::NotFound { entity: "report" })?;
    tx.commit().await?;
    Ok(row)
}

// ---------- moderator notes ----------

pub async fn add_note(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    subject_type: &str,
    subject: Uuid,
    body: &str,
) -> Result<NoteRow, AppError> {
    moderator(app, p, meta, "note.create", subject_type, Some(subject)).await?;
    let mut tx = AppTx::begin(&app.pool).await?;
    let s = load_settings(&mut tx).await?;
    let body = dc::text("body", body, s.limits.note_chars)?;
    if !repo::subject_exists(&mut tx, subject_type, subject).await? {
        return Err(AppError::NotFound { entity: "subject" });
    }
    let row = repo::insert_note(&mut tx, subject_type, subject, p.user_id, &body).await?;
    record(
        &mut tx,
        p,
        meta,
        "moderator_note.created",
        subject_type,
        subject,
        None,
        Some(json!({ "note_id": row.id })),
    )
    .await?;
    tx.commit().await?;
    Ok(row)
}

pub async fn list_notes(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    subject_type: &str,
    subject: Uuid,
    cursor: Option<&str>,
    limit: Option<i64>,
) -> Result<Page<NoteRow>, AppError> {
    moderator(app, p, meta, "note.list", subject_type, Some(subject)).await?;
    let limit = pagination::limit(limit)?;
    let mut conn = app.pool.acquire().await?;
    let rows = repo::list_notes(&mut conn, subject_type, subject, after(cursor)?, limit + 1).await?;
    Ok(Page::from_overfetch(rows, limit, |r| Cursor {
        ts: r.created_at,
        id: r.id,
    }))
}

// ---------- appeals ----------

pub async fn appeal(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    comment: Uuid,
    message: &str,
) -> Result<AppealRow, AppError> {
    let mut tx = AppTx::begin(&app.pool).await?;
    let s = load_settings(&mut tx).await?;
    let message = dc::text("message", message, s.limits.appeal_chars)?;
    gate(&mut tx, p, "appeal", &s).await?;
    let c = repo::get_comment(&mut tx, comment, false)
        .await?
        .ok_or(AppError::NotFound { entity: "comment" })?;
    // Only the author, and only against a moderator's decision; anyone else sees a missing comment.
    if c.author_id != p.user_id {
        return Err(AppError::NotFound { entity: "comment" });
    }
    if c.status != "hidden" {
        return Err(AppError::Conflict {
            code: "nothing_to_appeal",
            current_revision_id: None,
        });
    }
    throttle(&mut tx, p, "appeal", &s).await?;
    let row = settle(
        repo::insert_appeal(&mut tx, p.user_id, comment, &message)
            .await
            .map_err(Into::into),
    )?;
    record(
        &mut tx,
        p,
        meta,
        "appeal.created",
        "comment",
        comment,
        None,
        Some(json!({ "appeal_id": row.id })),
    )
    .await?;
    tx.commit().await?;
    Ok(row)
}

pub async fn my_appeals(
    app: &App,
    p: &Principal,
    cursor: Option<&str>,
    limit: Option<i64>,
) -> Result<Page<AppealRow>, AppError> {
    let limit = pagination::limit(limit)?;
    let mut conn = app.pool.acquire().await?;
    let rows = repo::my_appeals(&mut conn, p.user_id, after(cursor)?, limit + 1).await?;
    Ok(Page::from_overfetch(rows, limit, |r| Cursor {
        ts: r.created_at,
        id: r.id,
    }))
}

pub async fn appeal_queue(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    cursor: Option<&str>,
    limit: Option<i64>,
) -> Result<Page<AppealRow>, AppError> {
    moderator(app, p, meta, "appeal.queue", "appeal", None).await?;
    let limit = pagination::limit(limit)?;
    let mut conn = app.pool.acquire().await?;
    let rows = repo::appeal_queue(&mut conn, after(cursor)?, limit + 1).await?;
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
    moderator(app, p, meta, "appeal.decide", "appeal", Some(id)).await?;
    let mut tx = AppTx::begin(&app.pool).await?;
    let s = load_settings(&mut tx).await?;
    let note = note
        .filter(|n| !n.trim().is_empty())
        .map(|n| dc::text("note", n, s.limits.note_chars))
        .transpose()?;
    let a = repo::get_appeal(&mut tx, id, true)
        .await?
        .ok_or(AppError::NotFound { entity: "appeal" })?;
    if a.status != "open" {
        return Err(AppError::Conflict {
            code: "already_resolved",
            current_revision_id: None,
        });
    }
    let c = repo::get_comment(&mut tx, a.target_id, true)
        .await?
        .ok_or(AppError::NotFound { entity: "comment" })?;
    // No one reviews their own decision, and no one judges their own appeal.
    if c.hidden_by == Some(p.user_id) || a.appellant_id == p.user_id {
        return Err(AppError::Restricted { code: "own_decision" });
    }
    let status = if uphold { "upheld" } else { "denied" };
    repo::decide_appeal(&mut tx, id, p.user_id, status, note.as_deref()).await?;
    if uphold && c.status == "hidden" {
        repo::set_comment_status(&mut tx, c.id, "visible", None).await?;
        repo::reverse_penalties(&mut tx, c.author_id, "comment", c.id).await?;
    }
    record(
        &mut tx,
        p,
        meta,
        "appeal.decided",
        "comment",
        c.id,
        Some(json!({ "status": "open" })),
        Some(json!({ "appeal_id": id, "status": status })),
    )
    .await?;
    let row = repo::get_appeal(&mut tx, id, false)
        .await?
        .ok_or(AppError::NotFound { entity: "appeal" })?;
    tx.commit().await?;
    Ok(row)
}
