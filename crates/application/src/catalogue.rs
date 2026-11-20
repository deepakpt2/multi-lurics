//! Catalogue use cases: artists, compositions, recordings, albums, credits, relationships.
//!
//! Every write runs in one transaction that locks the aggregate row, checks authorization and
//! `expected_version`, validates the full resulting state, writes, and audits. Database
//! constraints back up each rule, so a lost race surfaces as a typed conflict, never as bad data.

use crate::localization::{self as loc, Listed, Names};
use crate::pagination::{self, Cursor, Page};
use crate::{authz, App, AppError, RequestMeta};
use domain::catalogue::{self as dc, AlbumKind, ArtistKind, CreditRole, CreditTarget, RelationshipKind};
use domain::localization::EntityType;
use domain::{Principal, Violations};
use infrastructure::audit::{self, AuditEntry};
use infrastructure::catalogue::{self as repo, *};
use infrastructure::AppTx;
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::PgConnection;
use std::collections::HashSet;
use uuid::Uuid;

const MAX_CREDITS: usize = 100;
const MAX_RELATIONSHIPS: usize = 50;
const MAX_TRACKS: usize = 999;
const DETAIL_CAP: i64 = 100;

// ---------- inputs ----------

#[derive(Debug, Clone)]
pub struct CreditInput {
    pub artist_id: Uuid,
    pub role: String,
}

#[derive(Debug, Clone)]
pub struct RelationshipInput {
    pub kind: String,
    pub related_recording_id: Uuid,
}

/// For PATCH: `None` = leave unchanged. Nullable fields use `Option<Option<_>>` (`Some(None)` clears).
#[derive(Debug, Clone, Default)]
pub struct ArtistInput {
    pub name: Option<String>,
    pub kind: Option<String>,
    pub disambiguation: Option<Option<String>>,
    pub isni: Option<Option<String>>,
}

#[derive(Debug, Clone, Default)]
pub struct CompositionInput {
    pub title: Option<String>,
    pub iswc: Option<Option<String>>,
    pub year: Option<Option<i32>>,
    pub category: Option<String>,
    pub raga: Option<Option<String>>,
    pub religion: Option<Option<String>>,
    pub deity: Option<Option<String>>,
    /// Replaces the whole list when present.
    pub tags: Option<Vec<String>>,
    pub credits: Option<Vec<CreditInput>>,
}

#[derive(Debug, Clone, Default)]
pub struct RecordingInput {
    pub composition_id: Option<Uuid>,
    pub title: Option<String>,
    pub isrc: Option<Option<String>>,
    pub duration_ms: Option<Option<i64>>,
    pub year: Option<Option<i32>>,
    pub credits: Option<Vec<CreditInput>>,
    pub relationships: Option<Vec<RelationshipInput>>,
}

#[derive(Debug, Clone, Default)]
pub struct AlbumInput {
    pub title: Option<String>,
    pub kind: Option<String>,
    pub release_year: Option<Option<i32>>,
    pub barcode: Option<Option<String>>,
    pub credits: Option<Vec<CreditInput>>,
    pub tracks: Option<Vec<Uuid>>,
}

// ---------- views ----------

pub struct ArtistDetail {
    pub artist: ArtistRow,
    pub credits: Vec<ArtistCreditRow>,
    pub names: Names,
}

pub struct CompositionDetail {
    pub composition: CompositionRow,
    pub credits: Vec<CreditRow>,
    pub recordings: Vec<RecordingSummaryRow>,
    pub recording_count: i64,
    pub names: Names,
}

pub struct RecordingDetail {
    pub recording: RecordingRow,
    pub composition: CompositionRow,
    pub credits: Vec<CreditRow>,
    pub relationships: Vec<RelationshipRow>,
    pub related_from: Vec<RelationshipRow>,
    pub albums: Vec<AppearanceRow>,
    pub names: Names,
}

pub struct AlbumDetail {
    pub album: AlbumRow,
    pub credits: Vec<CreditRow>,
    pub tracks: Vec<TrackRow>,
    pub names: Names,
}

type Ref = (EntityType, Uuid, String);

fn credit_refs(out: &mut Vec<Ref>, credits: &[CreditRow]) {
    for c in credits {
        out.push((EntityType::Artist, c.artist_id, c.artist_name.clone()));
        out.push((EntityType::CreditRole, c.role_id, c.role_label.clone()));
    }
}

/// Every read path resolves display through this one call.
async fn resolve_names(conn: &mut PgConnection, lang: Option<&str>, refs: &[Ref]) -> Result<Names, AppError> {
    let req = loc::resolve_requested(conn, lang).await?;
    loc::names(conn, &req, refs).await
}

// ---------- shared helpers ----------

fn not_found(entity: &'static str) -> AppError {
    AppError::NotFound { entity }
}

fn check_version(expected: Option<i32>, current: i32) -> Result<(), AppError> {
    match expected {
        None => Err(domain::DomainError::invalid("expected_version", "required").into()),
        Some(v) if v == current => Ok(()),
        Some(_) => Err(AppError::StaleVersion {
            current_version: current,
        }),
    }
}

fn identifier_field(i: Identifier) -> &'static str {
    match i {
        Identifier::ArtistIsni => "isni",
        Identifier::CompositionIswc => "iswc",
        Identifier::RecordingIsrc => "isrc",
        Identifier::AlbumBarcode => "barcode",
    }
}

