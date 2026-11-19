#![allow(dead_code)]

use application::App;
use axum::body::Body;
use axum::http::{header, HeaderMap, Method, Request, StatusCode};
use axum::Router;
use domain::Role;
use http_body_util::BodyExt;
use infrastructure::{repo, Config};
use serde_json::{json, Value};
use sqlx::PgPool;
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

pub const PASSWORD: &str = "correct horse battery staple";

pub fn app(pool: PgPool) -> App {
    let config = Config {
        database_url: "postgres://unused/lyrics_test".into(),
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        app_base_url: "http://localhost:3000/".parse().unwrap(),
        session_ttl_hours: 1,
        cookie_secure: true,
        db_max_connections: 5,
        dev_mailbox_dir: None,
        search_rate_per_min: 600,
    };
    App::new(pool, Arc::new(config))
}

pub fn router(pool: &PgPool) -> Router {
    api::router(app(pool.clone()))
}

pub struct Res {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Value,
}

impl Res {
    pub fn code(&self) -> &str {
        self.body["code"].as_str().unwrap_or("")
    }

    pub fn session_cookie(&self) -> Option<String> {
        self.headers
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .find(|c| c.starts_with("lyrics_session="))
            .map(|c| c.split(';').next().unwrap().to_owned())
    }

    pub fn assert_problem(&self, status: StatusCode, code: &str) {
        assert_eq!(self.status, status, "body: {}", self.body);
        assert_eq!(self.code(), code, "body: {}", self.body);
        assert_eq!(self.headers[header::CONTENT_TYPE], "application/problem+json");
        assert_eq!(self.body["status"], status.as_u16());
        assert_eq!(self.body["type"], format!("/problems/{code}"));
        assert!(self.body["request_id"].is_string(), "problem carries request id");
    }
}

pub async fn send(r: &Router, method: Method, uri: &str, body: Option<Value>, cookie: Option<&str>) -> Res {
    let mut b = Request::builder().method(method.clone()).uri(uri);
    if method != Method::GET {
        b = b.header("x-requested-with", "test");
    }
    if let Some(c) = cookie {
        b = b.header(header::COOKIE, c);
    }
    let req = match body {
        Some(v) => b
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(v.to_string())),
        None => b.body(Body::empty()),
    }
    .unwrap();
    raw(r, req).await
}

pub async fn raw(r: &Router, req: Request<Body>) -> Res {
    let res = r.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    Res { status, headers, body }
}

pub async fn get(r: &Router, uri: &str, cookie: Option<&str>) -> Res {
    send(r, Method::GET, uri, None, cookie).await
}

pub async fn post(r: &Router, uri: &str, body: Value, cookie: Option<&str>) -> Res {
    send(r, Method::POST, uri, Some(body), cookie).await
}

/// Latest single-use token for `email` delivered through the outbox (what the email would contain).
pub async fn token_from_outbox(pool: &PgPool, topic: &str, email: &str) -> String {
    let link: String = sqlx::query_scalar(
        "SELECT payload->>'link' FROM outbox WHERE topic = $1 AND payload->>'to' = $2 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(topic)
    .bind(email)
    .fetch_one(pool)
    .await
    .unwrap();
    link.split_once("#token=").unwrap().1.to_owned()
}

pub async fn register(r: &Router, email: &str) -> Res {
    post(
        r,
        "/api/v1/auth/register",
        json!({ "email": email, "display_name": "Test User", "password": PASSWORD }),
        None,
    )
    .await
}

/// Registers, verifies and optionally grants a role. Returns (user id, session cookie).
pub async fn signed_in(pool: &PgPool, r: &Router, email: &str, role: Option<Role>) -> (Uuid, String) {
    assert_eq!(register(r, email).await.status, StatusCode::ACCEPTED);
    let token = token_from_outbox(pool, "email.verify_requested", email).await;
    assert_eq!(
        post(r, "/api/v1/auth/verify-email", json!({ "token": token }), None)
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    let user_id: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE email = $1")
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap();
    if let Some(role) = role {
        let mut conn = pool.acquire().await.unwrap();
        repo::grant_role(&mut conn, user_id, role, None).await.unwrap();
    }
    let res = post(
        r,
        "/api/v1/auth/login",
        json!({ "email": email, "password": PASSWORD }),
        None,
    )
    .await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.body);
    (user_id, res.session_cookie().expect("session cookie"))
}
