//! HTTP adapter: routing, extractors, problem+json, OpenAPI. No business rules here.

pub mod catalogue;
pub mod extract;
pub mod localization;
pub mod lyrics;
pub mod middleware;
pub mod problem;
pub mod routes;

use application::App;
use axum::routing::{get, post};
use axum::{Json, Router};
use catalogue as cat;
use localization as loc;
use lyrics as lyr;
use tower_http::trace::TraceLayer;
use utoipa::openapi::security::{ApiKey, ApiKeyValue, SecurityScheme};
use utoipa::{Modify, OpenApi};

#[derive(OpenApi)]
#[openapi(
    info(title = "Community Lyrics API", version = "1.0.0"),
    paths(
        routes::healthz, routes::readyz,
        routes::register, routes::verify_email, routes::login, routes::logout,
        routes::request_password_reset, routes::confirm_password_reset,
        routes::me, routes::list_audit_events,
        catalogue::list_artists, catalogue::create_artist, catalogue::get_artist, catalogue::update_artist,
        catalogue::list_compositions, catalogue::create_composition, catalogue::get_composition,
        catalogue::update_composition,
        catalogue::list_recordings, catalogue::create_recording, catalogue::get_recording, catalogue::update_recording,
        catalogue::list_albums, catalogue::create_album, catalogue::get_album, catalogue::update_album,
        loc::list_languages, loc::list_scripts,
        loc::create_localization, loc::list_entity_localizations, loc::get_localization, loc::update_localization,
        loc::verify_localization, loc::reject_localization, loc::list_revisions, loc::moderation_queue,
        loc::create_alias, loc::list_aliases, loc::verify_alias, loc::reject_alias,
        loc::set_default_language, loc::list_taxonomy, loc::create_taxonomy, loc::get_taxonomy,
        lyr::create_contribution, lyr::list_contributions, lyr::get_contribution, lyr::create_revision,
        lyr::list_revisions, lyr::get_revision, lyr::diff_revisions, lyr::approve_contribution,
        lyr::publish_contribution, lyr::reject_contribution, lyr::contribution_queue, lyr::my_contributions,
        lyr::get_trust_policy, lyr::set_trust_policy,
    ),
    modifiers(&SessionCookie),
)]
pub struct ApiDoc;

struct SessionCookie;
impl Modify for SessionCookie {
    fn modify(&self, api: &mut utoipa::openapi::OpenApi) {
        api.components.get_or_insert_with(Default::default).add_security_scheme(
            "session",
            SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::new(extract::SESSION_COOKIE))),
        );
    }
}

pub fn openapi_json() -> String {
    ApiDoc::openapi().to_pretty_json().expect("openapi serializes")
}

pub fn router(app: App) -> Router {
    // Lyric responses depend on who is asking: never stored by a shared cache or CDN.
    let lyrics = Router::new()
        .route(
            "/recordings/{id}/contributions",
            get(lyr::list_contributions).post(lyr::create_contribution),
        )
        .route("/contributions/{id}", get(lyr::get_contribution))
        .route(
            "/contributions/{id}/revisions",
            get(lyr::list_revisions).post(lyr::create_revision),
        )
        .route("/contributions/{id}/approve", post(lyr::approve_contribution))
        .route("/contributions/{id}/publish", post(lyr::publish_contribution))
        .route("/contributions/{id}/reject", post(lyr::reject_contribution))
        .route("/revisions/{id}", get(lyr::get_revision))
        .route("/revisions/{a}/diff/{b}", get(lyr::diff_revisions))
        .route("/moderation/contributions", get(lyr::contribution_queue))
        .route("/me/contributions", get(lyr::my_contributions))
        .route(
            "/admin/trust-policy",
            get(lyr::get_trust_policy).put(lyr::set_trust_policy),
        )
        .layer(axum::middleware::from_fn(middleware::no_store));

    let v1 = Router::new()
        .route("/auth/register", post(routes::register))
        .route("/auth/verify-email", post(routes::verify_email))
        .route("/auth/login", post(routes::login))
        .route("/auth/logout", post(routes::logout))
        .route("/auth/password-reset", post(routes::request_password_reset))
        .route("/auth/password-reset/confirm", post(routes::confirm_password_reset))
        .route("/me", get(routes::me))
        .route("/admin/audit-events", get(routes::list_audit_events))
        .route("/artists", get(cat::list_artists).post(cat::create_artist))
        .route("/artists/{id}", get(cat::get_artist).patch(cat::update_artist))
        .route(
            "/compositions",
            get(cat::list_compositions).post(cat::create_composition),
        )
        .route(
            "/compositions/{id}",
            get(cat::get_composition).patch(cat::update_composition),
        )
        .route("/recordings", get(cat::list_recordings).post(cat::create_recording))
        .route("/recordings/{id}", get(cat::get_recording).patch(cat::update_recording))
        .route("/albums", get(cat::list_albums).post(cat::create_album))
        .route("/albums/{id}", get(cat::get_album).patch(cat::update_album))
        .route("/languages", get(loc::list_languages))
        .route("/scripts", get(loc::list_scripts))
        .route(
            "/entities/{entity_type}/{entity_id}/localizations",
            get(loc::list_entity_localizations).post(loc::create_localization),
        )
        .route(
            "/entities/{entity_type}/{entity_id}/aliases",
            get(loc::list_aliases).post(loc::create_alias),
        )
        .route(
            "/entities/{entity_type}/{entity_id}/default-language",
            axum::routing::put(loc::set_default_language),
        )
        .route(
            "/localizations/{id}",
            get(loc::get_localization).patch(loc::update_localization),
        )
        .route("/localizations/{id}/verify", post(loc::verify_localization))
        .route("/localizations/{id}/reject", post(loc::reject_localization))
        .route("/localizations/{id}/revisions", get(loc::list_revisions))
        .route("/aliases/{id}/verify", post(loc::verify_alias))
        .route("/aliases/{id}/reject", post(loc::reject_alias))
        .route("/moderation/localizations", get(loc::moderation_queue))
        .route("/taxonomy/{kind}", get(loc::list_taxonomy).post(loc::create_taxonomy))
        .route("/taxonomy/{kind}/{id}", get(loc::get_taxonomy))
        .route("/openapi.json", get(|| async { Json(ApiDoc::openapi()) }))
        .merge(lyrics);

    Router::new()
        .route("/healthz", get(routes::healthz))
        .route("/readyz", get(routes::readyz))
        .nest("/api/v1", v1)
        .fallback(routes::not_found)
        .method_not_allowed_fallback(routes::method_not_allowed)
        .layer(axum::middleware::from_fn(middleware::csrf))
        .layer(
            TraceLayer::new_for_http().make_span_with(|req: &axum::http::Request<_>| {
                let rid = req
                    .headers()
                    .get(&middleware::REQUEST_ID)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                // Path only: query strings are not logged.
                tracing::info_span!("http", method = %req.method(), path = %req.uri().path(), request_id = %rid)
            }),
        )
        .layer(axum::middleware::from_fn(middleware::request_id))
        .with_state(app)
}
