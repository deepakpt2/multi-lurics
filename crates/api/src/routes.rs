use crate::extract::{Admin, ApiJson, ApiPath, ApiQuery, Meta, RequireRole, RequireUser, SESSION_COOKIE};
use crate::problem::{ApiError, ApiResult, Problem};
use application::{admin, auth, App};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use axum_extra::extract::cookie::{Cookie, SameSite};
use axum_extra::extract::CookieJar;
use chrono::{DateTime, Utc};
use infrastructure::db::{migration_state, MigrationState};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

// ---------- DTOs ----------
// Request fields default to empty so a missing field yields a `required`
// field error from domain validation rather than an opaque deserialization error.

#[derive(Deserialize, ToSchema)]
pub struct RegisterRequest {
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub password: String,
    /// UI locale for emails. One of `en`, `ar`. Defaults to `en`.
    pub locale: Option<String>,
    /// Bot-check answer; required when `GET /api/v1/captcha` says it is enabled.
    pub captcha_token: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct LoginRequest {
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub password: String,
    /// Bot-check answer; required when `GET /api/v1/captcha` says it is enabled.
    pub captcha_token: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct TokenRequest {
    #[serde(default)]
    pub token: String,
}

#[derive(Deserialize, ToSchema)]
pub struct PasswordResetRequest {
    #[serde(default)]
    pub email: String,
    pub locale: Option<String>,
    /// Bot-check answer; required when `GET /api/v1/captcha` says it is enabled.
    pub captcha_token: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct PasswordResetConfirm {
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub new_password: String,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RoleDto {
    Moderator,
    Admin,
}

#[derive(Serialize, ToSchema)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    /// Shown verbatim; never machine-translated.
    pub display_name: String,
    pub roles: Vec<RoleDto>,
    pub created_at: DateTime<Utc>,
}

impl From<auth::UserView> for User {
    fn from(u: auth::UserView) -> Self {
        Self {
            id: u.id,
            email: u.email,
            display_name: u.display_name,
            roles: u
                .roles
                .into_iter()
                .map(|r| match r {
                    domain::Role::Moderator => RoleDto::Moderator,
                    domain::Role::Admin => RoleDto::Admin,
                })
                .collect(),
            created_at: u.created_at,
        }
    }
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PageQuery {
    /// Opaque cursor from a previous page's `next_cursor`.
    pub cursor: Option<String>,
    /// Page size, 1–100 (default 25).
    pub limit: Option<i64>,
}

#[derive(Serialize, ToSchema)]
pub struct AuditEvent {
    pub id: Uuid,
    pub occurred_at: DateTime<Utc>,
    pub actor_id: Option<Uuid>,
    pub action: String,
    pub target_type: String,
    pub target_id: Option<Uuid>,
    pub request_id: Option<String>,
}

/// Cursor pagination envelope used by every list endpoint.
#[derive(Serialize, ToSchema)]
pub struct AuditEventPage {
    pub items: Vec<AuditEvent>,
    /// `null` on the last page.
    pub next_cursor: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct Health {
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub migration_version: Option<i64>,
}

// ---------- probes ----------

/// Liveness: the process is up. No dependencies checked.
#[utoipa::path(get, path = "/healthz", tag = "ops", responses((status = 200, body = Health)))]
pub async fn healthz() -> Json<Health> {
    Json(Health {
        status: "ok",
        migration_version: None,
    })
}

/// Readiness: database reachable and schema exactly matches this build's migrations.
#[utoipa::path(get, path = "/readyz", tag = "ops", responses(
    (status = 200, body = Health),
    (status = 503, body = Problem, content_type = "application/problem+json")))]
pub async fn readyz(State(app): State<App>) -> Response {
    match migration_state(&app.pool).await {
        Ok(MigrationState::UpToDate { version }) => Json(Health {
            status: "ready",
            migration_version: Some(version),
        })
        .into_response(),
        Ok(state) => {
            let mut e = ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "migrations_not_current", "Not ready");
            e.0.detail = Some(format!("{state:?}"));
            e.into_response()
        }
        Err(err) => {
            tracing::warn!(error = %err, "readiness check failed");
            ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable", "Not ready").into_response()
        }
    }
}

// ---------- auth ----------

pub(crate) fn session_cookie(app: &App, value: String, max_age_hours: i64) -> Cookie<'static> {
    Cookie::build((SESSION_COOKIE, value))
        .path("/")
        .http_only(true)
        .secure(app.config.cookie_secure)
        .same_site(SameSite::Lax)
        .max_age(time_hours(max_age_hours))
        .build()
}

fn time_hours(h: i64) -> time::Duration {
    time::Duration::hours(h)
}

