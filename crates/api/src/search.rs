//! Public search. Responses describe results (what matched, how) and never the engine behind them.
//! Public, published-only data: cacheable by browsers and CDNs, revalidated by ETag.

use crate::extract::{Admin, ApiQuery, RequireRole};
use crate::localization::Display;
use crate::problem::{ApiResult, Problem};
use application::search::{self as uc, Item};
use application::App;
use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SearchQuery {
    /// What to look for. Case, spacing, punctuation and diacritics are ignored, and small typos are tolerated.
    #[serde(default)]
    pub q: String,
    /// `compositions`, `recordings`, `artists`, `albums` or `contributors`. Required to use `cursor`.
    #[serde(rename = "type")]
    #[param(rename = "type")]
    pub kind: Option<String>,
    /// Preferred display language, e.g. `ml`, `ar-EG`. Names in this language rank slightly higher.
    pub lang: Option<String>,
    /// Hits per group, 1-50 (default 10).
    pub limit: Option<i64>,
    /// From a group's `next_cursor`; requires `type`.
    pub cursor: Option<String>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SuggestQuery {
    /// At least 2 characters.
    #[serde(default)]
    pub q: String,
    pub lang: Option<String>,
    /// 1-20 (default 8).
    pub limit: Option<i64>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MatchKind {
    Exact,
    Normalized,
    Prefix,
    Fuzzy,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MatchVia {
    Name,
    Localization,
    Transliteration,
    Alias,
    Metadata,
    /// The words of a published lyric; `text` is the line that holds them.
    Lyrics,
}

#[derive(Serialize, ToSchema)]
pub struct Matched {
    /// The name that matched, as written. May differ from `display` (an alias or another script).
    pub text: String,
    pub kind: MatchKind,
    pub via: MatchVia,
}

#[derive(Serialize, ToSchema)]
pub struct ParentRef {
    pub id: Uuid,
    pub title: String,
}

#[derive(Serialize, ToSchema)]
pub struct SearchItem {
    /// `composition`, `recording`, `artist` or `album`.
    #[serde(rename = "type")]
    pub kind: String,
    pub id: Uuid,
    pub display: Display,
    #[serde(rename = "match")]
    pub matched: Matched,
    /// Artist kind or album kind.
    pub sub_kind: Option<String>,
    /// Artist disambiguation or composition ISWC.
    pub detail: Option<String>,
    pub year: Option<i16>,
    /// A recording's composition.
    pub parent: Option<ParentRef>,
}

impl From<Item> for SearchItem {
    fn from(i: Item) -> Self {
        Self {
            kind: i.entity.as_str().into(),
            id: i.id,
            display: i.display.into(),
            matched: Matched {
                text: i.matched.text,
                kind: match i.matched.kind {
                    uc::MatchKind::Exact => MatchKind::Exact,
                    uc::MatchKind::Normalized => MatchKind::Normalized,
                    uc::MatchKind::Prefix => MatchKind::Prefix,
                    uc::MatchKind::Fuzzy => MatchKind::Fuzzy,
                },
                via: match i.matched.via {
                    uc::MatchVia::Name => MatchVia::Name,
                    uc::MatchVia::Localization => MatchVia::Localization,
                    uc::MatchVia::Transliteration => MatchVia::Transliteration,
                    uc::MatchVia::Alias => MatchVia::Alias,
                    uc::MatchVia::Metadata => MatchVia::Metadata,
                    uc::MatchVia::Lyrics => MatchVia::Lyrics,
                },
            },
            sub_kind: i.sub_kind,
            detail: i.detail,
            year: i.year,
            parent: i.parent.map(|p| ParentRef {
                id: p.id,
                title: p.title,
            }),
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct SearchGroup {
    /// `compositions`, `recordings`, `artists`, `albums` or `contributors`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Best first.
    pub items: Vec<SearchItem>,
    /// Pass with `type` to get the next page of this group.
    pub next_cursor: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct SearchResponse {
    /// Always every requested group, in a fixed order; a group with no hits is empty.
    pub groups: Vec<SearchGroup>,
}

#[derive(Serialize, ToSchema)]
pub struct SuggestResponse {
    /// Mixed types, best first.
    pub items: Vec<SearchItem>,
}

/// JSON with a weak ETag, short public caching and a 304 for an unchanged answer.
pub(crate) fn cached<T: Serialize>(req: &HeaderMap, body: &T, max_age: u32) -> Response {
    let bytes = serde_json::to_vec(body).expect("search response serializes");
    let digest = Sha256::digest(&bytes);
    let etag = format!(
        "W/\"{}\"",
        digest.iter().take(12).map(|b| format!("{b:02x}")).collect::<String>()
    );
    let cache = format!("public, max-age={max_age}, stale-while-revalidate={}", max_age * 2);
    let mut res = if req.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) == Some(etag.as_str()) {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        ([(header::CONTENT_TYPE, "application/json")], bytes).into_response()
    };
    let h = res.headers_mut();
    h.insert(header::ETAG, HeaderValue::from_str(&etag).expect("hex etag"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_str(&cache).expect("ascii"));
    res
}

#[utoipa::path(get, path = "/api/v1/search", tag = "search", params(SearchQuery), responses(
    (status = 200, body = SearchResponse, description = "Published content only. Cacheable for a short time; revalidate with `ETag`."),
    (status = 304, description = "Unchanged since `If-None-Match`."),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn search(
    State(app): State<App>,
    headers: HeaderMap,
    ApiQuery(q): ApiQuery<SearchQuery>,
) -> ApiResult<Response> {
    let r = uc::search(
        &app,
        &q.q,
        q.kind.as_deref(),
        q.lang.as_deref(),
        q.limit,
        q.cursor.as_deref(),
    )
    .await?;
    let body = SearchResponse {
        groups: r
            .groups
            .into_iter()
            .map(|g| SearchGroup {
                kind: g.group.as_str().into(),
                items: g.items.into_iter().map(Into::into).collect(),
                next_cursor: g.next_cursor,
            })
            .collect(),
    };
    Ok(cached(&headers, &body, r.max_age_secs))
}

#[utoipa::path(get, path = "/api/v1/search/suggest", operation_id = "search_suggest", tag = "search", params(SuggestQuery), responses(
    (status = 200, body = SuggestResponse, description = "Rate limited per client; cacheable for a short time."),
    (status = 304, description = "Unchanged since `If-None-Match`."),
    (status = 422, body = Problem, content_type = "application/problem+json"),
    (status = 429, body = Problem, content_type = "application/problem+json", description = "Slow down; see `Retry-After`.")))]
pub async fn suggest(
    State(app): State<App>,
    headers: HeaderMap,
    ApiQuery(q): ApiQuery<SuggestQuery>,
) -> ApiResult<Response> {
    let (items, max_age) = uc::suggest(&app, &q.q, q.lang.as_deref(), q.limit).await?;
    let body = SuggestResponse {
        items: items.into_iter().map(Into::into).collect(),
    };
    Ok(cached(&headers, &body, max_age))
}

#[utoipa::path(post, path = "/api/v1/search/suggest/accepted", operation_id = "search_suggestion_accepted", tag = "search", responses(
    (status = 204, description = "Counted. Carries no query, user or address.")))]
pub async fn suggestion_accepted(State(app): State<App>) -> StatusCode {
    uc::suggestion_accepted(&app);
    StatusCode::NO_CONTENT
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct MetricsQuery {
    /// 1-90 (default 7).
    pub days: Option<i32>,
}

#[derive(Serialize, ToSchema)]
pub struct MetricsDay {
    pub day: chrono::NaiveDate,
    /// `search` or `suggest`.
    pub endpoint: String,
    pub requests: i64,
    pub empty_results: i64,
    pub empty_rate: f64,
    pub avg_latency_ms: f64,
    /// Suggestions the client reported as chosen (suggest only).
    pub accepted: i64,
    pub acceptance_rate: f64,
}

#[derive(Serialize, ToSchema)]
pub struct MetricsResponse {
    pub items: Vec<MetricsDay>,
}

#[utoipa::path(get, path = "/api/v1/admin/search/metrics", operation_id = "search_metrics", tag = "search", params(MetricsQuery), responses(
    (status = 200, body = MetricsResponse), (status = 401, body = Problem), (status = 403, body = Problem)))]
pub async fn metrics(
    State(app): State<App>,
    RequireRole(p, ..): RequireRole<Admin>,
    ApiQuery(q): ApiQuery<MetricsQuery>,
) -> ApiResult<Response> {
    let rows = uc::metrics(&app, &p, q.days).await?;
    let rate = |n: i64, d: i64| if d > 0 { n as f64 / d as f64 } else { 0.0 };
    let body = MetricsResponse {
        items: rows
            .into_iter()
            .map(|m| MetricsDay {
                day: m.day,
                empty_rate: rate(m.empty, m.requests),
                acceptance_rate: rate(m.accepted, m.requests),
                endpoint: m.endpoint,
                requests: m.requests,
                empty_results: m.empty,
                avg_latency_ms: m.avg_latency_ms,
                accepted: m.accepted,
            })
            .collect(),
    };
    let mut res = Json(body).into_response();
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    Ok(res)
}
