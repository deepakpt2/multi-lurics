//! Entity suggestion, proposals and merges, built on the normalized `search_keys` index.
//! Nothing here merges or creates an entity on its own judgement: likely duplicates are shown and
//! must be confirmed, and a merge is an explicit, versioned, admin-only act.

use crate::catalogue::{record, settle, snapshot};
use crate::localization::{names, resolve_requested, Display};
use crate::pagination::{self, Cursor, Page};
use crate::{authz, App, AppError, RequestMeta};
use domain::localization::EntityType;
use domain::{catalogue as dc, localization as dl, Principal, Role, Violations};
use infrastructure::entities::{self as repo, Moved, ProposalRow};
use infrastructure::localization as loc;
use infrastructure::AppTx;
use serde::Serialize;
use serde_json::json;
use sqlx::PgConnection;
use std::collections::HashMap;
use uuid::Uuid;

/// Every type the picker can search. Languages and scripts are governed reference data.
pub const SUGGEST_TYPES: [&str; 10] = [
    "artist",
    "album",
    "label",
    "publisher",
    "genre",
    "raga",
    "deity",
    "tag",
    "language",
    "script",
];
/// Types that can be proposed and merged.
pub const EDITABLE_TYPES: [&str; 8] = ["artist", "album", "label", "publisher", "genre", "raga", "deity", "tag"];
/// Similarity at or above which an existing entity counts as a likely match.
const LIKELY: f32 = 0.5;
const SUGGEST_DEFAULT: i64 = 8;
const SUGGEST_MAX: i64 = 20;