/// Always `202`, whether or not the email is already registered (prevents account enumeration).
#[utoipa::path(post, path = "/api/v1/auth/register", tag = "auth", request_body = RegisterRequest, responses(
    (status = 202, description = "Verification email queued if the address is new"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn register(
    State(app): State<App>,
    Meta(meta): Meta,
    headers: axum::http::HeaderMap,
    ApiJson(b): ApiJson<RegisterRequest>,
) -> ApiResult<StatusCode> {
    crate::captcha::check(&app, &headers, b.captcha_token.as_deref()).await?;
    auth::register(
        &app,
        &meta,
        auth::RegisterInput {
            email: b.email,
            display_name: b.display_name,
            password: b.password,
            locale: b.locale,
        },
    )
    .await?;
    Ok(StatusCode::ACCEPTED)
}

#[utoipa::path(post, path = "/api/v1/auth/verify-email", tag = "auth", request_body = TokenRequest, responses(
    (status = 204, description = "Email verified"),
    (status = 400, body = Problem, content_type = "application/problem+json")))]
pub async fn verify_email(
    State(app): State<App>,
    Meta(meta): Meta,
    ApiJson(b): ApiJson<TokenRequest>,
) -> ApiResult<StatusCode> {
    auth::verify_email(&app, &meta, &b.token).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Sets the `lyrics_session` HttpOnly cookie.
#[utoipa::path(post, path = "/api/v1/auth/login", tag = "auth", request_body = LoginRequest, responses(
    (status = 200, body = User),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn login(
    State(app): State<App>,
    Meta(meta): Meta,
    jar: CookieJar,
    headers: axum::http::HeaderMap,
    ApiJson(b): ApiJson<LoginRequest>,
) -> ApiResult<(CookieJar, Json<User>)> {
    crate::captcha::check(&app, &headers, b.captcha_token.as_deref()).await?;
    let s = auth::login(
        &app,
        &meta,
        auth::LoginInput {
            email: b.email,
            password: b.password,
        },
    )
    .await?;
    let ttl = app.config.session_ttl_hours;
    Ok((jar.add(session_cookie(&app, s.token, ttl)), Json(s.user.into())))
}

#[utoipa::path(post, path = "/api/v1/auth/logout", tag = "auth", security(("session" = [])), responses(
    (status = 204, description = "Session revoked"),
    (status = 401, body = Problem, content_type = "application/problem+json")))]
pub async fn logout(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireUser(p): RequireUser,
    jar: CookieJar,
) -> ApiResult<(CookieJar, StatusCode)> {
    auth::logout(&app, &meta, &p).await?;
    Ok((jar.add(session_cookie(&app, String::new(), 0)), StatusCode::NO_CONTENT))
}

