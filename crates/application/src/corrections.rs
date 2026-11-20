//! Suggested corrections to published lyrics. Anyone signed in may suggest one; a moderator applies
//! it (a new revision, published at once) or rejects it. Nothing changes publicly until then.

use crate::catalogue::record;
use crate::lyrics::{self, Decision, RevisionInput, Verdict};
use crate::{authz, App, AppError, RequestMeta};
use chrono::{DateTime, Utc};
use domain::lyrics as dl;
use domain::{Principal, Role, Violations};
use infrastructure::AppTx;
use serde_json::json;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Correction {
    pub id: Uuid,
    pub contribution_id: Uuid,
    pub body: String,
    pub note: Option<String>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    /// The text now public, for the moderator to compare with.
    pub current_body: String,
    pub language: String,
}

const SELECT: &str = "SELECT k.id, k.contribution_id, k.body, k.note, k.created_by, k.created_at,
        r.body AS current_body, coalesce(l.iso639_1, l.iso639_3) AS language
    FROM lyric_corrections k
    JOIN contributions c ON c.id = k.contribution_id
    JOIN lyric_revisions r ON r.id = c.published_revision_id
    JOIN languages l ON l.id = c.language_id";

pub async fn suggest(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    contribution_id: Uuid,
    body: &str,
    note: Option<&str>,
) -> Result<Uuid, AppError> {
    crate::community::throttle_member(app, p, "contribute").await?;
    let mut v = Violations::default();
    let body = v.check(dl::body(body));
    let note = v.check(dl::note("note", note, dl::MAX_NOTE_CHARS)).flatten();
    v.finish()?;
    let body = body.expect("valid");
    let mut tx = AppTx::begin(&app.pool).await?;
    let current: Option<String> = sqlx::query_scalar(
        "SELECT r.body FROM contributions c JOIN lyric_revisions r ON r.id = c.published_revision_id WHERE c.id = $1",
    )
    .bind(contribution_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(current) = current else {
        return Err(AppError::NotFound { entity: "contribution" });
    };
    if current == body {
        return Err(domain::DomainError::invalid("body", "unchanged").into());
    }
    let open: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM lyric_corrections WHERE contribution_id = $1 AND created_by = $2 AND status = 'pending')",
    )
    .bind(contribution_id)
    .bind(p.user_id)
    .fetch_one(&mut *tx)
    .await?;
    if open {
        return Err(domain::DomainError::invalid("body", "correction_pending").into());
    }
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO lyric_corrections (contribution_id, body, note, created_by) VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(contribution_id)
    .bind(&body)
    .bind(note)
    .bind(p.user_id)
    .fetch_one(&mut *tx)
    .await?;
    record(&mut tx, p, meta, "lyric_correction.suggest", "contribution", contribution_id, None, Some(json!({ "correction": id }))).await?;
    tx.commit().await?;
    Ok(id)
}

pub async fn queue(app: &App, p: &Principal) -> Result<Vec<Correction>, AppError> {
    authz::require_role(p, Role::Moderator)?;
    Ok(sqlx::query_as(&format!("{SELECT} WHERE k.status = 'pending' ORDER BY k.created_at, k.id LIMIT 100"))
        .fetch_all(&app.pool)
        .await?)
}

async fn load(app: &App, id: Uuid) -> Result<Correction, AppError> {
    sqlx::query_as(&format!("{SELECT} WHERE k.id = $1 AND k.status = 'pending'"))
        .bind(id)
        .fetch_optional(&app.pool)
        .await?
        .ok_or(AppError::NotFound { entity: "correction" })
}

async fn finish(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    k: &Correction,
    status: &str,
    note: Option<&str>,
) -> Result<(), AppError> {
    let mut tx = AppTx::begin(&app.pool).await?;
    sqlx::query("UPDATE lyric_corrections SET status = $2, decided_by = $3, decided_at = now(), decision_note = $4 WHERE id = $1")
        .bind(k.id)
        .bind(status)
        .bind(p.user_id)
        .bind(note)
        .execute(&mut *tx)
        .await?;
    record(&mut tx, p, meta, &format!("lyric_correction.{status}"), "contribution", k.contribution_id, None, Some(json!({ "correction": k.id }))).await?;
    tx.commit().await?;
    Ok(())
}

/// Makes the suggestion a new revision of the lyric and publishes it.
pub async fn apply(app: &App, p: &Principal, meta: &RequestMeta, id: Uuid, note: Option<&str>) -> Result<(), AppError> {
    authz::require_role(p, Role::Moderator)?;
    let k = load(app, id).await?;
    let head: Option<Uuid> = sqlx::query_scalar("SELECT current_revision_id FROM contributions WHERE id = $1")
        .bind(k.contribution_id)
        .fetch_one(&app.pool)
        .await?;
    let by = format!("Correction suggested by a reader{}", k.note.as_deref().map(|n| format!(": {n}")).unwrap_or_default());
    let revised = lyrics::add_revision(
        app,
        p,
        meta,
        k.contribution_id,
        None,
        RevisionInput { expected_parent_revision_id: head, body: Some(k.body.clone()), restore_from_revision_id: None, note: Some(by.chars().take(500).collect()) },
    )
    .await?;
    // A moderator's revision may publish itself (trust policy); otherwise publish it now.
    let row = &revised.value.row;
    if row.published_revision_id != row.current_revision_id {
        lyrics::decide(
            app,
            p,
            meta,
            k.contribution_id,
            None,
            Verdict::Publish,
            Decision { expected_parent_revision_id: row.current_revision_id, note: None },
        )
        .await?;
    }
    finish(app, p, meta, &k, "applied", note).await
}

pub async fn reject(app: &App, p: &Principal, meta: &RequestMeta, id: Uuid, note: Option<&str>) -> Result<(), AppError> {
    authz::require_role(p, Role::Moderator)?;
    let k = load(app, id).await?;
    finish(app, p, meta, &k, "rejected", note).await
}
