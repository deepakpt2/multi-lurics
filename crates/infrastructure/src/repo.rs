//! Identity persistence. All functions take a connection so callers decide the transaction.

use chrono::{DateTime, Utc};
use domain::Role;
use sqlx::PgConnection;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct UserRow {
    pub id: Uuid,
    pub email: String,
    pub display_name: String,
    pub password_hash: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

const USER_COLS: &str = "id, email::text AS email, display_name, password_hash, status, created_at";

pub async fn find_user_by_email(conn: &mut PgConnection, email: &str) -> Result<Option<UserRow>, sqlx::Error> {
    sqlx::query_as(&format!("SELECT {USER_COLS} FROM users WHERE email = $1::citext"))
        .bind(email)
        .fetch_optional(conn)
        .await
}

pub async fn find_user(conn: &mut PgConnection, id: Uuid) -> Result<Option<UserRow>, sqlx::Error> {
    sqlx::query_as(&format!("SELECT {USER_COLS} FROM users WHERE id = $1"))
        .bind(id)
        .fetch_optional(conn)
        .await
}

/// Returns `None` when the email is already registered (no error, no information leak).
pub async fn insert_user(
    conn: &mut PgConnection,
    email: &str,
    display_name: &str,
    password_hash: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO users (email, display_name, password_hash) VALUES ($1, $2, $3)
         ON CONFLICT (email) DO NOTHING RETURNING id",
    )
    .bind(email)
    .bind(display_name)
    .bind(password_hash)
    .fetch_optional(conn)
    .await
}

/// A user created through a verified external identity: active from the start.
pub async fn insert_active_user(
    conn: &mut PgConnection,
    email: &str,
    display_name: &str,
    password_hash: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO users (email, display_name, password_hash, status, email_verified_at)
         VALUES ($1, $2, $3, 'active', now()) ON CONFLICT (email) DO NOTHING RETURNING id",
    )
    .bind(email)
    .bind(display_name)
    .bind(password_hash)
    .fetch_optional(conn)
    .await
}

pub async fn find_external_identity(
    conn: &mut PgConnection,
    provider: &str,
    subject: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar("SELECT user_id FROM external_identities WHERE provider = $1 AND subject = $2")
        .bind(provider)
        .bind(subject)
        .fetch_optional(conn)
        .await
}

pub async fn insert_external_identity(
    conn: &mut PgConnection,
    user_id: Uuid,
    provider: &str,
    subject: &str,
    email: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO external_identities (user_id, provider, subject, email) VALUES ($1, $2, $3, $4)")
        .bind(user_id)
        .bind(provider)
        .bind(subject)
        .bind(email)
        .execute(conn)
        .await?;
    Ok(())
}

pub struct OauthState {
    pub code_verifier: String,
    pub nonce: String,
    pub locale: String,
}

pub async fn insert_oauth_state(
    conn: &mut PgConnection,
    state_hash: &[u8; 32],
    code_verifier: &str,
    nonce: &str,
    locale: &str,
    ttl_minutes: i32,
) -> Result<(), sqlx::Error> {
    // Opportunistic cleanup keeps the table small without a scheduled job.
    sqlx::query("DELETE FROM oauth_states WHERE expires_at < now()")
        .execute(&mut *conn)
        .await?;
    sqlx::query(
        "INSERT INTO oauth_states (state_hash, code_verifier, nonce, locale, expires_at)
         VALUES ($1, $2, $3, $4, now() + make_interval(mins => $5))",
    )
    .bind(&state_hash[..])
    .bind(code_verifier)
    .bind(nonce)
    .bind(locale)
    .bind(ttl_minutes)
    .execute(conn)
    .await?;
    Ok(())
}

/// Single use: the row is gone after the first successful call.
pub async fn consume_oauth_state(
    conn: &mut PgConnection,
    state_hash: &[u8; 32],
) -> Result<Option<OauthState>, sqlx::Error> {
    let row: Option<(String, String, String)> = sqlx::query_as(
        "DELETE FROM oauth_states WHERE state_hash = $1 AND expires_at > now()
         RETURNING code_verifier, nonce, locale",
    )
    .bind(&state_hash[..])
    .fetch_optional(conn)
    .await?;
    Ok(row.map(|(code_verifier, nonce, locale)| OauthState {
        code_verifier,
        nonce,
        locale,
    }))
}

/// Drops every outstanding single-use token for a user (used when an unverified registration is
/// taken over by the verified owner of the address).
pub async fn delete_user_tokens(conn: &mut PgConnection, user_id: Uuid) -> Result<u64, sqlx::Error> {
    Ok(sqlx::query("DELETE FROM account_tokens WHERE user_id = $1")
        .bind(user_id)
        .execute(conn)
        .await?
        .rows_affected())
}

pub async fn activate_user(conn: &mut PgConnection, id: Uuid) -> Result<bool, sqlx::Error> {
    let r = sqlx::query(
        "UPDATE users SET status = 'active', email_verified_at = now(), updated_at = now()
         WHERE id = $1 AND status = 'unverified'",
    )
    .bind(id)
    .execute(conn)
    .await?;
    Ok(r.rows_affected() == 1)
}

