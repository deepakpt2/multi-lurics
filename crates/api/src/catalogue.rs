//! Catalogue endpoints. Reads are public; writes need a session. Handlers only translate
//! HTTP ⇄ use cases; every rule (authorization included) is enforced in `application`.

use crate::extract::{ApiJson, ApiPath, ApiQuery, Meta, RequireUser};
use crate::localization::{Display, LangQuery, RoleCategory};
use crate::problem::{ApiResult, Problem};
use crate::routes::PageQuery;
use application::catalogue as uc;
use application::localization::Names;
use application::App;
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::Json;
use chrono::{DateTime, Utc};
use domain::localization::EntityType;
use infrastructure::catalogue as rows;
use serde::{Deserialize, Deserializer, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

/// Distinguishes an absent field (`None`, leave unchanged) from an explicit `null` (`Some(None)`, clear).
pub(crate) fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(d).map(Some)
}

// ---------- schema-only enums (inputs stay strings so bad values become field errors) ----------

#[derive(Serialize, ToSchema)]
#[allow(dead_code)]
#[serde(rename_all = "snake_case")]
pub enum ArtistKind {
    Person,
    Group,
    Other,
}

#[derive(Serialize, ToSchema)]
#[allow(dead_code)]
#[serde(rename_all = "snake_case")]
pub enum AlbumKind {
    Album,
    Single,
    Ep,
    Compilation,
    Soundtrack,
    Other,
}

/// Composition: composer, lyricist. Recording: performer, featured_performer, arranger, producer,
/// conductor. Album: primary_artist, producer.
#[derive(Serialize, ToSchema)]
#[allow(dead_code)]
#[serde(rename_all = "snake_case")]
pub enum CreditRole {
    Composer,
    Lyricist,
    Performer,
    FeaturedPerformer,
    Arranger,
    Producer,
    Conductor,
    PrimaryArtist,
}

/// All kinds except `related_performance` require both recordings to share a composition.
#[derive(Serialize, ToSchema)]
#[allow(dead_code)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipKind {
    Cover,
    Remake,
    Live,
    AlternateArrangement,
    RelatedPerformance,
}

// ---------- shared DTOs ----------

#[derive(Deserialize, ToSchema)]
pub struct CreditInput {
    pub artist_id: Uuid,
    #[schema(value_type = CreditRole)]
    pub role: String,
}

#[derive(Deserialize, ToSchema)]
pub struct RelationshipInput {
    #[schema(value_type = RelationshipKind)]
    pub kind: String,
    pub related_recording_id: Uuid,
}

fn credits_in(v: Option<Vec<CreditInput>>) -> Option<Vec<uc::CreditInput>> {
    v.map(|v| {
        v.into_iter()
            .map(|c| uc::CreditInput {
                artist_id: c.artist_id,
                role: c.role,
            })
            .collect()
    })
}

/// One artist in one role. Artist names are shown verbatim, never translated.
#[derive(Serialize, ToSchema)]
pub struct Credit {
    pub artist_id: Uuid,
    /// Base (original) name; use `artist_display` for the localized one.
    pub artist_name: String,
    pub artist_display: Display,
    #[schema(value_type = CreditRole)]
    pub role: String,
    /// Id of the credit role entity, which can be localized like any other.
    pub role_id: Uuid,
    /// Original songwriters, translators and adapters are distinct categories; group by this.
    pub role_category: RoleCategory,
    pub role_display: Display,
}

fn credits_out(v: Vec<rows::CreditRow>, n: &Names) -> Vec<Credit> {
    v.into_iter()
        .map(|c| Credit {
            artist_display: n.get(EntityType::Artist, c.artist_id, &c.artist_name).into(),
            role_display: n.get(EntityType::CreditRole, c.role_id, &c.role_label).into(),
            role_category: RoleCategory::from_code(&c.role_category),
            role_id: c.role_id,
            artist_id: c.artist_id,
            artist_name: c.artist_name,
            role: c.role,
        })
        .collect()
}

pub(crate) fn created<T>(path: &str, id: Uuid, body: T) -> (StatusCode, [(header::HeaderName, String); 1], Json<T>) {
    (
        StatusCode::CREATED,
        [(header::LOCATION, format!("/api/v1/{path}/{id}"))],
        Json(body),
    )
}

pub(crate) type Created<T> = (StatusCode, [(header::HeaderName, String); 1], Json<T>);

// ---------- artists ----------

