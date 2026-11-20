//! The home page's fixed lists, and view counting.

use crate::browse::{BrowsePageRef, BrowseRef};
use crate::extract::{ApiJson, ApiPath, MaybeUser, Moderator, RequireRole};
use crate::localization::Display;
use crate::problem::{ApiResult, Problem};
use crate::search::cached;
use application::home as uc;
use application::App;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::Response;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

#[derive(Serialize, ToSchema)]
pub struct HomeItem {
    pub rank: i16,
    pub id: Uuid,
    pub title: Display,
    /// Public page of a song or artist: `/{locale}/song/{slug}` or `/{locale}/artist/{slug}`.
    pub page: Option<BrowsePageRef>,
    /// Browse slug of an album or artist (`/browse?album=<slug>`).
    pub slug: Option<String>,
    /// Public image (albums and artists): `/api/v1/media/{image_id}/{width}`.
    pub image_id: Option<Uuid>,
    /// Singers and albums of a song; each filters browse by that value.
    pub singers: Vec<BrowseRef>,
    pub albums: Vec<BrowseRef>,
    /// Views in the last 30 days.
    pub views: i64,
    pub rating_average: Option<f64>,
    pub rating_count: i32,
    pub comments: i32,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum HomeSectionKey {
    EditorsPicks,
    PopularSongs,
    NewSongs,
    PopularAlbums,
    NewAlbums,
    PopularArtists,
}

#[derive(Serialize, ToSchema)]
pub struct HomeSection {
    pub key: HomeSectionKey,
    /// Empty when there is nothing to show yet (for example on a new install).
    pub items: Vec<HomeItem>,
}

#[derive(Serialize, ToSchema)]
pub struct HomeResponse {
    /// When the lists were last computed; they do not change between runs.
    pub generated_at: Option<DateTime<Utc>>,
    /// Always all six sections, in the default order.
    pub sections: Vec<HomeSection>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct HomeQuery {
    /// Display language tag for names.
    pub lang: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/home", tag = "home", params(HomeQuery), responses(
    (status = 200, body = HomeResponse, description = "Popular and new songs and albums, and most viewed artists"),
    (status = 304, description = "Unchanged since `If-None-Match`.")))]
pub async fn home(State(app): State<App>, headers: HeaderMap, Query(q): Query<HomeQuery>) -> ApiResult<Response> {
    let h = uc::home(&app, q.lang.as_deref()).await?;
    let body = HomeResponse {
        generated_at: h.generated_at,
        sections: h
            .sections
            .into_iter()
            .map(|s| HomeSection {
                key: match s.key {
                    "editors_picks" => HomeSectionKey::EditorsPicks,
                    "popular_songs" => HomeSectionKey::PopularSongs,
                    "new_songs" => HomeSectionKey::NewSongs,
                    "popular_albums" => HomeSectionKey::PopularAlbums,
                    "new_albums" => HomeSectionKey::NewAlbums,
                    _ => HomeSectionKey::PopularArtists,
                },
                items: s
                    .items
                    .into_iter()
                    .map(|i| HomeItem {
                        rank: i.rank,
                        id: i.id,
                        title: i.title.into(),
                        page: i.page.map(|c| BrowsePageRef {
                            locale: c.locale,
                            slug: c.slug,
                        }),
                        slug: i.slug,
                        image_id: i.image,
                        singers: i.singers.into_iter().map(browse_ref).collect(),
                        albums: i.albums.into_iter().map(browse_ref).collect(),
                        views: i.views,
                        rating_average: i.rating_avg,
                        rating_count: i.rating_count,
                        comments: i.comments,
                    })
                    .collect(),
            })
            .collect(),
    };
    // The lists are fixed for about an hour, so a few minutes of caching loses nothing.
    Ok(cached(&headers, &body, 300))
}

#[derive(Deserialize, Serialize, ToSchema, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum ViewKind {
    Song,
    Artist,
    Album,
}

#[derive(Deserialize, ToSchema)]
pub struct ViewCreate {
    pub kind: ViewKind,
    /// Composition, artist or album id.
    pub id: Uuid,
}

#[derive(Serialize, ToSchema)]
pub struct ViewCount {
    /// All-time views of the page.
    pub views: i64,
}

#[utoipa::path(post, path = "/api/v1/views", tag = "home", request_body = ViewCreate, responses(
    (status = 200, body = ViewCount, description = "Counted once per viewer and day; returns the total"),
    (status = 404, body = Problem, content_type = "application/problem+json"),
    (status = 429, body = Problem, content_type = "application/problem+json")))]
pub async fn record_view(
    State(app): State<App>,
    MaybeUser(user): MaybeUser,
    headers: HeaderMap,
    ApiJson(b): ApiJson<ViewCreate>,
) -> ApiResult<Json<ViewCount>> {
    // Signed-in people count once per account; visitors once per network address and browser.
    let viewer = match &user {
        Some(p) => format!("u:{}", p.user_id),
        None => {
            let ua = headers.get("user-agent").and_then(|v| v.to_str().ok()).unwrap_or("");
            format!("c:{}|{}", crate::ratelimit::client_key(&headers), ua)
        }
    };
    let kind = match b.kind {
        ViewKind::Song => "song",
        ViewKind::Artist => "artist",
        ViewKind::Album => "album",
    };
    Ok(Json(ViewCount {
        views: uc::record_view(&app, &viewer, kind, b.id).await?,
    }))
}

fn browse_ref(x: application::browse::Ref) -> BrowseRef {
    BrowseRef {
        id: x.id,
        image_id: x.image,
        slug: x.slug,
        name: x.name.into(),
    }
}

#[derive(Serialize, ToSchema)]
pub struct PickState {
    pub picked: bool,
}

#[utoipa::path(get, path = "/api/v1/editors-picks/{id}", tag = "home", params(("id" = Uuid, Path)),
    responses((status = 200, body = PickState)))]
pub async fn pick_state(State(app): State<App>, ApiPath(id): ApiPath<Uuid>) -> ApiResult<Json<PickState>> {
    Ok(Json(PickState {
        picked: uc::is_picked(&app, id).await?,
    }))
}

#[utoipa::path(put, path = "/api/v1/editors-picks/{id}", tag = "home", params(("id" = Uuid, Path)), security(("session" = [])),
    responses((status = 200, body = PickState), (status = 403, body = Problem, content_type = "application/problem+json"),
    (status = 404, body = Problem, content_type = "application/problem+json")))]
pub async fn pick(
    State(app): State<App>,
    RequireRole(p, ..): RequireRole<Moderator>,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<Json<PickState>> {
    uc::pick(&app, &p, id).await?;
    Ok(Json(PickState { picked: true }))
}

#[utoipa::path(delete, path = "/api/v1/editors-picks/{id}", tag = "home", params(("id" = Uuid, Path)), security(("session" = [])),
    responses((status = 200, body = PickState), (status = 403, body = Problem, content_type = "application/problem+json")))]
pub async fn unpick(
    State(app): State<App>,
    RequireRole(p, ..): RequireRole<Moderator>,
    ApiPath(id): ApiPath<Uuid>,
) -> ApiResult<Json<PickState>> {
    uc::unpick(&app, &p, id).await?;
    Ok(Json(PickState { picked: false }))
}