pub async fn set_password(conn: &mut PgConnection, id: Uuid, hash: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE users SET password_hash = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(hash)
        .execute(conn)
        .await
        .map(|_| ())
}

pub async fn roles(conn: &mut PgConnection, user_id: Uuid) -> Result<Vec<Role>, sqlx::Error> {
    let raw: Vec<String> = sqlx::query_scalar("SELECT role FROM user_roles WHERE user_id = $1 ORDER BY role")
        .bind(user_id)
        .fetch_all(conn)
        .await?;
    Ok(raw.iter().filter_map(|r| Role::parse(r)).collect())
}

pub async fn grant_role(
    conn: &mut PgConnection,
    user_id: Uuid,
    role: Role,
    granted_by: Option<Uuid>,
) -> Result<bool, sqlx::Error> {
    let r =
        sqlx::query("INSERT INTO user_roles (user_id, role, granted_by) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING")
            .bind(user_id)
            .bind(role.as_str())
            .bind(granted_by)
            .execute(conn)
            .await?;
    Ok(r.rows_affected() == 1)
}

// ---- sessions ----

pub async fn insert_session(
    conn: &mut PgConnection,
    user_id: Uuid,
    token_hash: &[u8; 32],
    ttl_hours: i64,
    user_agent: Option<&str>,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO sessions (user_id, token_hash, expires_at, user_agent)
         VALUES ($1, $2, now() + make_interval(hours => $3::int), left($4, 512)) RETURNING id",
    )
    .bind(user_id)
    .bind(&token_hash[..])
    .bind(ttl_hours as i32)
    .bind(user_agent)
    .fetch_one(conn)
    .await
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SessionRow {
    pub session_id: Uuid,
    pub user_id: Uuid,
}

/// Live session of an active user, or `None`. Status is re-checked on every request,
/// so disabling a user takes effect immediately.
pub async fn find_live_session(
    conn: &mut PgConnection,
    token_hash: &[u8; 32],
) -> Result<Option<SessionRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT s.id AS session_id, s.user_id
         FROM sessions s JOIN users u ON u.id = s.user_id
         WHERE s.token_hash = $1 AND s.expires_at > now() AND u.status = 'active'",
    )
    .bind(&token_hash[..])
    .fetch_optional(conn)
    .await
}

pub async fn delete_session(conn: &mut PgConnection, session_id: Uuid) -> Result<bool, sqlx::Error> {
    let r = sqlx::query("DELETE FROM sessions WHERE id = $1")
        .bind(session_id)
        .execute(conn)
        .await?;
    Ok(r.rows_affected() == 1)
}

pub async fn delete_user_sessions(conn: &mut PgConnection, user_id: Uuid) -> Result<u64, sqlx::Error> {
    let r = sqlx::query("DELETE FROM sessions WHERE user_id = $1")
        .bind(user_id)
        .execute(conn)
        .await?;
    Ok(r.rows_affected())
}

// ---- single-use account tokens ----

#[derive(Debug, Clone, Copy)]
pub enum TokenPurpose {
    VerifyEmail,
    PasswordReset,
}

impl TokenPurpose {
    fn as_str(self) -> &'static str {
        match self {
            Self::VerifyEmail => "verify_email",
            Self::PasswordReset => "password_reset",
        }
    }
}

pub async fn insert_token(
    conn: &mut PgConnection,
    user_id: Uuid,
    purpose: TokenPurpose,
    token_hash: &[u8; 32],
    ttl_minutes: i32,
) -> Result<(), sqlx::Error> {
    // Issuing a new token invalidates older unused ones of the same purpose.
    sqlx::query("UPDATE account_tokens SET used_at = now() WHERE user_id = $1 AND purpose = $2 AND used_at IS NULL")
        .bind(user_id)
        .bind(purpose.as_str())
        .execute(&mut *conn)
        .await?;
    sqlx::query(
        "INSERT INTO account_tokens (user_id, purpose, token_hash, expires_at)
         VALUES ($1, $2, $3, now() + make_interval(mins => $4))",
    )
    .bind(user_id)
    .bind(purpose.as_str())
    .bind(&token_hash[..])
    .bind(ttl_minutes)
    .execute(conn)
    .await
    .map(|_| ())
}

/// Atomically consumes a token. Of two concurrent callers with the same token,
/// exactly one gets `Some(user_id)`.
pub async fn consume_token(
    conn: &mut PgConnection,
    purpose: TokenPurpose,
    token_hash: &[u8; 32],
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "UPDATE account_tokens SET used_at = now()
         WHERE token_hash = $1 AND purpose = $2 AND used_at IS NULL AND expires_at > now()
         RETURNING user_id",
    )
    .bind(&token_hash[..])
    .bind(purpose.as_str())
    .fetch_optional(conn)
    .await
}
