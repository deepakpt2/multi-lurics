//! Bot check (Cloudflare Turnstile) for pages anyone can reach: sign-up, sign-in, password reset and
//! legal notices. Off unless both keys are configured. The browser solves a challenge and sends the
//! answer with the form; the server asks Cloudflare whether it is genuine. A missing or wrong answer
//! is refused, and so is an unreachable verifier (failing open would let a bot wait out an outage).

use crate::{App, AppError};
use async_trait::async_trait;
use domain::DomainError;
use infrastructure::Config;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;

pub const VERIFY_URL: &str = "https://challenges.cloudflare.com/turnstile/v0/siteverify";

#[async_trait]
pub trait Verifier: Send + Sync {
    /// `Ok(true)` when the answer is genuine. `Err` when the verifier could not be reached.
    async fn verify(&self, token: &str, remote_ip: Option<&str>) -> Result<bool, anyhow::Error>;
}

pub struct Turnstile {
    secret: String,
    url: String,
    http: reqwest::Client,
}

impl Turnstile {
    pub fn from_config(c: &Config) -> Option<Self> {
        Self::new(c.turnstile_secret_key.clone()?, VERIFY_URL)
    }

    pub fn new(secret: String, url: &str) -> Option<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .build()
            .ok()?;
        Some(Self {
            secret,
            url: url.to_owned(),
            http,
        })
    }
}

#[derive(Deserialize)]
struct Answer {
    success: bool,
}

#[async_trait]
impl Verifier for Turnstile {
    async fn verify(&self, token: &str, remote_ip: Option<&str>) -> Result<bool, anyhow::Error> {
        let mut form = vec![("secret", self.secret.as_str()), ("response", token)];
        if let Some(ip) = remote_ip {
            form.push(("remoteip", ip));
        }
        let res = self.http.post(&self.url).form(&form).send().await?.error_for_status()?;
        Ok(res.json::<Answer>().await?.success)
    }
}

/// What the browser needs to show the challenge.
pub fn site_key(app: &App) -> Option<&str> {
    app.captcha.as_ref()?;
    app.config.turnstile_site_key.as_deref()
}

/// Passes when no check is configured, or the answer is genuine.
pub async fn check(app: &App, token: Option<&str>, remote_ip: Option<&str>) -> Result<(), AppError> {
    let Some(v) = &app.captcha else { return Ok(()) };
    let token = token.map(str::trim).filter(|t| !t.is_empty());
    let Some(token) = token else {
        return Err(DomainError::invalid("captcha", "captcha_required").into());
    };
    // Tokens are short; refuse junk before sending it anywhere.
    if token.len() > 2048 {
        return Err(DomainError::invalid("captcha", "captcha_failed").into());
    }
    match v.verify(token, remote_ip).await {
        Ok(true) => Ok(()),
        Ok(false) => Err(DomainError::invalid("captcha", "captcha_failed").into()),
        Err(e) => {
            tracing::warn!(error = ?e, "bot check could not be verified");
            Err(AppError::Unavailable)
        }
    }
}

pub fn from_config(c: &Config) -> Option<Arc<dyn Verifier>> {
    Turnstile::from_config(c).map(|t| Arc::new(t) as Arc<dyn Verifier>)
}
