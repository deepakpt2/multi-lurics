//! Localized metadata use cases and the display resolver's I/O side.
//!
//! Display (`names`) is the single path every read uses; it only ever sees *verified*
//! localizations. Writes lock the row, check `expected_parent_revision_id`, validate the whole
//! resulting state, append an immutable revision and audit, all in one transaction.

use crate::catalogue::{record, settle, snapshot};
use crate::pagination::{self, Cursor, Page};
use crate::{App, AppError, RequestMeta};
use domain::localization::{
    self as dl, Candidate, EntityType, LocaleTag, Provenance, Requested, Resolution, VerificationState,
};
use domain::{Principal, Role, Violations};
use infrastructure::localization::{self as repo, *};
use infrastructure::AppTx;
use serde_json::json;
use sqlx::PgConnection;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

// ---------- display ----------

#[derive(Debug, Clone)]
pub struct Display {
    pub text: String,
    pub description: Option<String>,
    /// BCP 47-style tag of the served text (`ml`, `sr-Latn`); `None` for the base name.
    pub language: Option<String>,
    pub script: Option<String>,
    /// `rtl`/`ltr` from the served language or script; `auto` when the language is unknown.
    pub dir: &'static str,
    pub resolution: Resolution,
}

impl Display {
    /// Text is not in the reader's language: the UI must label it.
    pub fn fallback(&self) -> bool {
        self.resolution.is_fallback()
    }

    pub(crate) fn base(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            description: None,
            language: None,
            script: None,
            dir: "auto",
            resolution: Resolution::Base,
        }
    }
}

/// Resolved display for every entity a response mentions.
#[derive(Debug, Default, Clone)]
pub struct Names(HashMap<(EntityType, Uuid), Display>);

impl Names {
    pub fn get(&self, t: EntityType, id: Uuid, base: &str) -> Display {
        self.0.get(&(t, id)).cloned().unwrap_or_else(|| Display::base(base))
    }
}

/// A page plus the names needed to render it.
pub struct Listed<T> {
    pub page: Page<T>,
    pub names: Names,
}

/// `lang` is a tag like `ml`, `ar-EG` or `sr-Latn`; unknown languages are a field error, not a silent default.
pub async fn resolve_requested(conn: &mut PgConnection, lang: Option<&str>) -> Result<Requested, AppError> {
    let Some(raw) = lang.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(Requested::default());
    };
    let tag = LocaleTag::parse(raw)?;
    let language = repo::find_language(conn, &tag.language)
        .await?
        .ok_or_else(|| domain::DomainError::invalid("lang", "unknown_language"))?;
    let script_id = match &tag.script {
        Some(code) => Some(
            repo::find_script(conn, code)
                .await?
                .ok_or_else(|| domain::DomainError::invalid("lang", "unknown_script"))?
                .id,
        ),
        None => None,
    };
    Ok(Requested {
        language_id: Some(language.id),
        script_id,
    })
}

/// Resolver outcome for one entity: chosen localization, how it was reached, entity default language.
type Chosen = (Option<Candidate>, Resolution, Option<Uuid>);

