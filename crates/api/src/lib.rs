//! HTTP adapter: routing, extractors, problem+json, OpenAPI. No business rules here.

pub mod extract;
pub mod middleware;
pub mod problem;
pub mod routes;

use application::App;
use axum::routing::{get, post};
use axum::{Json, Router};
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
    let v1 = Router::new()
        .route("/auth/register", post(routes::register))
        .route("/auth/verify-email", post(routes::verify_email))
        .route("/auth/login", post(routes::login))
        .route("/auth/logout", post(routes::logout))
        .route("/auth/password-reset", post(routes::request_password_reset))
        .route("/auth/password-reset/confirm", post(routes::confirm_password_reset))
        .route("/me", get(routes::me))
        .route("/admin/audit-events", get(routes::list_audit_events))
        .route("/openapi.json", get(|| async { Json(ApiDoc::openapi()) }));

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
