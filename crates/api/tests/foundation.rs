//! Ops, conventions and schema: probes, migrate-from-zero, request ids, problem+json,
//! CSRF, config fail-fast, OpenAPI drift.

mod common;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use common::*;
use infrastructure::db::{migration_state, MigrationState, MIGRATOR};
use serde_json::json;
use sqlx::PgPool;

#[sqlx::test(migrations = false)]
async fn migrate_from_zero_then_ready(pool: PgPool) {
    let r = router(&pool);
    // Empty database: not ready.
    let res = get(&r, "/readyz", None).await;
    res.assert_problem(StatusCode::SERVICE_UNAVAILABLE, "migrations_not_current");
    assert!(matches!(
        migration_state(&pool).await.unwrap(),
        MigrationState::Pending { applied: None, .. }
    ));

    MIGRATOR.run(&pool).await.unwrap();
    let expected = MIGRATOR.iter().map(|m| m.version).max().unwrap();
    assert_eq!(
        migration_state(&pool).await.unwrap(),
        MigrationState::UpToDate { version: expected }
    );
    let res = get(&r, "/readyz", None).await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.body["migration_version"], expected);

    // Re-running is a no-op (idempotent deploys).
    MIGRATOR.run(&pool).await.unwrap();
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn readyz_detects_failed_migration(pool: PgPool) {
    sqlx::query("UPDATE _sqlx_migrations SET success = false WHERE version = 2")
        .execute(&pool)
        .await
        .unwrap();
    get(&router(&pool), "/readyz", None)
        .await
        .assert_problem(StatusCode::SERVICE_UNAVAILABLE, "migrations_not_current");
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn healthz_and_request_id_propagation(pool: PgPool) {
    let r = router(&pool);
    let res = get(&r, "/healthz", None).await;
    assert_eq!(res.status, StatusCode::OK);
    assert!(res.headers.contains_key("x-request-id"));

    let req = Request::get("/healthz")
        .header("x-request-id", "edge-abc.123")
        .body(Body::empty())
        .unwrap();
    assert_eq!(raw(&r, req).await.headers["x-request-id"], "edge-abc.123");

    // Unsafe inbound ids are replaced, not echoed into logs.
    let req = Request::get("/healthz")
        .header("x-request-id", "a b")
        .body(Body::empty())
        .unwrap();
    assert_ne!(raw(&r, req).await.headers["x-request-id"], "a b");
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn unknown_route_and_method_are_problem_json(pool: PgPool) {
    let r = router(&pool);
    get(&r, "/api/v1/nope", None)
        .await
        .assert_problem(StatusCode::NOT_FOUND, "not_found");
    let res = send(&r, Method::DELETE, "/api/v1/me", None, None).await;
    res.assert_problem(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed");
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn body_errors_are_problem_json(pool: PgPool) {
    let r = router(&pool);
    let req = |ct: Option<&str>, body: &str| {
        let mut b = Request::post("/api/v1/auth/login").header("x-requested-with", "t");
        if let Some(ct) = ct {
            b = b.header(header::CONTENT_TYPE, ct);
        }
        b.body(Body::from(body.to_owned())).unwrap()
    };
    raw(&r, req(Some("application/json"), "{nope"))
        .await
        .assert_problem(StatusCode::BAD_REQUEST, "malformed_json");
    raw(&r, req(None, "{}"))
        .await
        .assert_problem(StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported_media_type");
    raw(&r, req(Some("application/json"), r#"{"email": 5}"#))
        .await
        .assert_problem(StatusCode::BAD_REQUEST, "invalid_body");
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn validation_reports_every_field(pool: PgPool) {
    let r = router(&pool);
    let res = post(
        &r,
        "/api/v1/auth/register",
        json!({ "email": "bad", "display_name": "", "password": "short", "locale": "xx" }),
        None,
    )
    .await;
    res.assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    let mut got: Vec<(String, String)> = res.body["errors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| (e["field"].as_str().unwrap().into(), e["code"].as_str().unwrap().into()))
        .collect();
    got.sort();
    let want = [
        ("display_name", "required"),
        ("email", "invalid_email"),
        ("locale", "unsupported_locale"),
        ("password", "too_short"),
    ];
    assert_eq!(got, want.map(|(a, b)| (a.to_owned(), b.to_owned())));

    // Missing fields are field errors too, not deserialization failures.
    let res = post(&r, "/api/v1/auth/register", json!({}), None).await;
    assert_eq!(res.body["errors"].as_array().unwrap().len(), 3);
    let users: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(users, 0);
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn state_changing_requests_require_csrf_header(pool: PgPool) {
    let r = router(&pool);
    let req = Request::post("/api/v1/auth/register")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({ "email": "a@b.co", "display_name": "A", "password": PASSWORD }).to_string(),
        ))
        .unwrap();
    raw(&r, req)
        .await
        .assert_problem(StatusCode::FORBIDDEN, "csrf_header_missing");
    let users: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(users, 0, "rejected before reaching the handler");
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn audit_log_rejects_update_and_delete(pool: PgPool) {
    let r = router(&pool);
    register(&r, "a@b.co").await;
    for sql in [
        "UPDATE audit_log SET action = 'x.y'",
        "DELETE FROM audit_log",
        "TRUNCATE audit_log",
    ] {
        let err = sqlx::query(sql).execute(&pool).await.unwrap_err();
        assert!(err.to_string().contains("append-only"), "{sql}: {err}");
    }
}

#[test]
fn binary_refuses_to_start_with_invalid_config() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_lyrics-api"))
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("LYRICS_DATABASE_URL", "postgres://u:p@localhost/erp_dev")
        .env("LYRICS_BIND_ADDR", "nope")
        .current_dir(std::env::temp_dir()) // no .env pickup
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(78));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("must name a database starting with 'lyrics_'"), "{err}");
    assert!(err.contains("LYRICS_APP_BASE_URL is required"), "{err}");
    assert!(err.contains("LYRICS_BIND_ADDR has an invalid value"), "{err}");
}

/// The committed spec is what the frontend client is generated from; it must match the code.
/// Regenerate with `UPDATE_OPENAPI=1 cargo test -p api --test foundation openapi`.
#[test]
fn openapi_spec_is_committed_and_current() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../openapi/v1.json");
    let generated = api::openapi_json() + "\n";
    if std::env::var("UPDATE_OPENAPI").is_ok() {
        std::fs::write(path, &generated).unwrap();
    }
    let committed = std::fs::read_to_string(path).unwrap_or_default();
    assert!(
        committed == generated,
        "openapi/v1.json is stale; rerun with UPDATE_OPENAPI=1"
    );
}
