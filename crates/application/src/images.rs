//! Images for artists, albums, ragas, deities and the like. Uploading is open to any signed-in
//! person; what the public sees is governed the same way as lyrics: moderators and trusted
//! contributors (the lyrics trust policy) publish directly, everyone else waits for review, and a
//! new image never replaces the public one before it is approved.

use crate::catalogue::{record, settle, snapshot};
use crate::media::{self, MediaError};
use crate::pagination::{self, Cursor, Page};
use crate::{authz, lyrics, App, AppError, RequestMeta};
use domain::{DomainError, Principal, Role};
use infrastructure::entities as ent;
use infrastructure::images::{self as repo, ImageRow, NewImage, TYPES};
use infrastructure::AppTx;
use sha2::{Digest, Sha256};
use sqlx::PgConnection;
use std::collections::HashMap;
use uuid::Uuid;

/// Unsent uploads a person may hold at once, and reviews they may have waiting.
const MAX_DRAFTS: i64 = 5;
const MAX_PENDING: i64 = 10;

pub struct UploadInput {
    pub bytes: Vec<u8>,
    pub credit: Option<String>,
    /// The uploader confirms they may share the image.
    pub rights: bool,
    pub entity_type: Option<String>,
    pub entity_id: Option<Uuid>,
}

fn invalid(field: &'static str, code: &'static str) -> AppError {
    DomainError::invalid(field, code).into()
}

/// What an upload is allowed to be, for the upload form.
pub struct Limits {
    pub max_bytes: u64,
    pub min_side: u32,
    pub max_side: u32,
    pub sizes: Vec<u32>,
}

pub fn limits(app: &App) -> Limits {
    Limits {
        max_bytes: app.config.media_max_bytes,
        min_side: media::MIN_SIDE,
        max_side: app.config.media_max_side,
        sizes: app.config.media_sizes.clone(),
    }
}

pub async fn upload(app: &App, p: &Principal, meta: &RequestMeta, i: UploadInput) -> Result<ImageRow, AppError> {
    if !i.rights {
        return Err(invalid("rights", "must_confirm"));
    }
    if i.bytes.is_empty() {
        return Err(invalid("file", "required"));
    }
    if i.bytes.len() as u64 > app.config.media_max_bytes {
        return Err(invalid("file", "too_large"));
    }
    let credit = i.credit.as_deref().map(str::trim).filter(|c| !c.is_empty());
    if credit.is_some_and(|c| c.chars().count() > 300) {
        return Err(invalid("credit", "too_long"));
    }
    let target = match (i.entity_type.as_deref(), i.entity_id) {
        (None, None) => None,
        (Some(t), Some(id)) if TYPES.contains(&t) => Some((t, id)),
        (Some(_), Some(_)) => return Err(invalid("entity_type", "not_applicable")),
        _ => return Err(invalid("entity_id", "required")),
    };
    let sha = format!("{:x}", Sha256::digest(&i.bytes));
    let (bytes_len, sizes, max_side) = (
        i.bytes.len() as i32,
        app.config.media_sizes.clone(),
        app.config.media_max_side,
    );
    let processed = tokio::task::spawn_blocking(move || media::process(&i.bytes, &sizes, max_side))
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .map_err(|e| match e {
            MediaError::UnsupportedType => invalid("file", "unsupported_type"),
            MediaError::Corrupt => invalid("file", "corrupt"),
            MediaError::TooLarge => invalid("file", "dimensions_too_large"),
            MediaError::TooSmall => invalid("file", "dimensions_too_small"),
        })?;

    let id = Uuid::new_v4();
    let stored: Vec<i32> = processed.variants.iter().map(|v| v.0 as i32).collect();
    // Files first, so a row never points at a missing file; they are removed again if the row fails.
    for (size, bytes) in &processed.variants {
        app.media
            .put(&media::key(id, *size), bytes.clone())
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
    }
    let result = settle(
        async {
            let mut tx = AppTx::begin(&app.pool).await?;
            let status = match target {
                None => {
                    if repo::count_for_user(&mut tx, p.user_id, "draft").await? >= MAX_DRAFTS {
                        return Err(AppError::Restricted {
                            code: "too_many_unsent_images",
                        });
                    }
                    "draft"
                }
                Some((t, e)) => {
                    if !ent::entity_exists(&mut tx, t, e).await? {
                        return Err(AppError::NotFound { entity: "entity" });
                    }
                    if repo::same_as_current(&mut tx, t, e, &sha).await? {
                        return Err(AppError::Taken {
                            code: "image_already_current",
                            existing_id: None,
                        });
                    }
                    if repo::mine_pending(&mut tx, t, e, p.user_id).await?.is_some() {
                        return Err(AppError::Taken {
                            code: "image_pending",
                            existing_id: None,
                        });
                    }
                    if lyrics::auto_publishes(&mut tx, p).await? {
                        "approved"
                    } else {
                        if repo::count_for_user(&mut tx, p.user_id, "pending").await? >= MAX_PENDING {
                            return Err(AppError::Restricted {
                                code: "too_many_pending_images",
                            });
                        }
                        "pending"
                    }
                }
            };
            let mut replaced = None;
            if let (Some((t, e)), "approved") = (target, status) {
                repo::lock_entity(&mut tx, e).await?;
                replaced = repo::supersede_current(&mut tx, t, e).await?;
                if let Some(old) = replaced {
                    record(&mut tx, p, meta, "image.superseded", "entity_image", old, None, None).await?;
                }
            }
            let row = repo::insert(
                &mut tx,
                &NewImage {
                    id,
                    entity: target,
                    status,
                    width: processed.width as i32,
                    height: processed.height as i32,
                    bytes: bytes_len,
                    sizes: &stored,
                    sha256: &sha,
                    credit,
                    created_by: p.user_id,
                },
            )
            .await?;
            record(
                &mut tx,
                p,
                meta,
                "image.uploaded",
                "entity_image",
                id,
                None,
                snapshot(&row),
            )
            .await?;
            tx.commit().await?;
            Ok((row, replaced))
        }
        .await,
    );
    match result {
        Ok((row, replaced)) => {
            if let Some(old) = replaced {
                let _ = app.media.delete_prefix(&old.to_string()).await;
            }
            Ok(row)
        }
        Err(e) => {
            let _ = app.media.delete_prefix(&id.to_string()).await;
            Err(e)
        }
    }
}