#[derive(Debug, Clone, Serialize)]
pub struct RelatedWork {
    pub kind: String,
    pub id: Uuid,
    pub title: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalizedName {
    pub language: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct Suggestion {
    pub id: Uuid,
    pub entity_type: String,
    pub display: Display,
    /// `name`, `localization` or `alias`: which of the entity's names matched.
    pub matched_via: String,
    pub score: f32,
    pub kind: Option<String>,
    pub detail: Option<String>,
    pub year: Option<i16>,
    pub code: Option<String>,
    pub localized_names: Vec<LocalizedName>,
    pub related: Vec<RelatedWork>,
}

fn entity_type_of(kind: &str) -> Option<EntityType> {
    EntityType::parse(kind).filter(|t| EDITABLE_TYPES.contains(&t.as_str()))
}

fn check_type(
    v: &mut Violations,
    raw: &str,
    allowed: &[&'static str],
    field: &'static str,
    code: &'static str,
) -> Option<&'static str> {
    match allowed.iter().find(|t| **t == raw) {
        Some(t) => Some(*t),
        None => {
            v.push(field, code);
            None
        }
    }
}

async fn describe(
    conn: &mut PgConnection,
    kind: &'static str,
    hits: Vec<repo::Hit>,
    lang: Option<&str>,
) -> Result<Vec<Suggestion>, AppError> {
    if hits.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<Uuid> = hits.iter().map(|h| h.entity_id).collect();
    let facts: HashMap<Uuid, repo::Facts> = repo::facts(conn, kind, &ids)
        .await?
        .into_iter()
        .map(|f| (f.id, f))
        .collect();
    let display = match entity_type_of(kind) {
        Some(t) => {
            let req = resolve_requested(conn, lang).await?;
            let refs: Vec<_> = facts.values().map(|f| (t, f.id, f.name.clone())).collect();
            Some((t, names(conn, &req, &refs).await?))
        }
        None => None,
    };
    let mut local: HashMap<Uuid, Vec<LocalizedName>> = HashMap::new();
    for n in repo::local_names(conn, kind, &ids).await? {
        let list = local.entry(n.entity_id).or_default();
        if list.len() < 5 {
            list.push(LocalizedName {
                language: n.language,
                name: n.name,
            });
        }
    }
    let mut related: HashMap<Uuid, Vec<RelatedWork>> = HashMap::new();
    for r in repo::related(conn, kind, &ids).await? {
        let list = related.entry(r.entity_id).or_default();
        if list.len() < 3 {
            list.push(RelatedWork {
                kind: r.kind,
                id: r.target_id,
                title: r.title,
            });
        }
    }
    Ok(hits
        .into_iter()
        .filter_map(|h| {
            let f = facts.get(&h.entity_id)?;
            let shown = match &display {
                Some((t, n)) => n.get(*t, f.id, &f.name),
                None => Display {
                    text: f.name.clone(),
                    description: None,
                    language: None,
                    script: None,
                    dir: "auto",
                    resolution: dl::Resolution::Base,
                },
            };
            Some(Suggestion {
                id: f.id,
                entity_type: kind.to_owned(),
                display: shown,
                matched_via: h.origin,
                score: h.score,
                kind: f.kind.clone(),
                detail: f.detail.clone(),
                year: f.year,
                code: f.code.clone(),
                localized_names: local.remove(&f.id).unwrap_or_default(),
                related: related.remove(&f.id).unwrap_or_default(),
            })
        })
        .collect())
}

/// Public: only canonical names and verified localizations/aliases are indexed.
pub async fn suggest(
    app: &App,
    kind: &str,
    q: &str,
    limit: Option<i64>,
    lang: Option<&str>,
) -> Result<Vec<Suggestion>, AppError> {
    let mut v = Violations::default();
    let kind = check_type(&mut v, kind, &SUGGEST_TYPES, "type", "invalid_choice");
    let q = q.trim();
    if q.is_empty() {
        v.push("q", "required");
    } else if q.chars().count() > 200 {
        v.push("q", "too_long");
    }
    let limit = match limit {
        None => SUGGEST_DEFAULT,
        Some(n) if (1..=SUGGEST_MAX).contains(&n) => n,
        Some(_) => {
            v.push("limit", "out_of_range");
            SUGGEST_DEFAULT
        }
    };
    v.finish()?;
    let kind = kind.expect("valid when no violations");
    let mut conn = app.pool.acquire().await?;
    let hits = repo::suggest(&mut conn, kind, q, limit).await?;
    describe(&mut conn, kind, hits, lang).await
}

// ---------- proposals ----------

#[derive(Debug, Default)]
pub struct ProposalInput {
    pub entity_type: String,
    pub name: String,
    pub kind: Option<String>,
    pub disambiguation: Option<String>,
    pub aliases: Vec<String>,
    pub note: Option<String>,
    /// The proposer has seen the likely matches and says this is a different entity.
    pub confirm_distinct: bool,
}

fn opt_text(v: &mut Violations, field: &'static str, raw: Option<&str>, max: usize) -> Option<String> {
    match raw.map(str::trim).filter(|s| !s.is_empty()) {
        None => None,
        Some(s) if s.chars().count() > max => {
            v.push(field, "too_long");
            None
        }
        Some(s) => Some(s.to_owned()),
    }
}

async fn likely_matches(conn: &mut PgConnection, kind: &str, name: &str) -> Result<Vec<Uuid>, AppError> {
    Ok(repo::suggest(conn, kind, name, 5)
        .await?
        .into_iter()
        .filter(|h| h.score >= LIKELY)
        .map(|h| h.entity_id)
        .collect())
}

pub async fn propose(app: &App, p: &Principal, meta: &RequestMeta, i: ProposalInput) -> Result<ProposalRow, AppError> {
    settle(
        async {
            let mut v = Violations::default();
            let kind = check_type(&mut v, &i.entity_type, &EDITABLE_TYPES, "entity_type", "not_proposable");
            let name = v.check(dl::name(&i.name));
            let disambiguation = opt_text(&mut v, "disambiguation", i.disambiguation.as_deref(), 200);
            let note = opt_text(&mut v, "note", i.note.as_deref(), 1000);
            let sub = match kind {
                Some("artist") => i
                    .kind
                    .as_deref()
                    .and_then(dc::ArtistKind::parse)
                    .map(|k| Some(k.as_str().to_owned()))
                    .or_else(|| {
                        v.push("kind", "invalid_choice");
                        None
                    }),
                Some("album") => i
                    .kind
                    .as_deref()
                    .and_then(dc::AlbumKind::parse)
                    .map(|k| Some(k.as_str().to_owned()))
                    .or_else(|| {
                        v.push("kind", "invalid_choice");
                        None
                    }),
                _ => {
                    if i.kind.is_some() {
                        v.push("kind", "not_applicable");
                    }
                    Some(None)
                }
            };
            if disambiguation.is_some() && kind != Some("artist") {
                v.push("disambiguation", "not_applicable");
            }
            let mut aliases: Vec<String> = Vec::new();
            for a in &i.aliases {
                if let Some(a) = v.check(dl::alias(a)) {
                    if !aliases.iter().any(|x| x.to_lowercase() == a.to_lowercase()) {
                        aliases.push(a);
                    }
                }
            }
            if aliases.len() > 10 {
                v.push("aliases", "too_many");
            }
            let mut tx = AppTx::begin(&app.pool).await?;
            if name.is_some() && !repo::has_key(&mut tx, &i.name).await? {
                v.push("name", "no_letters");
            }
            v.finish()?;
            let (kind, name, sub) = (kind.expect("valid"), name.expect("valid"), sub.flatten());

            if let Some(existing) = repo::pending_proposal_like(&mut tx, kind, &name).await? {
                return Err(AppError::Taken {
                    code: "proposal_pending",
                    existing_id: Some(existing),
                });
            }
            let likely = likely_matches(&mut tx, kind, &name).await?;
            if !likely.is_empty() && !i.confirm_distinct {
                return Err(AppError::Ambiguous {
                    code: "likely_duplicates",
                    candidates: likely,
                });
            }
            let row = repo::insert_proposal(
                &mut tx,
                &repo::NewProposal {
                    entity_type: kind,
                    name: &name,
                    kind: sub.as_deref(),
                    disambiguation: disambiguation.as_deref(),
                    aliases: &aliases,
                    note: note.as_deref(),
                    shown_matches: &likely,
                    created_by: p.user_id,
                },
            )
            .await?;
            record(
                &mut tx,
                p,
                meta,
                "entity_proposal.created",
                "entity_proposal",
                row.id,
                None,
                snapshot(&row),
            )
            .await?;
            tx.commit().await?;
            Ok(row)
        }
        .await,
    )
}

pub async fn proposal_queue(
    app: &App,
    p: &Principal,
    cursor: Option<&str>,
    limit: Option<i64>,
) -> Result<Page<ProposalRow>, AppError> {
    authz::require_role(p, Role::Moderator)?;
    let limit = pagination::limit(limit)?;
    let after = cursor.map(Cursor::decode).transpose()?.map(|c| (c.ts, c.id));
    let mut conn = app.pool.acquire().await?;
    let rows = repo::list_pending_proposals(&mut conn, after, limit).await?;
    Ok(Page::from_overfetch(rows, limit, |r| Cursor {
        ts: r.created_at,
        id: r.id,
    }))
}

pub enum ProposalVerdict {
    /// `confirm_distinct`: the moderator has seen the likely matches and approves anyway.
    Approve { confirm_distinct: bool },
    /// `duplicate_of`: the existing entity the proposer should have used.
    Reject { duplicate_of: Option<Uuid> },
}

pub async fn decide_proposal(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    id: Uuid,
    verdict: ProposalVerdict,
    note: Option<&str>,
) -> Result<ProposalRow, AppError> {
    authz::require_role(p, Role::Moderator)?;
    settle(
        async {
            let mut v = Violations::default();
            let note = opt_text(&mut v, "note", note, 1000);
            v.finish()?;
            let mut tx = AppTx::begin(&app.pool).await?;
            let before = repo::get_proposal(&mut tx, id, true).await?.ok_or(AppError::NotFound {
                entity: "entity_proposal",
            })?;
            if before.status != "pending" {
                return Err(AppError::Conflict {
                    code: "invalid_state_transition",
                    current_revision_id: None,
                });
            }
            let (status, entity_id, duplicate_of) = match verdict {
                ProposalVerdict::Approve { confirm_distinct } => {
                    let likely = likely_matches(&mut tx, &before.entity_type, &before.name).await?;
                    if !likely.is_empty() && !confirm_distinct {
                        return Err(AppError::Ambiguous {
                            code: "likely_duplicates",
                            candidates: likely,
                        });
                    }
                    let created = create_entity(&mut tx, p, meta, &before).await?;
                    ("approved", Some(created), None)
                }
                ProposalVerdict::Reject { duplicate_of } => {
                    if let Some(d) = duplicate_of {
                        if !repo::entity_exists(&mut tx, &before.entity_type, d).await? {
                            return Err(domain::DomainError::invalid("duplicate_of_id", "unknown_entity").into());
                        }
                    }
                    ("rejected", None, duplicate_of)
                }
            };
            let row =
                repo::decide_proposal(&mut tx, id, status, p.user_id, note.as_deref(), entity_id, duplicate_of).await?;
            record(
                &mut tx,
                p,
                meta,
                if status == "approved" {
                    "entity_proposal.approved"
                } else {
                    "entity_proposal.rejected"
                },
                "entity_proposal",
                id,
                snapshot(&before),
                snapshot(&row),
            )
            .await?;
            tx.commit().await?;
            Ok(row)
        }
        .await,
    )
}

/// Creates the canonical entity (credited to the proposer) and the proposal's aliases, verified
/// by the approving moderator.
async fn create_entity(
    tx: &mut PgConnection,
    p: &Principal,
    meta: &RequestMeta,
    prop: &ProposalRow,
) -> Result<Uuid, AppError> {
    let kind = prop.entity_type.as_str();
    let (id, row) = match kind {
        "artist" => {
            let r = infrastructure::catalogue::insert_artist(
                tx,
                &infrastructure::catalogue::ArtistData {
                    name: prop.name.clone(),
                    kind: prop.kind.clone().unwrap_or_default(),
                    disambiguation: prop.disambiguation.clone(),
                    isni: None,
                },
                prop.created_by,
            )
            .await?;
            (r.id, snapshot(&r))
        }
        "album" => {
            let r = infrastructure::catalogue::insert_album(
                tx,
                &infrastructure::catalogue::AlbumData {
                    title: prop.name.clone(),
                    kind: prop.kind.clone().unwrap_or_default(),
                    release_year: None,
                    barcode: None,
                },
                prop.created_by,
            )
            .await?;
            (r.id, snapshot(&r))
        }
        _ => {
            let (table, _) = repo::table_of(kind).ok_or(AppError::BadRequest("unsupported_entity_type"))?;
            let r = loc::insert_taxonomy(tx, table, &prop.name, prop.created_by).await?;
            (r.id, snapshot(&r))
        }
    };
    record(tx, p, meta, &format!("{kind}.created"), kind, id, None, row).await?;
    let source = format!("Entity proposal {}", prop.id);
    for alias in &prop.aliases {
        loc::insert_alias(
            tx,
            &loc::NewAlias {
                entity_type: kind,
                entity_id: id,
                language_id: None,
                script_id: None,
                alias,
                source: &source,
                state: "verified",
                created_by: prop.created_by,
                verified_by: Some(p.user_id),
            },
        )
        .await?;
    }
    Ok(id)
}

// ---------- merge ----------

#[derive(Debug, Default)]
pub struct MergeInput {
    pub entity_type: String,
    pub from_id: Uuid,
    pub into_id: Uuid,
    pub expected_from_version: Option<i32>,
    pub expected_into_version: Option<i32>,
    pub note: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct MergeResult {
    pub entity_type: String,
    pub from_id: Uuid,
    pub into_id: Uuid,
    pub moved: Moved,
}

fn conflict(code: &'static str) -> AppError {
    AppError::Conflict {
        code,
        current_revision_id: None,
    }
}

/// Privileged and explicit. Records that disagree on a strong identifier, kind, or track listing
/// are never merged; the admin must resolve those first.
pub async fn merge(app: &App, p: &Principal, meta: &RequestMeta, i: MergeInput) -> Result<MergeResult, AppError> {
    authz::require_role(p, Role::Admin)?;
    settle(
        async {
            let mut v = Violations::default();
            let kind = check_type(&mut v, &i.entity_type, &EDITABLE_TYPES, "entity_type", "not_mergeable");
            if i.from_id == i.into_id {
                v.push("into_id", "same_entity");
            }
            if i.expected_from_version.is_none() {
                v.push("expected_from_version", "required");
            }
            if i.expected_into_version.is_none() {
                v.push("expected_into_version", "required");
            }
            let note = opt_text(&mut v, "note", i.note.as_deref(), 1000);
            v.finish()?;
            let kind = kind.expect("valid");

            let mut tx = AppTx::begin(&app.pool).await?;
            let locked = repo::lock_pair(&mut tx, kind, i.from_id, i.into_id).await?;
            let from = locked.iter().find(|l| l.id == i.from_id);
            let into = locked.iter().find(|l| l.id == i.into_id);
            let (Some(from), Some(into)) = (from, into) else {
                if from.is_none() && repo::already_merged_into(&mut tx, i.from_id).await?.is_some() {
                    return Err(conflict("already_merged"));
                }
                return Err(AppError::NotFound { entity: "entity" });
            };
            for (have, want) in [
                (from.version, i.expected_from_version),
                (into.version, i.expected_into_version),
            ] {
                if Some(have) != want {
                    return Err(AppError::StaleVersion { current_version: have });
                }
            }
            if matches!((&from.strong_id, &into.strong_id), (Some(a), Some(b)) if a != b) {
                return Err(conflict("identifier_mismatch"));
            }
            if from.kind != into.kind {
                return Err(conflict("kind_mismatch"));
            }
            if kind == "album" && repo::track_conflicts(&mut tx, from.id, into.id).await? > 0 {
                return Err(conflict("track_conflict"));
            }
            let moved = repo::merge_into(&mut tx, kind, from, into, p.user_id, note.as_deref()).await?;
            record(
                &mut tx,
                p,
                meta,
                &format!("{kind}.merged"),
                kind,
                into.id,
                Some(from.snapshot.clone()),
                Some(json!({ "into_id": into.id, "from_id": from.id, "moved": moved, "note": note })),
            )
            .await?;
            tx.commit().await?;
            Ok(MergeResult {
                entity_type: kind.to_owned(),
                from_id: from.id,
                into_id: into.id,
                moved,
            })
        }
        .await,
    )
}

/// The surviving entity when `id` was merged away (following chains), for route redirects.
pub async fn redirect_target(app: &App, kind: &str, id: Uuid) -> Result<Option<Uuid>, AppError> {
    if !EDITABLE_TYPES.contains(&kind) {
        return Ok(None);
    }
    let mut conn = app.pool.acquire().await?;
    Ok(repo::resolve_redirect(&mut conn, kind, id).await?)
}