/// Refuses an identifier already held by another record, naming that record.
async fn guard_identifier(
    conn: &mut PgConnection,
    ident: Identifier,
    value: Option<&str>,
    except: Option<Uuid>,
) -> Result<(), AppError> {
    let Some(value) = value else { return Ok(()) };
    match repo::find_by_identifier(conn, ident, value, except).await? {
        Some(existing) => Err(AppError::Duplicate {
            field: identifier_field(ident),
            existing_id: Some(existing),
        }),
        None => Ok(()),
    }
}

/// Turns constraint violations that escaped the pre-checks (lost races) into typed conflicts.
pub(crate) fn settle<T>(r: Result<T, AppError>) -> Result<T, AppError> {
    let Err(AppError::Internal(e)) = r else { return r };
    let Some(v) = e.downcast_ref::<sqlx::Error>().and_then(repo::violation) else {
        return Err(AppError::Internal(e));
    };
    Err(match v {
        DbViolation::Unique(c) if c == "localizations_slot_key" => AppError::Taken {
            code: "localization_slot_taken",
            existing_id: None,
        },
        DbViolation::Unique(c) if c == "localizations_preferred_key" => AppError::Taken {
            code: "preferred_slot_taken",
            existing_id: None,
        },
        DbViolation::Unique(c) if c == "contributions_primary_key" => AppError::Taken {
            code: "primary_slot_taken",
            existing_id: None,
        },
        DbViolation::Unique(c) if c == "entity_proposals_pending_key" => AppError::Taken {
            code: "proposal_pending",
            existing_id: None,
        },
        DbViolation::Unique(c) if c == "reports_open_key" => AppError::Taken {
            code: "report_exists",
            existing_id: None,
        },
        DbViolation::Unique(c) if c == "appeals_open_key" => AppError::Taken {
            code: "appeal_pending",
            existing_id: None,
        },
        DbViolation::Unique(c) if c == "takedown_appeals_open_key" => AppError::Taken {
            code: "appeal_pending",
            existing_id: None,
        },
        DbViolation::Unique(c) if c == "aliases_key" => AppError::Taken {
            code: "alias_exists",
            existing_id: None,
        },
        DbViolation::Unique(c) => match Identifier::from_constraint(&c) {
            Some(ident) => AppError::Duplicate {
                field: identifier_field(ident),
                existing_id: None,
            },
            None => AppError::Conflict {
                code: "concurrent_update",
                current_revision_id: None,
            },
        },
        DbViolation::ForeignKey(_) => AppError::Conflict {
            code: "referential_conflict",
            current_revision_id: None,
        },
        DbViolation::Concurrency => AppError::Conflict {
            code: "concurrent_update",
            current_revision_id: None,
        },
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn record(
    conn: &mut PgConnection,
    p: &Principal,
    meta: &RequestMeta,
    action: &str,
    target_type: &str,
    id: Uuid,
    before: Option<Value>,
    after: Option<Value>,
) -> Result<(), AppError> {
    audit::record(
        conn,
        AuditEntry {
            actor_id: Some(p.user_id),
            request_id: meta.request_id.as_deref(),
            before,
            after,
            ..AuditEntry::new(action, target_type, Some(id))
        },
    )
    .await?;
    Ok(())
}

pub(crate) fn snapshot<T: Serialize>(v: &T) -> Option<Value> {
    serde_json::to_value(v).ok()
}

fn parse_kind<K: Copy>(v: &mut Violations, raw: &str, parse: fn(&str) -> Option<K>) -> Option<K> {
    let k = parse(raw);
    if k.is_none() {
        v.push(
            "kind",
            if raw.trim().is_empty() {
                "required"
            } else {
                "invalid_choice"
            },
        );
    }
    k
}

/// Validates a full credit list for `target`; each artist appears at most once per role.
async fn validate_credits(
    conn: &mut PgConnection,
    v: &mut Violations,
    target: CreditTarget,
    input: &[CreditInput],
) -> Result<Vec<CreditData>, AppError> {
    if input.len() > MAX_CREDITS {
        v.push("credits", "too_many");
        return Ok(Vec::new());
    }
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(input.len());
    for c in input {
        match CreditRole::parse(&c.role) {
            None => v.push("credits", "invalid_role"),
            Some(role) if !role.applies_to(target) => v.push("credits", "role_not_applicable"),
            Some(role) if !seen.insert((c.artist_id, role)) => v.push("credits", "duplicate_credit"),
            Some(role) => out.push(CreditData {
                artist_id: c.artist_id,
                role: role.as_str().to_owned(),
            }),
        }
    }
    let ids: Vec<Uuid> = out.iter().map(|c| c.artist_id).collect();
    if !repo::missing_artists(conn, &ids).await?.is_empty() {
        v.push("credits", "unknown_artist");
    }
    Ok(out)
}

/// Validates outgoing relationships of a recording that will belong to `composition_id`.
/// Related recordings are read under a key-share lock so their composition cannot change
/// before commit; the composite FKs enforce the same rule if anything slips through.
async fn validate_relationships(
    conn: &mut PgConnection,
    v: &mut Violations,
    self_id: Option<Uuid>,
    composition_id: Uuid,
    input: &[RelationshipInput],
) -> Result<Vec<RelationshipData>, AppError> {
    if input.len() > MAX_RELATIONSHIPS {
        v.push("relationships", "too_many");
        return Ok(Vec::new());
    }
    let mut seen = HashSet::new();
    let mut parsed = Vec::new();
    for r in input {
        match RelationshipKind::parse(&r.kind) {
            None => v.push("relationships", "invalid_kind"),
            Some(_) if Some(r.related_recording_id) == self_id => v.push("relationships", "self_reference"),
            Some(k) if !seen.insert((k, r.related_recording_id)) => v.push("relationships", "duplicate_relationship"),
            Some(k) => parsed.push((k, r.related_recording_id)),
        }
    }
    let ids: Vec<Uuid> = parsed.iter().map(|(_, id)| *id).collect();
    let found: std::collections::HashMap<Uuid, Uuid> =
        repo::recording_compositions(conn, &ids).await?.into_iter().collect();
    let mut out = Vec::with_capacity(parsed.len());
    for (kind, related) in parsed {
        match found.get(&related) {
            None => v.push("relationships", "unknown_recording"),
            Some(c) if kind.requires_same_composition() && *c != composition_id => {
                v.push("relationships", "composition_mismatch")
            }
            Some(_) => out.push(RelationshipData {
                kind: kind.as_str().to_owned(),
                related_recording_id: related,
                composition_id: kind.requires_same_composition().then_some(composition_id),
            }),
        }
    }
    Ok(out)
}

async fn validate_tracks(conn: &mut PgConnection, v: &mut Violations, input: &[Uuid]) -> Result<(), AppError> {
    if input.len() > MAX_TRACKS {
        v.push("tracks", "too_many");
        return Ok(());
    }
    if input.iter().collect::<HashSet<_>>().len() != input.len() {
        v.push("tracks", "duplicate_track");
    }
    if !repo::missing_recordings(conn, input).await?.is_empty() {
        v.push("tracks", "unknown_recording");
    }
    Ok(())
}

async fn credits_changed(
    conn: &mut PgConnection,
    owner: CreditOwner,
    new: &[CreditData],
) -> Result<Option<Vec<CreditData>>, AppError> {
    let old: Vec<CreditData> = repo::list_credits(conn, owner)
        .await?
        .into_iter()
        .map(|c| CreditData {
            artist_id: c.artist_id,
            role: c.role,
        })
        .collect();
    Ok((old != new).then_some(old))
}

async fn write_credits(
    conn: &mut PgConnection,
    p: &Principal,
    meta: &RequestMeta,
    owner: CreditOwner,
    before: Option<Vec<CreditData>>,
    after: &[CreditData],
) -> Result<(), AppError> {
    repo::replace_credits(conn, owner, after).await?;
    let (target_type, id) = match owner {
        CreditOwner::Composition(id) => ("composition", id),
        CreditOwner::Recording(id) => ("recording", id),
        CreditOwner::Album(id) => ("album", id),
    };
    record(
        conn,
        p,
        meta,
        "credits.changed",
        target_type,
        id,
        before.as_ref().and_then(snapshot),
        snapshot(&after),
    )
    .await
}

fn page<T>(rows: Vec<T>, limit: i64, key: impl Fn(&T) -> Cursor) -> Page<T> {
    Page::from_overfetch(rows, limit, key)
}

fn after(cursor: Option<&str>) -> Result<Option<(chrono::DateTime<chrono::Utc>, Uuid)>, AppError> {
    Ok(cursor.map(Cursor::decode).transpose()?.map(|c| (c.ts, c.id)))
}

// ---------- artists ----------

fn artist_data(v: &mut Violations, i: ArtistInput, base: Option<ArtistData>) -> Option<ArtistData> {
    let b = base.unwrap_or(ArtistData {
        name: String::new(),
        kind: String::new(),
        disambiguation: None,
        isni: None,
    });
    let name = v.check(dc::title("name", &i.name.unwrap_or(b.name)));
    let kind = parse_kind(v, &i.kind.unwrap_or(b.kind), ArtistKind::parse);
    let disambiguation = v.check(dc::optional_note(
        "disambiguation",
        i.disambiguation.unwrap_or(b.disambiguation).as_deref(),
    ));
    let isni = v.check(dc::isni("isni", i.isni.unwrap_or(b.isni).as_deref()));
    Some(ArtistData {
        name: name?,
        kind: kind?.as_str().to_owned(),
        disambiguation: disambiguation?,
        isni: isni?,
    })
}

pub async fn create_artist(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    i: ArtistInput,
) -> Result<ArtistDetail, AppError> {
    let id = settle(
        async {
            let mut v = Violations::default();
            let d = artist_data(&mut v, i, None);
            v.finish()?;
            let d = d.expect("valid when no violations");
            let mut tx = AppTx::begin(&app.pool).await?;
            guard_identifier(&mut tx, Identifier::ArtistIsni, d.isni.as_deref(), None).await?;
            let row = repo::insert_artist(&mut tx, &d, p.user_id).await?;
            record(
                &mut tx,
                p,
                meta,
                "artist.created",
                "artist",
                row.id,
                None,
                snapshot(&row),
            )
            .await?;
            tx.commit().await?;
            Ok(row.id)
        }
        .await,
    )?;
    get_artist(app, id, None).await
}

pub async fn update_artist(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    id: Uuid,
    expected_version: Option<i32>,
    i: ArtistInput,
) -> Result<ArtistDetail, AppError> {
    settle(
        async {
            let mut tx = AppTx::begin(&app.pool).await?;
            let cur = repo::get_artist(&mut tx, id, true).await?.ok_or(not_found("artist"))?;
            authz::require_editor(p, cur.created_by)?;
            check_version(expected_version, cur.version)?;
            let mut v = Violations::default();
            let d = artist_data(&mut v, i, Some(cur.data()));
            v.finish()?;
            let d = d.expect("valid when no violations");
            if d != cur.data() {
                guard_identifier(&mut tx, Identifier::ArtistIsni, d.isni.as_deref(), Some(id)).await?;
                let row = repo::update_artist(&mut tx, id, &d).await?;
                record(
                    &mut tx,
                    p,
                    meta,
                    "artist.updated",
                    "artist",
                    id,
                    snapshot(&cur),
                    snapshot(&row),
                )
                .await?;
            }
            tx.commit().await?;
            Ok(())
        }
        .await,
    )?;
    get_artist(app, id, None).await
}

pub async fn get_artist(app: &App, id: Uuid, lang: Option<&str>) -> Result<ArtistDetail, AppError> {
    let mut conn = app.pool.acquire().await?;
    let artist = repo::get_artist(&mut conn, id, false)
        .await?
        .ok_or(not_found("artist"))?;
    let credits = repo::artist_credits(&mut conn, id, DETAIL_CAP).await?;
    let mut refs: Vec<Ref> = vec![(EntityType::Artist, id, artist.name.clone())];
    for c in &credits {
        let t = match c.target_type.as_str() {
            "composition" => EntityType::Composition,
            "recording" => EntityType::Recording,
            _ => EntityType::Album,
        };
        refs.push((t, c.target_id, c.title.clone()));
        refs.push((EntityType::CreditRole, c.role_id, c.role_label.clone()));
    }
    let names = resolve_names(&mut conn, lang, &refs).await?;
    Ok(ArtistDetail { artist, credits, names })
}

pub async fn list_artists(
    app: &App,
    mut f: ArtistFilter,
    cursor: Option<&str>,
    limit: Option<i64>,
    lang: Option<&str>,
) -> Result<Listed<ArtistRow>, AppError> {
    let limit = pagination::limit(limit)?;
    let after = after(cursor)?;
    let mut v = Violations::default();
    if let Some(k) = &f.kind {
        parse_kind(&mut v, k, ArtistKind::parse);
    }
    f.isni = v.check(dc::isni("isni", f.isni.as_deref())).flatten();
    v.finish()?;
    let mut conn = app.pool.acquire().await?;
    let rows = repo::list_artists(&mut conn, &f, after, limit + 1).await?;
    let page = page(rows, limit, |r| Cursor {
        ts: r.created_at,
        id: r.id,
    });
    let refs: Vec<Ref> = page
        .items
        .iter()
        .map(|r| (EntityType::Artist, r.id, r.name.clone()))
        .collect();
    let names = resolve_names(&mut conn, lang, &refs).await?;
    Ok(Listed { page, names })
}

// ---------- compositions ----------

fn composition_data(
    v: &mut Violations,
    i: &CompositionInput,
    base: Option<CompositionData>,
) -> Option<CompositionData> {
    let b = base.unwrap_or(CompositionData {
        title: String::new(),
        iswc: None,
        year: None,
        category: "mainstream".into(),
        raga: None,
        religion: None,
        deity: None,
        tags: Vec::new(),
    });
    let title = v.check(dc::title("title", i.title.as_deref().unwrap_or(&b.title)));
    let iswc = v.check(dc::iswc("iswc", i.iswc.clone().unwrap_or(b.iswc).as_deref()));
    let year = v.check(dc::year("year", i.year.unwrap_or(b.year.map(i32::from))));
    let category = v.check(dc::category("category", i.category.as_deref().unwrap_or(&b.category)));
    let raga = v.check(dc::short_name("raga", i.raga.clone().unwrap_or(b.raga).as_deref()));
    let tags = v.check(dc::tags("tags", i.tags.as_deref().unwrap_or(&b.tags)));
    // Religion and deity belong to devotional songs. Leaving "devotional" clears them; sending them for a
    // mainstream song is an error rather than silently dropped.
    let devotional = category.as_deref() == Some("devotional");
    let religion_in = i.religion.clone().unwrap_or(b.religion);
    let deity_in = i.deity.clone().unwrap_or(b.deity);
    let (religion, deity) = if devotional {
        (
            v.check(dc::religion("religion", religion_in.as_deref())),
            v.check(dc::short_name("deity", deity_in.as_deref())),
        )
    } else {
        let sent = |o: &Option<Option<String>>| {
            o.as_ref()
                .is_some_and(|x| x.as_deref().is_some_and(|s| !s.trim().is_empty()))
        };
        if category.is_some() && (sent(&i.religion) || sent(&i.deity)) {
            v.check::<()>(Err(domain::DomainError::invalid("religion", "devotional_only")));
        }
        (Some(None), Some(None))
    };
    Some(CompositionData {
        title: title?,
        iswc: iswc?,
        year: year?,
        category: category?,
        raga: raga?,
        religion: religion?,
        deity: deity?,
        tags: tags?,
    })
}

/// Settles which entities a song's raga, deity and tags are: an existing one when the text means it (any
/// spelling, script, localization or alias), otherwise a new one. The data then carries the canonical names.
async fn canonical_spellings(
    tx: &mut AppTx,
    mut d: CompositionData,
    by: Uuid,
) -> Result<(CompositionData, repo::Terms), AppError> {
    let mut t = repo::Terms::default();
    if let Some(text) = d.raga.take() {
        let (id, name) = repo::resolve_term(tx, "raga", &text, by).await?;
        (t.raga_id, d.raga) = (Some(id), Some(name));
    }
    if let Some(text) = d.deity.take() {
        let (id, name) = repo::resolve_term(tx, "deity", &text, by).await?;
        (t.deity_id, d.deity) = (Some(id), Some(name));
    }
    let mut names = Vec::new();
    for text in std::mem::take(&mut d.tags) {
        let (id, name) = repo::resolve_term(tx, "tag", &text, by).await?;
        // "bhajan" and "ഭജൻ" in one song are one tag.
        if !t.tag_ids.contains(&id) {
            t.tag_ids.push(id);
            names.push(name);
        }
    }
    d.tags = names;
    Ok((d, t))
}

pub async fn create_composition(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    i: CompositionInput,
) -> Result<CompositionDetail, AppError> {
    let id = settle(
        async {
            let mut tx = AppTx::begin(&app.pool).await?;
            let mut v = Violations::default();
            let d = composition_data(&mut v, &i, None);
            let credits = validate_credits(
                &mut tx,
                &mut v,
                CreditTarget::Composition,
                i.credits.as_deref().unwrap_or_default(),
            )
            .await?;
            v.finish()?;
            let (d, terms) = canonical_spellings(&mut tx, d.expect("valid when no violations"), p.user_id).await?;
            guard_identifier(&mut tx, Identifier::CompositionIswc, d.iswc.as_deref(), None).await?;
            let row = repo::insert_composition(&mut tx, &d, &terms, p.user_id).await?;
            record(
                &mut tx,
                p,
                meta,
                "composition.created",
                "composition",
                row.id,
                None,
                snapshot(&row),
            )
            .await?;
            if !credits.is_empty() {
                write_credits(&mut tx, p, meta, CreditOwner::Composition(row.id), None, &credits).await?;
            }
            tx.commit().await?;
            Ok(row.id)
        }
        .await,
    )?;
    get_composition(app, id, None).await
}

pub async fn update_composition(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    id: Uuid,
    expected_version: Option<i32>,
    i: CompositionInput,
) -> Result<CompositionDetail, AppError> {
    settle(
        async {
            let mut tx = AppTx::begin(&app.pool).await?;
            let cur = repo::get_composition(&mut tx, id, true)
                .await?
                .ok_or(not_found("composition"))?;
            authz::require_editor(p, cur.created_by)?;
            check_version(expected_version, cur.version)?;
            let mut v = Violations::default();
            let d = composition_data(&mut v, &i, Some(cur.data()));
            let credits = match &i.credits {
                Some(c) => Some(validate_credits(&mut tx, &mut v, CreditTarget::Composition, c).await?),
                None => None,
            };
            v.finish()?;
            let (d, terms) = canonical_spellings(&mut tx, d.expect("valid when no violations"), p.user_id).await?;
            let owner = CreditOwner::Composition(id);
            let credits_before = match &credits {
                Some(c) => credits_changed(&mut tx, owner, c).await?,
                None => None,
            };
            if d == cur.data() && credits_before.is_none() {
                return Ok(());
            }
            guard_identifier(&mut tx, Identifier::CompositionIswc, d.iswc.as_deref(), Some(id)).await?;
            let row = repo::update_composition(&mut tx, id, &d, &terms).await?;
            if d != cur.data() {
                record(
                    &mut tx,
                    p,
                    meta,
                    "composition.updated",
                    "composition",
                    id,
                    snapshot(&cur),
                    snapshot(&row),
                )
                .await?;
            }
            if let (Some(before), Some(after)) = (credits_before, credits) {
                write_credits(&mut tx, p, meta, owner, Some(before), &after).await?;
            }
            tx.commit().await?;
            Ok(())
        }
        .await,
    )?;
    get_composition(app, id, None).await
}

pub async fn get_composition(app: &App, id: Uuid, lang: Option<&str>) -> Result<CompositionDetail, AppError> {
    let mut conn = app.pool.acquire().await?;
    let composition = repo::get_composition(&mut conn, id, false)
        .await?
        .ok_or(not_found("composition"))?;
    let credits = repo::list_credits(&mut conn, CreditOwner::Composition(id)).await?;
    let (recordings, recording_count) = repo::composition_recordings(&mut conn, id, DETAIL_CAP).await?;
    let mut refs: Vec<Ref> = vec![(EntityType::Composition, id, composition.title.clone())];
    credit_refs(&mut refs, &credits);
    refs.extend(
        recordings
            .iter()
            .map(|r| (EntityType::Recording, r.id, r.title.clone())),
    );
    let names = resolve_names(&mut conn, lang, &refs).await?;
    Ok(CompositionDetail {
        composition,
        credits,
        recordings,
        recording_count,
        names,
    })
}

pub async fn list_compositions(
    app: &App,
    mut f: CompositionFilter,
    cursor: Option<&str>,
    limit: Option<i64>,
    lang: Option<&str>,
) -> Result<Listed<CompositionRow>, AppError> {
    let limit = pagination::limit(limit)?;
    let after = after(cursor)?;
    let mut v = Violations::default();
    f.iswc = v.check(dc::iswc("iswc", f.iswc.as_deref())).flatten();
    f.category = v
        .check(f.category.as_deref().map(|c| dc::category("category", c)).transpose())
        .flatten();
    v.finish()?;
    let mut conn = app.pool.acquire().await?;
    let rows = repo::list_compositions(&mut conn, &f, after, limit + 1).await?;
    let page = page(rows, limit, |r| Cursor {
        ts: r.created_at,
        id: r.id,
    });
    let refs: Vec<Ref> = page
        .items
        .iter()
        .map(|r| (EntityType::Composition, r.id, r.title.clone()))
        .collect();
    let names = resolve_names(&mut conn, lang, &refs).await?;
    Ok(Listed { page, names })
}

// ---------- recordings ----------

fn recording_scalars(v: &mut Violations, i: &RecordingInput, b: &RecordingData) -> Option<RecordingData> {
    let title = v.check(dc::title("title", i.title.as_deref().unwrap_or(&b.title)));
    let isrc = v.check(dc::isrc("isrc", i.isrc.clone().unwrap_or(b.isrc.clone()).as_deref()));
    let duration = v.check(dc::duration_ms(
        "duration_ms",
        i.duration_ms.unwrap_or(b.duration_ms.map(i64::from)),
    ));
    let year = v.check(dc::year("year", i.year.unwrap_or(b.year.map(i32::from))));
    Some(RecordingData {
        composition_id: i.composition_id.unwrap_or(b.composition_id),
        title: title?,
        isrc: isrc?,
        duration_ms: duration?,
        year: year?,
    })
}

/// Creates a recording of an existing composition. A cover (or remake, live take, arrangement)
/// links to the *same* composition and copies nothing from the recording it relates to.
pub async fn create_recording(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    i: RecordingInput,
) -> Result<RecordingDetail, AppError> {
    let id = settle(
        async {
            let mut tx = AppTx::begin(&app.pool).await?;
            let mut v = Violations::default();
            let composition_id = match i.composition_id {
                None => {
                    v.push("composition_id", "required");
                    Uuid::nil()
                }
                Some(c) if !repo::composition_exists(&mut tx, c).await? => {
                    v.push("composition_id", "not_found");
                    c
                }
                Some(c) => c,
            };
            let base = RecordingData {
                composition_id,
                title: String::new(),
                isrc: None,
                duration_ms: None,
                year: None,
            };
            let d = recording_scalars(&mut v, &i, &base);
            let credits = validate_credits(
                &mut tx,
                &mut v,
                CreditTarget::Recording,
                i.credits.as_deref().unwrap_or_default(),
            )
            .await?;
            let rels = validate_relationships(
                &mut tx,
                &mut v,
                None,
                composition_id,
                i.relationships.as_deref().unwrap_or_default(),
            )
            .await?;
            v.finish()?;
            let d = d.expect("valid when no violations");
            guard_identifier(&mut tx, Identifier::RecordingIsrc, d.isrc.as_deref(), None).await?;
            let row = repo::insert_recording(&mut tx, &d, p.user_id).await?;
            record(
                &mut tx,
                p,
                meta,
                "recording.created",
                "recording",
                row.id,
                None,
                snapshot(&row),
            )
            .await?;
            if !credits.is_empty() {
                write_credits(&mut tx, p, meta, CreditOwner::Recording(row.id), None, &credits).await?;
            }
            if !rels.is_empty() {
                repo::insert_relationships(&mut tx, row.id, &rels, p.user_id).await?;
                record(
                    &mut tx,
                    p,
                    meta,
                    "recording.relationships_changed",
                    "recording",
                    row.id,
                    None,
                    snapshot(&rels),
                )
                .await?;
            }
            tx.commit().await?;
            Ok(row.id)
        }
        .await,
    )?;
    get_recording(app, id, None).await
}

pub async fn update_recording(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    id: Uuid,
    expected_version: Option<i32>,
    i: RecordingInput,
) -> Result<RecordingDetail, AppError> {
    settle(
        async {
            let mut tx = AppTx::begin(&app.pool).await?;
            let cur = repo::get_recording(&mut tx, id, true)
                .await?
                .ok_or(not_found("recording"))?;
            authz::require_editor(p, cur.created_by)?;
            check_version(expected_version, cur.version)?;
            let mut v = Violations::default();
            let d = recording_scalars(&mut v, &i, &cur.data());
            let moved = i.composition_id.is_some_and(|c| c != cur.composition_id);
            if moved
                && !repo::composition_exists(&mut tx, d.as_ref().map_or(cur.composition_id, |d| d.composition_id))
                    .await?
            {
                v.push("composition_id", "not_found");
            }
            let target_composition = d.as_ref().map_or(cur.composition_id, |d| d.composition_id);
            let credits = match &i.credits {
                Some(c) => Some(validate_credits(&mut tx, &mut v, CreditTarget::Recording, c).await?),
                None => None,
            };
            let rels = match &i.relationships {
                Some(r) => Some(validate_relationships(&mut tx, &mut v, Some(id), target_composition, r).await?),
                None => None,
            };
            v.finish()?;
            let d = d.expect("valid when no violations");

            let owner = CreditOwner::Recording(id);
            let credits_before = match &credits {
                Some(c) => credits_changed(&mut tx, owner, c).await?,
                None => None,
            };
            let rels_before = match &rels {
                Some(r) => {
                    let old = repo::current_relationships(&mut tx, id).await?;
                    (old != *r).then_some(old)
                }
                None => None,
            };
            if d == cur.data() && credits_before.is_none() && rels_before.is_none() {
                return Ok(());
            }
            if rels_before.is_some() {
                repo::delete_relationships(&mut tx, id).await?;
            }
            if moved && repo::composition_bound_relationships(&mut tx, id).await? > 0 {
                // Re-pointing would orphan covers/live takes from their work. Unlink them first.
                return Err(AppError::Conflict {
                    code: "recording_has_relationships",
                    current_revision_id: None,
                });
            }
            guard_identifier(&mut tx, Identifier::RecordingIsrc, d.isrc.as_deref(), Some(id)).await?;
            let row = repo::update_recording(&mut tx, id, &d).await?;
            if d != cur.data() {
                record(
                    &mut tx,
                    p,
                    meta,
                    "recording.updated",
                    "recording",
                    id,
                    snapshot(&cur),
                    snapshot(&row),
                )
                .await?;
            }
            if let (Some(before), Some(after)) = (credits_before, credits) {
                write_credits(&mut tx, p, meta, owner, Some(before), &after).await?;
            }
            if let (Some(before), Some(after)) = (rels_before, rels) {
                repo::insert_relationships(&mut tx, id, &after, p.user_id).await?;
                record(
                    &mut tx,
                    p,
                    meta,
                    "recording.relationships_changed",
                    "recording",
                    id,
                    snapshot(&before),
                    snapshot(&after),
                )
                .await?;
            }
            tx.commit().await?;
            Ok(())
        }
        .await,
    )?;
    get_recording(app, id, None).await
}

pub async fn get_recording(app: &App, id: Uuid, lang: Option<&str>) -> Result<RecordingDetail, AppError> {
    let mut conn = app.pool.acquire().await?;
    let recording = repo::get_recording(&mut conn, id, false)
        .await?
        .ok_or(not_found("recording"))?;
    let composition = repo::get_composition(&mut conn, recording.composition_id, false)
        .await?
        .ok_or(not_found("composition"))?;
    let credits = repo::list_credits(&mut conn, CreditOwner::Recording(id)).await?;
    let relationships = repo::outgoing_relationships(&mut conn, id).await?;
    let related_from = repo::incoming_relationships(&mut conn, id).await?;
    let albums = repo::recording_albums(&mut conn, id).await?;
    let mut refs: Vec<Ref> = vec![
        (EntityType::Recording, id, recording.title.clone()),
        (EntityType::Composition, composition.id, composition.title.clone()),
    ];
    credit_refs(&mut refs, &credits);
    refs.extend(
        relationships
            .iter()
            .chain(&related_from)
            .map(|r| (EntityType::Recording, r.other_id, r.other_title.clone())),
    );
    refs.extend(albums.iter().map(|a| (EntityType::Album, a.album_id, a.title.clone())));
    let names = resolve_names(&mut conn, lang, &refs).await?;
    Ok(RecordingDetail {
        credits,
        relationships,
        related_from,
        albums,
        recording,
        composition,
        names,
    })
}

pub async fn list_recordings(
    app: &App,
    mut f: RecordingFilter,
    cursor: Option<&str>,
    limit: Option<i64>,
    lang: Option<&str>,
) -> Result<Listed<RecordingRow>, AppError> {
    let limit = pagination::limit(limit)?;
    let after = after(cursor)?;
    let mut v = Violations::default();
    f.isrc = v.check(dc::isrc("isrc", f.isrc.as_deref())).flatten();
    v.finish()?;
    let mut conn = app.pool.acquire().await?;
    let rows = repo::list_recordings(&mut conn, &f, after, limit + 1).await?;
    let page = page(rows, limit, |r| Cursor {
        ts: r.created_at,
        id: r.id,
    });
    let refs: Vec<Ref> = page
        .items
        .iter()
        .map(|r| (EntityType::Recording, r.id, r.title.clone()))
        .collect();
    let names = resolve_names(&mut conn, lang, &refs).await?;
    Ok(Listed { page, names })
}

// ---------- albums ----------

fn album_data(v: &mut Violations, i: &AlbumInput, base: Option<AlbumData>) -> Option<AlbumData> {
    let b = base.unwrap_or(AlbumData {
        title: String::new(),
        kind: String::new(),
        release_year: None,
        barcode: None,
    });
    let title = v.check(dc::title("title", i.title.as_deref().unwrap_or(&b.title)));
    let kind = parse_kind(v, i.kind.as_deref().unwrap_or(&b.kind), AlbumKind::parse);
    let year = v.check(dc::year(
        "release_year",
        i.release_year.unwrap_or(b.release_year.map(i32::from)),
    ));
    let barcode = v.check(dc::barcode(
        "barcode",
        i.barcode.clone().unwrap_or(b.barcode).as_deref(),
    ));
    Some(AlbumData {
        title: title?,
        kind: kind?.as_str().to_owned(),
        release_year: year?,
        barcode: barcode?,
    })
}

pub async fn create_album(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    i: AlbumInput,
) -> Result<AlbumDetail, AppError> {
    let id = settle(
        async {
            let mut tx = AppTx::begin(&app.pool).await?;
            let mut v = Violations::default();
            let d = album_data(&mut v, &i, None);
            let credits = validate_credits(
                &mut tx,
                &mut v,
                CreditTarget::Album,
                i.credits.as_deref().unwrap_or_default(),
            )
            .await?;
            let tracks = i.tracks.clone().unwrap_or_default();
            validate_tracks(&mut tx, &mut v, &tracks).await?;
            v.finish()?;
            let d = d.expect("valid when no violations");
            guard_identifier(&mut tx, Identifier::AlbumBarcode, d.barcode.as_deref(), None).await?;
            let row = repo::insert_album(&mut tx, &d, p.user_id).await?;
            record(&mut tx, p, meta, "album.created", "album", row.id, None, snapshot(&row)).await?;
            if !credits.is_empty() {
                write_credits(&mut tx, p, meta, CreditOwner::Album(row.id), None, &credits).await?;
            }
            if !tracks.is_empty() {
                repo::replace_tracks(&mut tx, row.id, &tracks).await?;
                record(
                    &mut tx,
                    p,
                    meta,
                    "album.tracks_changed",
                    "album",
                    row.id,
                    None,
                    snapshot(&tracks),
                )
                .await?;
            }
            tx.commit().await?;
            Ok(row.id)
        }
        .await,
    )?;
    get_album(app, id, None).await
}

pub async fn update_album(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    id: Uuid,
    expected_version: Option<i32>,
    i: AlbumInput,
) -> Result<AlbumDetail, AppError> {
    settle(
        async {
            let mut tx = AppTx::begin(&app.pool).await?;
            let cur = repo::get_album(&mut tx, id, true).await?.ok_or(not_found("album"))?;
            authz::require_editor(p, cur.created_by)?;
            check_version(expected_version, cur.version)?;
            let mut v = Violations::default();
            let d = album_data(&mut v, &i, Some(cur.data()));
            let credits = match &i.credits {
                Some(c) => Some(validate_credits(&mut tx, &mut v, CreditTarget::Album, c).await?),
                None => None,
            };
            if let Some(t) = &i.tracks {
                validate_tracks(&mut tx, &mut v, t).await?;
            }
            v.finish()?;
            let d = d.expect("valid when no violations");
            let owner = CreditOwner::Album(id);
            let credits_before = match &credits {
                Some(c) => credits_changed(&mut tx, owner, c).await?,
                None => None,
            };
            let tracks_before = match &i.tracks {
                Some(t) => {
                    let old = repo::track_ids(&mut tx, id).await?;
                    (old != *t).then_some(old)
                }
                None => None,
            };
            if d == cur.data() && credits_before.is_none() && tracks_before.is_none() {
                return Ok(());
            }
            guard_identifier(&mut tx, Identifier::AlbumBarcode, d.barcode.as_deref(), Some(id)).await?;
            let row = repo::update_album(&mut tx, id, &d).await?;
            if d != cur.data() {
                record(
                    &mut tx,
                    p,
                    meta,
                    "album.updated",
                    "album",
                    id,
                    snapshot(&cur),
                    snapshot(&row),
                )
                .await?;
            }
            if let (Some(before), Some(after)) = (credits_before, credits) {
                write_credits(&mut tx, p, meta, owner, Some(before), &after).await?;
            }
            if let (Some(before), Some(after)) = (tracks_before, &i.tracks) {
                repo::replace_tracks(&mut tx, id, after).await?;
                record(
                    &mut tx,
                    p,
                    meta,
                    "album.tracks_changed",
                    "album",
                    id,
                    Some(json!(before)),
                    Some(json!(after)),
                )
                .await?;
            }
            tx.commit().await?;
            Ok(())
        }
        .await,
    )?;
    get_album(app, id, None).await
}

pub async fn get_album(app: &App, id: Uuid, lang: Option<&str>) -> Result<AlbumDetail, AppError> {
    let mut conn = app.pool.acquire().await?;
    let album = repo::get_album(&mut conn, id, false).await?.ok_or(not_found("album"))?;
    let credits = repo::list_credits(&mut conn, CreditOwner::Album(id)).await?;
    let tracks = repo::list_tracks(&mut conn, id).await?;
    let mut refs: Vec<Ref> = vec![(EntityType::Album, id, album.title.clone())];
    credit_refs(&mut refs, &credits);
    refs.extend(
        tracks
            .iter()
            .map(|t| (EntityType::Recording, t.recording_id, t.title.clone())),
    );
    let names = resolve_names(&mut conn, lang, &refs).await?;
    Ok(AlbumDetail {
        album,
        credits,
        tracks,
        names,
    })
}

pub async fn list_albums(
    app: &App,
    mut f: AlbumFilter,
    cursor: Option<&str>,
    limit: Option<i64>,
    lang: Option<&str>,
) -> Result<Listed<AlbumRow>, AppError> {
    let limit = pagination::limit(limit)?;
    let after = after(cursor)?;
    let mut v = Violations::default();
    if let Some(k) = &f.kind {
        parse_kind(&mut v, k, AlbumKind::parse);
    }
    f.barcode = v.check(dc::barcode("barcode", f.barcode.as_deref())).flatten();
    v.finish()?;
    let mut conn = app.pool.acquire().await?;
    let rows = repo::list_albums(&mut conn, &f, after, limit + 1).await?;
    let page = page(rows, limit, |r| Cursor {
        ts: r.created_at,
        id: r.id,
    });
    let refs: Vec<Ref> = page
        .items
        .iter()
        .map(|r| (EntityType::Album, r.id, r.title.clone()))
        .collect();
    let names = resolve_names(&mut conn, lang, &refs).await?;
    Ok(Listed { page, names })
}
