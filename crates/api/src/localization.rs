//! Localized metadata endpoints. Reads are public and only ever serve verified content;
//! pending/rejected rows are visible to their creator and moderators only.

use crate::catalogue::{created, present, Created};
use crate::extract::{ApiJson, ApiPath, ApiQuery, MaybeUser, Meta, Moderator, RequireRole, RequireUser};
use crate::problem::{ApiResult, Problem};
use crate::routes::PageQuery;
use application::localization as uc;
use application::App;
use axum::extract::State;
use axum::Json;
use chrono::{DateTime, Utc};
use domain::localization::Resolution;
use infrastructure::localization as rows;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

// ---------- shared DTOs ----------

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Dir {
    Ltr,
    Rtl,
    Auto,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DisplayResolution {
    /// A localization in the requested language.
    Requested,
    /// Shown in the entity's own (original) language.
    DefaultLanguage,
    /// Some other available language.
    Fallback,
    /// No localization exists; the entity's base name.
    Base,
}

/// How to show an entity's name. When `fallback` is true the text is **not** in the requested
/// language and the UI must say so. Set `lang` and `dir` on the element that renders `text`.
#[derive(Serialize, ToSchema)]
pub struct Display {
    pub text: String,
    pub description: Option<String>,
    /// Language of `text` (`ml`, `sr-Latn`); null for a base name of unknown language.
    pub language: Option<String>,
    pub script: Option<String>,
    pub dir: Dir,
    pub fallback: bool,
    pub resolution: DisplayResolution,
}

impl From<uc::Display> for Display {
    fn from(d: uc::Display) -> Self {
        Self {
            fallback: d.fallback(),
            resolution: match d.resolution {
                Resolution::Requested => DisplayResolution::Requested,
                Resolution::DefaultLanguage => DisplayResolution::DefaultLanguage,
                Resolution::Fallback => DisplayResolution::Fallback,
                Resolution::Base => DisplayResolution::Base,
            },
            dir: match d.dir {
                "rtl" => Dir::Rtl,
                "ltr" => Dir::Ltr,
                _ => Dir::Auto,
            },
            text: d.text,
            description: d.description,
            language: d.language,
            script: d.script,
        }
    }
}

/// Credit categories. Original songwriters, translators and adapters are always separate groups.
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RoleCategory {
    OriginalSongwriter,
    Translator,
    Adapter,
    Performance,
    Production,
    Other,
}

impl RoleCategory {
    pub fn from_code(c: &str) -> Self {
        match c {
            "original_songwriter" => Self::OriginalSongwriter,
            "translator" => Self::Translator,
            "adapter" => Self::Adapter,
            "performance" => Self::Performance,
            "production" => Self::Production,
            _ => Self::Other,
        }
    }
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct LangQuery {
    /// Display language tag: `ml`, `ar-EG`, `sr-Latn`. Unknown languages are rejected (422).
    pub lang: Option<String>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct StateQuery {
    /// `pending`, `verified` or `rejected`. Only moderators may ask for anything but `verified`.
    pub state: Option<String>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    Original,
    HumanTranslation,
    Transliteration,
    Community,
    Imported,
    /// Never accepted for artist names.
    Machine,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum VerificationState {
    Pending,
    Verified,
    Rejected,
}

// ---------- reference data ----------

#[derive(Serialize, ToSchema)]
pub struct Language {
    pub id: Uuid,
    /// Primary key for clients: ISO 639-3.
    pub iso639_3: String,
    pub iso639_1: Option<String>,
    pub iso639_2: Option<String>,
    pub name_en: String,
    /// The language's own name for itself.
    pub endonym: String,
    pub is_rtl: bool,
    /// ISO 15924 code of the usual script.
    pub default_script: Option<String>,
    /// Governed private-use extension (ISO 639-3 qaa–qtz).
    pub is_extension: bool,
}

#[derive(Serialize, ToSchema)]
pub struct Script {
    pub id: Uuid,
    pub iso15924: String,
    pub numeric_code: Option<i32>,
    pub name_en: String,
    pub is_rtl: bool,
    pub is_extension: bool,
}

#[derive(Serialize, ToSchema)]
pub struct LanguagePage {
    pub items: Vec<Language>,
    pub next_cursor: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct ScriptPage {
    pub items: Vec<Script>,
    pub next_cursor: Option<String>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct LanguageQuery {
    /// Search instead of paging: a language code or part of its English name or its own name
    /// (`sans` finds Sanskrit). Best matches first, no `next_cursor`. An empty value lists by name.
    pub q: Option<String>,
    /// Opaque cursor from a previous page's `next_cursor` (ignored when searching).
    pub cursor: Option<String>,
    /// Page size, 1–100 (default 25).
    pub limit: Option<i64>,
}

#[utoipa::path(get, path = "/api/v1/languages", tag = "localization", params(LanguageQuery), responses(
    (status = 200, body = LanguagePage),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn list_languages(
    State(app): State<App>,
    ApiQuery(q): ApiQuery<LanguageQuery>,
) -> ApiResult<Json<LanguagePage>> {
    let page = match q.q.as_deref() {
        Some(text) => application::pagination::Page {
            items: uc::search_languages(&app, text, q.limit).await?,
            next_cursor: None,
        },
        None => uc::list_languages(&app, q.cursor.as_deref(), q.limit).await?,
    };
    Ok(Json(LanguagePage {
        items: page
            .items
            .into_iter()
            .map(|l| Language {
                id: l.id,
                iso639_3: l.iso639_3,
                iso639_1: l.iso639_1,
                iso639_2: l.iso639_2,
                name_en: l.name_en,
                endonym: l.endonym,
                is_rtl: l.is_rtl,
                default_script: l.default_script,
                is_extension: l.is_extension,
            })
            .collect(),
        next_cursor: page.next_cursor,
    }))
}

#[utoipa::path(get, path = "/api/v1/scripts", tag = "localization", params(PageQuery), responses(
    (status = 200, body = ScriptPage),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn list_scripts(State(app): State<App>, ApiQuery(q): ApiQuery<PageQuery>) -> ApiResult<Json<ScriptPage>> {
    let page = uc::list_scripts(&app, q.cursor.as_deref(), q.limit).await?;
    Ok(Json(ScriptPage {
        items: page
            .items
            .into_iter()
            .map(|s| Script {
                id: s.id,
                iso15924: s.iso15924,
                numeric_code: s.numeric_code,
                name_en: s.name_en,
                is_rtl: s.is_rtl,
                is_extension: s.is_extension,
            })
            .collect(),
        next_cursor: page.next_cursor,
    }))
}

// ---------- localizations ----------

/// One name of an entity in one language (and optionally script). Personal names are never machine-made.
#[derive(Serialize, ToSchema)]
pub struct Localization {
    pub id: Uuid,
    /// `composition`, `recording`, `artist`, `album`, `label`, `publisher`, `genre` or `credit_role`.
    pub entity_type: String,
    pub entity_id: Uuid,
    pub language: String,
    pub script: Option<String>,
    pub dir: Dir,
    pub name: String,
    pub description: Option<String>,
    pub is_preferred: bool,
    #[schema(value_type = Provenance)]
    pub provenance: String,
    pub source: Option<String>,
    #[schema(value_type = VerificationState)]
    pub verification_state: String,
    pub created_by: Uuid,
    pub verified_by: Option<Uuid>,
    pub verified_at: Option<DateTime<Utc>>,
    /// Send as `expected_parent_revision_id` on the next write.
    pub current_revision_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<rows::LocalizationRow> for Localization {
    fn from(r: rows::LocalizationRow) -> Self {
        Self {
            dir: if r.is_rtl { Dir::Rtl } else { Dir::Ltr },
            id: r.id,
            entity_type: r.entity_type,
            entity_id: r.entity_id,
            language: r.language_code,
            script: r.script_code,
            name: r.name,
            description: r.description,
            is_preferred: r.is_preferred,
            provenance: r.provenance,
            source: r.source,
            verification_state: r.verification_state,
            created_by: r.created_by,
            verified_by: r.verified_by,
            verified_at: r.verified_at,
            current_revision_id: r.current_revision_id,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct LocalizationPage {
    pub items: Vec<Localization>,
    pub next_cursor: Option<String>,
}

fn loc_page(p: application::pagination::Page<rows::LocalizationRow>) -> LocalizationPage {
    LocalizationPage {
        items: p.items.into_iter().map(Into::into).collect(),
        next_cursor: p.next_cursor,
    }
}

#[derive(Deserialize, ToSchema)]
pub struct LocalizationCreate {
    /// ISO 639-1 or 639-3 code, e.g. `ml`.
    #[serde(default)]
    pub language: String,
    /// ISO 15924 code, e.g. `Mlym`. Optional.
    pub script: Option<String>,
    #[serde(default)]
    pub name: String,
    pub description: Option<String>,
    /// Omitted: preferred iff it is the first name in that language. `true` when another preferred
    /// name exists is `409 preferred_slot_taken`; names are never swapped silently.
    pub is_preferred: Option<bool>,
    #[serde(default)]
    #[schema(value_type = Provenance)]
    pub provenance: String,
    /// Required for artist names (where the name comes from).
    pub source: Option<String>,
}

/// Absent fields are left unchanged; `null` clears an optional field.
#[derive(Deserialize, ToSchema)]
pub struct LocalizationPatch {
    /// The `current_revision_id` the client last read. Stale → `409 stale_revision` carrying the current one.
    pub expected_parent_revision_id: Option<Uuid>,
    pub language: Option<String>,
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>)]
    pub script: Option<Option<String>>,
    pub name: Option<String>,
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>)]
    pub description: Option<Option<String>>,
    pub is_preferred: Option<bool>,
    #[schema(value_type = Option<Provenance>)]
    pub provenance: Option<String>,
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>)]
    pub source: Option<Option<String>>,
}

#[derive(Deserialize, ToSchema)]
pub struct Decision {
    pub expected_parent_revision_id: Option<Uuid>,
}

#[utoipa::path(post, path = "/api/v1/entities/{entity_type}/{entity_id}/localizations", tag = "localization",
    request_body = LocalizationCreate, params(("entity_type" = String, Path), ("entity_id" = Uuid, Path)),
    security(("session" = [])), responses(
    (status = 201, body = Localization, description = "Pending review, or published when the author is a moderator"),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 404, body = Problem, content_type = "application/problem+json"),
    (status = 409, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn create_localization(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireUser(p): RequireUser,
    ApiPath((entity, id)): ApiPath<(String, Uuid)>,
    ApiJson(b): ApiJson<LocalizationCreate>,
) -> ApiResult<Created<Localization>> {
    let row = uc::create_localization(
        &app,
        &p,
        &meta,
        &entity,
        id,
        uc::LocalizationInput {
            language: b.language,
            script: b.script,
            name: b.name,
            description: b.description,
            is_preferred: b.is_preferred,
            provenance: b.provenance,
            source: b.source,
        },
    )
    .await?;
    Ok(created("localizations", row.id, row.into()))
}

#[utoipa::path(get, path = "/api/v1/entities/{entity_type}/{entity_id}/localizations", tag = "localization",
    params(("entity_type" = String, Path), ("entity_id" = Uuid, Path), PageQuery, StateQuery), responses(
    (status = 200, body = LocalizationPage),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json"),
    (status = 404, body = Problem, content_type = "application/problem+json")))]
pub async fn list_entity_localizations(
    State(app): State<App>,
    MaybeUser(viewer): MaybeUser,
    ApiPath((entity, id)): ApiPath<(String, Uuid)>,
    ApiQuery(q): ApiQuery<PageQuery>,
    ApiQuery(s): ApiQuery<StateQuery>,
) -> ApiResult<Json<LocalizationPage>> {
    let page = uc::list_entity_localizations(
        &app,
        viewer.as_ref(),
        &entity,
        id,
        s.state.as_deref(),
        q.cursor.as_deref(),
        q.limit,
    )
    .await?;
    Ok(Json(loc_page(page)))
}

#[utoipa::path(get, path = "/api/v1/localizations/{id}", tag = "localization", params(("id" = Uuid, Path)), responses(
    (status = 200, body = Localization),
    (status = 404, body = Problem, content_type = "application/problem+json", description = "Also for pending/rejected rows the caller may not see")))]