/// Resolves `(type, id, base name)` references with the one fallback algorithm.
pub async fn names(
    conn: &mut PgConnection,
    req: &Requested,
    refs: &[(EntityType, Uuid, String)],
) -> Result<Names, AppError> {
    let mut by_type: HashMap<EntityType, Vec<Uuid>> = HashMap::new();
    let mut seen = HashSet::new();
    for (t, id, _) in refs {
        if seen.insert((*t, *id)) {
            by_type.entry(*t).or_default().push(*id);
        }
    }
    let mut chosen: HashMap<(EntityType, Uuid), Chosen> = HashMap::new();
    for (t, ids) in &by_type {
        let mut cands: HashMap<Uuid, Vec<Candidate>> = HashMap::new();
        for r in repo::verified_candidates(conn, t.as_str(), ids).await? {
            cands.entry(r.entity_id).or_default().push(Candidate {
                language_id: r.language_id,
                script_id: r.script_id,
                name: r.name,
                description: r.description,
                is_preferred: r.is_preferred,
            });
        }
        let defaults: HashMap<Uuid, Option<Uuid>> = repo::default_languages(conn, t.table(), ids)
            .await?
            .into_iter()
            .collect();
        for id in ids {
            let list = cands.remove(id).unwrap_or_default();
            let default = defaults.get(id).copied().flatten();
            let (c, res) = dl::resolve(&list, req, default);
            chosen.insert((*t, *id), (c.cloned(), res, default));
        }
    }
    let lang_ids: Vec<Uuid> = chosen
        .values()
        .filter_map(|(c, _, d)| c.as_ref().map(|c| c.language_id).or(*d))
        .collect();
    let script_ids: Vec<Uuid> = chosen
        .values()
        .filter_map(|(c, _, _)| c.as_ref().and_then(|c| c.script_id))
        .collect();
    let langs: HashMap<Uuid, LangInfo> = repo::language_infos(conn, &lang_ids)
        .await?
        .into_iter()
        .map(|l| (l.id, l))
        .collect();
    let scripts: HashMap<Uuid, ScriptInfo> = repo::script_infos(conn, &script_ids)
        .await?
        .into_iter()
        .map(|s| (s.id, s))
        .collect();

    let base: HashMap<(EntityType, Uuid), &str> = refs.iter().map(|(t, i, b)| ((*t, *i), b.as_str())).collect();
    let mut out = HashMap::new();
    for (key, (cand, resolution, default)) in chosen {
        let display = match cand {
            None => {
                let text = base.get(&key).copied().unwrap_or_default();
                let mut d = Display::base(text);
                // Base text is written in the declared default language, when there is one.
                if let Some(lang) = default
                    .and_then(|l| langs.get(&l))
                    .filter(|_| resolution == Resolution::DefaultLanguage)
                {
                    d.resolution = resolution;
                    d.language = Some(lang.tag.clone());
                    d.dir = if lang.is_rtl { "rtl" } else { "ltr" };
                }
                d
            }
            Some(c) => {
                let lang = langs.get(&c.language_id);
                let script = c.script_id.and_then(|s| scripts.get(&s));
                let tag = lang.map(|l| match script {
                    Some(s) => format!("{}-{}", l.tag, s.code),
                    None => l.tag.clone(),
                });
                let rtl = script.map(|s| s.is_rtl).or(lang.map(|l| l.is_rtl));
                Display {
                    text: c.name,
                    description: c.description,
                    language: tag,
                    script: script.map(|s| s.code.clone()),
                    dir: match rtl {
                        Some(true) => "rtl",
                        Some(false) => "ltr",
                        None => "auto",
                    },
                    resolution,
                }
            }
        };
        out.insert(key, display);
    }
    Ok(Names(out))
}

// ---------- reference data ----------

fn after(cursor: Option<&str>) -> Result<Option<(chrono::DateTime<chrono::Utc>, Uuid)>, AppError> {
    Ok(cursor.map(Cursor::decode).transpose()?.map(|c| (c.ts, c.id)))
}

fn paged<T>(rows: Vec<T>, limit: i64, key: impl Fn(&T) -> Cursor) -> Page<T> {
    Page::from_overfetch(rows, limit, key)
}

pub async fn list_languages(
    app: &App,
    cursor: Option<&str>,
    limit: Option<i64>,
) -> Result<Page<LanguageRow>, AppError> {
    let limit = pagination::limit(limit)?;
    let after = after(cursor)?;
    let mut conn = app.pool.acquire().await?;
    let rows = repo::list_languages(&mut conn, after, limit + 1).await?;
    Ok(paged(rows, limit, |r| Cursor {
        ts: r.created_at,
        id: r.id,
    }))
}

pub async fn list_scripts(app: &App, cursor: Option<&str>, limit: Option<i64>) -> Result<Page<ScriptRow>, AppError> {
    let limit = pagination::limit(limit)?;
    let after = after(cursor)?;
    let mut conn = app.pool.acquire().await?;
    let rows = repo::list_scripts(&mut conn, after, limit + 1).await?;
    Ok(paged(rows, limit, |r| Cursor {
        ts: r.created_at,
        id: r.id,
    }))
}