pub struct QueueItem {
    pub row: ImageRow,
    pub uploader: String,
    /// The entity's (or proposed entity's) stored name.
    pub subject: String,
    pub current_id: Option<Uuid>,
}

pub async fn queue(
    app: &App,
    p: &Principal,
    cursor: Option<&str>,
    limit: Option<i64>,
) -> Result<Page<QueueItem>, AppError> {
    authz::require_role(p, Role::Moderator)?;
    let limit = pagination::limit(limit)?;
    let after = cursor.map(Cursor::decode).transpose()?.map(|c| (c.ts, c.id));
    let mut conn = app.pool.acquire().await?;
    let rows = repo::queue(&mut conn, after, limit).await?;
    let mut by_type: HashMap<String, Vec<Uuid>> = HashMap::new();
    for r in &rows {
        if let (Some(t), Some(e)) = (&r.image.entity_type, r.image.entity_id) {
            by_type.entry(t.clone()).or_default().push(e);
        }
    }
    let mut names: HashMap<Uuid, String> = HashMap::new();
    for (t, ids) in by_type {
        names.extend(repo::entity_names(&mut conn, &t, &ids).await?);
    }
    let items: Vec<QueueItem> = rows
        .into_iter()
        .map(|r| QueueItem {
            subject: r
                .image
                .entity_id
                .and_then(|e| names.get(&e).cloned())
                .or(r.proposal_name)
                .unwrap_or_default(),
            current_id: r.current_id,
            uploader: r.uploader,
            row: r.image,
        })
        .collect();
    Ok(Page::from_overfetch(items, limit, |r| Cursor {
        ts: r.row.created_at,
        id: r.row.id,
    }))
}

