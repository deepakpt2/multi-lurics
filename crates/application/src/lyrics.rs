//! Lyric contribution use cases.
//!
//! Every write is one transaction: lock the contribution row, authorize, check
//! `expected_parent_revision_id`, append an immutable revision, move the head, and audit.
//! Anything that can decide a slot's primary also takes an advisory lock on the slot, and
//! the partial unique index `contributions_primary_key` backs that up.

use crate::catalogue::{record, settle, snapshot};
use crate::pagination::{self, Cursor, Page};
use crate::{authz, App, AppError, RequestMeta};
use domain::lyrics::{self as dl, ContributionType, Hunk, Status, TrustPolicy};
use domain::{Principal, Role, Violations};
use infrastructure::lyrics::{self as repo, Claim, ContributionRow, NewContribution, RevisionRow};
use infrastructure::{localization as loc, outbox, AppTx};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::PgConnection;
use uuid::Uuid;

pub use repo::{ContributionRow as Contribution, RevisionRow as Revision};

const TRUST_KEY: &str = "contribution_trust";

// ---------- inputs and views ----------

#[derive(Debug, Clone)]
pub struct ContributionInput {
    pub kind: String,
    pub language: String,
    pub script: String,
    pub body: String,
    pub note: Option<String>,
    pub source_contribution_id: Option<Uuid>,
}