pub async fn get_localization(
    State(app): State<App>,
    MaybeUser(viewer): MaybeUser,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<Json<Localization>> {
    Ok(Json(uc::get_localization(&app, viewer.as_ref(), id).await?.into()))
}

#[utoipa::path(patch, path = "/api/v1/localizations/{id}", tag = "localization", request_body = LocalizationPatch,
    params(("id" = Uuid, Path)), security(("session" = [])), responses(
    (status = 200, body = Localization),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json"),
    (status = 404, body = Problem, content_type = "application/problem+json"),
    (status = 409, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn update_localization(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireUser(p): RequireUser,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(b): ApiJson<LocalizationPatch>,
) -> ApiResult<Json<Localization>> {
    let row = uc::update_localization(
        &app,
        &p,
        &meta,
        id,
        b.expected_parent_revision_id,
        uc::LocalizationPatch {
            language: b.language,
            script: b.script,
            name: b.name,
            description: b.description,
            is_preferred: b.is_preferred,
            provenance: b.provenance,
            source: b.source,
        },
    )
    .await?;
    Ok(Json(row.into()))
}

#[utoipa::path(post, path = "/api/v1/localizations/{id}/verify", tag = "moderation", request_body = Decision,
    params(("id" = Uuid, Path)), security(("session" = [])), responses(
    (status = 200, body = Localization),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json"),
    (status = 409, body = Problem, content_type = "application/problem+json")))]
pub async fn verify_localization(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireRole(p, ..): RequireRole<Moderator>,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(b): ApiJson<Decision>,
) -> ApiResult<Json<Localization>> {
    Ok(Json(
        uc::decide_localization(&app, &p, &meta, id, b.expected_parent_revision_id, uc::Verdict::Verify)
            .await?
            .into(),
    ))
}

#[utoipa::path(post, path = "/api/v1/localizations/{id}/reject", tag = "moderation", request_body = Decision,
    params(("id" = Uuid, Path)), security(("session" = [])), responses(
    (status = 200, body = Localization),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json"),
    (status = 409, body = Problem, content_type = "application/problem+json")))]