// ---------- shared ----------

fn is_moderator(p: &Principal) -> bool {
    p.has_role(Role::Moderator)
}

fn entity_type(raw: &str) -> Result<EntityType, AppError> {
    EntityType::parse(raw).ok_or(AppError::NotFound { entity: "entity" })
}

async fn require_entity(conn: &mut PgConnection, t: EntityType, id: Uuid) -> Result<(), AppError> {
    repo::entity_meta(conn, t.table(), id, false)
        .await?
        .map(|_| ())
        .ok_or(AppError::NotFound { entity: "entity" })
}

fn state_filter(raw: Option<&str>) -> Result<Option<VerificationState>, AppError> {
    match raw {
        None => Ok(None),
        Some(s) => VerificationState::parse(s)
            .map(Some)
            .ok_or_else(|| domain::DomainError::invalid("state", "invalid_choice").into()),
    }
}

/// Non-moderators only ever see verified rows. Asking for anything else is refused outright.
fn effective_state(
    viewer: Option<&Principal>,
    asked: Option<VerificationState>,
) -> Result<Option<&'static str>, AppError> {
    if viewer.is_some_and(is_moderator) {
        return Ok(asked.map(VerificationState::as_str));
    }
    match asked {
        None | Some(VerificationState::Verified) => Ok(Some("verified")),
        Some(_) if viewer.is_none() => Err(AppError::Unauthenticated),
        Some(_) => Err(AppError::Forbidden),
    }
}

// ---------- localizations ----------

#[derive(Debug, Clone)]
pub struct LocalizationInput {
    pub language: String,
    pub script: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub is_preferred: Option<bool>,
    pub provenance: String,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct LocalizationPatch {
    pub language: Option<String>,
    pub script: Option<Option<String>>,
    pub name: Option<String>,
    pub description: Option<Option<String>>,
    pub is_preferred: Option<bool>,
    pub provenance: Option<String>,
    pub source: Option<Option<String>>,
}

struct Draft {
    language: String,
    script: Option<String>,
    name: String,
    description: Option<String>,
    provenance: String,
    source: Option<String>,
    is_preferred: Option<bool>,
}

/// Validates every field and resolves language/script codes to ids.
async fn validate(
    conn: &mut PgConnection,
    v: &mut Violations,
    entity: EntityType,
    d: &Draft,
) -> Result<Option<(LocalizationData, Option<bool>)>, AppError> {
    let name = v.check(dl::name(&d.name));
    let description = v.check(dl::description(d.description.as_deref()));
    let source = v.check(dl::source(d.source.as_deref(), entity.is_personal_name()));
    let provenance = match Provenance::parse(&d.provenance) {
        Some(p) => {
            v.check(dl::check_provenance(entity, p));
            Some(p)
        }
        None => {
            v.push("provenance", "invalid_choice");
            None
        }
    };
    let language = if d.language.trim().is_empty() {
        v.push("language", "required");
        None
    } else {
        let l = repo::find_language(conn, d.language.trim()).await?;
        if l.is_none() {
            v.push("language", "unknown_language");
        }
        l
    };
    let script = match d.script.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None => Some(None),
        Some(code) => match repo::find_script(conn, code).await? {
            Some(s) => Some(Some(s.id)),
            None => {
                v.push("script", "unknown_script");
                None
            }
        },
    };
    let (Some(name), Some(description), Some(source), Some(provenance), Some(language), Some(script)) =
        (name, description, source, provenance, language, script)
    else {
        return Ok(None);
    };
    Ok(Some((
        LocalizationData {
            language_id: language.id,
            script_id: script,
            name,
            description,
            is_preferred: false,
            provenance: provenance.as_str().to_owned(),
            source,
        },
        d.is_preferred,
    )))
}

fn taken(code: &'static str, existing: Uuid) -> AppError {
    AppError::Taken {
        code,
        existing_id: Some(existing),
    }
}