pub async fn approve(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    id: Uuid,
    note: Option<&str>,
) -> Result<ImageRow, AppError> {
    authz::require_role(p, Role::Moderator)?;
    let note = note.map(str::trim).filter(|n| !n.is_empty());
    if note.is_some_and(|n| n.chars().count() > 1000) {
        return Err(invalid("note", "too_long"));
    }
    let (row, old) = settle(
        async {
            let mut tx = AppTx::begin(&app.pool).await?;
            let before = repo::get(&mut tx, id, true)
                .await?
                .ok_or(AppError::NotFound { entity: "image" })?;
            if before.status != "pending" {
                return Err(AppError::Conflict {
                    code: "invalid_state_transition",
                    current_revision_id: None,
                });
            }
            let (Some(t), Some(e)) = (before.entity_type.clone(), before.entity_id) else {
                // An image that came with a new-entity proposal is approved with the proposal.
                return Err(AppError::Conflict {
                    code: "decided_with_proposal",
                    current_revision_id: None,
                });
            };
            repo::lock_entity(&mut tx, e).await?;
            let old = repo::supersede_current(&mut tx, &t, e).await?;
            let row = repo::approve(&mut tx, id, None, p.user_id, note).await?;
            record(
                &mut tx,
                p,
                meta,
                "image.approved",
                "entity_image",
                id,
                snapshot(&before),
                snapshot(&row),
            )
            .await?;
            tx.commit().await?;
            Ok((row, old))
        }
        .await,
    )?;
    if let Some(old) = old {
        let _ = app.media.delete_prefix(&old.to_string()).await;
    }
    Ok(row)
}

pub async fn reject(app: &App, p: &Principal, meta: &RequestMeta, id: Uuid, note: &str) -> Result<ImageRow, AppError> {
    authz::require_role(p, Role::Moderator)?;
    let note = note.trim();
    if note.is_empty() {
        return Err(invalid("note", "required"));
    }
    if note.chars().count() > 1000 {
        return Err(invalid("note", "too_long"));
    }
    let row = settle(
        async {
            let mut tx = AppTx::begin(&app.pool).await?;
            let before = repo::get(&mut tx, id, true)
                .await?
                .ok_or(AppError::NotFound { entity: "image" })?;
            // A moderator may also take down a public image.
            if !matches!(before.status.as_str(), "pending" | "approved") {
                return Err(AppError::Conflict {
                    code: "invalid_state_transition",
                    current_revision_id: None,
                });
            }
            let row = repo::reject(&mut tx, id, p.user_id, Some(note)).await?;
            record(
                &mut tx,
                p,
                meta,
                "image.rejected",
                "entity_image",
                id,
                snapshot(&before),
                snapshot(&row),
            )
            .await?;
            tx.commit().await?;
            Ok(row)
        }
        .await,
    )?;
    let _ = app.media.delete_prefix(&id.to_string()).await;
    Ok(row)
}

/// The uploader takes back an image that is still a draft or waiting for review.
pub async fn withdraw(app: &App, p: &Principal, meta: &RequestMeta, id: Uuid) -> Result<(), AppError> {
    settle(
        async {
            let mut tx = AppTx::begin(&app.pool).await?;
            let before = repo::get(&mut tx, id, true)
                .await?
                .ok_or(AppError::NotFound { entity: "image" })?;
            if before.created_by != p.user_id {
                // Not yours: indistinguishable from missing.
                return Err(AppError::NotFound { entity: "image" });
            }
            if !matches!(before.status.as_str(), "draft" | "pending") {
                return Err(AppError::Conflict {
                    code: "invalid_state_transition",
                    current_revision_id: None,
                });
            }
            record(
                &mut tx,
                p,
                meta,
                "image.withdrawn",
                "entity_image",
                id,
                snapshot(&before),
                None,
            )
            .await?;
            // A proposal keeps its place in the queue; it just loses its picture.
            repo::delete(&mut tx, id).await?;
            tx.commit().await?;
            Ok(())
        }
        .await,
    )?;
    let _ = app.media.delete_prefix(&id.to_string()).await;
    Ok(())
}

pub struct EntityImages {
    pub current: Option<Uuid>,
    pub mine: Option<ImageRow>,
}

pub async fn for_entity(
    app: &App,
    p: Option<&Principal>,
    entity_type: &str,
    id: Uuid,
) -> Result<EntityImages, AppError> {
    if !TYPES.contains(&entity_type) {
        return Err(AppError::NotFound { entity: "entity" });
    }
    let mut conn = app.pool.acquire().await?;
    let current = repo::current_for(&mut conn, entity_type, &[id]).await?.remove(&id);
    let mine = match p {
        Some(p) => repo::mine_pending(&mut conn, entity_type, id, p.user_id).await?,
        None => None,
    };
    Ok(EntityImages { current, mine })
}