pub async fn reject_localization(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireRole(p, ..): RequireRole<Moderator>,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(b): ApiJson<Decision>,
) -> ApiResult<Json<Localization>> {
    Ok(Json(
        uc::decide_localization(&app, &p, &meta, id, b.expected_parent_revision_id, uc::Verdict::Reject)
            .await?
            .into(),
    ))
}

#[derive(Serialize, ToSchema)]
pub struct Revision {
    pub id: Uuid,
    pub localization_id: Uuid,
    pub revision_no: i32,
    pub parent_revision_id: Option<Uuid>,
    /// `created`, `edited`, `verified` or `rejected`.
    pub action: String,
    /// Immutable snapshot: `{ data: {...}, state }`.
    #[schema(value_type = Object)]
    pub data: serde_json::Value,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize, ToSchema)]
pub struct RevisionPage {
    pub items: Vec<Revision>,
    pub next_cursor: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/localizations/{id}/revisions", operation_id = "list_localization_revisions", tag = "localization",
    params(("id" = Uuid, Path), PageQuery), responses(
    (status = 200, body = RevisionPage),
    (status = 404, body = Problem, content_type = "application/problem+json")))]
pub async fn list_revisions(
    State(app): State<App>,
    MaybeUser(viewer): MaybeUser,
    ApiPath(id): ApiPath<Uuid>,
    ApiQuery(q): ApiQuery<PageQuery>,
) -> ApiResult<Json<RevisionPage>> {
    let page = uc::list_revisions(&app, viewer.as_ref(), id, q.cursor.as_deref(), q.limit).await?;
    Ok(Json(RevisionPage {
        items: page
            .items
            .into_iter()
            .map(|r| Revision {
                id: r.id,
                localization_id: r.localization_id,
                revision_no: r.revision_no,
                parent_revision_id: r.parent_revision_id,
                action: r.action,
                data: r.data,
                created_by: r.created_by,
                created_at: r.created_at,
            })
            .collect(),
        next_cursor: page.next_cursor,
    }))
}