/// Members submit (pending); moderators and admins publish directly. Never merges into an existing slot.
pub async fn create_localization(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    entity: &str,
    entity_id: Uuid,
    i: LocalizationInput,
) -> Result<LocalizationRow, AppError> {
    let entity = entity_type(entity)?;
    let id = settle(
        async {
            let mut tx = AppTx::begin(&app.pool).await?;
            require_entity(&mut tx, entity, entity_id).await?;
            let mut v = Violations::default();
            let draft = Draft {
                language: i.language,
                script: i.script,
                name: i.name,
                description: i.description,
                provenance: i.provenance,
                source: i.source,
                is_preferred: i.is_preferred,
            };
            let checked = validate(&mut tx, &mut v, entity, &draft).await?;
            v.finish()?;
            let (mut data, preferred) = checked.expect("valid when no violations");
            if let Some(existing) = repo::slot_holder(
                &mut tx,
                entity.as_str(),
                entity_id,
                data.language_id,
                data.script_id,
                None,
            )
            .await?
            {
                return Err(taken("localization_slot_taken", existing));
            }
            let has = repo::has_preferred(&mut tx, entity.as_str(), entity_id, data.language_id).await?;
            data.is_preferred = match preferred {
                None => !has,
                Some(true) if has => {
                    let holder =
                        repo::preferred_holder(&mut tx, entity.as_str(), entity_id, data.language_id, None).await?;
                    return Err(holder.map_or(AppError::Unavailable, |h| taken("preferred_slot_taken", h)));
                }
                Some(b) => b,
            };
            let (state, verified_by) = if is_moderator(p) {
                (VerificationState::Verified, Some(p.user_id))
            } else {
                (VerificationState::Pending, None)
            };
            let id = repo::insert_localization(
                &mut tx,
                entity.as_str(),
                entity_id,
                &data,
                state.as_str(),
                p.user_id,
                verified_by,
            )
            .await?;
            let rev = repo::append_revision(
                &mut tx,
                id,
                None,
                "created",
                json!({ "data": data, "state": state.as_str() }),
                p.user_id,
            )
            .await?;
            repo::set_current_revision(&mut tx, id, rev).await?;
            record(
                &mut tx,
                p,
                meta,
                "localization.created",
                "localization",
                id,
                None,
                snapshot(&data),
            )
            .await?;
            tx.commit().await?;
            Ok(id)
        }
        .await,
    )?;
    let mut conn = app.pool.acquire().await?;
    repo::get_localization(&mut conn, id, false)
        .await?
        .ok_or(AppError::NotFound { entity: "localization" })
}

fn check_parent(expected: Option<Uuid>, row: &LocalizationRow) -> Result<(), AppError> {
    match expected {
        None => Err(domain::DomainError::invalid("expected_parent_revision_id", "required").into()),
        Some(e) if Some(e) == row.current_revision_id => Ok(()),
        Some(_) => Err(AppError::Conflict {
            code: "stale_revision",
            current_revision_id: row.current_revision_id,
        }),
    }
}

