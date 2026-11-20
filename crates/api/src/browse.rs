//! Public song browsing: the list, the values of one kind of metadata, and typed suggestions.
//! Filters are `facet=slug` query pairs (repeat a key to require several of the same kind); different
//! filters combine with AND. Published content only, so every answer is cacheable by ETag.

use crate::localization::Display;
use crate::problem::{ApiResult, Problem};
use crate::search::cached;
use application::browse::{self as uc, FacetKind, Selection};
use application::App;
use axum::extract::{Path, RawQuery, State};
use axum::http::HeaderMap;
use axum::response::Response;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

/// Every kind of metadata songs can be browsed by. The same words are the filter parameter names.
#[derive(Serialize, Deserialize, ToSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BrowseFacet {
    Artist,
    Singer,
    Lyricist,
    Composer,
    Album,
    Raga,
    Deity,
    Religion,
    Category,
    Language,
    Tag,
}

impl From<FacetKind> for BrowseFacet {
    fn from(f: FacetKind) -> Self {
        match f {
            FacetKind::Artist => Self::Artist,
            FacetKind::Singer => Self::Singer,
            FacetKind::Lyricist => Self::Lyricist,
            FacetKind::Composer => Self::Composer,
            FacetKind::Album => Self::Album,
            FacetKind::Raga => Self::Raga,
            FacetKind::Deity => Self::Deity,
            FacetKind::Religion => Self::Religion,
            FacetKind::Category => Self::Category,
            FacetKind::Language => Self::Language,
            FacetKind::Tag => Self::Tag,
        }
    }
}