#[utoipa::path(get, path = "/api/v1/moderation/localizations", tag = "moderation",
    params(PageQuery, StateQuery), security(("session" = [])), responses(
    (status = 200, body = LocalizationPage),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json")))]
pub async fn moderation_queue(
    State(app): State<App>,
    RequireRole(p, ..): RequireRole<Moderator>,
    ApiQuery(q): ApiQuery<PageQuery>,
    ApiQuery(s): ApiQuery<StateQuery>,
) -> ApiResult<Json<LocalizationPage>> {
    Ok(Json(loc_page(
        uc::moderation_queue(&app, &p, s.state.as_deref(), q.cursor.as_deref(), q.limit).await?,
    )))
}

// ---------- aliases ----------

/// A verified alternate spelling or name. Always carries its source.
#[derive(Serialize, ToSchema)]
pub struct Alias {
    pub id: Uuid,
    pub entity_type: String,
    pub entity_id: Uuid,
    pub alias: String,
    pub language: Option<String>,
    pub script: Option<String>,
    pub dir: Dir,
    pub source: String,
    #[schema(value_type = VerificationState)]
    pub verification_state: String,
    pub created_by: Uuid,
    pub verified_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

impl From<rows::AliasRow> for Alias {
    fn from(r: rows::AliasRow) -> Self {
        Self {
            dir: if r.is_rtl.unwrap_or(false) { Dir::Rtl } else { Dir::Auto },
            id: r.id,
            entity_type: r.entity_type,
            entity_id: r.entity_id,
            alias: r.alias,
            language: r.language_code,
            script: r.script_code,
            source: r.source,
            verification_state: r.verification_state,
            created_by: r.created_by,
            verified_by: r.verified_by,
            created_at: r.created_at,
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct AliasPage {
    pub items: Vec<Alias>,
    pub next_cursor: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct AliasCreate {
    #[serde(default)]
    pub alias: String,
    pub language: Option<String>,
    pub script: Option<String>,
    /// Required: where this alternate name is attested.
    #[serde(default)]
    pub source: String,
}

#[utoipa::path(post, path = "/api/v1/entities/{entity_type}/{entity_id}/aliases", tag = "localization",
    request_body = AliasCreate, params(("entity_type" = String, Path), ("entity_id" = Uuid, Path)),
    security(("session" = [])), responses(
    (status = 201, body = Alias),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 404, body = Problem, content_type = "application/problem+json"),
    (status = 409, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn create_alias(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireUser(p): RequireUser,
    ApiPath((entity, id)): ApiPath<(String, Uuid)>,
    ApiJson(b): ApiJson<AliasCreate>,
) -> ApiResult<Created<Alias>> {
    let row = uc::create_alias(
        &app,
        &p,
        &meta,
        &entity,
        id,
        uc::AliasInput {
            alias: b.alias,
            language: b.language,
            script: b.script,
            source: b.source,
        },
    )
    .await?;
    Ok(created("aliases", row.id, row.into()))
}

#[utoipa::path(get, path = "/api/v1/entities/{entity_type}/{entity_id}/aliases", tag = "localization",
    params(("entity_type" = String, Path), ("entity_id" = Uuid, Path), PageQuery, StateQuery), responses(
    (status = 200, body = AliasPage),
    (status = 404, body = Problem, content_type = "application/problem+json")))]
pub async fn list_aliases(
    State(app): State<App>,
    MaybeUser(viewer): MaybeUser,
    ApiPath((entity, id)): ApiPath<(String, Uuid)>,
    ApiQuery(q): ApiQuery<PageQuery>,
    ApiQuery(s): ApiQuery<StateQuery>,
) -> ApiResult<Json<AliasPage>> {
    let page = uc::list_aliases(
        &app,
        viewer.as_ref(),
        &entity,
        id,
        s.state.as_deref(),
        q.cursor.as_deref(),
        q.limit,
    )
    .await?;
    Ok(Json(AliasPage {
        items: page.items.into_iter().map(Into::into).collect(),
        next_cursor: page.next_cursor,
    }))
}

#[utoipa::path(post, path = "/api/v1/aliases/{id}/verify", tag = "moderation", params(("id" = Uuid, Path)),
    security(("session" = [])), responses(
    (status = 200, body = Alias),
    (status = 403, body = Problem, content_type = "application/problem+json"),
    (status = 409, body = Problem, content_type = "application/problem+json")))]
pub async fn verify_alias(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireRole(p, ..): RequireRole<Moderator>,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<Json<Alias>> {
    Ok(Json(
        uc::decide_alias(&app, &p, &meta, id, uc::Verdict::Verify).await?.into(),
    ))
}

#[utoipa::path(post, path = "/api/v1/aliases/{id}/reject", tag = "moderation", params(("id" = Uuid, Path)),
    security(("session" = [])), responses(
    (status = 200, body = Alias),
    (status = 403, body = Problem, content_type = "application/problem+json"),
    (status = 409, body = Problem, content_type = "application/problem+json")))]
pub async fn reject_alias(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireRole(p, ..): RequireRole<Moderator>,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<Json<Alias>> {
    Ok(Json(
        uc::decide_alias(&app, &p, &meta, id, uc::Verdict::Reject).await?.into(),
    ))
}

// ---------- default language ----------

#[derive(Deserialize, ToSchema)]
pub struct DefaultLanguageBody {
    /// ISO 639-1/639-3 code of the language the entity was originally written in.
    #[serde(default)]
    pub language: String,
    pub expected_version: Option<i32>,
}

#[derive(Serialize, ToSchema)]
pub struct VersionBody {
    pub version: i32,
}

#[utoipa::path(put, path = "/api/v1/entities/{entity_type}/{entity_id}/default-language", tag = "localization",
    request_body = DefaultLanguageBody, params(("entity_type" = String, Path), ("entity_id" = Uuid, Path)),
    security(("session" = [])), responses(
    (status = 200, body = VersionBody),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json"),
    (status = 404, body = Problem, content_type = "application/problem+json"),
    (status = 409, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn set_default_language(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireUser(p): RequireUser,
    ApiPath((entity, id)): ApiPath<(String, Uuid)>,
    ApiJson(b): ApiJson<DefaultLanguageBody>,
) -> ApiResult<Json<VersionBody>> {
    let version = uc::set_default_language(&app, &p, &meta, &entity, id, &b.language, b.expected_version).await?;
    Ok(Json(VersionBody { version }))
}

// ---------- taxonomy: labels, publishers, genres ----------

#[derive(Serialize, ToSchema)]
pub struct TaxonomyItem {
    pub id: Uuid,
    pub name: String,
    pub display: Display,
    pub version: i32,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

fn taxonomy_item(
    kind: domain::localization::EntityType,
    r: rows::TaxonomyRow,
    n: &application::localization::Names,
) -> TaxonomyItem {
    TaxonomyItem {
        display: n.get(kind, r.id, &r.name).into(),
        id: r.id,
        name: r.name,
        version: r.version,
        created_by: r.created_by,
        created_at: r.created_at,
    }
}

#[derive(Serialize, ToSchema)]
pub struct TaxonomyPage {
    pub items: Vec<TaxonomyItem>,
    pub next_cursor: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct TaxonomyCreate {
    #[serde(default)]
    pub name: String,
}

fn kind_of(plural: &str) -> &str {
    plural.strip_suffix('s').unwrap_or(plural)
}

#[utoipa::path(get, path = "/api/v1/taxonomy/{kind}", tag = "localization",
    params(("kind" = String, Path, description = "labels, publishers or genres"), PageQuery, LangQuery), responses(
    (status = 200, body = TaxonomyPage),
    (status = 404, body = Problem, content_type = "application/problem+json")))]
pub async fn list_taxonomy(
    State(app): State<App>,
    ApiPath(kind): ApiPath<String>,
    ApiQuery(q): ApiQuery<PageQuery>,
    ApiQuery(l): ApiQuery<LangQuery>,
) -> ApiResult<Json<TaxonomyPage>> {
    let k = kind_of(&kind);
    let et = domain::localization::EntityType::parse(k).unwrap_or(domain::localization::EntityType::Label);
    let listed = uc::list_taxonomy(&app, k, q.cursor.as_deref(), q.limit, l.lang.as_deref()).await?;
    Ok(Json(TaxonomyPage {
        items: listed
            .page
            .items
            .into_iter()
            .map(|r| taxonomy_item(et, r, &listed.names))
            .collect(),
        next_cursor: listed.page.next_cursor,
    }))
}

#[utoipa::path(post, path = "/api/v1/taxonomy/{kind}", tag = "localization", request_body = TaxonomyCreate,
    params(("kind" = String, Path)), security(("session" = [])), responses(
    (status = 201, body = TaxonomyItem),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 404, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn create_taxonomy(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireUser(p): RequireUser,
    ApiPath(kind): ApiPath<String>,
    ApiJson(b): ApiJson<TaxonomyCreate>,
) -> ApiResult<Created<TaxonomyItem>> {
    let k = kind_of(&kind);
    let et = domain::localization::EntityType::parse(k).unwrap_or(domain::localization::EntityType::Label);
    let (row, names) = uc::create_taxonomy(&app, &p, &meta, k, &b.name).await?;
    let id = row.id;
    Ok(created(&format!("taxonomy/{kind}"), id, taxonomy_item(et, row, &names)))
}

#[utoipa::path(get, path = "/api/v1/taxonomy/{kind}/{id}", tag = "localization",
    params(("kind" = String, Path), ("id" = Uuid, Path), LangQuery), responses(
    (status = 200, body = TaxonomyItem),
    (status = 301, description = "Merged into another record; follow `Location`"),
    (status = 404, body = Problem, content_type = "application/problem+json")))]
pub async fn get_taxonomy(
    State(app): State<App>,
    ApiPath((kind, id)): ApiPath<(String, Uuid)>,
    ApiQuery(l): ApiQuery<LangQuery>,
) -> ApiResult<axum::response::Response> {
    let k = kind_of(&kind);
    let et = domain::localization::EntityType::parse(k).unwrap_or(domain::localization::EntityType::Label);
    let found = uc::get_taxonomy(&app, k, id, l.lang.as_deref())
        .await
        .map(|(row, names)| taxonomy_item(et, row, &names));
    crate::entities::or_redirect(&app, k, &format!("taxonomy/{kind}"), id, l.lang.as_deref(), found).await
}