/// Creators edit their own unpublished rows; moderators edit anything. Published rows are moderator-only.
pub async fn update_localization(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    id: Uuid,
    expected_parent: Option<Uuid>,
    patch: LocalizationPatch,
) -> Result<LocalizationRow, AppError> {
    settle(
        async {
            let mut tx = AppTx::begin(&app.pool).await?;
            let cur = repo::get_localization(&mut tx, id, true)
                .await?
                .ok_or(AppError::NotFound { entity: "localization" })?;
            let entity = entity_type(&cur.entity_type)?;
            let moderator = is_moderator(p);
            let own_open = cur.created_by == p.user_id && cur.verification_state != "verified";
            if !(moderator || own_open) {
                return Err(if cur.created_by == p.user_id || cur.verification_state == "verified" {
                    AppError::Forbidden
                } else {
                    AppError::NotFound { entity: "localization" }
                });
            }
            check_parent(expected_parent, &cur)?;
            let mut v = Violations::default();
            let draft = Draft {
                language: patch.language.unwrap_or(cur.language_code.clone()),
                script: patch.script.unwrap_or(cur.script_code.clone()),
                name: patch.name.unwrap_or(cur.name.clone()),
                description: patch.description.unwrap_or(cur.description.clone()),
                provenance: patch.provenance.unwrap_or(cur.provenance.clone()),
                source: patch.source.unwrap_or(cur.source.clone()),
                is_preferred: Some(patch.is_preferred.unwrap_or(cur.is_preferred)),
            };
            let checked = validate(&mut tx, &mut v, entity, &draft).await?;
            v.finish()?;
            let (mut data, preferred) = checked.expect("valid when no violations");
            data.is_preferred = preferred.unwrap_or(false);
            if data == cur.data() {
                tx.commit().await?;
                return Ok(());
            }
            if let Some(existing) = repo::slot_holder(
                &mut tx,
                &cur.entity_type,
                cur.entity_id,
                data.language_id,
                data.script_id,
                Some(id),
            )
            .await?
            {
                return Err(taken("localization_slot_taken", existing));
            }
            if data.is_preferred {
                if let Some(h) =
                    repo::preferred_holder(&mut tx, &cur.entity_type, cur.entity_id, data.language_id, Some(id)).await?
                {
                    return Err(taken("preferred_slot_taken", h));
                }
            }
            // A moderator's edit keeps the current state; anyone else's returns the row to review.
            let (state, verified_by) = if moderator {
                (cur.verification_state.clone(), cur.verified_by)
            } else {
                (VerificationState::Pending.as_str().to_owned(), None)
            };
            repo::update_localization(&mut tx, id, &data, &state, verified_by).await?;
            let rev = repo::append_revision(
                &mut tx,
                id,
                cur.current_revision_id,
                "edited",
                json!({ "data": data, "state": state }),
                p.user_id,
            )
            .await?;
            repo::set_current_revision(&mut tx, id, rev).await?;
            record(
                &mut tx,
                p,
                meta,
                "localization.edited",
                "localization",
                id,
                snapshot(&cur.data()),
                snapshot(&data),
            )
            .await?;
            tx.commit().await?;
            Ok(())
        }
        .await,
    )?;
    let mut conn = app.pool.acquire().await?;
    repo::get_localization(&mut conn, id, false)
        .await?
        .ok_or(AppError::NotFound { entity: "localization" })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Verify,
    Reject,
}

/// Moderator decision. A rejected row gives up its preferred slot so it cannot block others.
pub async fn decide_localization(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    id: Uuid,
    expected_parent: Option<Uuid>,
    verdict: Verdict,
) -> Result<LocalizationRow, AppError> {
    crate::authz::require_role(p, Role::Moderator)?;
    settle(
        async {
            let mut tx = AppTx::begin(&app.pool).await?;
            let cur = repo::get_localization(&mut tx, id, true)
                .await?
                .ok_or(AppError::NotFound { entity: "localization" })?;
            check_parent(expected_parent, &cur)?;
            let (target, action, audit) = match verdict {
                Verdict::Verify => (VerificationState::Verified, "verified", "localization.verified"),
                Verdict::Reject => (VerificationState::Rejected, "rejected", "localization.rejected"),
            };
            if cur.verification_state == target.as_str() {
                return Err(AppError::Conflict {
                    code: "invalid_state_transition",
                    current_revision_id: cur.current_revision_id,
                });
            }
            let mut data = cur.data();
            if verdict == Verdict::Reject {
                data.is_preferred = false;
            }
            let verified_by = (verdict == Verdict::Verify).then_some(p.user_id);
            repo::update_localization(&mut tx, id, &data, target.as_str(), verified_by).await?;
            let rev = repo::append_revision(
                &mut tx,
                id,
                cur.current_revision_id,
                action,
                json!({ "data": data, "state": target.as_str() }),
                p.user_id,
            )
            .await?;
            repo::set_current_revision(&mut tx, id, rev).await?;
            record(
                &mut tx,
                p,
                meta,
                audit,
                "localization",
                id,
                Some(json!({ "state": cur.verification_state })),
                Some(json!({ "state": target.as_str() })),
            )
            .await?;
            tx.commit().await?;
            Ok(())
        }
        .await,
    )?;
    let mut conn = app.pool.acquire().await?;
    repo::get_localization(&mut conn, id, false)
        .await?
        .ok_or(AppError::NotFound { entity: "localization" })
}

