//! Entity suggestion (the one search every picker uses), proposals, merges and merge redirects.

use crate::extract::{Admin, ApiJson, ApiPath, ApiQuery, Meta, Moderator, RequireRole, RequireUser};
use crate::localization::Display;
use crate::problem::{ApiResult, Problem};
use crate::routes::PageQuery;
use application::entities::{self as uc, MergeInput, ProposalInput, ProposalVerdict, Suggestion};
use application::{App, AppError};
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use infrastructure::entities::ProposalRow;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

// ---------- suggest ----------

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SuggestQuery {
    /// `artist`, `album`, `label`, `publisher`, `genre`, `language` or `script`. Songwriters and
    /// translators are artists.
    #[serde(rename = "type", default)]
    pub kind: String,
    /// Typed text. Case, spacing, punctuation and diacritics do not matter.
    #[serde(default)]
    pub q: String,
    /// 1–20, default 8.
    pub limit: Option<i64>,
    /// Display language tag for the returned names.
    pub lang: Option<String>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MatchedVia {
    Name,
    Localization,
    Alias,
}

#[derive(Serialize, ToSchema)]
pub struct LocalizedName {
    pub language: String,
    pub name: String,
}

#[derive(Serialize, ToSchema)]
pub struct RelatedWork {
    /// `composition`, `recording`, `album` or `artist`.
    pub kind: String,
    pub id: Uuid,
    pub title: String,
}

/// One candidate. Store `id`, never the name.
#[derive(Serialize, ToSchema)]
pub struct EntitySuggestion {
    pub id: Uuid,
    #[serde(rename = "type")]
    pub kind: String,
    pub display: Display,
    /// Which of the entity's names matched what was typed.
    pub matched_via: MatchedVia,
    /// 1 = exact ignoring case/spacing/punctuation/diacritics, 0.9 = prefix, otherwise similarity.
    pub score: f32,
    /// Artist kind or album kind.
    pub sub_kind: Option<String>,
    /// Artist disambiguation, or a language's endonym.
    pub detail: Option<String>,
    pub year: Option<i16>,
    /// ISO code for languages and scripts.
    pub code: Option<String>,
    pub localized_names: Vec<LocalizedName>,
    pub related: Vec<RelatedWork>,
}

impl From<Suggestion> for EntitySuggestion {
    fn from(s: Suggestion) -> Self {
        Self {
            id: s.id,
            kind: s.entity_type,
            display: s.display.into(),
            matched_via: match s.matched_via.as_str() {
                "alias" => MatchedVia::Alias,
                "localization" => MatchedVia::Localization,
                _ => MatchedVia::Name,
            },
            score: s.score,
            sub_kind: s.kind,
            detail: s.detail,
            year: s.year,
            code: s.code,
            localized_names: s
                .localized_names
                .into_iter()
                .map(|n| LocalizedName {
                    language: n.language,
                    name: n.name,
                })
                .collect(),
            related: s
                .related
                .into_iter()
                .map(|r| RelatedWork {
                    kind: r.kind,
                    id: r.id,
                    title: r.title,
                })
                .collect(),
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct SuggestionList {
    pub items: Vec<EntitySuggestion>,
}

#[utoipa::path(get, path = "/api/v1/entities/suggest", tag = "entities", params(SuggestQuery), responses(
    (status = 200, body = SuggestionList, description = "Best matches first. Only canonical names and verified localizations or aliases are searched."),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn suggest(State(app): State<App>, ApiQuery(q): ApiQuery<SuggestQuery>) -> ApiResult<Json<SuggestionList>> {
    let items = uc::suggest(&app, &q.kind, &q.q, q.limit, q.lang.as_deref()).await?;
    Ok(Json(SuggestionList {
        items: items.into_iter().map(Into::into).collect(),
    }))
}

// ---------- proposals ----------

#[derive(Deserialize, ToSchema)]
pub struct ProposalCreate {
    /// `artist`, `album`, `label`, `publisher` or `genre`.
    #[serde(default)]
    pub entity_type: String,
    #[serde(default)]
    pub name: String,
    /// Required for artists (`person`, `group`, `other`) and albums (`album`, `single`, …).
    pub kind: Option<String>,
    pub disambiguation: Option<String>,
    /// Other spellings; verified as aliases when the proposal is approved.
    #[serde(default)]
    pub aliases: Vec<String>,
    pub note: Option<String>,
    /// Set after looking at the `409 likely_duplicates` candidates and deciding this is a different entity.
    #[serde(default)]
    pub confirm_distinct: bool,
    /// Id of an image uploaded with `POST /api/v1/images` (no entity). It is reviewed with the proposal and
    /// shown with the new entity once approved. Only for entity types that can have images.
    pub image_id: Option<Uuid>,
}

#[derive(Serialize, ToSchema)]
pub struct Proposal {
    pub id: Uuid,
    pub entity_type: String,
    pub name: String,
    pub kind: Option<String>,
    pub disambiguation: Option<String>,
    pub aliases: Vec<String>,
    pub note: Option<String>,
    /// The likely matches the proposer was shown and set aside.
    pub shown_matches: Vec<Uuid>,
    pub status: ProposalStatus,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub decided_at: Option<DateTime<Utc>>,
    pub decision_note: Option<String>,
    /// The canonical entity an approval created.
    pub entity_id: Option<Uuid>,
    /// The existing entity a rejection pointed to.
    pub duplicate_of_id: Option<Uuid>,
    /// The image proposed with it, awaiting the same decision.
    pub image_id: Option<Uuid>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProposalStatus {
    Pending,
    Approved,
    Rejected,
}

impl From<ProposalRow> for Proposal {
    fn from(r: ProposalRow) -> Self {
        Self {
            status: match r.status.as_str() {
                "approved" => ProposalStatus::Approved,
                "rejected" => ProposalStatus::Rejected,
                _ => ProposalStatus::Pending,
            },
            id: r.id,
            entity_type: r.entity_type,
            name: r.name,
            kind: r.kind,
            disambiguation: r.disambiguation,
            aliases: r.aliases,
            note: r.note,
            shown_matches: r.shown_matches,
            created_by: r.created_by,
            created_at: r.created_at,
            decided_at: r.decided_at,
            decision_note: r.decision_note,
            entity_id: r.entity_id,
            duplicate_of_id: r.duplicate_of_id,
            image_id: None,
        }
    }
}

#[utoipa::path(post, path = "/api/v1/entities/proposals", tag = "entities", request_body = ProposalCreate,
    security(("session" = [])), responses(
    (status = 201, body = Proposal),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 409, body = Problem, content_type = "application/problem+json",
        description = "`likely_duplicates` (see `candidate_ids`; resubmit with `confirm_distinct`) or `proposal_pending` (see `existing_id`)"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn create_proposal(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireUser(p): RequireUser,
    ApiJson(b): ApiJson<ProposalCreate>,
) -> ApiResult<Response> {
    let row = uc::propose(
        &app,
        &p,
        &meta,
        ProposalInput {
            entity_type: b.entity_type,
            name: b.name,
            kind: b.kind,
            disambiguation: b.disambiguation,
            aliases: b.aliases,
            note: b.note,
            confirm_distinct: b.confirm_distinct,
            image_id: b.image_id,
        },
    )
    .await?;
    let id = row.id;
    let image_id = b.image_id;
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, format!("/api/v1/entities/proposals/{id}"))],
        Json(Proposal {
            image_id,
            ..Proposal::from(row)
        }),
    )
        .into_response())
}

#[derive(Serialize, ToSchema)]
pub struct ProposalPage {
    pub items: Vec<Proposal>,
    pub next_cursor: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/moderation/entity-proposals", tag = "moderation", params(PageQuery),
    security(("session" = [])), responses(
    (status = 200, body = ProposalPage, description = "Pending proposals, oldest first"),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json")))]
pub async fn proposal_queue(
    State(app): State<App>,
    RequireRole(p, ..): RequireRole<Moderator>,
    ApiQuery(q): ApiQuery<PageQuery>,
) -> ApiResult<Json<ProposalPage>> {
    let page = uc::proposal_queue(&app, &p, q.cursor.as_deref(), q.limit).await?;
    let ids: Vec<Uuid> = page.items.iter().map(|r| r.id).collect();
    let images = application::images::for_proposals(&app, &ids).await?;
    Ok(Json(ProposalPage {
        items: page
            .items
            .into_iter()
            .map(|r| Proposal {
                image_id: images.get(&r.id).copied(),
                ..Proposal::from(r)
            })
            .collect(),
        next_cursor: page.next_cursor,
    }))
}

#[derive(Deserialize, ToSchema)]
pub struct ProposalDecision {
    pub note: Option<String>,
    /// Approve only: approve although likely duplicates exist.
    #[serde(default)]
    pub confirm_distinct: bool,
    /// Reject only: the existing entity that should have been used.
    pub duplicate_of_id: Option<Uuid>,
}

macro_rules! proposal_decision {
    ($name:ident, $path:literal, $doc:literal, |$b:ident| $verdict:expr) => {
        #[utoipa::path(post, path = $path, tag = "moderation", request_body = ProposalDecision,
                                            params(("id" = Uuid, Path)), security(("session" = [])), responses(
                                            (status = 200, body = Proposal, description = $doc),
                                            (status = 401, body = Problem, content_type = "application/problem+json"),
                                            (status = 403, body = Problem, content_type = "application/problem+json"),
                                            (status = 404, body = Problem, content_type = "application/problem+json"),
                                            (status = 409, body = Problem, content_type = "application/problem+json")))]
        pub async fn $name(
            State(app): State<App>,
            Meta(meta): Meta,
            RequireRole(p, ..): RequireRole<Moderator>,
            ApiPath(id): ApiPath<Uuid>,
            ApiJson($b): ApiJson<ProposalDecision>,
        ) -> ApiResult<Json<Proposal>> {
            let note = $b.note.clone();
            let row = uc::decide_proposal(&app, &p, &meta, id, $verdict, note.as_deref()).await?;
            Ok(Json(row.into()))
        }
    };
}

proposal_decision!(
    approve_proposal,
    "/api/v1/entities/proposals/{id}/approve",
    "The canonical entity and its aliases were created",
    |b| ProposalVerdict::Approve {
        confirm_distinct: b.confirm_distinct
    }
);
proposal_decision!(
    reject_proposal,
    "/api/v1/entities/proposals/{id}/reject",
    "Rejected, optionally pointing at the existing entity",
    |b| ProposalVerdict::Reject {
        duplicate_of: b.duplicate_of_id
    }
);

// ---------- merge ----------

#[derive(Deserialize, ToSchema)]
pub struct MergeCreate {
    /// `artist`, `album`, `label`, `publisher` or `genre`. Languages and scripts are never merged.
    #[serde(default)]
    pub entity_type: String,
    /// The duplicate; it disappears and its id redirects to `into_id`.
    pub from_id: Uuid,
    /// The survivor.
    pub into_id: Uuid,
    /// Both versions are required, so a merge is never decided on a stale view.
    pub expected_from_version: Option<i32>,
    pub expected_into_version: Option<i32>,
    pub note: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct MergeMoved {
    pub credits: u64,
    pub duplicate_credits_dropped: u64,
    pub tracks: u64,
    pub localizations: u64,
    pub localizations_retired: u64,
    pub aliases: u64,
    pub duplicate_aliases_dropped: u64,
    pub names_kept_as_aliases: u64,
}

#[derive(Serialize, ToSchema)]
pub struct MergeOutcome {
    pub entity_type: String,
    pub from_id: Uuid,
    pub into_id: Uuid,
    pub moved: MergeMoved,
}

#[utoipa::path(post, path = "/api/v1/entities/merges", tag = "entities", request_body = MergeCreate,
    security(("session" = [])), responses(
    (status = 200, body = MergeOutcome),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json"),
    (status = 404, body = Problem, content_type = "application/problem+json"),
    (status = 409, body = Problem, content_type = "application/problem+json",
        description = "`version_conflict`, `already_merged`, or an ambiguity that is never merged: `identifier_mismatch`, `kind_mismatch`, `track_conflict`"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn merge_entities(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireRole(p, ..): RequireRole<Admin>,
    ApiJson(b): ApiJson<MergeCreate>,
) -> ApiResult<Json<MergeOutcome>> {
    let r = uc::merge(
        &app,
        &p,
        &meta,
        MergeInput {
            entity_type: b.entity_type,
            from_id: b.from_id,
            into_id: b.into_id,
            expected_from_version: b.expected_from_version,
            expected_into_version: b.expected_into_version,
            note: b.note,
        },
    )
    .await?;
    let m = r.moved;
    Ok(Json(MergeOutcome {
        entity_type: r.entity_type,
        from_id: r.from_id,
        into_id: r.into_id,
        moved: MergeMoved {
            credits: m.credits,
            duplicate_credits_dropped: m.duplicate_credits_dropped,
            tracks: m.tracks,
            localizations: m.localizations,
            localizations_retired: m.localizations_retired,
            aliases: m.aliases,
            duplicate_aliases_dropped: m.duplicate_aliases_dropped,
            names_kept_as_aliases: m.names_kept_as_aliases,
        },
    }))
}

/// Serves `body`, or, when the id was merged away, a permanent redirect to the survivor.
pub(crate) async fn or_redirect<T: Serialize>(
    app: &App,
    kind: &str,
    path: &str,
    id: Uuid,
    lang: Option<&str>,
    found: Result<T, AppError>,
) -> ApiResult<Response> {
    match found {
        Ok(body) => Ok(Json(body).into_response()),
        Err(AppError::NotFound { entity }) => match uc::redirect_target(app, kind, id).await? {
            Some(to) => {
                let mut loc = format!("/api/v1/{path}/{to}");
                if let Some(l) = lang {
                    loc.push_str("?lang=");
                    loc.push_str(
                        &l.chars()
                            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
                            .collect::<String>(),
                    );
                }
                Ok((StatusCode::MOVED_PERMANENTLY, [(header::LOCATION, loc)]).into_response())
            }
            None => Err(AppError::NotFound { entity }.into()),
        },
        Err(e) => Err(e.into()),
    }
}
