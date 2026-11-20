//! First-admin bootstrap and Google sign-in (with a fake identity provider; no network).

mod common;

use application::oauth::{self, IdentityClaims, IdentityProvider};
use async_trait::async_trait;
use axum::http::{header, StatusCode};
use axum::Router;
use common::*;
use serde_json::json;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Fake(Mutex<HashMap<String, IdentityClaims>>);

impl Fake {
    fn grant(&self, code: &str, sub: &str, email: &str, verified: bool) {
        self.0.lock().unwrap().insert(
            code.into(),
            IdentityClaims {
                subject: sub.into(),
                email: email.into(),
                email_verified: verified,
                name: Some("Gina Google".into()),
            },
        );
    }
}

#[async_trait]
impl IdentityProvider for Fake {
    fn client_id(&self) -> &str {
        "test-client"
    }
    async fn exchange(
        &self,
        code: &str,
        verifier: &str,
        redirect: &str,
        nonce: &str,
    ) -> anyhow::Result<IdentityClaims> {
        anyhow::ensure!(!verifier.is_empty() && !nonce.is_empty() && redirect.ends_with("/auth/google/callback"));
        self.0
            .lock()
            .unwrap()
            .get(code)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("bad code"))
    }
}

fn setup(pool: &PgPool) -> (Router, Arc<Fake>) {
    let fake = Arc::new(Fake::default());
    let app = app_with(pool.clone(), |c| {
        c.google_client_id = Some("test-client".into());
        c.google_client_secret = Some("secret".into());
    })
    .with_google(fake.clone());
    (api::router(app), fake)
}

struct Started {
    state: String,
    cookie: String,
    location: String,
}

async fn start(r: &Router) -> Started {
    let res = get(r, "/api/v1/auth/google/start?locale=ar", None).await;
    assert_eq!(res.status, StatusCode::SEE_OTHER);
    let location = res.headers[header::LOCATION].to_str().unwrap().to_owned();
    let url = url::Url::parse(&location).unwrap();
    let state = url.query_pairs().find(|(k, _)| k == "state").unwrap().1.into_owned();
    let cookie = res
        .headers
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap())
        .find(|c| c.starts_with("lyrics_oauth_state="))
        .unwrap();
    assert!(cookie.contains("HttpOnly") && cookie.contains("Path=/api/v1/auth/google"));
    Started {
        state,
        cookie: cookie.split(';').next().unwrap().into(),
        location,
    }
}

async fn finish(r: &Router, state: &str, code: &str, cookie: Option<&str>) -> Res {
    get(
        r,
        &format!("/api/v1/auth/google/callback?code={code}&state={state}"),
        cookie,
    )
    .await
}

fn error_of(res: &Res) -> String {
    assert_eq!(res.status, StatusCode::SEE_OTHER);
    assert!(
        res.session_cookie().is_none_or(|c| c == "lyrics_session="),
        "no session may be issued"
    );
    let loc = url::Url::parse(res.headers[header::LOCATION].to_str().unwrap()).unwrap();
    assert!(loc.path().ends_with("/login"), "{loc}");
    loc.query_pairs().find(|(k, _)| k == "error").unwrap().1.into_owned()
}

async fn count(pool: &PgPool, sql: &str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
}

// ---------- first admin ----------