/// Pending and rejected rows are visible only to their creator and to moderators; everyone else gets 404.
fn visible(viewer: Option<&Principal>, row: &LocalizationRow) -> bool {
    row.verification_state == "verified" || viewer.is_some_and(|p| is_moderator(p) || p.user_id == row.created_by)
}

pub async fn get_localization(app: &App, viewer: Option<&Principal>, id: Uuid) -> Result<LocalizationRow, AppError> {
    let mut conn = app.pool.acquire().await?;
    repo::get_localization(&mut conn, id, false)
        .await?
        .filter(|r| visible(viewer, r))
        .ok_or(AppError::NotFound { entity: "localization" })
}

pub async fn list_revisions(
    app: &App,
    viewer: Option<&Principal>,
    id: Uuid,
    cursor: Option<&str>,
    limit: Option<i64>,
) -> Result<Page<RevisionRow>, AppError> {
    let limit = pagination::limit(limit)?;
    let after = after(cursor)?;
    get_localization(app, viewer, id).await?;
    let mut conn = app.pool.acquire().await?;
    let rows = repo::list_revisions(&mut conn, id, after, limit + 1).await?;
    Ok(paged(rows, limit, |r| Cursor {
        ts: r.created_at,
        id: r.id,
    }))
}

/// Localizations of one entity. Anonymous callers and members get verified rows only.
pub async fn list_entity_localizations(
    app: &App,
    viewer: Option<&Principal>,
    entity: &str,
    entity_id: Uuid,
    state: Option<&str>,
    cursor: Option<&str>,
    limit: Option<i64>,
) -> Result<Page<LocalizationRow>, AppError> {
    let entity = entity_type(entity)?;
    let limit = pagination::limit(limit)?;
    let after = after(cursor)?;
    let state = effective_state(viewer, state_filter(state)?)?;
    let mut conn = app.pool.acquire().await?;
    require_entity(&mut conn, entity, entity_id).await?;
    let rows = repo::list_localizations(&mut conn, Some((entity.as_str(), entity_id)), state, after, limit + 1).await?;
    Ok(paged(rows, limit, |r| Cursor {
        ts: r.created_at,
        id: r.id,
    }))
}

/// Moderation queue across all entities (default: pending).
pub async fn moderation_queue(
    app: &App,
    p: &Principal,
    state: Option<&str>,
    cursor: Option<&str>,
    limit: Option<i64>,
) -> Result<Page<LocalizationRow>, AppError> {
    crate::authz::require_role(p, Role::Moderator)?;
    let limit = pagination::limit(limit)?;
    let after = after(cursor)?;
    let state = state_filter(state)?.unwrap_or(VerificationState::Pending);
    let mut conn = app.pool.acquire().await?;
    let rows = repo::list_localizations(&mut conn, None, Some(state.as_str()), after, limit + 1).await?;
    Ok(paged(rows, limit, |r| Cursor {
        ts: r.created_at,
        id: r.id,
    }))
}

// ---------- aliases ----------

#[derive(Debug, Clone)]
pub struct AliasInput {
    pub alias: String,
    pub language: Option<String>,
    pub script: Option<String>,
    pub source: String,
}