#[derive(Deserialize, ToSchema)]
pub struct ArtistCreate {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    #[schema(value_type = ArtistKind)]
    pub kind: String,
    pub disambiguation: Option<String>,
    /// ISNI; spaces and hyphens allowed, check digit verified.
    pub isni: Option<String>,
}

/// Absent fields are left unchanged; `null` clears an optional field.
#[derive(Deserialize, ToSchema)]
pub struct ArtistPatch {
    /// The `version` the client last read. Stale → `409 version_conflict`.
    pub expected_version: Option<i32>,
    pub name: Option<String>,
    #[schema(value_type = Option<ArtistKind>)]
    pub kind: Option<String>,
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>)]
    pub disambiguation: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>)]
    pub isni: Option<Option<String>>,
}

#[derive(Serialize, ToSchema)]
pub struct Artist {
    pub id: Uuid,
    /// Name resolved for the requested `lang` (see `fallback`).
    pub display: Display,
    /// As written, in any script. Never translated.
    pub name: String,
    #[schema(value_type = ArtistKind)]
    pub kind: String,
    pub disambiguation: Option<String>,
    pub isni: Option<String>,
    pub version: i32,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Artist {
    fn of(r: rows::ArtistRow, n: &Names) -> Self {
        let display = n.get(EntityType::Artist, r.id, &r.name).into();
        Self {
            display,
            id: r.id,
            name: r.name,
            kind: r.kind,
            disambiguation: r.disambiguation,
            isni: r.isni,
            version: r.version,
            created_by: r.created_by,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct ArtistCredit {
    /// `composition`, `recording` or `album`.
    pub target_type: String,
    pub target_id: Uuid,
    pub title: String,
    pub title_display: Display,
    pub role_id: Uuid,
    pub role_category: RoleCategory,
    pub role_display: Display,
    #[schema(value_type = CreditRole)]
    pub role: String,
}

#[derive(Serialize, ToSchema)]
pub struct ArtistDetail {
    #[serde(flatten)]
    pub artist: Artist,
    /// Most recent 100 credits.
    pub credits: Vec<ArtistCredit>,
}

impl From<uc::ArtistDetail> for ArtistDetail {
    fn from(d: uc::ArtistDetail) -> Self {
        let n = d.names;
        Self {
            artist: Artist::of(d.artist, &n),
            credits: d
                .credits
                .into_iter()
                .map(|c| {
                    let et = match c.target_type.as_str() {
                        "composition" => EntityType::Composition,
                        "recording" => EntityType::Recording,
                        _ => EntityType::Album,
                    };
                    ArtistCredit {
                        title_display: n.get(et, c.target_id, &c.title).into(),
                        role_display: n.get(EntityType::CreditRole, c.role_id, &c.role_label).into(),
                        role_category: RoleCategory::from_code(&c.role_category),
                        role_id: c.role_id,
                        target_type: c.target_type,
                        target_id: c.target_id,
                        title: c.title,
                        role: c.role,
                    }
                })
                .collect(),
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct ArtistPage {
    pub items: Vec<Artist>,
    pub next_cursor: Option<String>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ArtistFilter {
    #[param(value_type = Option<ArtistKind>)]
    pub kind: Option<String>,
    pub isni: Option<String>,
    /// Case-insensitive name prefix.
    pub prefix: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/artists", tag = "catalogue", request_body = ArtistCreate,
    security(("session" = [])), responses(
    (status = 201, body = ArtistDetail),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 409, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn create_artist(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireUser(p): RequireUser,
    ApiJson(b): ApiJson<ArtistCreate>,
) -> ApiResult<Created<ArtistDetail>> {
    let d = uc::create_artist(
        &app,
        &p,
        &meta,
        uc::ArtistInput {
            name: Some(b.name),
            kind: Some(b.kind),
            disambiguation: Some(b.disambiguation),
            isni: Some(b.isni),
        },
    )
    .await?;
    Ok(created("artists", d.artist.id, d.into()))
}

#[utoipa::path(patch, path = "/api/v1/artists/{id}", tag = "catalogue", request_body = ArtistPatch,
    params(("id" = Uuid, Path)), security(("session" = [])), responses(
    (status = 200, body = ArtistDetail),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json"),
    (status = 404, body = Problem, content_type = "application/problem+json"),
    (status = 409, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn update_artist(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireUser(p): RequireUser,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(b): ApiJson<ArtistPatch>,
) -> ApiResult<Json<ArtistDetail>> {
    let d = uc::update_artist(
        &app,
        &p,
        &meta,
        id,
        b.expected_version,
        uc::ArtistInput {
            name: b.name,
            kind: b.kind,
            disambiguation: b.disambiguation,
            isni: b.isni,
        },
    )
    .await?;
    Ok(Json(d.into()))
}

#[utoipa::path(get, path = "/api/v1/artists/{id}", tag = "catalogue", params(("id" = Uuid, Path), LangQuery), responses(
    (status = 200, body = ArtistDetail),
    (status = 301, description = "Merged into another record; follow `Location`"),
    (status = 404, body = Problem, content_type = "application/problem+json")))]
pub async fn get_artist(
    State(app): State<App>,
    ApiPath(id): ApiPath<Uuid>,
    ApiQuery(l): ApiQuery<LangQuery>,
) -> ApiResult<axum::response::Response> {
    let found = uc::get_artist(&app, id, l.lang.as_deref())
        .await
        .map(ArtistDetail::from);
    crate::entities::or_redirect(&app, "artist", "artists", id, l.lang.as_deref(), found).await
}

#[utoipa::path(get, path = "/api/v1/artists", tag = "catalogue", params(PageQuery, ArtistFilter, LangQuery), responses(
    (status = 200, body = ArtistPage),
    (status = 400, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn list_artists(
    State(app): State<App>,
    ApiQuery(q): ApiQuery<PageQuery>,
    ApiQuery(f): ApiQuery<ArtistFilter>,
    ApiQuery(lq): ApiQuery<LangQuery>,
) -> ApiResult<Json<ArtistPage>> {
    let filter = rows::ArtistFilter {
        kind: f.kind,
        isni: f.isni,
        name_prefix: f.prefix,
    };
    let l = uc::list_artists(&app, filter, q.cursor.as_deref(), q.limit, lq.lang.as_deref()).await?;
    Ok(Json(ArtistPage {
        items: l.page.items.into_iter().map(|r| Artist::of(r, &l.names)).collect(),
        next_cursor: l.page.next_cursor,
    }))
}

// ---------- compositions ----------

#[derive(Deserialize, ToSchema)]
pub struct CompositionCreate {
    #[serde(default)]
    pub title: String,
    /// ISWC, e.g. `T-034.524.680-1`; check digit verified.
    pub iswc: Option<String>,
    pub year: Option<i32>,
    /// `mainstream` (film, album, independent; the default) or `devotional`.
    pub category: Option<String>,
    /// Raga, as written (any script).
    pub raga: Option<String>,
    /// `hindu`, `sikh`, `buddhist`, `jain`, `christian`, `muslim` or `other`. Devotional songs only.
    pub religion: Option<String>,
    /// Deity or revered figure, as written. Devotional songs only.
    pub deity: Option<String>,
    /// Free tags (at most 20, 40 characters each).
    #[serde(default)]
    pub tags: Vec<String>,
    /// Devotional songs only: `bhajan`, `kirtan`, `stotra`, `aarti`, `mantra`, `chalisa`, `shabad`, `qawwali`,
    /// `hymn` and others (see the `devotional_forms` list in the web app).
    pub devotional_form: Option<String>,
    /// Devotional songs only: the scripture or text it comes from, e.g. `Bhagavad Gita 2.47`.
    pub source_work: Option<String>,
    /// Devotional songs only: festivals and times it is sung at (at most 10).
    #[serde(default)]
    pub occasions: Vec<String>,
    /// A YouTube link in any common form (watch, youtu.be, shorts, embed) or a video id. Stored as the id.
    pub youtube: Option<String>,
    /// Work-level credits only: composer (the music director) and lyricist.
    pub credits: Option<Vec<CreditInput>>,
}

/// Absent fields are left unchanged; `null` clears; `credits`, when present, replaces the list.
#[derive(Deserialize, ToSchema)]
pub struct CompositionPatch {
    pub expected_version: Option<i32>,
    pub title: Option<String>,
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>)]
    pub iswc: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<i32>)]
    pub year: Option<Option<i32>>,
    pub category: Option<String>,
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>)]
    pub raga: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>)]
    pub religion: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>)]
    pub deity: Option<Option<String>>,
    /// When present, replaces all tags.
    pub tags: Option<Vec<String>>,
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>)]
    pub devotional_form: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>)]
    pub source_work: Option<Option<String>>,
    /// When present, replaces all occasions.
    pub occasions: Option<Vec<String>>,
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>)]
    pub youtube: Option<Option<String>>,
    pub credits: Option<Vec<CreditInput>>,
}