fn admin_app(pool: &PgPool, email: &str, password: &str) -> application::App {
    let (email, password) = (email.to_owned(), password.to_owned());
    app_with(pool.clone(), move |c| {
        c.admin_email = Some(email);
        c.admin_password = Some(password);
    })
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn bootstrap_creates_admin_who_can_sign_in_and_is_idempotent(pool: PgPool) {
    let app = admin_app(&pool, "Boss@Example.com", "a very long admin password");
    oauth::bootstrap_admin(&app).await.unwrap();
    oauth::bootstrap_admin(&app).await.unwrap();
    assert_eq!(count(&pool, "SELECT count(*) FROM users").await, 1);
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM audit_log WHERE action = 'user.admin_bootstrapped'"
        )
        .await,
        1
    );

    let r = api::router(app);
    let res = post(
        &r,
        "/api/v1/auth/login",
        json!({"email":"boss@example.com","password":"a very long admin password"}),
        None,
    )
    .await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.body);
    assert_eq!(res.body["roles"], json!(["admin"]));
    let cookie = res.session_cookie().unwrap();
    assert_eq!(
        get(&r, "/api/v1/admin/audit-events", Some(&cookie)).await.status,
        StatusCode::OK,
        "admin-only endpoint"
    );
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn bootstrap_promotes_existing_user_without_touching_their_password(pool: PgPool) {
    let r = router(&pool);
    let (_, _) = signed_in(&pool, &r, "boss@example.com", None).await;
    let app = admin_app(&pool, "boss@example.com", "a different env password");
    oauth::bootstrap_admin(&app).await.unwrap();
    let ok = post(
        &r,
        "/api/v1/auth/login",
        json!({"email":"boss@example.com","password":PASSWORD}),
        None,
    )
    .await;
    assert_eq!(
        ok.body["roles"],
        json!(["admin"]),
        "promoted, original password still valid"
    );
    let env_pw = post(
        &r,
        "/api/v1/auth/login",
        json!({"email":"boss@example.com","password":"a different env password"}),
        None,
    )
    .await;
    env_pw.assert_problem(StatusCode::UNAUTHORIZED, "invalid_credentials");
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn bootstrap_rejects_weak_password_and_concurrent_starts_make_one_admin(pool: PgPool) {
    assert!(oauth::bootstrap_admin(&admin_app(&pool, "boss@example.com", "short"))
        .await
        .is_err());
    assert_eq!(count(&pool, "SELECT count(*) FROM users").await, 0);

    let app = admin_app(&pool, "boss@example.com", "a very long admin password");
    let (a, b, c) = tokio::join!(
        oauth::bootstrap_admin(&app),
        oauth::bootstrap_admin(&app),
        oauth::bootstrap_admin(&app)
    );
    assert!(a.is_ok() && b.is_ok() && c.is_ok());
    assert_eq!(count(&pool, "SELECT count(*) FROM users").await, 1);
    assert_eq!(
        count(&pool, "SELECT count(*) FROM user_roles WHERE role = 'admin'").await,
        1
    );
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn bootstrap_does_not_resurrect_a_disabled_account(pool: PgPool) {
    let r = router(&pool);
    signed_in(&pool, &r, "boss@example.com", None).await;
    sqlx::query("UPDATE users SET status = 'disabled'")
        .execute(&pool)
        .await
        .unwrap();
    oauth::bootstrap_admin(&admin_app(&pool, "boss@example.com", "a very long admin password"))
        .await
        .unwrap();
    assert_eq!(count(&pool, "SELECT count(*) FROM user_roles").await, 0);
}

// ---------- google sign-in ----------

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn disabled_when_not_configured(pool: PgPool) {
    let r = router(&pool);
    assert_eq!(
        get(&r, "/api/v1/auth/providers", None).await.body,
        json!({"google": false})
    );
    get(&r, "/api/v1/auth/google/start", None)
        .await
        .assert_problem(StatusCode::NOT_FOUND, "not_found");
    assert_eq!(
        error_of(&get(&r, "/api/v1/auth/google/callback?code=x&state=y", None).await),
        "google_failed"
    );
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn new_google_user_is_created_active_and_signed_in(pool: PgPool) {
    let (r, fake) = setup(&pool);
    assert_eq!(
        get(&r, "/api/v1/auth/providers", None).await.body,
        json!({"google": true})
    );
    let s = start(&r).await;
    assert!(s.location.starts_with("https://accounts.google.com/"));
    assert!(s.location.contains("code_challenge_method=S256") && s.location.contains("client_id=test-client"));

    fake.grant("c1", "sub-1", "gina@example.com", true);
    let res = finish(&r, &s.state, "c1", Some(&s.cookie)).await;
    assert_eq!(res.status, StatusCode::SEE_OTHER);
    assert!(
        res.headers[header::LOCATION].to_str().unwrap().ends_with("/ar/account"),
        "locale carried through"
    );
    let cookie = res.session_cookie().expect("session issued");
    let me = get(&r, "/api/v1/me", Some(&cookie)).await;
    assert_eq!(me.body["email"], "gina@example.com");
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM users WHERE status = 'active' AND email_verified_at IS NOT NULL"
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM audit_log WHERE action = 'auth.google_registered'"
        )
        .await,
        1
    );

    // Same Google account again: same user, no duplicate.
    let s2 = start(&r).await;
    fake.grant("c2", "sub-1", "gina@example.com", true);
    assert!(finish(&r, &s2.state, "c2", Some(&s2.cookie))
        .await
        .session_cookie()
        .is_some());
    assert_eq!(count(&pool, "SELECT count(*) FROM users").await, 1);

    // A Google-only user has no usable password.
    let bad = post(
        &r,
        "/api/v1/auth/login",
        json!({"email":"gina@example.com","password":PASSWORD}),
        None,
    )
    .await;
    bad.assert_problem(StatusCode::UNAUTHORIZED, "invalid_credentials");
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn state_is_bound_to_the_browser_and_single_use(pool: PgPool) {
    let (r, fake) = setup(&pool);
    fake.grant("c", "sub-9", "x@example.com", true);
    let s = start(&r).await;
    // Missing cookie, or a cookie from a different flow: login-CSRF attempt.
    assert_eq!(error_of(&finish(&r, &s.state, "c", None).await), "google_failed");
    let other = start(&r).await;
    assert_eq!(
        error_of(&finish(&r, &s.state, "c", Some(&other.cookie)).await),
        "google_failed"
    );
    // Unknown state.
    assert_eq!(
        error_of(&finish(&r, "nope", "c", Some("lyrics_oauth_state=nope")).await),
        "google_failed"
    );
    assert_eq!(count(&pool, "SELECT count(*) FROM users").await, 0);
    // Success once, replay fails.
    assert!(finish(&r, &s.state, "c", Some(&s.cookie))
        .await
        .session_cookie()
        .is_some());
    assert_eq!(
        error_of(&finish(&r, &s.state, "c", Some(&s.cookie)).await),
        "google_failed"
    );
    // Expired state.
    let e = start(&r).await;
    sqlx::query("UPDATE oauth_states SET expires_at = now() - interval '1 minute'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        error_of(&finish(&r, &e.state, "c", Some(&e.cookie)).await),
        "google_failed"
    );
    // User denied at Google.
    assert_eq!(
        error_of(&get(&r, "/api/v1/auth/google/callback?error=access_denied", None).await),
        "google_denied"
    );
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn unverified_google_email_is_refused(pool: PgPool) {
    let (r, fake) = setup(&pool);
    fake.grant("c", "sub-2", "x@example.com", false);
    let s = start(&r).await;
    assert_eq!(
        error_of(&finish(&r, &s.state, "c", Some(&s.cookie)).await),
        "google_email_unverified"
    );
    assert_eq!(count(&pool, "SELECT count(*) FROM users").await, 0);
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn existing_active_account_is_linked_not_duplicated(pool: PgPool) {
    let (r, fake) = setup(&pool);
    let (uid, _) = signed_in(&pool, &r, "same@example.com", Some(domain::Role::Moderator)).await;
    fake.grant("c", "sub-3", "same@example.com", true);
    let s = start(&r).await;
    let res = finish(&r, &s.state, "c", Some(&s.cookie)).await;
    let me = get(&r, "/api/v1/me", res.session_cookie().as_deref()).await;
    assert_eq!(me.body["id"], uid.to_string());
    assert_eq!(me.body["roles"], json!(["moderator"]));
    assert_eq!(count(&pool, "SELECT count(*) FROM users").await, 1);
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM audit_log WHERE action = 'auth.google_linked'"
        )
        .await,
        1
    );
    // The password still works: linking adds a method, it does not remove one.
    let pw = post(
        &r,
        "/api/v1/auth/login",
        json!({"email":"same@example.com","password":PASSWORD}),
        None,
    )
    .await;
    assert_eq!(pw.status, StatusCode::OK);
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn squatted_unverified_registration_is_taken_over_by_the_google_owner(pool: PgPool) {
    let (r, fake) = setup(&pool);
    assert_eq!(register(&r, "victim@example.com").await.status, StatusCode::ACCEPTED);
    let squat_token = token_from_outbox(&pool, "email.verify_requested", "victim@example.com").await;
    fake.grant("c", "sub-4", "victim@example.com", true);
    let s = start(&r).await;
    assert!(finish(&r, &s.state, "c", Some(&s.cookie))
        .await
        .session_cookie()
        .is_some());
    // The squatter's password is dead and their verify link no longer works.
    post(
        &r,
        "/api/v1/auth/login",
        json!({"email":"victim@example.com","password":PASSWORD}),
        None,
    )
    .await
    .assert_problem(StatusCode::UNAUTHORIZED, "invalid_credentials");
    post(&r, "/api/v1/auth/verify-email", json!({"token": squat_token}), None)
        .await
        .assert_problem(StatusCode::BAD_REQUEST, "invalid_token");
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn disabled_account_cannot_sign_in_with_google(pool: PgPool) {
    let (r, fake) = setup(&pool);
    signed_in(&pool, &r, "off@example.com", None).await;
    sqlx::query("UPDATE users SET status = 'disabled'")
        .execute(&pool)
        .await
        .unwrap();
    fake.grant("c", "sub-5", "off@example.com", true);
    let s = start(&r).await;
    assert_eq!(
        error_of(&finish(&r, &s.state, "c", Some(&s.cookie)).await),
        "google_disabled"
    );
    assert_eq!(count(&pool, "SELECT count(*) FROM external_identities").await, 0);
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn concurrent_callbacks_for_one_new_google_user_create_one_account(pool: PgPool) {
    let (r, fake) = setup(&pool);
    fake.grant("c", "sub-6", "race@example.com", true);
    let (a, b) = (start(&r).await, start(&r).await);
    let (ra, rb) = tokio::join!(
        finish(&r, &a.state, "c", Some(&a.cookie)),
        finish(&r, &b.state, "c", Some(&b.cookie))
    );
    // At least one wins; the loser may be told to retry, but never produces a duplicate or a 500.
    assert!(ra.session_cookie().is_some() || rb.session_cookie().is_some());
    assert_eq!(count(&pool, "SELECT count(*) FROM users").await, 1);
    assert_eq!(count(&pool, "SELECT count(*) FROM external_identities").await, 1);
}
