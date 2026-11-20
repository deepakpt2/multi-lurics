//! The bot check (Cloudflare Turnstile): what the browser needs, and the check the public forms run.

use crate::problem::ApiResult;
use crate::ratelimit::client_key;
use application::{captcha as uc, App};
use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct CaptchaConfig {
    /// Whether sign-up, sign-in, password reset and legal notices ask for a bot check.
    pub enabled: bool,
    /// Turnstile site key (public); the browser shows the challenge with it.
    pub site_key: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/captcha", operation_id = "captcha_config", tag = "auth", responses(
    (status = 200, body = CaptchaConfig, description = "Whether forms need a bot check, and the key to show it with")))]
pub async fn config(State(app): State<App>) -> ApiResult<Json<CaptchaConfig>> {
    let key = uc::site_key(&app).map(str::to_owned);
    Ok(Json(CaptchaConfig {
        enabled: key.is_some(),
        site_key: key,
    }))
}

/// Runs the check for a public form. The caller's address goes to the verifier only when known.
pub async fn check(app: &App, headers: &HeaderMap, token: Option<&str>) -> ApiResult<()> {
    let ip = client_key(headers);
    let ip = (ip != "unknown").then_some(ip);
    Ok(uc::check(app, token, ip.as_deref()).await?)
}