pub async fn create_alias(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    entity: &str,
    entity_id: Uuid,
    i: AliasInput,
) -> Result<AliasRow, AppError> {
    let entity = entity_type(entity)?;
    let id = settle(
        async {
            let mut tx = AppTx::begin(&app.pool).await?;
            require_entity(&mut tx, entity, entity_id).await?;
            let mut v = Violations::default();
            let alias = v.check(dl::alias(&i.alias));
            let source = v.check(dl::source(Some(&i.source), true)).flatten();
            let language = match i.language.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                None => Some(None),
                Some(c) => match repo::find_language(&mut tx, c).await? {
                    Some(l) => Some(Some(l.id)),
                    None => {
                        v.push("language", "unknown_language");
                        None
                    }
                },
            };
            let script = match i.script.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                None => Some(None),
                Some(c) => match repo::find_script(&mut tx, c).await? {
                    Some(s) => Some(Some(s.id)),
                    None => {
                        v.push("script", "unknown_script");
                        None
                    }
                },
            };
            v.finish()?;
            let (alias, source, language, script) =
                (alias.unwrap(), source.unwrap(), language.unwrap(), script.unwrap());
            let (state, verified_by) = if is_moderator(p) {
                (VerificationState::Verified, Some(p.user_id))
            } else {
                (VerificationState::Pending, None)
            };
            let id = repo::insert_alias(
                &mut tx,
                &NewAlias {
                    entity_type: entity.as_str(),
                    entity_id,
                    language_id: language,
                    script_id: script,
                    alias: &alias,
                    source: &source,
                    state: state.as_str(),
                    created_by: p.user_id,
                    verified_by,
                },
            )
            .await?;
            record(
                &mut tx,
                p,
                meta,
                "alias.created",
                "alias",
                id,
                None,
                Some(json!({ "alias": alias, "source": source, "state": state.as_str() })),
            )
            .await?;
            tx.commit().await?;
            Ok(id)
        }
        .await,
    )?;
    let mut conn = app.pool.acquire().await?;
    repo::get_alias(&mut conn, id, false)
        .await?
        .ok_or(AppError::NotFound { entity: "alias" })
}

pub async fn decide_alias(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    id: Uuid,
    verdict: Verdict,
) -> Result<AliasRow, AppError> {
    crate::authz::require_role(p, Role::Moderator)?;
    let mut tx = AppTx::begin(&app.pool).await?;
    let cur = repo::get_alias(&mut tx, id, true)
        .await?
        .ok_or(AppError::NotFound { entity: "alias" })?;
    let (target, audit) = match verdict {
        Verdict::Verify => (VerificationState::Verified, "alias.verified"),
        Verdict::Reject => (VerificationState::Rejected, "alias.rejected"),
    };
    if cur.verification_state == target.as_str() {
        return Err(AppError::Conflict {
            code: "invalid_state_transition",
            current_revision_id: None,
        });
    }
    repo::set_alias_state(&mut tx, id, target.as_str(), p.user_id).await?;
    record(
        &mut tx,
        p,
        meta,
        audit,
        "alias",
        id,
        Some(json!({ "state": cur.verification_state })),
        Some(json!({ "state": target.as_str() })),
    )
    .await?;
    let row = repo::get_alias(&mut tx, id, false)
        .await?
        .ok_or(AppError::NotFound { entity: "alias" })?;
    tx.commit().await?;
    Ok(row)
}

pub async fn list_aliases(
    app: &App,
    viewer: Option<&Principal>,
    entity: &str,
    entity_id: Uuid,
    state: Option<&str>,
    cursor: Option<&str>,
    limit: Option<i64>,
) -> Result<Page<AliasRow>, AppError> {
    let entity = entity_type(entity)?;
    let limit = pagination::limit(limit)?;
    let after = after(cursor)?;
    let state = effective_state(viewer, state_filter(state)?)?;
    let mut conn = app.pool.acquire().await?;
    require_entity(&mut conn, entity, entity_id).await?;
    let rows = repo::list_aliases(&mut conn, Some((entity.as_str(), entity_id)), state, after, limit + 1).await?;
    Ok(paged(rows, limit, |r| Cursor {
        ts: r.created_at,
        id: r.id,
    }))
}

// ---------- default language ----------

