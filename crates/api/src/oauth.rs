//! Google sign-in endpoints. Browser-navigated (redirects), so failures redirect back to the
//! login page with an error code instead of returning problem+json.

use crate::extract::Meta;
use crate::problem::{ApiResult, Problem};
use crate::routes::session_cookie;
use application::{oauth, App};
use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::Json;
use axum_extra::extract::cookie::{Cookie, SameSite};
use axum_extra::extract::CookieJar;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

const STATE_COOKIE: &str = "lyrics_oauth_state";
const STATE_COOKIE_PATH: &str = "/api/v1/auth/google";

#[derive(Serialize, ToSchema)]
pub struct AuthProviders {
    /// Whether "Sign in with Google" is configured on this server.
    pub google: bool,
}

/// Which sign-in methods this server offers besides email and password.
#[utoipa::path(get, path = "/api/v1/auth/providers", tag = "auth", responses((status = 200, body = AuthProviders)))]
pub async fn providers(State(app): State<App>) -> Json<AuthProviders> {
    Json(AuthProviders {
        google: app.google.is_some(),
    })
}

#[derive(Deserialize, IntoParams)]
pub struct StartQuery {
    /// UI locale to return to after sign-in (`en` or `ar`).
    pub locale: Option<String>,
}

fn state_cookie(app: &App, value: String, max_age_secs: i64) -> Cookie<'static> {
    Cookie::build((STATE_COOKIE, value))
        .path(STATE_COOKIE_PATH)
        .http_only(true)
        .secure(app.config.cookie_secure)
        // Lax: the cookie must accompany the top-level redirect back from Google.
        .same_site(SameSite::Lax)
        .max_age(time::Duration::seconds(max_age_secs))
        .build()
}

fn login_error(app: &App, locale: &str, code: &str) -> Redirect {
    let mut url = app
        .config
        .app_base_url
        .join(&format!("{locale}/login"))
        .expect("base URL validated at start-up");
    url.query_pairs_mut().append_pair("error", code);
    Redirect::to(url.as_str())
}

/// Redirects the browser to Google. Sets a short-lived state cookie.
#[utoipa::path(get, path = "/api/v1/auth/google/start", tag = "auth", params(StartQuery), responses(
    (status = 303, description = "Redirect to Google's consent screen"),
    (status = 404, body = Problem, content_type = "application/problem+json", description = "Google sign-in is not configured")))]
pub async fn google_start(
    State(app): State<App>,
    Query(q): Query<StartQuery>,
    jar: CookieJar,
) -> ApiResult<(CookieJar, Redirect)> {
    let started = oauth::start(&app, q.locale.as_deref()).await?;
    Ok((
        jar.add(state_cookie(&app, started.state, 600)),
        Redirect::to(&started.redirect_to),
    ))
}

#[derive(Deserialize, IntoParams)]
pub struct CallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    /// Set by Google when the user declined or the request was invalid.
    pub error: Option<String>,
}

/// Completes the sign-in, sets the `lyrics_session` cookie and redirects into the app.
#[utoipa::path(get, path = "/api/v1/auth/google/callback", tag = "auth", params(CallbackQuery), responses(
    (status = 303, description = "Redirect to the account page, or to the login page with `?error=`")))]
pub async fn google_callback(
    State(app): State<App>,
    Meta(meta): Meta,
    Query(q): Query<CallbackQuery>,
    jar: CookieJar,
) -> Response {
    let cookie = jar.get(STATE_COOKIE).map(|c| c.value().to_owned());
    // The flow is over either way; never leave the state cookie behind.
    let jar = jar.add(state_cookie(&app, String::new(), 0));
    let headers = [(header::CACHE_CONTROL, "no-store")];
    if q.error.is_some() {
        return (
            StatusCode::SEE_OTHER,
            headers,
            jar,
            login_error(&app, "en", "google_denied"),
        )
            .into_response();
    }
    let out = oauth::callback(
        &app,
        &meta,
        q.state.as_deref().unwrap_or(""),
        cookie.as_deref(),
        q.code.as_deref().unwrap_or(""),
    )
    .await;
    match out.result {
        Ok(s) => {
            let ttl = app.config.session_ttl_hours;
            let jar = jar.add(session_cookie(&app, s.token, ttl));
            let to = app
                .config
                .app_base_url
                .join(&format!("{}/account", out.locale))
                .expect("base URL validated at start-up");
            (headers, jar, Redirect::to(to.as_str())).into_response()
        }
        Err(e) => {
            let code = match &e {
                application::AppError::Forbidden => "google_disabled",
                application::AppError::BadRequest("google_email_unverified") => "google_email_unverified",
                _ => "google_failed",
            };
            tracing::warn!(error = %e, "google sign-in failed");
            (headers, jar, login_error(&app, &out.locale, code)).into_response()
        }
    }
}
