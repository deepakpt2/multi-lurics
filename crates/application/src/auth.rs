//! Account lifecycle: register → verify email → login/logout, password reset.
//! Responses are shaped so they never reveal whether an email is registered.

use crate::{App, AppError, RequestMeta};
use chrono::{DateTime, Utc};
use domain::{DisplayName, Email, Password, Principal, Role, UserStatus, Violations};
use infrastructure::audit::{self, AuditEntry};
use infrastructure::repo::{self, TokenPurpose};
use infrastructure::{outbox, security, AppTx};
use serde_json::json;
use uuid::Uuid;

pub const SUPPORTED_LOCALES: &[&str] = &["en", "ar"];
const VERIFY_TTL_MINUTES: i32 = 24 * 60;
const RESET_TTL_MINUTES: i32 = 60;

pub struct RegisterInput {
    pub email: String,
    pub display_name: String,
    pub password: String,
    pub locale: Option<String>,
}

pub struct LoginInput {
    pub email: String,
    pub password: String,
}

/// A freshly issued session. `token` goes to the client exactly once, as an HttpOnly cookie.
pub struct NewSession {
    pub token: String,
    pub user: UserView,
}

#[derive(Debug, Clone)]
pub struct UserView {
    pub id: Uuid,
    pub email: String,
    pub display_name: String,
    pub roles: Vec<Role>,
    pub created_at: DateTime<Utc>,
}

fn parse_locale(v: &mut Violations, raw: Option<&str>) -> &'static str {
    match raw {
        None => "en",
        Some(l) => SUPPORTED_LOCALES.iter().copied().find(|s| *s == l).unwrap_or_else(|| {
            v.push("locale", "unsupported_locale");
            "en"
        }),
    }
}

fn link(app: &App, locale: &str, page: &str, token: &str) -> Result<String, AppError> {
    // The token travels in the fragment so it never reaches server logs or Referer headers.
    let mut url = app
        .config
        .app_base_url
        .join(&format!("{locale}/{page}"))
        .map_err(|e| AppError::Internal(e.into()))?;
    url.set_fragment(Some(&format!("token={token}")));
    Ok(url.into())
}

pub async fn register(app: &App, meta: &RequestMeta, input: RegisterInput) -> Result<(), AppError> {
    let mut v = Violations::default();
    let email = v.check(Email::parse(&input.email));
    let name = v.check(DisplayName::parse(&input.display_name));
    let password = v.check(Password::parse("password", &input.password));
    let locale = parse_locale(&mut v, input.locale.as_deref());
    v.finish()?;
    let (email, name, password) = (email.unwrap(), name.unwrap(), password.unwrap());

    let hash = security::hash_password(password.expose().to_owned()).await?;
    let mut tx = AppTx::begin(&app.pool).await?;
    match repo::insert_user(&mut tx, email.as_str(), name.as_str(), &hash).await? {
        Some(user_id) => {
            let (token, token_hash) = security::new_token();
            repo::insert_token(
                &mut tx,
                user_id,
                TokenPurpose::VerifyEmail,
                &token_hash,
                VERIFY_TTL_MINUTES,
            )
            .await?;
            outbox::enqueue(
                &mut tx,
                "email.verify_requested",
                "user",
                user_id,
                json!({ "to": email.as_str(), "display_name": name.as_str(), "locale": locale,
                        "link": link(app, locale, "verify-email", &token)? }),
            )
            .await?;
            audit::record(
                &mut tx,
                AuditEntry {
                    actor_id: Some(user_id),
                    request_id: meta.request_id.as_deref(),
                    after: Some(json!({ "display_name": name.as_str(), "status": "unverified" })),
                    ..AuditEntry::new("user.registered", "user", Some(user_id))
                },
            )
            .await?;
        }
        None => {
            // Same response as success; the real owner is told someone tried to register.
            let existing = repo::find_user_by_email(&mut tx, email.as_str()).await?.map(|u| u.id);
            if let Some(user_id) = existing {
                outbox::enqueue(
                    &mut tx,
                    "email.registration_attempted",
                    "user",
                    user_id,
                    json!({ "to": email.as_str(), "locale": locale }),
                )
                .await?;
            }
        }
    }
    tx.commit().await?;
    Ok(())
}