/// Declares the language an entity was originally written in (the second step of the fallback chain).
pub async fn set_default_language(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    entity: &str,
    entity_id: Uuid,
    language: &str,
    expected_version: Option<i32>,
) -> Result<i32, AppError> {
    let entity = entity_type(entity)?;
    settle(
        async {
            let mut tx = AppTx::begin(&app.pool).await?;
            let (version, created_by) = repo::entity_meta(&mut tx, entity.table(), entity_id, true)
                .await?
                .ok_or(AppError::NotFound { entity: "entity" })?;
            if !(is_moderator(p) || created_by == Some(p.user_id)) {
                return Err(AppError::Forbidden);
            }
            match expected_version {
                None => return Err(domain::DomainError::invalid("expected_version", "required").into()),
                Some(v) if v != version => {
                    return Err(AppError::StaleVersion {
                        current_version: version,
                    })
                }
                Some(_) => {}
            }
            let lang = repo::find_language(&mut tx, language)
                .await?
                .ok_or_else(|| domain::DomainError::invalid("language", "unknown_language"))?;
            let before = repo::current_default_language(&mut tx, entity.table(), entity_id).await?;
            let new_version = repo::set_default_language(&mut tx, entity.table(), entity_id, lang.id).await?;
            record(
                &mut tx,
                p,
                meta,
                "entity.default_language_changed",
                entity.as_str(),
                entity_id,
                Some(json!({ "language_id": before })),
                Some(json!({ "language_id": lang.id })),
            )
            .await?;
            tx.commit().await?;
            Ok(new_version)
        }
        .await,
    )
}

// ---------- taxonomy: labels, publishers, genres ----------

fn taxonomy_type(raw: &str) -> Result<EntityType, AppError> {
    match EntityType::parse(raw) {
        Some(t @ (EntityType::Label | EntityType::Publisher | EntityType::Genre)) => Ok(t),
        _ => Err(AppError::NotFound { entity: "taxonomy" }),
    }
}

pub async fn create_taxonomy(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    kind: &str,
    name: &str,
) -> Result<(TaxonomyRow, Names), AppError> {
    let kind = taxonomy_type(kind)?;
    let name = dl::name(name)?;
    let mut tx = AppTx::begin(&app.pool).await?;
    let row = repo::insert_taxonomy(&mut tx, kind.table(), &name, p.user_id).await?;
    record(
        &mut tx,
        p,
        meta,
        &format!("{}.created", kind.as_str()),
        kind.as_str(),
        row.id,
        None,
        Some(json!({ "name": row.name })),
    )
    .await?;
    tx.commit().await?;
    get_taxonomy(app, kind.as_str(), row.id, None).await
}

pub async fn get_taxonomy(
    app: &App,
    kind: &str,
    id: Uuid,
    lang: Option<&str>,
) -> Result<(TaxonomyRow, Names), AppError> {
    let kind = taxonomy_type(kind)?;
    let mut conn = app.pool.acquire().await?;
    let row = repo::get_taxonomy(&mut conn, kind.table(), id)
        .await?
        .ok_or(AppError::NotFound { entity: "taxonomy" })?;
    let req = resolve_requested(&mut conn, lang).await?;
    let names = names(&mut conn, &req, &[(kind, row.id, row.name.clone())]).await?;
    Ok((row, names))
}

pub async fn list_taxonomy(
    app: &App,
    kind: &str,
    cursor: Option<&str>,
    limit: Option<i64>,
    lang: Option<&str>,
) -> Result<Listed<TaxonomyRow>, AppError> {
    let kind = taxonomy_type(kind)?;
    let limit = pagination::limit(limit)?;
    let after = after(cursor)?;
    let mut conn = app.pool.acquire().await?;
    let req = resolve_requested(&mut conn, lang).await?;
    let rows = repo::list_taxonomy(&mut conn, kind.table(), after, limit + 1).await?;
    let page = paged(rows, limit, |r| Cursor {
        ts: r.created_at,
        id: r.id,
    });
    let refs: Vec<_> = page.items.iter().map(|r| (kind, r.id, r.name.clone())).collect();
    let names = names(&mut conn, &req, &refs).await?;
    Ok(Listed { page, names })
}
