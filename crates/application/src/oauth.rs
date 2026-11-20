//! Sign-in with Google (OpenID Connect, authorization-code flow with PKCE) and first-admin bootstrap.
//!
//! Security properties:
//! - `state` is random, single-use, stored hashed, and must also match an HttpOnly cookie, so a
//!   callback cannot be replayed or injected from another browser (login CSRF).
//! - The ID token is fetched by this server straight from Google's token endpoint over TLS, then
//!   its `iss`, `aud`, `exp`, `nonce` and `email_verified` claims are checked (OIDC Core 3.1.3.7).
//! - Accounts are matched on Google's stable `sub`. An existing account is linked by email only
//!   when Google vouches for that email; an unverified password registration for the same address
//!   is taken over (its password and tokens are destroyed) so a squatter keeps no access.

use crate::auth::{start_session, NewSession};
use crate::{App, AppError, RequestMeta};
use async_trait::async_trait;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use domain::{DisplayName, Email, Password, Role, UserStatus};
use infrastructure::audit::{self, AuditEntry};
use infrastructure::repo;
use infrastructure::{security, AppTx, Config};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::time::Duration;
use uuid::Uuid;

const PROVIDER: &str = "google";
const STATE_TTL_MINUTES: i32 = 10;
const AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";

/// The verified claims we rely on.
#[derive(Debug, Clone)]
pub struct IdentityClaims {
    pub subject: String,
    pub email: String,
    pub email_verified: bool,
    pub name: Option<String>,
}

#[async_trait]
pub trait IdentityProvider: Send + Sync {
    fn client_id(&self) -> &str;
    async fn exchange(
        &self,
        code: &str,
        code_verifier: &str,
        redirect_uri: &str,
        nonce: &str,
    ) -> anyhow::Result<IdentityClaims>;
}

pub struct GoogleHttp {
    client_id: String,
    client_secret: String,
    http: reqwest::Client,
}

impl GoogleHttp {
    pub fn from_config(c: &Config) -> Option<Self> {
        let (client_id, client_secret) = (c.google_client_id.clone()?, c.google_client_secret.clone()?);
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .ok()?;
        Some(Self {
            client_id,
            client_secret,
            http,
        })
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    id_token: String,
}

#[derive(Deserialize)]
struct RawClaims {
    iss: String,
    aud: String,
    exp: i64,
    sub: String,
    nonce: Option<String>,
    email: Option<String>,
    email_verified: Option<serde_json::Value>,
    name: Option<String>,
}

/// Validates the claims of an ID token received directly from the token endpoint.
pub fn parse_id_token(id_token: &str, client_id: &str, nonce: &str, now: i64) -> anyhow::Result<IdentityClaims> {
    let payload = id_token
        .split('.')
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("malformed id_token"))?;
    let raw: RawClaims = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload)?)?;
    anyhow::ensure!(
        raw.iss == "https://accounts.google.com" || raw.iss == "accounts.google.com",
        "unexpected issuer"
    );
    anyhow::ensure!(raw.aud == client_id, "audience mismatch");
    anyhow::ensure!(raw.exp > now, "id_token expired");
    anyhow::ensure!(raw.nonce.as_deref() == Some(nonce), "nonce mismatch");
    anyhow::ensure!(!raw.sub.is_empty() && raw.sub.len() <= 255, "bad subject");
    let verified = match raw.email_verified {
        Some(serde_json::Value::Bool(b)) => b,
        Some(serde_json::Value::String(s)) => s == "true",
        _ => false,
    };
    Ok(IdentityClaims {
        subject: raw.sub,
        email: raw.email.ok_or_else(|| anyhow::anyhow!("no email claim"))?,
        email_verified: verified,
        name: raw.name,
    })
}

#[async_trait]
impl IdentityProvider for GoogleHttp {
    fn client_id(&self) -> &str {
        &self.client_id
    }

    async fn exchange(
        &self,
        code: &str,
        code_verifier: &str,
        redirect_uri: &str,
        nonce: &str,
    ) -> anyhow::Result<IdentityClaims> {
        let resp = self
            .http
            .post(TOKEN_ENDPOINT)
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("code_verifier", code_verifier),
                ("redirect_uri", redirect_uri),
                ("client_id", &self.client_id),
                ("client_secret", &self.client_secret),
            ])
            .send()
            .await?;
        anyhow::ensure!(resp.status().is_success(), "token endpoint answered {}", resp.status());
        let tokens: TokenResponse = resp.json().await?;
        parse_id_token(&tokens.id_token, &self.client_id, nonce, chrono::Utc::now().timestamp())
    }
}

pub fn callback_url(app: &App) -> Result<String, AppError> {
    Ok(app
        .config
        .app_base_url
        .join("api/v1/auth/google/callback")
        .map_err(|e| AppError::Internal(e.into()))?
        .into())
}