/// Public images of a set of entities, for lists.
pub async fn current_for(
    conn: &mut PgConnection,
    entity_type: &str,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, Uuid>, AppError> {
    if !TYPES.contains(&entity_type) || ids.is_empty() {
        return Ok(HashMap::new());
    }
    Ok(repo::current_for(conn, entity_type, ids).await?)
}

pub struct MediaFile {
    pub bytes: Vec<u8>,
    /// Anyone may cache it; otherwise it is private to the uploader and moderators.
    pub public: bool,
}

/// The stored file for `id` at the smallest generated size that is at least `width`.
pub async fn file(app: &App, p: Option<&Principal>, id: Uuid, width: u32) -> Result<MediaFile, AppError> {
    let mut conn = app.pool.acquire().await?;
    let row = repo::get(&mut conn, id, false)
        .await?
        .ok_or(AppError::NotFound { entity: "image" })?;
    drop(conn);
    let public = row.status == "approved";
    if !public {
        let allowed = p.is_some_and(|p| p.user_id == row.created_by || p.has_role(Role::Moderator));
        if !allowed || !matches!(row.status.as_str(), "draft" | "pending") {
            return Err(AppError::NotFound { entity: "image" });
        }
    }
    let size = row
        .sizes
        .iter()
        .copied()
        .filter(|s| *s as u32 >= width)
        .min()
        .or_else(|| row.sizes.iter().copied().max())
        .ok_or(AppError::NotFound { entity: "image" })?;
    let bytes = app
        .media
        .get(&media::key(id, size as u32))
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .ok_or(AppError::NotFound { entity: "image" })?;
    Ok(MediaFile { bytes, public })
}

/// Attaches an unsent upload to a new-entity proposal (called inside the proposal's transaction).
pub(crate) async fn check_draft(conn: &mut PgConnection, p: &Principal, id: Uuid) -> Result<(), AppError> {
    match repo::get(conn, id, true).await? {
        Some(r) if r.created_by == p.user_id && r.status == "draft" => Ok(()),
        _ => Err(invalid("image_id", "unknown_image")),
    }
}

pub(crate) async fn attach_to_proposal(conn: &mut PgConnection, id: Uuid, proposal: Uuid) -> Result<(), AppError> {
    Ok(repo::attach_to_proposal(conn, id, proposal).await?)
}

/// Approving a proposal publishes its image for the entity that was just created.
pub(crate) async fn publish_for_new_entity(
    conn: &mut PgConnection,
    p: &Principal,
    meta: &RequestMeta,
    proposal: Uuid,
    entity_type: &str,
    entity_id: Uuid,
) -> Result<(), AppError> {
    let Some(image) = repo::for_proposals(conn, &[proposal]).await?.remove(&proposal) else {
        return Ok(());
    };
    let row = repo::approve(conn, image, Some((entity_type, entity_id)), p.user_id, None).await?;
    record(
        conn,
        p,
        meta,
        "image.approved",
        "entity_image",
        image,
        None,
        snapshot(&row),
    )
    .await
}

/// Rejecting a proposal rejects its image; returns the image id so the files can be removed after commit.
pub(crate) async fn reject_for_proposal(
    conn: &mut PgConnection,
    p: &Principal,
    meta: &RequestMeta,
    proposal: Uuid,
) -> Result<Option<Uuid>, AppError> {
    let Some(image) = repo::for_proposals(conn, &[proposal]).await?.remove(&proposal) else {
        return Ok(None);
    };
    let row = repo::reject(conn, image, p.user_id, Some("proposal rejected")).await?;
    record(
        conn,
        p,
        meta,
        "image.rejected",
        "entity_image",
        image,
        None,
        snapshot(&row),
    )
    .await?;
    Ok(Some(image))
}

pub async fn for_proposals(app: &App, ids: &[Uuid]) -> Result<HashMap<Uuid, Uuid>, AppError> {
    let mut conn = app.pool.acquire().await?;
    Ok(repo::for_proposals(&mut conn, ids).await?)
}

pub(crate) async fn on_merge(
    conn: &mut PgConnection,
    kind: &str,
    from: Uuid,
    into: Uuid,
) -> Result<Vec<Uuid>, AppError> {
    Ok(repo::merge(conn, kind, from, into).await?)
}
