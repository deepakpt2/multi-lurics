//! Public pages and the sitemap feed behind the web's `/{locale}/song|artist|recording/{slug}`
//! routes. Public, published-only, cacheable; unknown or unpublished content is a plain 404
//! that is never cached.

use crate::extract::{ApiPath, ApiQuery};
use crate::localization::{Dir, Display};
use crate::problem::{ApiError, ApiResult, Problem};
use application::seo::{self as uc, Canonical, Resolved};
use application::{App, AppError};
use axum::extract::State;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

/// A page address: UI locale plus slug.
#[derive(Serialize, ToSchema)]
pub struct PageRef {
    pub locale: String,
    pub slug: String,
}

impl From<Canonical> for PageRef {
    fn from(c: Canonical) -> Self {
        Self {
            locale: c.locale,
            slug: c.slug,
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct PageLink {
    pub id: Uuid,
    pub label: Display,
    /// `song`, `recording` or `artist`.
    pub scope: String,
    /// Where it lives; absent when it has no public page.
    pub page: Option<PageRef>,
}

impl From<uc::Link> for PageLink {
    fn from(l: uc::Link) -> Self {
        Self {
            id: l.id,
            label: l.label.into(),
            scope: l.scope.into(),
            page: l.target.map(Into::into),
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct PageCredit {
    pub artist: PageLink,
    pub role: String,
}

#[derive(Serialize, ToSchema)]
pub struct PageLyric {
    pub id: Uuid,
    pub recording_id: Uuid,
    /// `original`, `translation` or `transliteration`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Language tag (`ml`, `ar`) and script (`Mlym`, `Arab`): separate on purpose.
    pub language: String,
    pub script: String,
    pub dir: Dir,
    pub is_primary: bool,
    pub body: String,
    pub revised_at: DateTime<Utc>,
}

#[derive(Serialize, ToSchema)]
pub struct PublicPage {
    /// `song`, `recording` or `artist`.
    pub scope: String,
    pub id: Uuid,
    pub locale: String,
    /// The one URL search engines should index.
    pub canonical: PageRef,
    /// Locales that really have content for this page, each with its own slug (includes `canonical`).
    pub alternates: Vec<PageRef>,
    pub x_default: PageRef,
    pub title: Display,
    pub year: Option<i16>,
    /// ISWC (song), ISRC (recording) or ISNI (artist).
    pub code: Option<String>,
    /// Artist kind.
    pub kind: Option<String>,
    /// Artist disambiguation.
    pub detail: Option<String>,
    pub duration_ms: Option<i32>,
    /// Song only: `mainstream` or `devotional`.
    pub category: Option<String>,
    /// Song only.
    pub raga: Option<String>,
    /// Song only, devotional songs.
    pub religion: Option<String>,
    /// Song only, devotional songs.
    pub deity: Option<String>,
    /// Song only; empty otherwise.
    pub tags: Vec<String>,
    /// A recording's song.
    pub parent: Option<PageLink>,
    pub credits: Vec<PageCredit>,
    /// An artist's songs.
    pub songs: Vec<PageLink>,
    /// A song's or an artist's recordings.
    pub recordings: Vec<PageLink>,
    /// Published lyrics only.
    pub lyrics: Vec<PageLyric>,
    pub updated_at: DateTime<Utc>,
}

impl From<uc::Page> for PublicPage {
    fn from(p: uc::Page) -> Self {
        Self {
            scope: p.scope.into(),
            id: p.id,
            locale: p.locale,
            canonical: p.canonical.into(),
            alternates: p.alternates.into_iter().map(Into::into).collect(),
            x_default: p.x_default.into(),
            title: p.title.into(),
            year: p.year,
            code: p.code,
            kind: p.kind,
            detail: p.detail,
            duration_ms: p.duration_ms,
            category: p.category,
            raga: p.raga,
            religion: p.religion,
            deity: p.deity,
            tags: p.tags,
            parent: p.parent.map(Into::into),
            credits: p
                .credits
                .into_iter()
                .map(|c| PageCredit {
                    artist: c.artist.into(),
                    role: c.role,
                })
                .collect(),
            songs: p.songs.into_iter().map(Into::into).collect(),
            recordings: p.recordings.into_iter().map(Into::into).collect(),
            lyrics: p
                .lyrics
                .into_iter()
                .map(|l| PageLyric {
                    id: l.id,
                    recording_id: l.recording_id,
                    kind: l.kind,
                    language: l.language,
                    script: l.script,
                    dir: if l.rtl { Dir::Rtl } else { Dir::Ltr },
                    is_primary: l.is_primary,
                    body: l.body,
                    revised_at: l.revised_at,
                })
                .collect(),
            updated_at: p.updated_at,
        }
    }
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PageQuery {
    /// UI locale of the page (`en`, `ar`).
    #[serde(default)]
    pub locale: String,
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

#[utoipa::path(get, path = "/api/v1/pages/{scope}/{slug}", operation_id = "get_public_page", tag = "seo",
    params(("scope" = String, Path, description = "`song`, `recording` or `artist`"), ("slug" = String, Path, description = "Unicode slug, percent-encoded"), PageQuery),
    responses(
        (status = 200, body = PublicPage, description = "Published content only."),
        (status = 301, body = PageRef, description = "Not the canonical address (an old slug, another spelling, or a locale without content). Body and `Location` name the canonical one."),
        (status = 404, body = Problem, content_type = "application/problem+json", description = "Unknown, or not published. Never cached.")))]
pub async fn get_page(
    State(app): State<App>,
    ApiPath((scope, slug)): ApiPath<(String, String)>,
    ApiQuery(q): ApiQuery<PageQuery>,
) -> ApiResult<Response> {
    let public = |mut res: Response, value: &'static str| {
        res.headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static(value));
        res
    };
    Ok(match uc::page(&app, &scope, &q.locale, &slug).await? {
        Some(Resolved::Page(p)) => public(
            Json(PublicPage::from(*p)).into_response(),
            "public, max-age=60, stale-while-revalidate=300",
        ),
        Some(Resolved::Moved(c)) => {
            let location = format!("/api/v1/pages/{scope}/{}?locale={}", encode(&c.slug), c.locale);
            let mut res = (StatusCode::MOVED_PERMANENTLY, Json(PageRef::from(c))).into_response();
            res.headers_mut()
                .insert(header::LOCATION, HeaderValue::from_str(&location).expect("encoded"));
            public(res, "public, max-age=300")
        }
        None => {
            let mut res = ApiError::from(AppError::NotFound { entity: "page" }).into_response();
            res.headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            res
        }
    })
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SitemapQuery {
    /// Zero-based page of `page_size` entries.
    pub page: Option<i64>,
}

#[derive(Serialize, ToSchema)]
pub struct SitemapItem {
    /// `song`, `recording` or `artist`.
    pub scope: String,
    pub id: Uuid,
    pub lastmod: DateTime<Utc>,
    pub canonical: PageRef,
    /// Locales with content, including the canonical one.
    pub alternates: Vec<PageRef>,
}

#[derive(Serialize, ToSchema)]
pub struct SitemapResponse {
    pub total: i64,
    pub page_size: i64,
    pub pages: i64,
    pub items: Vec<SitemapItem>,
}

#[utoipa::path(get, path = "/api/v1/sitemap", operation_id = "get_sitemap", tag = "seo", params(SitemapQuery), responses(
    (status = 200, body = SitemapResponse, description = "Published content only, in a stable order."),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn sitemap(State(app): State<App>, ApiQuery(q): ApiQuery<SitemapQuery>) -> ApiResult<Response> {
    let s = uc::sitemap(&app, q.page.unwrap_or(0)).await?;
    let body = SitemapResponse {
        total: s.total,
        page_size: s.page_size,
        pages: s.pages,
        items: s
            .items
            .into_iter()
            .map(|e| SitemapItem {
                scope: e.scope.into(),
                id: e.id,
                lastmod: e.lastmod,
                canonical: e.canonical.into(),
                alternates: e.alternates.into_iter().map(Into::into).collect(),
            })
            .collect(),
    };
    let mut res = Json(body).into_response();
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("public, max-age=300"));
    Ok(res)
}