pub struct StartedSignIn {
    /// Goes into the short-lived HttpOnly cookie that binds the flow to this browser.
    pub state: String,
    pub redirect_to: String,
}

pub async fn start(app: &App, locale: Option<&str>) -> Result<StartedSignIn, AppError> {
    let google = app.google.as_ref().ok_or(AppError::NotFound {
        entity: "google_sign_in",
    })?;
    let locale = match locale {
        Some(l) if crate::auth::SUPPORTED_LOCALES.contains(&l) => l,
        _ => "en",
    };
    let (state, state_hash) = security::new_token();
    let (nonce, _) = security::new_token();
    let (verifier, _) = security::new_token();
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));

    let mut conn = app.pool.acquire().await?;
    repo::insert_oauth_state(&mut conn, &state_hash, &verifier, &nonce, locale, STATE_TTL_MINUTES).await?;
    drop(conn);

    let mut url = url::Url::parse(AUTH_ENDPOINT).map_err(|e| AppError::Internal(e.into()))?;
    url.query_pairs_mut()
        .append_pair("client_id", google.client_id())
        .append_pair("redirect_uri", &callback_url(app)?)
        .append_pair("response_type", "code")
        .append_pair("scope", "openid email profile")
        .append_pair("state", &state)
        .append_pair("nonce", &nonce)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("prompt", "select_account");
    Ok(StartedSignIn {
        state,
        redirect_to: url.into(),
    })
}

pub struct CallbackOutcome {
    pub locale: String,
    pub result: Result<NewSession, AppError>,
}

pub async fn callback(
    app: &App,
    meta: &RequestMeta,
    state_param: &str,
    state_cookie: Option<&str>,
    code: &str,
) -> CallbackOutcome {
    let mut locale = "en".to_owned();
    let result = callback_inner(app, meta, state_param, state_cookie, code, &mut locale).await;
    CallbackOutcome { locale, result }
}

async fn callback_inner(
    app: &App,
    meta: &RequestMeta,
    state_param: &str,
    state_cookie: Option<&str>,
    code: &str,
    locale: &mut String,
) -> Result<NewSession, AppError> {
    let google = app.google.as_ref().ok_or(AppError::NotFound {
        entity: "google_sign_in",
    })?;
    if state_param.is_empty() || state_param.len() > 128 || code.is_empty() || code.len() > 2048 {
        return Err(AppError::InvalidToken);
    }
    // Compare digests so the check does not short-circuit on the first differing byte.
    let cookie_ok = state_cookie.is_some_and(|c| security::hash_token(c) == security::hash_token(state_param));
    if !cookie_ok {
        return Err(AppError::InvalidToken);
    }
    let mut conn = app.pool.acquire().await?;
    let st = repo::consume_oauth_state(&mut conn, &security::hash_token(state_param))
        .await?
        .ok_or(AppError::InvalidToken)?;
    drop(conn);
    locale.clone_from(&st.locale);

    let claims = google
        .exchange(code, &st.code_verifier, &callback_url(app)?, &st.nonce)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "google token exchange failed");
            AppError::BadRequest("google_exchange_failed")
        })?;
    if !claims.email_verified {
        return Err(AppError::BadRequest("google_email_unverified"));
    }
    let email = Email::parse(&claims.email)?;

    let mut tx = AppTx::begin(&app.pool).await?;
    let user = match repo::find_external_identity(&mut tx, PROVIDER, &claims.subject).await? {
        Some(user_id) => repo::find_user(&mut tx, user_id)
            .await?
            .ok_or(AppError::InvalidCredentials)?,
        None => link_or_create(&mut tx, meta, &claims, &email).await?,
    };
    match UserStatus::parse(&user.status) {
        Some(s) if s.can_sign_in() => {}
        _ => return Err(AppError::Forbidden),
    }
    let session = start_session(&mut tx, app, meta, user).await?;
    tx.commit().await?;
    Ok(session)
}

/// An unusable password: random, hashed, never revealed. The owner can set a real one via reset.
async fn unusable_password_hash() -> Result<String, AppError> {
    let (secret, _) = security::new_token();
    Ok(security::hash_password(secret).await?)
}