/// Documentation of the shared query parameters. Parsing is done from the raw query string so that
/// a filter can repeat; this struct only describes it.
#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[allow(dead_code)]
pub struct BrowseQuery {
    /// Page number, from 1 (default 1).
    pub page: Option<i64>,
    /// 1-50 (default 20).
    pub page_size: Option<i64>,
    /// Preferred display language, e.g. `ml`. Names in this language are shown when they exist.
    pub lang: Option<String>,
    /// Filters: each is the slug of an artist, singer, lyricist, composer or album, or the value of a raga,
    /// deity, religion, category, language (ISO 639-3) or tag. May repeat. All filters must match (AND).
    pub artist: Option<String>,
    pub singer: Option<String>,
    pub lyricist: Option<String>,
    pub composer: Option<String>,
    pub album: Option<String>,
    pub raga: Option<String>,
    pub deity: Option<String>,
    pub religion: Option<String>,
    pub category: Option<String>,
    pub language: Option<String>,
    pub tag: Option<String>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[allow(dead_code)]
pub struct SongsQuery {
    /// `newest` (default) or `title`.
    pub sort: Option<String>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[allow(dead_code)]
pub struct ValuesQuery {
    /// Narrow the list to names that start with, contain a word starting with, or nearly match this text.
    pub q: Option<String>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[allow(dead_code)]
pub struct SuggestQuery {
    /// At least 2 characters.
    pub q: String,
    /// 1-20 (default 10).
    pub limit: Option<i64>,
}

/// The query string, split into the pieces every browse endpoint shares.
struct Parsed {
    selection: Selection,
    page: Option<i64>,
    page_size: Option<i64>,
    lang: Option<String>,
    sort: Option<String>,
    q: Option<String>,
    limit: Option<i64>,
}

fn parse(raw: Option<String>) -> Result<Parsed, application::AppError> {
    let mut p = Parsed {
        selection: Selection::default(),
        page: None,
        page_size: None,
        lang: None,
        sort: None,
        q: None,
        limit: None,
    };
    let mut bad = domain::Violations::default();
    let number = |bad: &mut domain::Violations, field: &'static str, v: &str| match v.parse::<i64>() {
        Ok(n) => Some(n),
        Err(_) => {
            bad.push(field, "invalid_value");
            None
        }
    };
    for (k, v) in url::form_urlencoded::parse(raw.unwrap_or_default().as_bytes()) {
        match &*k {
            "page" => p.page = number(&mut bad, "page", &v),
            "page_size" => p.page_size = number(&mut bad, "page_size", &v),
            "limit" => p.limit = number(&mut bad, "limit", &v),
            "lang" => p.lang = Some(v.into_owned()),
            "sort" => p.sort = Some(v.into_owned()),
            "q" => p.q = Some(v.into_owned()),
            other => {
                if let Some(f) = FacetKind::parse(other) {
                    p.selection.0.push((f, v.into_owned()));
                }
            }
        }
    }
    bad.finish()?;
    Ok(p)
}

#[derive(Serialize, ToSchema)]
pub struct BrowseFilter {
    pub facet: BrowseFacet,
    /// The current slug; a client should rewrite its URL when this differs from what it sent.
    pub slug: String,
    pub label: Display,
    /// False when nothing has this slug: the list is then empty.
    pub resolved: bool,
}

impl From<uc::ResolvedFilter> for BrowseFilter {
    fn from(f: uc::ResolvedFilter) -> Self {
        Self {
            facet: f.facet.into(),
            slug: f.slug,
            label: f.label.into(),
            resolved: f.resolved,
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct BrowseRef {
    /// Slug to filter by (`singer=…` or `album=…`).
    pub slug: String,
    pub name: Display,
}

#[derive(Serialize, ToSchema)]
pub struct BrowseLanguage {
    /// ISO 639-1 where there is one, otherwise 639-3.
    pub code: String,
    pub name: String,
}

#[derive(Serialize, ToSchema)]
pub struct BrowsePageRef {
    pub locale: String,
    /// The song's public page is `/{locale}/song/{slug}`.
    pub slug: String,
}

#[derive(Serialize, ToSchema)]
pub struct BrowseSong {
    pub page: Option<BrowsePageRef>,
    pub title: Display,
    pub year: Option<i16>,
    pub singers: Vec<BrowseRef>,
    pub albums: Vec<BrowseRef>,
    pub original_language: Option<BrowseLanguage>,
    pub translations: Vec<BrowseLanguage>,
}

#[derive(Serialize, ToSchema)]
pub struct BrowseSongsResponse {
    pub items: Vec<BrowseSong>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
    pub total_pages: i64,
    /// The filters as understood, with current slugs and localized labels.
    pub filters: Vec<BrowseFilter>,
}

fn pages(total: i64, size: i64) -> i64 {
    (total + size - 1) / size
}

#[utoipa::path(get, path = "/api/v1/browse/songs", operation_id = "browse_songs", tag = "browse",
    params(BrowseQuery, SongsQuery), responses(
    (status = 200, body = BrowseSongsResponse, description = "Songs with a published lyric that match every filter."),
    (status = 304, description = "Unchanged since `If-None-Match`."),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn songs(State(app): State<App>, headers: HeaderMap, RawQuery(raw): RawQuery) -> ApiResult<Response> {
    let p = parse(raw)?;
    let r = uc::songs(
        &app,
        &p.selection,
        p.page,
        p.page_size,
        p.sort.as_deref(),
        p.lang.as_deref(),
    )
    .await?;
    let max_age = app.search.settings().await?.cache_max_age_secs;
    let body = BrowseSongsResponse {
        total_pages: pages(r.total, r.page_size),
        items: r
            .items
            .into_iter()
            .map(|s| BrowseSong {
                page: s.page.map(|c| BrowsePageRef {
                    locale: c.locale,
                    slug: c.slug,
                }),
                title: s.title.into(),
                year: s.year,
                singers: s
                    .singers
                    .into_iter()
                    .map(|x| BrowseRef {
                        slug: x.slug,
                        name: x.name.into(),
                    })
                    .collect(),
                albums: s
                    .albums
                    .into_iter()
                    .map(|x| BrowseRef {
                        slug: x.slug,
                        name: x.name.into(),
                    })
                    .collect(),
                original_language: s.original_language.map(|l| BrowseLanguage {
                    code: l.code,
                    name: l.name,
                }),
                translations: s
                    .translations
                    .into_iter()
                    .map(|l| BrowseLanguage {
                        code: l.code,
                        name: l.name,
                    })
                    .collect(),
            })
            .collect(),
        page: r.page,
        page_size: r.page_size,
        total: r.total,
        filters: r.filters.into_iter().map(Into::into).collect(),
    };
    Ok(cached(&headers, &body, max_age))
}

#[derive(Serialize, ToSchema)]
pub struct BrowseFacetValue {
    /// Use as the filter value for this facet.
    pub slug: String,
    pub label: Display,
    /// Songs that have it and match the other filters.
    pub songs: i64,
}

#[derive(Serialize, ToSchema)]
pub struct BrowseFacetResponse {
    pub facet: BrowseFacet,
    pub items: Vec<BrowseFacetValue>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
    pub total_pages: i64,
    pub filters: Vec<BrowseFilter>,
}

#[utoipa::path(get, path = "/api/v1/browse/facets/{facet}", operation_id = "browse_facet", tag = "browse",
    params(("facet" = BrowseFacet, Path, description = "Kind of metadata to list"), BrowseQuery, ValuesQuery), responses(
    (status = 200, body = BrowseFacetResponse, description = "The values of one kind of metadata, paged, A to Z."),
    (status = 304, description = "Unchanged since `If-None-Match`."),
    (status = 404, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn facet(
    State(app): State<App>,
    headers: HeaderMap,
    Path(name): Path<String>,
    RawQuery(raw): RawQuery,
) -> ApiResult<Response> {
    let kind = FacetKind::parse(&name).ok_or(application::AppError::NotFound { entity: "facet" })?;
    let p = parse(raw)?;
    let r = uc::values(
        &app,
        kind,
        p.q.as_deref(),
        &p.selection,
        p.page,
        p.page_size,
        p.lang.as_deref(),
    )
    .await?;
    let max_age = app.search.settings().await?.cache_max_age_secs;
    let body = BrowseFacetResponse {
        facet: kind.into(),
        total_pages: pages(r.total, r.page_size),
        items: r
            .items
            .into_iter()
            .map(|v| BrowseFacetValue {
                slug: v.slug,
                label: v.label.into(),
                songs: v.songs,
            })
            .collect(),
        page: r.page,
        page_size: r.page_size,
        total: r.total,
        filters: r.filters.into_iter().map(Into::into).collect(),
    };
    Ok(cached(&headers, &body, max_age))
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum BrowseSuggestionKind {
    Song,
    Artist,
    Singer,
    Lyricist,
    Composer,
    Album,
    Raga,
    Deity,
    Religion,
    Category,
    Language,
    Tag,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum BrowseVia {
    Name,
    Localization,
    Transliteration,
    Alias,
}

#[derive(Serialize, ToSchema)]
pub struct BrowseSuggestion {
    /// What it is. `song` opens the song; every other kind is a filter to add.
    pub kind: BrowseSuggestionKind,
    /// A song's page slug (with `locale`), or the filter value for the other kinds.
    pub slug: String,
    pub locale: Option<String>,
    pub label: Display,
    /// The name that matched, as written (an alias or another script may differ from `label`).
    pub matched: String,
    pub via: BrowseVia,
}

#[derive(Serialize, ToSchema)]
pub struct BrowseSuggestResponse {
    /// Best first; already restricted to what exists together with the active filters.
    pub items: Vec<BrowseSuggestion>,
}

#[utoipa::path(get, path = "/api/v1/browse/suggest", operation_id = "browse_suggest", tag = "browse",
    params(BrowseQuery, SuggestQuery), responses(
    (status = 200, body = BrowseSuggestResponse, description = "Rate limited per client; cacheable for a short time."),
    (status = 304, description = "Unchanged since `If-None-Match`."),
    (status = 422, body = Problem, content_type = "application/problem+json"),
    (status = 429, body = Problem, content_type = "application/problem+json", description = "Slow down; see `Retry-After`.")))]
pub async fn suggest(State(app): State<App>, headers: HeaderMap, RawQuery(raw): RawQuery) -> ApiResult<Response> {
    let p = parse(raw)?;
    let (items, max_age) = uc::suggest(
        &app,
        p.q.as_deref().unwrap_or(""),
        &p.selection,
        p.lang.as_deref(),
        p.limit,
    )
    .await?;
    let body = BrowseSuggestResponse {
        items: items
            .into_iter()
            .map(|s| BrowseSuggestion {
                kind: match s.kind {
                    "song" => BrowseSuggestionKind::Song,
                    "artist" => BrowseSuggestionKind::Artist,
                    "singer" => BrowseSuggestionKind::Singer,
                    "lyricist" => BrowseSuggestionKind::Lyricist,
                    "composer" => BrowseSuggestionKind::Composer,
                    "album" => BrowseSuggestionKind::Album,
                    "raga" => BrowseSuggestionKind::Raga,
                    "deity" => BrowseSuggestionKind::Deity,
                    "religion" => BrowseSuggestionKind::Religion,
                    "category" => BrowseSuggestionKind::Category,
                    "language" => BrowseSuggestionKind::Language,
                    _ => BrowseSuggestionKind::Tag,
                },
                slug: s.slug,
                locale: s.locale,
                label: s.label.into(),
                matched: s.matched,
                via: match s.via {
                    uc::Via::Name => BrowseVia::Name,
                    uc::Via::Localization => BrowseVia::Localization,
                    uc::Via::Transliteration => BrowseVia::Transliteration,
                    uc::Via::Alias => BrowseVia::Alias,
                },
            })
            .collect(),
    };
    Ok(cached(&headers, &body, max_age))
}