#[derive(Debug, Clone, Default)]
pub struct RevisionInput {
    pub expected_parent_revision_id: Option<Uuid>,
    pub body: Option<String>,
    pub restore_from_revision_id: Option<Uuid>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Decision {
    pub expected_parent_revision_id: Option<Uuid>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Approve,
    Publish,
    Reject,
}

/// A contribution as one viewer may see it. `published` is what the public reads;
/// `head` (the newest revision, possibly unpublished) only reaches the author and moderators.
#[derive(Debug, Clone)]
pub struct ContributionView {
    pub row: ContributionRow,
    pub published: Option<RevisionRow>,
    pub head: Option<RevisionRow>,
    pub privileged: bool,
}

/// Result of a retryable write: `replayed` is true when an earlier identical request is returned.
#[derive(Debug, Clone)]
pub struct Outcome<T> {
    pub value: T,
    pub replayed: bool,
}

#[derive(Debug, Clone)]
pub struct RevisionDiff {
    pub from: RevisionRow,
    pub to: RevisionRow,
    pub hunks: Vec<Hunk>,
}

fn is_moderator(p: &Principal) -> bool {
    p.has_role(Role::Moderator)
}

fn privileged(viewer: Option<&Principal>, row: &ContributionRow) -> bool {
    viewer.is_some_and(|p| is_moderator(p) || p.user_id == row.created_by)
}

fn not_found() -> AppError {
    AppError::NotFound { entity: "contribution" }
}

fn after(cursor: Option<&str>) -> Result<Option<(chrono::DateTime<chrono::Utc>, Uuid)>, AppError> {
    Ok(cursor.map(Cursor::decode).transpose()?.map(|c| (c.ts, c.id)))
}

fn page<T>(rows: Vec<T>, limit: i64, key: impl Fn(&T) -> Cursor) -> Page<T> {
    Page::from_overfetch(rows, limit, key)
}

// ---------- idempotency ----------

struct Idem {
    key: Option<String>,
    replay: Option<Uuid>,
}

fn fingerprint(parts: serde_json::Value) -> String {
    let mut h = Sha256::new();
    h.update(parts.to_string().as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Claims `key` inside the transaction. A repeat of the same request yields the earlier
/// result id; the same key with a different request is rejected, never silently reused.
async fn begin_idem(
    conn: &mut PgConnection,
    p: &Principal,
    key: Option<&str>,
    kind: &str,
    parts: serde_json::Value,
) -> Result<Idem, AppError> {
    let Some(key) = key else {
        return Ok(Idem {
            key: None,
            replay: None,
        });
    };
    dl::idempotency_key(key)?;
    let fp = fingerprint(parts);
    match repo::claim_key(conn, p.user_id, key, &fp).await? {
        Claim::Fresh => Ok(Idem {
            key: Some(key.to_owned()),
            replay: None,
        }),
        Claim::Seen {
            fingerprint,
            result_kind,
            result_id,
        } => {
            if fingerprint != fp || result_kind.as_deref() != Some(kind) {
                return Err(domain::DomainError::invalid("idempotency_key", "reused_with_different_request").into());
            }
            Ok(Idem {
                key: None,
                replay: result_id,
            })
        }
    }
}

async fn end_idem(conn: &mut PgConnection, p: &Principal, i: &Idem, kind: &str, id: Uuid) -> Result<(), AppError> {
    if let Some(k) = &i.key {
        repo::finish_key(conn, p.user_id, k, kind, id).await?;
    }
    Ok(())
}

// ---------- trust policy ----------

async fn load_policy(conn: &mut PgConnection) -> Result<TrustPolicy, AppError> {
    let v = repo::get_setting(conn, TRUST_KEY)
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("setting {TRUST_KEY} is missing")))?;
    serde_json::from_value(v).map_err(|e| AppError::Internal(anyhow::anyhow!("setting {TRUST_KEY} is invalid: {e}")))
}

pub async fn get_trust_policy(app: &App, p: &Principal) -> Result<TrustPolicy, AppError> {
    authz::require_role(p, Role::Admin)?;
    let mut conn = app.pool.acquire().await?;
    load_policy(&mut conn).await
}

pub async fn set_trust_policy(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    policy: TrustPolicy,
) -> Result<TrustPolicy, AppError> {
    authz::require_role(p, Role::Admin)?;
    policy.validate()?;
    let mut tx = AppTx::begin(&app.pool).await?;
    let before = load_policy(&mut tx).await?;
    let value = serde_json::to_value(&policy).map_err(|e| AppError::Internal(e.into()))?;
    repo::put_setting(&mut tx, TRUST_KEY, &value, p.user_id).await?;
    record(
        &mut tx,
        p,
        meta,
        "settings.trust_changed",
        "settings",
        Uuid::nil(),
        snapshot(&before),
        snapshot(&policy),
    )
    .await?;
    tx.commit().await?;
    Ok(policy)
}

async fn auto_publishes(conn: &mut PgConnection, p: &Principal) -> Result<bool, AppError> {
    let policy = load_policy(conn).await?;
    let count = if policy.auto_publish_min_published.is_some() {
        repo::published_count(conn, p.user_id).await?
    } else {
        0
    };
    Ok(policy.auto_publishes(p, count))
}

// ---------- publishing ----------

/// Makes `revision` the public one. The slot lock serialises the "is there a primary yet?"
/// decision, so two contributions racing for an empty slot cannot both become primary.
async fn publish_head(
    conn: &mut PgConnection,
    p: &Principal,
    meta: &RequestMeta,
    c: &ContributionRow,
    revision: Uuid,
    note: Option<&str>,
) -> Result<bool, AppError> {
    repo::lock_slot(conn, c.recording_id, &c.kind, c.language_id, c.script_id).await?;
    let primary = repo::slot_primary(conn, c.recording_id, &c.kind, c.language_id, c.script_id)
        .await?
        .is_none();
    repo::publish(conn, c.id, revision, p.user_id, primary, note).await?;
    record(
        conn,
        p,
        meta,
        "contribution.published",
        "contribution",
        c.id,
        Some(json!({ "status": c.status, "published_revision_id": c.published_revision_id })),
        Some(
            json!({ "status": "published", "published_revision_id": revision, "is_primary": c.is_primary || primary }),
        ),
    )
    .await?;
    outbox::enqueue(
        conn,
        "lyrics.published",
        "contribution",
        c.id,
        json!({ "contribution_id": c.id, "revision_id": revision, "recording_id": c.recording_id, "primary": c.is_primary || primary }),
    )
    .await?;
    Ok(primary)
}

// ---------- reads ----------

/// Hides `source_contribution_id` when it points at work the public cannot see.
async fn hide_private_sources(conn: &mut PgConnection, rows: &mut [ContributionRow]) -> Result<(), AppError> {
    let ids: Vec<Uuid> = rows.iter().filter_map(|r| r.source_contribution_id).collect();
    if ids.is_empty() {
        return Ok(());
    }
    let public = repo::published_ids(conn, &ids).await?;
    for r in rows {
        if r.source_contribution_id.is_some_and(|s| !public.contains(&s)) {
            r.source_contribution_id = None;
        }
    }
    Ok(())
}

async fn view(
    conn: &mut PgConnection,
    row: ContributionRow,
    viewer: Option<&Principal>,
) -> Result<ContributionView, AppError> {
    let privileged = privileged(viewer, &row);
    let mut row = row;
    if !privileged {
        hide_private_sources(conn, std::slice::from_mut(&mut row)).await?;
    }
    let published = match row.published_revision_id {
        Some(id) => repo::get_revision(conn, id).await?,
        None => None,
    };
    let head = match (privileged, row.current_revision_id) {
        (true, Some(h)) if Some(h) == row.published_revision_id => published.clone(),
        (true, Some(h)) => repo::get_revision(conn, h).await?,
        _ => None,
    };
    Ok(ContributionView {
        row,
        published,
        head,
        privileged,
    })
}

/// Anonymous callers and other members see published contributions only; a missing and a
/// hidden contribution are indistinguishable (404).
async fn visible_row(
    conn: &mut PgConnection,
    viewer: Option<&Principal>,
    id: Uuid,
) -> Result<ContributionRow, AppError> {
    repo::get_contribution(conn, id, false)
        .await?
        .filter(|r| r.published_revision_id.is_some() || privileged(viewer, r))
        .ok_or_else(not_found)
}

pub async fn get_contribution(app: &App, viewer: Option<&Principal>, id: Uuid) -> Result<ContributionView, AppError> {
    let mut conn = app.pool.acquire().await?;
    let row = visible_row(&mut conn, viewer, id).await?;
    view(&mut conn, row, viewer).await
}

pub async fn list_for_recording(
    app: &App,
    recording_id: Uuid,
    kind: Option<&str>,
    language: Option<&str>,
    cursor: Option<&str>,
    limit: Option<i64>,
) -> Result<Page<ContributionRow>, AppError> {
    let limit = pagination::limit(limit)?;
    let after = after(cursor)?;
    let kind = kind
        .map(|k| {
            ContributionType::parse(k)
                .map(ContributionType::as_str)
                .ok_or(domain::DomainError::invalid("type", "invalid_choice"))
        })
        .transpose()?;
    let mut conn = app.pool.acquire().await?;
    if !repo::recording_exists(&mut conn, recording_id).await? {
        return Err(AppError::NotFound { entity: "recording" });
    }
    let language = match language {
        Some(code) => Some(
            loc::find_language(&mut conn, code)
                .await?
                .ok_or(domain::DomainError::invalid("language", "unknown_language"))?
                .id,
        ),
        None => None,
    };
    let mut rows = repo::list_published(&mut conn, recording_id, kind, language, after, limit + 1).await?;
    hide_private_sources(&mut conn, &mut rows).await?;
    Ok(page(rows, limit, |r| Cursor {
        ts: r.created_at,
        id: r.id,
    }))
}

/// The author's own contributions in any state, so they can follow their submissions.
pub async fn list_mine(
    app: &App,
    p: &Principal,
    cursor: Option<&str>,
    limit: Option<i64>,
) -> Result<Page<ContributionRow>, AppError> {
    let limit = pagination::limit(limit)?;
    let after = after(cursor)?;
    let mut conn = app.pool.acquire().await?;
    let rows = repo::list_by_creator(&mut conn, p.user_id, after, limit + 1).await?;
    Ok(page(rows, limit, |r| Cursor {
        ts: r.created_at,
        id: r.id,
    }))
}

pub async fn moderation_queue(
    app: &App,
    p: &Principal,
    cursor: Option<&str>,
    limit: Option<i64>,
) -> Result<Page<ContributionRow>, AppError> {
    authz::require_role(p, Role::Moderator)?;
    let limit = pagination::limit(limit)?;
    let after = after(cursor)?;
    let mut conn = app.pool.acquire().await?;
    let rows = repo::list_queue(&mut conn, after, limit + 1).await?;
    Ok(page(rows, limit, |r| Cursor {
        ts: r.created_at,
        id: r.id,
    }))
}

/// History, newest first. The public sees only revisions that were ever published, so an
/// unreviewed draft never leaks through an ancestor link.
pub async fn list_revisions(
    app: &App,
    viewer: Option<&Principal>,
    id: Uuid,
    cursor: Option<&str>,
    limit: Option<i64>,
) -> Result<Page<RevisionRow>, AppError> {
    let limit = pagination::limit(limit)?;
    let after = after(cursor)?;
    let mut conn = app.pool.acquire().await?;
    let row = visible_row(&mut conn, viewer, id).await?;
    let rows = repo::list_revisions(&mut conn, id, !privileged(viewer, &row), after, limit + 1).await?;
    Ok(page(rows, limit, |r| Cursor {
        ts: r.created_at,
        id: r.id,
    }))
}

async fn visible_revision(
    conn: &mut PgConnection,
    viewer: Option<&Principal>,
    id: Uuid,
) -> Result<RevisionRow, AppError> {
    let rev = repo::get_revision(conn, id).await?.ok_or_else(not_found)?;
    let row = visible_row(conn, viewer, rev.contribution_id).await?;
    if rev.published || privileged(viewer, &row) {
        Ok(rev)
    } else {
        Err(not_found())
    }
}

pub async fn get_revision(app: &App, viewer: Option<&Principal>, id: Uuid) -> Result<RevisionRow, AppError> {
    let mut conn = app.pool.acquire().await?;
    visible_revision(&mut conn, viewer, id).await
}

/// Line diff between two revisions of one contribution. Comparing across contributions is
/// refused: alternatives are never merged or blended.
pub async fn diff(app: &App, viewer: Option<&Principal>, a: Uuid, b: Uuid) -> Result<RevisionDiff, AppError> {
    let mut conn = app.pool.acquire().await?;
    let from = visible_revision(&mut conn, viewer, a).await?;
    let to = visible_revision(&mut conn, viewer, b).await?;
    if from.contribution_id != to.contribution_id {
        return Err(domain::DomainError::invalid("revision", "different_contribution").into());
    }
    let hunks = dl::diff(&from.body, &to.body);
    Ok(RevisionDiff { from, to, hunks })
}

// ---------- writes ----------

async fn resolve_slot(
    conn: &mut PgConnection,
    v: &mut Violations,
    i: &ContributionInput,
) -> Result<Option<(ContributionType, Uuid, Uuid)>, AppError> {
    let kind = ContributionType::parse(i.kind.trim());
    if kind.is_none() {
        v.push(
            "type",
            if i.kind.trim().is_empty() {
                "required"
            } else {
                "invalid_choice"
            },
        );
    }
    let language = loc::find_language(conn, i.language.trim()).await?;
    if language.is_none() {
        v.push(
            "language",
            if i.language.trim().is_empty() {
                "required"
            } else {
                "unknown_language"
            },
        );
    }
    let script = loc::find_script(conn, i.script.trim()).await?;
    if script.is_none() {
        v.push(
            "script",
            if i.script.trim().is_empty() {
                "required"
            } else {
                "unknown_script"
            },
        );
    }
    Ok(match (kind, language, script) {
        (Some(k), Some(l), Some(s)) => Some((k, l.id, s.id)),
        _ => None,
    })
}

/// Creates a contribution with its first revision. Translations and transliterations never
/// need an original to exist first; `source_contribution_id` is only a pointer.
pub async fn create_contribution(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    recording_id: Uuid,
    idempotency_key: Option<&str>,
    i: ContributionInput,
) -> Result<Outcome<ContributionView>, AppError> {
    let parts = json!({ "op": "create", "recording": recording_id, "in": [&i.kind, &i.language, &i.script, &i.body, &i.note, i.source_contribution_id] });
    let id = settle(
        async {
            let mut tx = AppTx::begin(&app.pool).await?;
            let idem = begin_idem(&mut tx, p, idempotency_key, "contribution", parts).await?;
            if let Some(id) = idem.replay {
                return Ok((id, true));
            }
            if !repo::recording_exists(&mut tx, recording_id).await? {
                return Err(AppError::NotFound { entity: "recording" });
            }
            let mut v = Violations::default();
            let slot = resolve_slot(&mut tx, &mut v, &i).await?;
            let body = v.check(dl::body(&i.body));
            let note = v.check(dl::note("note", i.note.as_deref(), dl::MAX_NOTE_CHARS)).flatten();
            if let (Some(source), Some((kind, ..))) = (i.source_contribution_id, slot) {
                let src = repo::get_contribution(&mut tx, source, false)
                    .await?
                    .filter(|s| s.recording_id == recording_id && (s.published_revision_id.is_some() || privileged(Some(p), s)));
                if src.is_none() {
                    v.push("source_contribution_id", "invalid_source");
                } else if kind == ContributionType::Original {
                    v.push("source_contribution_id", "not_allowed_for_original");
                }
            }
            v.finish()?;
            let ((kind, language_id, script_id), body) = (slot.expect("valid"), body.expect("valid"));

            let auto = auto_publishes(&mut tx, p).await?;
            if auto {
                // Before inserting: whoever creates into a slot queues behind the current decider.
                repo::lock_slot(&mut tx, recording_id, kind.as_str(), language_id, script_id).await?;
            }
            let id = repo::insert_contribution(
                &mut tx,
                &NewContribution {
                    recording_id,
                    kind: kind.as_str().to_owned(),
                    language_id,
                    script_id,
                    source_contribution_id: i.source_contribution_id,
                    created_by: p.user_id,
                },
            )
            .await?;
            let rev = repo::insert_revision(&mut tx, id, None, &body, note.as_deref(), None, p.user_id).await?;
            repo::set_head(&mut tx, id, rev, Status::Pending.as_str(), None).await?;
            let row = repo::get_contribution(&mut tx, id, false).await?.ok_or_else(not_found)?;
            record(
                &mut tx,
                p,
                meta,
                "contribution.created",
                "contribution",
                id,
                None,
                Some(json!({ "recording_id": recording_id, "type": kind.as_str(), "language": row.language, "script": row.script, "revision_id": rev })),
            )
            .await?;
            if auto {
                publish_head(&mut tx, p, meta, &row, rev, None).await?;
            }
            end_idem(&mut tx, p, &idem, "contribution", id).await?;
            tx.commit().await?;
            Ok((id, false))
        }
        .await,
    )?;
    let (id, replayed) = id;
    let mut conn = app.pool.acquire().await?;
    let row = repo::get_contribution(&mut conn, id, false)
        .await?
        .ok_or_else(not_found)?;
    Ok(Outcome {
        value: view(&mut conn, row, Some(p)).await?,
        replayed,
    })
}

fn check_parent(expected: Option<Uuid>, row: &ContributionRow) -> Result<(), AppError> {
    match expected {
        None => Err(domain::DomainError::invalid("expected_parent_revision_id", "required").into()),
        Some(e) if Some(e) == row.current_revision_id => Ok(()),
        Some(_) => Err(AppError::Conflict {
            code: "stale_revision",
            current_revision_id: row.current_revision_id,
        }),
    }
}

/// Authors edit their own contributions and moderators any. Everyone else gets 404 for an
/// unpublished contribution and 403 for a published one (they may add an alternative instead).
fn require_author(p: &Principal, row: &ContributionRow) -> Result<(), AppError> {
    if privileged(Some(p), row) {
        Ok(())
    } else if row.published_revision_id.is_some() {
        Err(AppError::Forbidden)
    } else {
        Err(not_found())
    }
}

/// Appends a revision (a new body, or a restore of an older one). Restoring never rewrites
/// history: it is a new revision whose body equals the old one.
pub async fn add_revision(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    id: Uuid,
    idempotency_key: Option<&str>,
    i: RevisionInput,
) -> Result<Outcome<ContributionView>, AppError> {
    let parts = json!({ "op": "revise", "id": id, "in": [i.expected_parent_revision_id, &i.body, i.restore_from_revision_id, &i.note] });
    let (id, replayed) = settle(
        async {
            let mut tx = AppTx::begin(&app.pool).await?;
            let idem = begin_idem(&mut tx, p, idempotency_key, "contribution", parts).await?;
            if let Some(done) = idem.replay {
                return Ok((done, true));
            }
            let row = repo::get_contribution(&mut tx, id, true).await?.ok_or_else(not_found)?;
            require_author(p, &row)?;
            check_parent(i.expected_parent_revision_id, &row)?;

            let mut v = Violations::default();
            let note = v
                .check(dl::note("note", i.note.as_deref(), dl::MAX_NOTE_CHARS))
                .flatten();
            let (body, restored_from) = match (&i.body, i.restore_from_revision_id) {
                (Some(b), None) => (v.check(dl::body(b)), None),
                (None, Some(r)) => match repo::get_revision(&mut tx, r)
                    .await?
                    .filter(|x| x.contribution_id == id)
                {
                    Some(old) => (Some(old.body), Some(r)),
                    None => {
                        v.push("restore_from_revision_id", "invalid_revision");
                        (None, None)
                    }
                },
                (None, None) => {
                    v.push("body", "required");
                    (None, None)
                }
                (Some(_), Some(_)) => {
                    v.push("restore_from_revision_id", "conflicts_with_body");
                    (None, None)
                }
            };
            let head = match row.current_revision_id {
                Some(h) => repo::get_revision(&mut tx, h).await?,
                None => None,
            };
            if body
                .as_deref()
                .is_some_and(|b| head.as_ref().is_some_and(|h| h.body == b))
            {
                v.push("body", "unchanged");
            }
            v.finish()?;
            let body = body.expect("valid");

            let rev = repo::insert_revision(
                &mut tx,
                id,
                row.current_revision_id,
                &body,
                note.as_deref(),
                restored_from,
                p.user_id,
            )
            .await?;
            repo::set_head(&mut tx, id, rev, Status::Pending.as_str(), None).await?;
            record(
                &mut tx,
                p,
                meta,
                if restored_from.is_some() {
                    "contribution.restored"
                } else {
                    "contribution.revised"
                },
                "contribution",
                id,
                Some(json!({ "revision_id": row.current_revision_id, "status": row.status })),
                Some(json!({ "revision_id": rev, "restored_from": restored_from })),
            )
            .await?;
            if auto_publishes(&mut tx, p).await? {
                let fresh = repo::get_contribution(&mut tx, id, false)
                    .await?
                    .ok_or_else(not_found)?;
                publish_head(&mut tx, p, meta, &fresh, rev, None).await?;
            }
            end_idem(&mut tx, p, &idem, "contribution", id).await?;
            tx.commit().await?;
            Ok((id, false))
        }
        .await,
    )?;
    let mut conn = app.pool.acquire().await?;
    let row = repo::get_contribution(&mut conn, id, false)
        .await?
        .ok_or_else(not_found)?;
    Ok(Outcome {
        value: view(&mut conn, row, Some(p)).await?,
        replayed,
    })
}

/// Approve / publish / reject the head revision. `expected_parent_revision_id` is the head the
/// moderator reviewed; if the author edited meanwhile, the decision is stale (409), not applied.
pub async fn decide(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    id: Uuid,
    idempotency_key: Option<&str>,
    verdict: Verdict,
    d: Decision,
) -> Result<Outcome<ContributionView>, AppError> {
    authz::require_role(p, Role::Moderator)?;
    let parts = json!({ "op": format!("{verdict:?}"), "id": id, "in": [d.expected_parent_revision_id, &d.note] });
    let (id, replayed) = settle(
        async {
            let mut tx = AppTx::begin(&app.pool).await?;
            let idem = begin_idem(&mut tx, p, idempotency_key, "contribution", parts).await?;
            if let Some(done) = idem.replay {
                return Ok((done, true));
            }
            let row = repo::get_contribution(&mut tx, id, true).await?.ok_or_else(not_found)?;
            check_parent(d.expected_parent_revision_id, &row)?;
            let note = dl::note("note", d.note.as_deref(), 1000)?;
            let head = row
                .current_revision_id
                .ok_or_else(|| AppError::Internal(anyhow::anyhow!("contribution without head")))?;
            let allowed = match verdict {
                Verdict::Approve => row.status == "pending",
                Verdict::Publish | Verdict::Reject => matches!(row.status.as_str(), "pending" | "approved"),
            };
            if !allowed {
                return Err(AppError::Conflict {
                    code: "invalid_state_transition",
                    current_revision_id: row.current_revision_id,
                });
            }
            match verdict {
                Verdict::Publish => {
                    publish_head(&mut tx, p, meta, &row, head, note.as_deref()).await?;
                }
                Verdict::Approve | Verdict::Reject => {
                    let (status, action) = if verdict == Verdict::Approve {
                        (Status::Approved, "contribution.approved")
                    } else {
                        (Status::Rejected, "contribution.rejected")
                    };
                    repo::set_status(&mut tx, id, status.as_str(), note.as_deref()).await?;
                    record(
                        &mut tx,
                        p,
                        meta,
                        action,
                        "contribution",
                        id,
                        Some(json!({ "status": row.status })),
                        Some(json!({ "status": status.as_str(), "revision_id": head, "note": note })),
                    )
                    .await?;
                }
            }
            end_idem(&mut tx, p, &idem, "contribution", id).await?;
            tx.commit().await?;
            Ok((id, false))
        }
        .await,
    )?;
    let mut conn = app.pool.acquire().await?;
    let row = repo::get_contribution(&mut conn, id, false)
        .await?
        .ok_or_else(not_found)?;
    Ok(Outcome {
        value: view(&mut conn, row, Some(p)).await?,
        replayed,
    })
}