async fn link_or_create(
    tx: &mut AppTx,
    meta: &RequestMeta,
    claims: &IdentityClaims,
    email: &Email,
) -> Result<repo::UserRow, AppError> {
    let existing = repo::find_user_by_email(tx, email.as_str()).await?;
    let (user_id, action) = match existing {
        Some(u) => {
            match UserStatus::parse(&u.status) {
                Some(UserStatus::Disabled) | None => return Err(AppError::Forbidden),
                Some(UserStatus::Unverified) => {
                    // Whoever registered this address by password never proved they own it; Google
                    // just did. Destroy what the squatter set up before activating.
                    let hash = unusable_password_hash().await?;
                    repo::set_password(tx, u.id, &hash).await?;
                    repo::delete_user_sessions(tx, u.id).await?;
                    repo::delete_user_tokens(tx, u.id).await?;
                    repo::activate_user(tx, u.id).await?;
                }
                Some(_) => {}
            }
            (u.id, "auth.google_linked")
        }
        None => {
            let name = claims
                .name
                .as_deref()
                .and_then(|n| DisplayName::parse(n).ok())
                .or_else(|| DisplayName::parse(email.as_str().split('@').next().unwrap_or("member")).ok())
                .or_else(|| DisplayName::parse("Member").ok())
                .ok_or(AppError::BadRequest("google_name_invalid"))?;
            let hash = unusable_password_hash().await?;
            let id = repo::insert_active_user(tx, email.as_str(), name.as_str(), &hash).await?;
            match id {
                Some(id) => (id, "auth.google_registered"),
                // Lost a race with a concurrent registration of the same address: nothing was
                // linked, so the caller simply retries and takes the "existing" branch.
                None => {
                    return Err(AppError::Conflict {
                        code: "retry",
                        current_revision_id: None,
                    })
                }
            }
        }
    };
    repo::insert_external_identity(tx, user_id, PROVIDER, &claims.subject, email.as_str()).await?;
    audit::record(
        tx,
        AuditEntry {
            actor_id: Some(user_id),
            request_id: meta.request_id.as_deref(),
            metadata: json!({ "provider": PROVIDER }),
            ..AuditEntry::new(action, "user", Some(user_id))
        },
    )
    .await?;
    repo::find_user(tx, user_id).await?.ok_or(AppError::InvalidCredentials)
}

/// Creates (or promotes) the administrator named in the environment. Idempotent and safe to run on
/// every start: an existing account keeps its password; only the missing pieces are added.
pub async fn bootstrap_admin(app: &App) -> Result<(), AppError> {
    let (Some(raw_email), Some(raw_password)) = (&app.config.admin_email, &app.config.admin_password) else {
        return Ok(());
    };
    let email = Email::parse(raw_email)?;
    let password = Password::parse("LYRICS_ADMIN_PASSWORD", raw_password)?;
    let name = DisplayName::parse(&app.config.admin_display_name)?;
    let hash = security::hash_password(password.expose().to_owned()).await?;

    let mut tx = AppTx::begin(&app.pool).await?;
    // Serialise concurrent API replicas starting together.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('lyrics.bootstrap_admin'))")
        .execute(&mut *tx)
        .await?;
    let existing = repo::find_user_by_email(&mut tx, email.as_str()).await?;
    let (user_id, created) = match existing {
        Some(u) => {
            if UserStatus::parse(&u.status) == Some(UserStatus::Disabled) {
                tracing::warn!("configured admin account is disabled; not promoting it");
                return Ok(());
            }
            repo::activate_user(&mut tx, u.id).await?;
            (u.id, false)
        }
        None => {
            let id: Uuid = repo::insert_active_user(&mut tx, email.as_str(), name.as_str(), &hash)
                .await?
                .ok_or(AppError::Conflict {
                    code: "retry",
                    current_revision_id: None,
                })?;
            (id, true)
        }
    };
    if repo::grant_role(&mut tx, user_id, Role::Admin, None).await? || created {
        audit::record(
            &mut tx,
            AuditEntry {
                actor_id: None,
                after: Some(json!({ "role": "admin", "created": created })),
                metadata: json!({ "source": "environment" }),
                ..AuditEntry::new("user.admin_bootstrapped", "user", Some(user_id))
            },
        )
        .await?;
        tracing::info!(created, "first administrator ensured");
    }
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(claims: serde_json::Value) -> String {
        format!("h.{}.s", URL_SAFE_NO_PAD.encode(claims.to_string()))
    }

    fn good() -> serde_json::Value {
        json!({"iss":"https://accounts.google.com","aud":"cid","exp":2000,"sub":"123","nonce":"n",
               "email":"a@b.co","email_verified":true,"name":"A"})
    }

    #[test]
    fn accepts_valid_and_rejects_each_bad_claim() {
        assert!(parse_id_token(&token(good()), "cid", "n", 1000).is_ok());
        for (k, v) in [
            ("iss", json!("https://evil.example")),
            ("aud", json!("other")),
            ("exp", json!(999)),
            ("nonce", json!("x")),
        ] {
            let mut c = good();
            c[k] = v;
            assert!(
                parse_id_token(&token(c), "cid", "n", 1000).is_err(),
                "{k} must be checked"
            );
        }
        let mut c = good();
        c["email_verified"] = json!(false);
        assert!(!parse_id_token(&token(c), "cid", "n", 1000).unwrap().email_verified);
        assert!(parse_id_token("garbage", "cid", "n", 1000).is_err());
    }
}
