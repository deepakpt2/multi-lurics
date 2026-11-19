//! Request extractors. Identity and roles come only from the server-side session,
//! never from request bodies, query strings or client-supplied headers.

use crate::middleware::RequestId;
use crate::problem::ApiError;
use application::{auth, authz, App, AppError, RequestMeta};
use axum::extract::{FromRequest, FromRequestParts};
use axum::http::header::USER_AGENT;
use axum::http::request::Parts;
use axum_extra::extract::CookieJar;
use domain::{Principal, Role};
use std::convert::Infallible;
use std::marker::PhantomData;

pub const SESSION_COOKIE: &str = "lyrics_session";

/// JSON body whose rejections become problem+json.
#[derive(FromRequest)]
#[from_request(via(axum::Json), rejection(ApiError))]
pub struct ApiJson<T>(pub T);

/// Query string whose rejections become problem+json.
#[derive(FromRequestParts)]
#[from_request(via(axum::extract::Query), rejection(ApiError))]
pub struct ApiQuery<T>(pub T);

/// Audit/trace metadata for the current request.
pub struct Meta(pub RequestMeta);

impl<S: Send + Sync> FromRequestParts<S> for Meta {
    type Rejection = Infallible;
    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Infallible> {
        Ok(Self(RequestMeta {
            request_id: parts.extensions.get::<RequestId>().map(|r| r.0.clone()),
            user_agent: parts
                .headers
                .get(USER_AGENT)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned),
        }))
    }
}

/// Any signed-in, active user. Rejects with `401 unauthenticated`.
pub struct RequireUser(pub Principal);

impl FromRequestParts<App> for RequireUser {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, app: &App) -> Result<Self, ApiError> {
        if let Some(p) = parts.extensions.get::<Principal>() {
            return Ok(Self(p.clone()));
        }
        let jar = CookieJar::from_headers(&parts.headers);
        let token = jar
            .get(SESSION_COOKIE)
            .map(|c| c.value().to_owned())
            .unwrap_or_default();
        let principal = auth::authenticate(app, &token)
            .await?
            .ok_or(AppError::Unauthenticated)?;
        parts.extensions.insert(principal.clone());
        Ok(Self(principal))
    }
}

pub trait RoleMarker: Send + Sync + 'static {
    const ROLE: Role;
}

pub struct Admin;
impl RoleMarker for Admin {
    const ROLE: Role = Role::Admin;
}

pub struct Moderator;
impl RoleMarker for Moderator {
    const ROLE: Role = Role::Moderator;
}

/// Signed-in user holding role `R` (admin implies all). `401` if anonymous, `403` if lacking the role.
pub struct RequireRole<R: RoleMarker>(pub Principal, pub PhantomData<R>);

impl<R: RoleMarker> FromRequestParts<App> for RequireRole<R> {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, app: &App) -> Result<Self, ApiError> {
        let RequireUser(p) = RequireUser::from_request_parts(parts, app).await?;
        authz::require_role(&p, R::ROLE)?;
        Ok(Self(p, PhantomData))
    }
}
