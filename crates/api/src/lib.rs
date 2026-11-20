//! HTTP adapter: routing, extractors, problem+json, OpenAPI. No business rules here.

pub mod articles;
pub mod browse;
pub mod captcha;
pub mod corrections;
pub mod catalogue;
pub mod community;
pub mod contribute;
pub mod entities;
pub mod extract;
pub mod home;
pub mod images;
pub mod import;
pub mod legal;
pub mod localization;
pub mod lyrics;
pub mod metadata;
pub mod metrics;
pub mod middleware;
pub mod oauth;
pub mod problem;
pub mod ratelimit;
pub mod routes;
pub mod search;
pub mod seo;

use application::App;
use articles as art;
use axum::routing::{get, post};
use axum::{Json, Router};
use browse as brw;
use captcha as cpt;
use catalogue as cat;
use community as com;
use contribute as ctb;
use entities as ent;
use home as hm;
use images as img;
use import as imp;
use localization as loc;
use lyrics as lyr;
use metadata as mdt;
use search as srch;
use seo as seo_api;
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
        oauth::providers, oauth::google_start, oauth::google_callback,
        routes::me, routes::list_audit_events,
        routes::list_users, routes::grant_user_role, routes::revoke_user_role, routes::disable_user, routes::enable_user,
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
        corrections::suggest, corrections::queue, corrections::apply, corrections::reject,
        ent::suggest, ent::create_proposal, ent::proposal_queue, ent::approve_proposal, ent::reject_proposal,
        ent::merge_entities,
        brw::songs, brw::facet, brw::suggest,
        hm::home, hm::record_view, hm::pick_state, hm::pick, hm::unpick,
        imp::import_songs, ctb::contribute_song, ctb::add_song_lyrics, mdt::create_metadata,
        cpt::config, art::list, art::home, art::read, art::create, art::update, art::set_home, art::delete, art::images, art::upload_image, art::delete_image,
        img::limits, img::upload, img::withdraw, img::entity_images, img::media, img::queue, img::approve, img::reject,
        srch::search, srch::suggest, srch::suggestion_accepted, srch::metrics,
        seo_api::get_page, seo_api::sitemap,
        com::resolve, com::save_comparison, com::list_saved, com::get_saved, com::delete_saved,
        com::engagement, com::put_rating, com::delete_rating, com::put_reaction, com::delete_reaction,
        com::put_bookmark, com::delete_bookmark, com::my_bookmarks,
        com::list_comments, com::my_comments, com::create_comment, com::edit_comment, com::remove_comment,
        com::get_profile, com::put_profile, com::my_reputation,
        com::create_report, com::report_queue, com::resolve_report, com::hide_comment,
        com::create_note, com::list_notes,
        com::create_appeal, com::my_appeals, com::appeal_queue, com::decide_appeal,
        com::get_settings, com::put_settings,
        legal::file_legal_notice, legal::legal_notice_queue, legal::decide_legal_notice, legal::file_takedown_appeal,
        legal::takedown_appeal_queue, legal::decide_takedown_appeal,
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
        .route("/contributions/{id}/corrections", post(corrections::suggest))
        .route("/moderation/corrections", get(corrections::queue))
        .route("/moderation/corrections/{id}/apply", post(corrections::apply))
        .route("/moderation/corrections/{id}/reject", post(corrections::reject))
        .route("/moderation/contributions", get(lyr::contribution_queue))
        .route("/me/contributions", get(lyr::my_contributions))
        .route(
            "/admin/trust-policy",
            get(lyr::get_trust_policy).put(lyr::set_trust_policy),
        )
        .layer(axum::middleware::from_fn(middleware::no_store));

    // Comparison and community: caller-specific or must reflect unpublishing at once.
    let community = Router::new()
        .route("/comparisons/resolve", get(com::resolve))
        .route("/me/comparisons", get(com::list_saved).post(com::save_comparison))
        .route("/me/comparisons/{id}", get(com::get_saved).delete(com::delete_saved))
        .route("/contributions/{id}/engagement", get(com::engagement))
        .route(
            "/contributions/{id}/rating",
            axum::routing::put(com::put_rating).delete(com::delete_rating),
        )
        .route(
            "/contributions/{id}/reactions/{kind}",
            axum::routing::put(com::put_reaction).delete(com::delete_reaction),
        )
        .route(
            "/contributions/{id}/bookmark",
            axum::routing::put(com::put_bookmark).delete(com::delete_bookmark),
        )
        .route("/me/bookmarks", get(com::my_bookmarks))
        .route(
            "/contributions/{id}/comments",
            get(com::list_comments).post(com::create_comment),
        )
        .route("/me/comments", get(com::my_comments))
        .route(
            "/comments/{id}",
            axum::routing::patch(com::edit_comment).delete(com::remove_comment),
        )
        .route("/users/{id}/profile", get(com::get_profile))
        .route("/me/profile", axum::routing::put(com::put_profile))
        .route("/me/reputation", get(com::my_reputation))
        .route("/reports", post(com::create_report))
        .route("/moderation/reports", get(com::report_queue))
        .route("/moderation/reports/{id}/resolve", post(com::resolve_report))
        .route("/moderation/comments/{id}/hide", post(com::hide_comment))
        .route("/moderation/notes", get(com::list_notes).post(com::create_note))
        .route("/appeals", post(com::create_appeal))
        .route("/me/appeals", get(com::my_appeals))
        .route("/moderation/appeals", get(com::appeal_queue))
        .route("/moderation/appeals/{id}/decide", post(com::decide_appeal))
        .route(
            "/admin/community-settings",
            get(com::get_settings).put(com::put_settings),
        )
        .layer(axum::middleware::from_fn(middleware::no_store));

    // Legal: filing is public but throttled per client; staff and appeal steps are caller-specific.
    let legal_limiter = std::sync::Arc::new(ratelimit::RateLimiter::new(app.config.auth_rate_per_min));
    let legal = Router::new()
        .route("/legal/notices", post(legal::file_legal_notice))
        .route_layer(axum::middleware::from_fn_with_state(legal_limiter, ratelimit::limit))
        .route("/legal/notices/{id}/appeal", post(legal::file_takedown_appeal))
        .route("/moderation/legal/notices", get(legal::legal_notice_queue))
        .route(
            "/moderation/legal/notices/{id}/decide",
            post(legal::decide_legal_notice),
        )
        .route("/admin/legal/appeals", get(legal::takedown_appeal_queue))
        .route("/admin/legal/appeals/{id}/decide", post(legal::decide_takedown_appeal))
        .layer(axum::middleware::from_fn(middleware::no_store));

    // Typeahead is public and cheap to abuse: a per-client bucket sits in front of it.
    let limiter = std::sync::Arc::new(ratelimit::RateLimiter::new(app.config.search_rate_per_min));
    let typeahead = Router::new()
        .route("/search/suggest", get(srch::suggest))
        .route("/search/suggest/accepted", post(srch::suggestion_accepted))
        .route("/browse/suggest", get(brw::suggest))
        .route("/views", post(hm::record_view))
        .route_layer(axum::middleware::from_fn_with_state(limiter, ratelimit::limit));

    // Credential endpoints get their own per-client bucket (guessing, enumeration, mail flooding).
    let auth_limiter = std::sync::Arc::new(ratelimit::RateLimiter::new(app.config.auth_rate_per_min));
    let auth = Router::new()
        .route("/auth/register", post(routes::register))
        .route("/auth/verify-email", post(routes::verify_email))
        .route("/auth/login", post(routes::login))
        .route("/auth/providers", get(oauth::providers))
        .route("/captcha", get(cpt::config))
        .route("/auth/google/start", get(oauth::google_start))
        .route("/auth/google/callback", get(oauth::google_callback))
        .route("/auth/password-reset", post(routes::request_password_reset))
        .route("/auth/password-reset/confirm", post(routes::confirm_password_reset))
        .route_layer(axum::middleware::from_fn_with_state(auth_limiter, ratelimit::limit));

    let v1 = Router::new()
        .merge(auth)
        .route("/auth/logout", post(routes::logout))
        .route("/me", get(routes::me))
        .route("/admin/audit-events", get(routes::list_audit_events))
        .route("/admin/users", get(routes::list_users))
        .route(
            "/admin/users/{id}/roles/{role}",
            axum::routing::put(routes::grant_user_role).delete(routes::revoke_user_role),
        )
        .route("/admin/users/{id}/disable", post(routes::disable_user))
        .route("/admin/users/{id}/enable", post(routes::enable_user))
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
        .route("/entities/suggest", get(ent::suggest))
        .route("/entities/proposals", post(ent::create_proposal))
        .route("/entities/proposals/{id}/approve", post(ent::approve_proposal))
        .route("/entities/proposals/{id}/reject", post(ent::reject_proposal))
        .route("/entities/merges", post(ent::merge_entities))
        .route("/moderation/entity-proposals", get(ent::proposal_queue))
        .route("/pages/{scope}/{slug}", get(seo_api::get_page))
        .route("/sitemap", get(seo_api::sitemap))
        .route("/search", get(srch::search))
        .route("/home", get(hm::home))
        .route(
            "/editors-picks/{id}",
            get(hm::pick_state).put(hm::pick).delete(hm::unpick),
        )
        .route(
            "/import/songs",
            post(imp::import_songs).layer(axum::extract::DefaultBodyLimit::max(16 * 1024 * 1024)),
        )
        .route(
            "/contribute/song",
            post(ctb::contribute_song).layer(axum::extract::DefaultBodyLimit::max(8 * 1024 * 1024)),
        )
        .route("/compositions/{id}/lyrics", post(ctb::add_song_lyrics))
        .route("/admin/metadata", post(mdt::create_metadata))
        .route("/articles", get(art::list).post(art::create))
        .route("/articles/home", get(art::home))
        .route("/articles/{key}", get(art::read).patch(art::update).delete(art::delete))
        .route("/articles/{key}/home", axum::routing::put(art::set_home))
        .route(
            "/articles/{key}/images",
            get(art::images)
                .post(art::upload_image)
                .layer(axum::extract::DefaultBodyLimit::max(
                    app.config.media_max_bytes as usize + 64 * 1024,
                )),
        )
        .route(
            "/articles/{key}/images/{image_id}",
            axum::routing::delete(art::delete_image),
        )
        .route("/images/limits", get(img::limits))
        .route(
            "/images",
            post(img::upload).layer(axum::extract::DefaultBodyLimit::max(
                app.config.media_max_bytes as usize + 64 * 1024,
            )),
        )
        .route("/images/{id}", axum::routing::delete(img::withdraw))
        .route("/images/{id}/approve", post(img::approve))
        .route("/images/{id}/reject", post(img::reject))
        .route("/entities/{entity_type}/{entity_id}/images", get(img::entity_images))
        .route("/media/{id}/{width}", get(img::media))
        .route("/moderation/images", get(img::queue))
        .route("/browse/songs", get(brw::songs))
        .route("/browse/facets/{facet}", get(brw::facet))
        .route("/admin/search/metrics", get(srch::metrics))
        .route("/openapi.json", get(|| async { Json(ApiDoc::openapi()) }))
        .merge(typeahead)
        .merge(community)
        .merge(legal)
        .merge(lyrics);

    Router::new()
        .route("/healthz", get(routes::healthz))
        .route("/readyz", get(routes::readyz))
        .route("/metrics", get(metrics::scrape))
        .nest("/api/v1", v1)
        .fallback(routes::not_found)
        .method_not_allowed_fallback(routes::method_not_allowed)
        .layer(axum::middleware::from_fn(middleware::csrf))
        .layer(axum::middleware::from_fn(metrics::track))
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