#[derive(Serialize, ToSchema)]
pub struct Composition {
    pub id: Uuid,
    /// Name resolved for the requested `lang` (see `fallback`).
    pub display: Display,
    pub title: String,
    pub iswc: Option<String>,
    pub year: Option<i16>,
    pub category: String,
    pub raga: Option<String>,
    pub religion: Option<String>,
    pub deity: Option<String>,
    pub tags: Vec<String>,
    pub devotional_form: Option<String>,
    pub source_work: Option<String>,
    pub occasions: Vec<String>,
    /// The song's YouTube video id (embed it; never store links).
    pub youtube_id: Option<String>,
    pub version: i32,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Composition {
    fn of(r: rows::CompositionRow, n: &Names) -> Self {
        let display = n.get(EntityType::Composition, r.id, &r.title).into();
        Self {
            display,
            id: r.id,
            title: r.title,
            iswc: r.iswc,
            year: r.year,
            category: r.category,
            raga: r.raga,
            religion: r.religion,
            deity: r.deity,
            tags: r.tags,
            devotional_form: r.devotional_form,
            source_work: r.source_work,
            occasions: r.occasions,
            youtube_id: r.youtube_id,
            version: r.version,
            created_by: r.created_by,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct RecordingSummary {
    pub id: Uuid,
    pub title: String,
    pub display: Display,
    pub composition_id: Uuid,
    pub isrc: Option<String>,
    pub duration_ms: Option<i32>,
}

#[derive(Serialize, ToSchema)]
pub struct CompositionDetail {
    #[serde(flatten)]
    pub composition: Composition,
    pub credits: Vec<Credit>,
    /// Newest 100 recordings; page through `GET /recordings?composition_id=` for all.
    pub recordings: Vec<RecordingSummary>,
    pub recording_count: i64,
}

impl From<uc::CompositionDetail> for CompositionDetail {
    fn from(d: uc::CompositionDetail) -> Self {
        let n = d.names;
        Self {
            composition: Composition::of(d.composition, &n),
            credits: credits_out(d.credits, &n),
            recordings: d
                .recordings
                .into_iter()
                .map(|r| RecordingSummary {
                    display: n.get(EntityType::Recording, r.id, &r.title).into(),
                    id: r.id,
                    title: r.title,
                    composition_id: r.composition_id,
                    isrc: r.isrc,
                    duration_ms: r.duration_ms,
                })
                .collect(),
            recording_count: d.recording_count,
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct CompositionPage {
    pub items: Vec<Composition>,
    pub next_cursor: Option<String>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct CompositionFilter {
    /// Compositions crediting this artist.
    pub artist_id: Option<Uuid>,
    pub iswc: Option<String>,
    /// Case-insensitive title prefix.
    pub prefix: Option<String>,
    /// `mainstream` or `devotional`.
    pub category: Option<String>,
    pub raga: Option<String>,
    pub deity: Option<String>,
    /// Songs carrying this tag.
    pub tag: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/compositions", tag = "catalogue", request_body = CompositionCreate,
    security(("session" = [])), responses(
    (status = 201, body = CompositionDetail),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 409, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn create_composition(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireUser(p): RequireUser,
    ApiJson(b): ApiJson<CompositionCreate>,
) -> ApiResult<Created<CompositionDetail>> {
    let d = uc::create_composition(
        &app,
        &p,
        &meta,
        uc::CompositionInput {
            title: Some(b.title),
            iswc: Some(b.iswc),
            year: Some(b.year),
            category: b.category,
            raga: Some(b.raga),
            religion: Some(b.religion),
            deity: Some(b.deity),
            tags: Some(b.tags),
            devotional_form: Some(b.devotional_form),
            source_work: Some(b.source_work),
            occasions: Some(b.occasions),
            youtube: Some(b.youtube),
            credits: credits_in(b.credits),
        },
    )
    .await?;
    Ok(created("compositions", d.composition.id, d.into()))
}

#[utoipa::path(patch, path = "/api/v1/compositions/{id}", tag = "catalogue", request_body = CompositionPatch,
    params(("id" = Uuid, Path)), security(("session" = [])), responses(
    (status = 200, body = CompositionDetail),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json"),
    (status = 404, body = Problem, content_type = "application/problem+json"),
    (status = 409, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn update_composition(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireUser(p): RequireUser,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(b): ApiJson<CompositionPatch>,
) -> ApiResult<Json<CompositionDetail>> {
    let d = uc::update_composition(
        &app,
        &p,
        &meta,
        id,
        b.expected_version,
        uc::CompositionInput {
            title: b.title,
            iswc: b.iswc,
            year: b.year,
            category: b.category,
            raga: b.raga,
            religion: b.religion,
            deity: b.deity,
            tags: b.tags,
            devotional_form: b.devotional_form,
            source_work: b.source_work,
            occasions: b.occasions,
            youtube: b.youtube,
            credits: credits_in(b.credits),
        },
    )
    .await?;
    Ok(Json(d.into()))
}

#[utoipa::path(get, path = "/api/v1/compositions/{id}", tag = "catalogue", params(("id" = Uuid, Path), LangQuery), responses(
    (status = 200, body = CompositionDetail),
    (status = 404, body = Problem, content_type = "application/problem+json")))]
pub async fn get_composition(
    State(app): State<App>,
    ApiPath(id): ApiPath<Uuid>,
    ApiQuery(l): ApiQuery<LangQuery>,
) -> ApiResult<Json<CompositionDetail>> {
    Ok(Json(uc::get_composition(&app, id, l.lang.as_deref()).await?.into()))
}

#[utoipa::path(get, path = "/api/v1/compositions", tag = "catalogue", params(PageQuery, CompositionFilter, LangQuery), responses(
    (status = 200, body = CompositionPage),
    (status = 400, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn list_compositions(
    State(app): State<App>,
    ApiQuery(q): ApiQuery<PageQuery>,
    ApiQuery(f): ApiQuery<CompositionFilter>,
    ApiQuery(lq): ApiQuery<LangQuery>,
) -> ApiResult<Json<CompositionPage>> {
    let filter = rows::CompositionFilter {
        artist_id: f.artist_id,
        iswc: f.iswc,
        title_prefix: f.prefix,
        category: f.category,
        raga: f.raga,
        deity: f.deity,
        tag: f.tag,
    };
    let l = uc::list_compositions(&app, filter, q.cursor.as_deref(), q.limit, lq.lang.as_deref()).await?;
    Ok(Json(CompositionPage {
        items: l.page.items.into_iter().map(|r| Composition::of(r, &l.names)).collect(),
        next_cursor: l.page.next_cursor,
    }))
}

// ---------- recordings ----------

#[derive(Deserialize, ToSchema)]
pub struct RecordingCreate {
    /// The existing composition this is a recording of (covers reuse the original's composition).
    pub composition_id: Option<Uuid>,
    #[serde(default)]
    pub title: String,
    /// ISRC, e.g. `US-RC1-76-07839`.
    pub isrc: Option<String>,
    pub duration_ms: Option<i64>,
    pub year: Option<i32>,
    /// Performance-level credits (performer, featured_performer, arranger, producer, conductor).
    pub credits: Option<Vec<CreditInput>>,
    /// Links from this recording to others (e.g. this is a `cover` of `related_recording_id`).
    pub relationships: Option<Vec<RelationshipInput>>,
}

/// Absent fields are left unchanged; `null` clears; `credits`/`relationships`, when present, replace the list.
#[derive(Deserialize, ToSchema)]
pub struct RecordingPatch {
    pub expected_version: Option<i32>,
    pub composition_id: Option<Uuid>,
    pub title: Option<String>,
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>)]
    pub isrc: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<i64>)]
    pub duration_ms: Option<Option<i64>>,
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<i32>)]
    pub year: Option<Option<i32>>,
    pub credits: Option<Vec<CreditInput>>,
    pub relationships: Option<Vec<RelationshipInput>>,
}

#[derive(Serialize, ToSchema)]
pub struct Recording {
    pub id: Uuid,
    /// Name resolved for the requested `lang` (see `fallback`).
    pub display: Display,
    pub composition_id: Uuid,
    pub title: String,
    pub isrc: Option<String>,
    pub duration_ms: Option<i32>,
    pub year: Option<i16>,
    pub version: i32,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Recording {
    fn of(r: rows::RecordingRow, n: &Names) -> Self {
        let display = n.get(EntityType::Recording, r.id, &r.title).into();
        Self {
            display,
            id: r.id,
            composition_id: r.composition_id,
            title: r.title,
            isrc: r.isrc,
            duration_ms: r.duration_ms,
            year: r.year,
            version: r.version,
            created_by: r.created_by,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct CompositionRef {
    pub id: Uuid,
    pub title: String,
    pub display: Display,
}

#[derive(Serialize, ToSchema)]
pub struct RelatedRecording {
    pub display: Display,
    #[schema(value_type = RelationshipKind)]
    pub kind: String,
    pub recording_id: Uuid,
    pub title: String,
    pub composition_id: Uuid,
}

#[derive(Serialize, ToSchema)]
pub struct AlbumAppearance {
    pub display: Display,
    pub album_id: Uuid,
    pub title: String,
    pub position: i32,
}

#[derive(Serialize, ToSchema)]
pub struct RecordingDetail {
    #[serde(flatten)]
    pub recording: Recording,
    pub composition: CompositionRef,
    pub credits: Vec<Credit>,
    /// This recording → others (e.g. it is a `cover` of …).
    pub relationships: Vec<RelatedRecording>,
    /// Others → this recording (e.g. … is a `cover` of it).
    pub related_from: Vec<RelatedRecording>,
    pub albums: Vec<AlbumAppearance>,
}

fn related(v: Vec<rows::RelationshipRow>, n: &Names) -> Vec<RelatedRecording> {
    v.into_iter()
        .map(|r| RelatedRecording {
            display: n.get(EntityType::Recording, r.other_id, &r.other_title).into(),
            kind: r.kind,
            recording_id: r.other_id,
            title: r.other_title,
            composition_id: r.other_composition_id,
        })
        .collect()
}

impl From<uc::RecordingDetail> for RecordingDetail {
    fn from(d: uc::RecordingDetail) -> Self {
        let n = d.names;
        Self {
            recording: Recording::of(d.recording, &n),
            composition: CompositionRef {
                display: n
                    .get(EntityType::Composition, d.composition.id, &d.composition.title)
                    .into(),
                id: d.composition.id,
                title: d.composition.title,
            },
            credits: credits_out(d.credits, &n),
            relationships: related(d.relationships, &n),
            related_from: related(d.related_from, &n),
            albums: d
                .albums
                .into_iter()
                .map(|a| AlbumAppearance {
                    display: n.get(EntityType::Album, a.album_id, &a.title).into(),
                    album_id: a.album_id,
                    title: a.title,
                    position: a.position,
                })
                .collect(),
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct RecordingPage {
    pub items: Vec<Recording>,
    pub next_cursor: Option<String>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct RecordingFilter {
    pub composition_id: Option<Uuid>,
    /// Recordings crediting this artist.
    pub artist_id: Option<Uuid>,
    /// Recordings on this album.
    pub album_id: Option<Uuid>,
    pub isrc: Option<String>,
    /// Case-insensitive title prefix.
    pub prefix: Option<String>,
}

fn rels_in(v: Option<Vec<RelationshipInput>>) -> Option<Vec<uc::RelationshipInput>> {
    v.map(|v| {
        v.into_iter()
            .map(|r| uc::RelationshipInput {
                kind: r.kind,
                related_recording_id: r.related_recording_id,
            })
            .collect()
    })
}

#[utoipa::path(post, path = "/api/v1/recordings", tag = "catalogue", request_body = RecordingCreate,
    security(("session" = [])), responses(
    (status = 201, body = RecordingDetail),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 409, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn create_recording(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireUser(p): RequireUser,
    ApiJson(b): ApiJson<RecordingCreate>,
) -> ApiResult<Created<RecordingDetail>> {
    let d = uc::create_recording(
        &app,
        &p,
        &meta,
        uc::RecordingInput {
            composition_id: b.composition_id,
            title: Some(b.title),
            isrc: Some(b.isrc),
            duration_ms: Some(b.duration_ms),
            year: Some(b.year),
            credits: credits_in(b.credits),
            relationships: rels_in(b.relationships),
        },
    )
    .await?;
    Ok(created("recordings", d.recording.id, d.into()))
}

#[utoipa::path(patch, path = "/api/v1/recordings/{id}", tag = "catalogue", request_body = RecordingPatch,
    params(("id" = Uuid, Path)), security(("session" = [])), responses(
    (status = 200, body = RecordingDetail),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json"),
    (status = 404, body = Problem, content_type = "application/problem+json"),
    (status = 409, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn update_recording(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireUser(p): RequireUser,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(b): ApiJson<RecordingPatch>,
) -> ApiResult<Json<RecordingDetail>> {
    let d = uc::update_recording(
        &app,
        &p,
        &meta,
        id,
        b.expected_version,
        uc::RecordingInput {
            composition_id: b.composition_id,
            title: b.title,
            isrc: b.isrc,
            duration_ms: b.duration_ms,
            year: b.year,
            credits: credits_in(b.credits),
            relationships: rels_in(b.relationships),
        },
    )
    .await?;
    Ok(Json(d.into()))
}

#[utoipa::path(get, path = "/api/v1/recordings/{id}", tag = "catalogue", params(("id" = Uuid, Path), LangQuery), responses(
    (status = 200, body = RecordingDetail),
    (status = 404, body = Problem, content_type = "application/problem+json")))]
pub async fn get_recording(
    State(app): State<App>,
    ApiPath(id): ApiPath<Uuid>,
    ApiQuery(l): ApiQuery<LangQuery>,
) -> ApiResult<Json<RecordingDetail>> {
    Ok(Json(uc::get_recording(&app, id, l.lang.as_deref()).await?.into()))
}

#[utoipa::path(get, path = "/api/v1/recordings", tag = "catalogue", params(PageQuery, RecordingFilter, LangQuery), responses(
    (status = 200, body = RecordingPage),
    (status = 400, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn list_recordings(
    State(app): State<App>,
    ApiQuery(q): ApiQuery<PageQuery>,
    ApiQuery(f): ApiQuery<RecordingFilter>,
    ApiQuery(lq): ApiQuery<LangQuery>,
) -> ApiResult<Json<RecordingPage>> {
    let filter = rows::RecordingFilter {
        composition_id: f.composition_id,
        artist_id: f.artist_id,
        album_id: f.album_id,
        isrc: f.isrc,
        title_prefix: f.prefix,
    };
    let l = uc::list_recordings(&app, filter, q.cursor.as_deref(), q.limit, lq.lang.as_deref()).await?;
    Ok(Json(RecordingPage {
        items: l.page.items.into_iter().map(|r| Recording::of(r, &l.names)).collect(),
        next_cursor: l.page.next_cursor,
    }))
}

// ---------- albums ----------

#[derive(Deserialize, ToSchema)]
pub struct AlbumCreate {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    #[schema(value_type = AlbumKind)]
    pub kind: String,
    pub release_year: Option<i32>,
    /// UPC/EAN (GTIN-8/12/13/14); check digit verified.
    pub barcode: Option<String>,
    /// Album credits (primary_artist, producer).
    pub credits: Option<Vec<CreditInput>>,
    /// Recording ids in track order (positions 1..n).
    pub tracks: Option<Vec<Uuid>>,
}

/// Absent fields are left unchanged; `null` clears; `credits`/`tracks`, when present, replace the list.
#[derive(Deserialize, ToSchema)]
pub struct AlbumPatch {
    pub expected_version: Option<i32>,
    pub title: Option<String>,
    #[schema(value_type = Option<AlbumKind>)]
    pub kind: Option<String>,
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<i32>)]
    pub release_year: Option<Option<i32>>,
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>)]
    pub barcode: Option<Option<String>>,
    pub credits: Option<Vec<CreditInput>>,
    pub tracks: Option<Vec<Uuid>>,
}

#[derive(Serialize, ToSchema)]
pub struct Album {
    pub id: Uuid,
    /// Name resolved for the requested `lang` (see `fallback`).
    pub display: Display,
    pub title: String,
    #[schema(value_type = AlbumKind)]
    pub kind: String,
    pub release_year: Option<i16>,
    pub barcode: Option<String>,
    pub version: i32,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Album {
    fn of(r: rows::AlbumRow, n: &Names) -> Self {
        let display = n.get(EntityType::Album, r.id, &r.title).into();
        Self {
            display,
            id: r.id,
            title: r.title,
            kind: r.kind,
            release_year: r.release_year,
            barcode: r.barcode,
            version: r.version,
            created_by: r.created_by,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct Track {
    pub display: Display,
    pub position: i32,
    pub recording_id: Uuid,
    pub title: String,
    pub composition_id: Uuid,
    pub duration_ms: Option<i32>,
}

#[derive(Serialize, ToSchema)]
pub struct AlbumDetail {
    #[serde(flatten)]
    pub album: Album,
    pub credits: Vec<Credit>,
    pub tracks: Vec<Track>,
}

impl From<uc::AlbumDetail> for AlbumDetail {
    fn from(d: uc::AlbumDetail) -> Self {
        let n = d.names;
        Self {
            album: Album::of(d.album, &n),
            credits: credits_out(d.credits, &n),
            tracks: d
                .tracks
                .into_iter()
                .map(|t| Track {
                    display: n.get(EntityType::Recording, t.recording_id, &t.title).into(),
                    position: t.position,
                    recording_id: t.recording_id,
                    title: t.title,
                    composition_id: t.composition_id,
                    duration_ms: t.duration_ms,
                })
                .collect(),
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct AlbumPage {
    pub items: Vec<Album>,
    pub next_cursor: Option<String>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AlbumFilter {
    /// Albums crediting this artist.
    pub artist_id: Option<Uuid>,
    #[param(value_type = Option<AlbumKind>)]
    pub kind: Option<String>,
    pub barcode: Option<String>,
    /// Case-insensitive title prefix.
    pub prefix: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/albums", tag = "catalogue", request_body = AlbumCreate,
    security(("session" = [])), responses(
    (status = 201, body = AlbumDetail),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 409, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn create_album(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireUser(p): RequireUser,
    ApiJson(b): ApiJson<AlbumCreate>,
) -> ApiResult<Created<AlbumDetail>> {
    let d = uc::create_album(
        &app,
        &p,
        &meta,
        uc::AlbumInput {
            title: Some(b.title),
            kind: Some(b.kind),
            release_year: Some(b.release_year),
            barcode: Some(b.barcode),
            credits: credits_in(b.credits),
            tracks: b.tracks,
        },
    )
    .await?;
    Ok(created("albums", d.album.id, d.into()))
}

#[utoipa::path(patch, path = "/api/v1/albums/{id}", tag = "catalogue", request_body = AlbumPatch,
    params(("id" = Uuid, Path)), security(("session" = [])), responses(
    (status = 200, body = AlbumDetail),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json"),
    (status = 404, body = Problem, content_type = "application/problem+json"),
    (status = 409, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn update_album(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireUser(p): RequireUser,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(b): ApiJson<AlbumPatch>,
) -> ApiResult<Json<AlbumDetail>> {
    let d = uc::update_album(
        &app,
        &p,
        &meta,
        id,
        b.expected_version,
        uc::AlbumInput {
            title: b.title,
            kind: b.kind,
            release_year: b.release_year,
            barcode: b.barcode,
            credits: credits_in(b.credits),
            tracks: b.tracks,
        },
    )
    .await?;
    Ok(Json(d.into()))
}

#[utoipa::path(get, path = "/api/v1/albums/{id}", tag = "catalogue", params(("id" = Uuid, Path), LangQuery), responses(
    (status = 200, body = AlbumDetail),
    (status = 301, description = "Merged into another record; follow `Location`"),
    (status = 404, body = Problem, content_type = "application/problem+json")))]
pub async fn get_album(
    State(app): State<App>,
    ApiPath(id): ApiPath<Uuid>,
    ApiQuery(l): ApiQuery<LangQuery>,
) -> ApiResult<axum::response::Response> {
    let found = uc::get_album(&app, id, l.lang.as_deref()).await.map(AlbumDetail::from);
    crate::entities::or_redirect(&app, "album", "albums", id, l.lang.as_deref(), found).await
}

#[utoipa::path(get, path = "/api/v1/albums", tag = "catalogue", params(PageQuery, AlbumFilter, LangQuery), responses(
    (status = 200, body = AlbumPage),
    (status = 400, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn list_albums(
    State(app): State<App>,
    ApiQuery(q): ApiQuery<PageQuery>,
    ApiQuery(f): ApiQuery<AlbumFilter>,
    ApiQuery(lq): ApiQuery<LangQuery>,
) -> ApiResult<Json<AlbumPage>> {
    let filter = rows::AlbumFilter {
        artist_id: f.artist_id,
        kind: f.kind,
        barcode: f.barcode,
        title_prefix: f.prefix,
    };
    let l = uc::list_albums(&app, filter, q.cursor.as_deref(), q.limit, lq.lang.as_deref()).await?;
    Ok(Json(AlbumPage {
        items: l.page.items.into_iter().map(|r| Album::of(r, &l.names)).collect(),
        next_cursor: l.page.next_cursor,
    }))
}