/// Always `202` (prevents account enumeration).
#[utoipa::path(post, path = "/api/v1/auth/password-reset", tag = "auth", request_body = PasswordResetRequest, responses(
    (status = 202, description = "Reset email queued if the account exists"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn request_password_reset(
    State(app): State<App>,
    Meta(meta): Meta,
    headers: axum::http::HeaderMap,
    ApiJson(b): ApiJson<PasswordResetRequest>,
) -> ApiResult<StatusCode> {
    crate::captcha::check(&app, &headers, b.captcha_token.as_deref()).await?;
    auth::request_password_reset(&app, &meta, &b.email, b.locale.as_deref()).await?;
    Ok(StatusCode::ACCEPTED)
}

/// Sets a new password and revokes every session of the account.
#[utoipa::path(post, path = "/api/v1/auth/password-reset/confirm", tag = "auth", request_body = PasswordResetConfirm, responses(
    (status = 204, description = "Password changed"),
    (status = 400, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn confirm_password_reset(
    State(app): State<App>,
    Meta(meta): Meta,
    ApiJson(b): ApiJson<PasswordResetConfirm>,
) -> ApiResult<StatusCode> {
    auth::confirm_password_reset(&app, &meta, &b.token, &b.new_password).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(get, path = "/api/v1/me", tag = "account", security(("session" = [])), responses(
    (status = 200, body = User),
    (status = 401, body = Problem, content_type = "application/problem+json")))]
pub async fn me(State(app): State<App>, RequireUser(p): RequireUser) -> ApiResult<Json<User>> {
    Ok(Json(auth::me(&app, &p).await?.into()))
}

// ---------- admin ----------

#[utoipa::path(get, path = "/api/v1/admin/audit-events", tag = "admin", params(PageQuery),
    security(("session" = [])), responses(
    (status = 200, body = AuditEventPage),
    (status = 400, body = Problem, content_type = "application/problem+json"),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn list_audit_events(
    State(app): State<App>,
    RequireRole(p, ..): RequireRole<Admin>,
    ApiQuery(q): ApiQuery<PageQuery>,
) -> ApiResult<Json<AuditEventPage>> {
    let page = admin::list_audit(&app, &p, q.cursor.as_deref(), q.limit).await?;
    Ok(Json(AuditEventPage {
        items: page
            .items
            .into_iter()
            .map(|r| AuditEvent {
                id: r.id,
                occurred_at: r.occurred_at,
                actor_id: r.actor_id,
                action: r.action,
                target_type: r.target_type,
                target_id: r.target_id,
                request_id: r.request_id,
            })
            .collect(),
        next_cursor: page.next_cursor,
    }))
}

pub async fn not_found() -> ApiError {
    ApiError::new(StatusCode::NOT_FOUND, "not_found", "Not found")
}

pub async fn method_not_allowed() -> ApiError {
    ApiError::new(
        StatusCode::METHOD_NOT_ALLOWED,
        "method_not_allowed",
        "Method not allowed",
    )
}

// ---------- user administration ----------

#[derive(Serialize, ToSchema)]
pub struct AdminUser {
    pub id: Uuid,
    pub email: String,
    /// Shown verbatim; never machine-translated.
    pub display_name: String,
    /// `unverified`, `active` or `disabled`.
    pub status: String,
    /// Elevated roles only (`moderator`, `admin`); every user is implicitly a member.
    pub roles: Vec<String>,
    pub created_at: DateTime<Utc>,
}

impl From<admin::AdminUser> for AdminUser {
    fn from(u: admin::AdminUser) -> Self {
        Self {
            id: u.id,
            email: u.email,
            display_name: u.display_name,
            status: u.status,
            roles: u.roles,
            created_at: u.created_at,
        }
    }
}

#[derive(Serialize, ToSchema)]
pub struct AdminUserPage {
    pub items: Vec<AdminUser>,
    pub next_cursor: Option<String>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct UserQuery {
    /// Case-insensitive substring of email or display name.
    pub q: Option<String>,
    pub cursor: Option<String>,
    /// Page size, 1–100 (default 25).
    pub limit: Option<i64>,
}

#[utoipa::path(get, path = "/api/v1/admin/users", tag = "admin", params(UserQuery),
    security(("session" = [])), responses(
    (status = 200, body = AdminUserPage),
    (status = 400, body = Problem, content_type = "application/problem+json"),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 403, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn list_users(
    State(app): State<App>,
    RequireRole(p, ..): RequireRole<Admin>,
    ApiQuery(q): ApiQuery<UserQuery>,
) -> ApiResult<Json<AdminUserPage>> {
    let page = admin::list_users(&app, &p, q.q.as_deref(), q.cursor.as_deref(), q.limit).await?;
    Ok(Json(AdminUserPage {
        items: page.items.into_iter().map(Into::into).collect(),
        next_cursor: page.next_cursor,
    }))
}

macro_rules! user_change {
    ($name:ident, $method:ident, $path:literal, $doc:literal, |$role:ident| $change:expr, $($pp:tt)*) => {
        #[utoipa::path($method, path = $path, tag = "admin", params($($pp)*),
            security(("session" = [])), description = $doc, responses(
            (status = 200, body = AdminUser),
            (status = 401, body = Problem, content_type = "application/problem+json"),
            (status = 403, body = Problem, content_type = "application/problem+json"),
            (status = 404, body = Problem, content_type = "application/problem+json"),
            (status = 409, body = Problem, content_type = "application/problem+json", description = "`own_account`: admins cannot change their own account"),
            (status = 422, body = Problem, content_type = "application/problem+json")))]
        pub async fn $name(
            State(app): State<App>,
            Meta(meta): Meta,
            RequireRole(p, ..): RequireRole<Admin>,
            ApiPath(path): ApiPath<UserRolePath>,
        ) -> ApiResult<Json<AdminUser>> {
            let $role = path.role.as_deref().map(admin::parse_role).transpose()?;
            Ok(Json(admin::change_user(&app, &p, &meta, path.id, $change).await?.into()))
        }
    };
}

#[derive(Deserialize)]
pub struct UserRolePath {
    pub id: Uuid,
    pub role: Option<String>,
}

user_change!(
    grant_user_role,
    put,
    "/api/v1/admin/users/{id}/roles/{role}",
    "Grants `moderator` or `admin`. Idempotent; audited.",
    |role| admin::Change::Grant(role.expect("route has role")),
    ("id" = Uuid, Path),
    ("role" = String, Path)
);
user_change!(
    revoke_user_role,
    delete,
    "/api/v1/admin/users/{id}/roles/{role}",
    "Revokes a role. Idempotent; audited.",
    |role| admin::Change::Revoke(role.expect("route has role")),
    ("id" = Uuid, Path),
    ("role" = String, Path)
);
user_change!(
    disable_user,
    post,
    "/api/v1/admin/users/{id}/disable",
    "Disables the account and ends all its sessions. Idempotent; audited.",
    |_role| admin::Change::Disable,
    ("id" = Uuid, Path)
);
user_change!(
    enable_user,
    post,
    "/api/v1/admin/users/{id}/enable",
    "Re-enables a disabled account. Idempotent; audited.",
    |_role| admin::Change::Enable,
    ("id" = Uuid, Path)
);