pub async fn verify_email(app: &App, meta: &RequestMeta, token: &str) -> Result<(), AppError> {
    if token.is_empty() || token.len() > 128 {
        return Err(AppError::InvalidToken);
    }
    let mut tx = AppTx::begin(&app.pool).await?;
    let user_id = repo::consume_token(&mut tx, TokenPurpose::VerifyEmail, &security::hash_token(token))
        .await?
        .ok_or(AppError::InvalidToken)?;
    if repo::activate_user(&mut tx, user_id).await? {
        audit::record(
            &mut tx,
            AuditEntry {
                actor_id: Some(user_id),
                request_id: meta.request_id.as_deref(),
                before: Some(json!({ "status": "unverified" })),
                after: Some(json!({ "status": "active" })),
                ..AuditEntry::new("user.email_verified", "user", Some(user_id))
            },
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn login(app: &App, meta: &RequestMeta, input: LoginInput) -> Result<NewSession, AppError> {
    let mut v = Violations::default();
    if input.email.trim().is_empty() {
        v.push("email", "required");
    }
    if input.password.is_empty() {
        v.push("password", "required");
    }
    v.finish()?;

    let mut conn = app.pool.acquire().await?;
    let user = repo::find_user_by_email(&mut conn, input.email.trim()).await?;
    drop(conn);
    // Always pay the hashing cost so response time does not reveal whether the email exists.
    let ok = security::verify_password(input.password, user.as_ref().map(|u| u.password_hash.clone())).await?;
    let user = match (ok, user) {
        (true, Some(u)) => u,
        _ => return Err(AppError::InvalidCredentials),
    };
    match UserStatus::parse(&user.status) {
        Some(s) if s.can_sign_in() => {}
        Some(UserStatus::Unverified) => return Err(AppError::EmailNotVerified),
        _ => return Err(AppError::Forbidden),
    }

    let (token, token_hash) = security::new_token();
    let mut tx = AppTx::begin(&app.pool).await?;
    let session_id = repo::insert_session(
        &mut tx,
        user.id,
        &token_hash,
        app.config.session_ttl_hours,
        meta.user_agent.as_deref(),
    )
    .await?;
    audit::record(
        &mut tx,
        AuditEntry {
            actor_id: Some(user.id),
            request_id: meta.request_id.as_deref(),
            metadata: json!({ "session_id": session_id }),
            ..AuditEntry::new("auth.session_created", "user", Some(user.id))
        },
    )
    .await?;
    let roles = repo::roles(&mut tx, user.id).await?;
    tx.commit().await?;
    Ok(NewSession {
        token,
        user: UserView {
            id: user.id,
            email: user.email,
            display_name: user.display_name,
            roles,
            created_at: user.created_at,
        },
    })
}

pub async fn logout(app: &App, meta: &RequestMeta, p: &Principal) -> Result<(), AppError> {
    let mut tx = AppTx::begin(&app.pool).await?;
    if repo::delete_session(&mut tx, p.session_id).await? {
        audit::record(
            &mut tx,
            AuditEntry {
                actor_id: Some(p.user_id),
                request_id: meta.request_id.as_deref(),
                metadata: json!({ "session_id": p.session_id }),
                ..AuditEntry::new("auth.session_revoked", "user", Some(p.user_id))
            },
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn request_password_reset(
    app: &App,
    meta: &RequestMeta,
    email: &str,
    locale: Option<&str>,
) -> Result<(), AppError> {
    let mut v = Violations::default();
    let email = v.check(Email::parse(email));
    let locale = parse_locale(&mut v, locale);
    v.finish()?;
    let email = email.unwrap();

    let mut tx = AppTx::begin(&app.pool).await?;
    let user = repo::find_user_by_email(&mut tx, email.as_str()).await?;
    if let Some(u) = user.filter(|u| u.status != UserStatus::Disabled.as_str()) {
        let (token, token_hash) = security::new_token();
        repo::insert_token(
            &mut tx,
            u.id,
            TokenPurpose::PasswordReset,
            &token_hash,
            RESET_TTL_MINUTES,
        )
        .await?;
        outbox::enqueue(
            &mut tx,
            "email.password_reset_requested",
            "user",
            u.id,
            json!({ "to": u.email, "locale": locale, "link": link(app, locale, "reset-password", &token)? }),
        )
        .await?;
        audit::record(
            &mut tx,
            AuditEntry {
                request_id: meta.request_id.as_deref(),
                ..AuditEntry::new("auth.password_reset_requested", "user", Some(u.id))
            },
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn confirm_password_reset(
    app: &App,
    meta: &RequestMeta,
    token: &str,
    new_password: &str,
) -> Result<(), AppError> {
    let password = Password::parse("new_password", new_password)?;
    if token.is_empty() || token.len() > 128 {
        return Err(AppError::InvalidToken);
    }
    let hash = security::hash_password(password.expose().to_owned()).await?;
    let mut tx = AppTx::begin(&app.pool).await?;
    let user_id = repo::consume_token(&mut tx, TokenPurpose::PasswordReset, &security::hash_token(token))
        .await?
        .ok_or(AppError::InvalidToken)?;
    repo::set_password(&mut tx, user_id, &hash).await?;
    // Proving control of the inbox also verifies it.
    repo::activate_user(&mut tx, user_id).await?;
    let revoked = repo::delete_user_sessions(&mut tx, user_id).await?;
    audit::record(
        &mut tx,
        AuditEntry {
            actor_id: Some(user_id),
            request_id: meta.request_id.as_deref(),
            metadata: json!({ "sessions_revoked": revoked }),
            ..AuditEntry::new("user.password_reset", "user", Some(user_id))
        },
    )
    .await?;
    let to = repo::find_user(&mut tx, user_id).await?.map(|u| u.email);
    outbox::enqueue(&mut tx, "email.password_changed", "user", user_id, json!({ "to": to })).await?;
    tx.commit().await?;
    Ok(())
}

/// Resolves a session cookie to a principal. Status and roles are read fresh each time.
pub async fn authenticate(app: &App, token: &str) -> Result<Option<Principal>, AppError> {
    if token.is_empty() || token.len() > 128 {
        return Ok(None);
    }
    let mut conn = app.pool.acquire().await?;
    let Some(s) = repo::find_live_session(&mut conn, &security::hash_token(token)).await? else {
        return Ok(None);
    };
    let roles = repo::roles(&mut conn, s.user_id).await?;
    Ok(Some(Principal {
        user_id: s.user_id,
        session_id: s.session_id,
        roles,
    }))
}

pub async fn me(app: &App, p: &Principal) -> Result<UserView, AppError> {
    let mut conn = app.pool.acquire().await?;
    let u = repo::find_user(&mut conn, p.user_id)
        .await?
        .ok_or(AppError::Unauthenticated)?;
    Ok(UserView {
        id: u.id,
        email: u.email,
        display_name: u.display_name,
        roles: p.roles.clone(),
        created_at: u.created_at,
    })
}
