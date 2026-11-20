//! The bot check on public forms: off unless configured, then required on sign-up, sign-in,
//! password reset and legal notices; unreachable verifier fails closed.

mod common;

use application::captcha::{Turnstile, Verifier};
use async_trait::async_trait;
use axum::http::StatusCode;
use axum::Router;
use common::*;
use serde_json::json;
use sqlx::PgPool;
use std::sync::Arc;

struct Fake;

#[async_trait]
impl Verifier for Fake {
    async fn verify(&self, token: &str, _ip: Option<&str>) -> Result<bool, anyhow::Error> {
        match token {
            "good" => Ok(true),
            "down" => Err(anyhow::anyhow!("verifier unreachable")),
            _ => Ok(false),
        }
    }
}

fn protected(pool: &PgPool) -> Router {
    let app = app_with(pool.clone(), |c| {
        c.turnstile_site_key = Some("site-key".into());
        c.turnstile_secret_key = Some("secret".into());
    })
    .with_captcha(Arc::new(Fake));
    api::router(app)
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn off_by_default_and_forms_need_no_token(pool: PgPool) {
    let r = router(&pool);
    let c = get(&r, "/api/v1/captcha", None).await;
    assert_eq!(c.body, json!({"enabled": false, "site_key": null}));
    assert_eq!(register(&r, "a@example.com").await.status, StatusCode::ACCEPTED);
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn enabled_forms_need_a_genuine_answer(pool: PgPool) {
    let r = protected(&pool);
    let c = get(&r, "/api/v1/captcha", None).await;
    assert_eq!(c.body, json!({"enabled": true, "site_key": "site-key"}));
    assert!(!c.body.to_string().contains("secret"));

    let reg = |token: Option<&str>| json!({"email": "a@example.com", "display_name": "A", "password": PASSWORD, "captcha_token": token});
    // Missing, wrong, and unreachable verifier are all refused.
    let missing = post(&r, "/api/v1/auth/register", reg(None), None).await;
    assert_eq!(missing.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        missing.body["errors"][0],
        json!({"field": "captcha", "code": "captcha_required"})
    );
    let wrong = post(&r, "/api/v1/auth/register", reg(Some("bad")), None).await;
    assert_eq!(
        wrong.body["errors"][0],
        json!({"field": "captcha", "code": "captcha_failed"})
    );
    assert_eq!(
        post(&r, "/api/v1/auth/register", reg(Some("down")), None).await.status,
        StatusCode::SERVICE_UNAVAILABLE
    );
    let none: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(none, 0, "nothing was created by refused attempts");
    assert_eq!(
        post(&r, "/api/v1/auth/register", reg(Some("good")), None).await.status,
        StatusCode::ACCEPTED
    );

    // Sign-in, password reset and legal notices ask too.
    let login = |t: Option<&str>| json!({"email": "a@example.com", "password": PASSWORD, "captcha_token": t});
    post(&r, "/api/v1/auth/login", login(None), None)
        .await
        .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    assert_ne!(
        post(&r, "/api/v1/auth/login", login(Some("good")), None).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    post(
        &r,
        "/api/v1/auth/password-reset",
        json!({"email": "a@example.com"}),
        None,
    )
    .await
    .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    assert_eq!(
        post(
            &r,
            "/api/v1/auth/password-reset",
            json!({"email": "a@example.com", "captcha_token": "good"}),
            None
        )
        .await
        .status,
        StatusCode::ACCEPTED
    );
    post(&r, "/api/v1/legal/notices", json!({"kind": "copyright", "contact_name": "x", "contact_email": "x@example.org", "statement": "y", "good_faith": true}), None)
        .await
        .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
}

/// The real verifier sends the secret, the answer and the address to the verification URL.
#[tokio::test]
async fn turnstile_asks_cloudflare_the_documented_way() {
    use axum::extract::Form;
    use axum::routing::post as post_route;
    use std::collections::HashMap;
    let seen = Arc::new(std::sync::Mutex::new(Vec::<HashMap<String, String>>::new()));
    let log = seen.clone();
    let mock = Router::new().route(
        "/siteverify",
        post_route(move |Form(f): Form<HashMap<String, String>>| {
            let log = log.clone();
            async move {
                let ok = f.get("response").map(String::as_str) == Some("good");
                log.lock().unwrap().push(f);
                axum::Json(json!({"success": ok}))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/siteverify", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, mock).await.unwrap() });

    let t = Turnstile::new("the-secret".into(), &url).unwrap();
    assert!(t.verify("good", Some("203.0.113.9")).await.unwrap());
    assert!(!t.verify("nope", None).await.unwrap());
    {
        let seen = seen.lock().unwrap();
        assert_eq!(seen[0]["secret"], "the-secret");
        assert_eq!(seen[0]["response"], "good");
        assert_eq!(seen[0]["remoteip"], "203.0.113.9");
        assert!(!seen[1].contains_key("remoteip"));
    }
    // A verifier that cannot be reached is an error, not a pass.
    let dead = Turnstile::new("s".into(), "http://127.0.0.1:1/siteverify").unwrap();
    assert!(dead.verify("good", None).await.is_err());
}
