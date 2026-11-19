//! Account lifecycle, sessions, authorization (allow/deny), pagination, concurrency.

mod common;

use axum::http::StatusCode;
use common::*;
use domain::Role;
use serde_json::json;
use sqlx::PgPool;

async fn count(pool: &PgPool, sql: &str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn full_lifecycle_is_audited(pool: PgPool) {
    let r = router(&pool);
    let (user_id, cookie) = signed_in(&pool, &r, "ana@example.com", None).await;

    let me = get(&r, "/api/v1/me", Some(&cookie)).await;
    assert_eq!(me.status, StatusCode::OK);
    assert_eq!(me.body["id"], user_id.to_string());
    assert_eq!(me.body["roles"], json!([]));
    assert!(me.body.get("password_hash").is_none());

    assert_eq!(
        post(&r, "/api/v1/auth/logout", json!({}), Some(&cookie)).await.status,
        StatusCode::NO_CONTENT
    );
    get(&r, "/api/v1/me", Some(&cookie))
        .await
        .assert_problem(StatusCode::UNAUTHORIZED, "unauthenticated");

    let actions: Vec<String> =
        sqlx::query_scalar("SELECT action FROM audit_log WHERE target_id = $1 ORDER BY occurred_at")
            .bind(user_id)
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        actions,
        [
            "user.registered",
            "user.email_verified",
            "auth.session_created",
            "auth.session_revoked"
        ]
    );
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn session_cookie_is_hardened(pool: PgPool) {
    let r = router(&pool);
    signed_in(&pool, &r, "c@example.com", None).await;
    let res = post(
        &r,
        "/api/v1/auth/login",
        json!({ "email": "c@example.com", "password": PASSWORD }),
        None,
    )
    .await;
    let set_cookie = res.headers["set-cookie"].to_str().unwrap().to_owned();
    for attr in ["HttpOnly", "Secure", "SameSite=Lax", "Path=/", "Max-Age=3600"] {
        assert!(set_cookie.contains(attr), "{set_cookie} lacks {attr}");
    }
    let stored: i64 = count(&pool, "SELECT count(*) FROM sessions WHERE length(token_hash) = 32").await;
    assert_eq!(stored, 2, "only hashes are stored");
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn protected_endpoint_rejects_anonymous_and_forged_sessions(pool: PgPool) {
    let r = router(&pool);
    get(&r, "/api/v1/me", None)
        .await
        .assert_problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    get(&r, "/api/v1/me", Some("lyrics_session=forged"))
        .await
        .assert_problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    let long = format!("lyrics_session={}", "a".repeat(500));
    get(&r, "/api/v1/me", Some(&long))
        .await
        .assert_problem(StatusCode::UNAUTHORIZED, "unauthenticated");
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn expired_session_and_disabled_user_are_rejected(pool: PgPool) {
    let r = router(&pool);
    let (uid, cookie) = signed_in(&pool, &r, "d@example.com", None).await;
    sqlx::query("UPDATE sessions SET expires_at = now() - interval '1 second'")
        .execute(&pool)
        .await
        .unwrap();
    get(&r, "/api/v1/me", Some(&cookie))
        .await
        .assert_problem(StatusCode::UNAUTHORIZED, "unauthenticated");

    let res = post(
        &r,
        "/api/v1/auth/login",
        json!({ "email": "d@example.com", "password": PASSWORD }),
        None,
    )
    .await;
    let cookie = res.session_cookie().unwrap();
    sqlx::query("UPDATE users SET status = 'disabled' WHERE id = $1")
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();
    get(&r, "/api/v1/me", Some(&cookie))
        .await
        .assert_problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    post(
        &r,
        "/api/v1/auth/login",
        json!({ "email": "d@example.com", "password": PASSWORD }),
        None,
    )
    .await
    .assert_problem(StatusCode::FORBIDDEN, "forbidden");
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn admin_endpoint_allow_and_deny(pool: PgPool) {
    let r = router(&pool);
    let uri = "/api/v1/admin/audit-events";
    get(&r, uri, None)
        .await
        .assert_problem(StatusCode::UNAUTHORIZED, "unauthenticated");

    let (_, member) = signed_in(&pool, &r, "member@example.com", None).await;
    get(&r, uri, Some(&member))
        .await
        .assert_problem(StatusCode::FORBIDDEN, "forbidden");

    let (_, moderator) = signed_in(&pool, &r, "mod@example.com", Some(Role::Moderator)).await;
    get(&r, uri, Some(&moderator))
        .await
        .assert_problem(StatusCode::FORBIDDEN, "forbidden");

    let (_, admin) = signed_in(&pool, &r, "admin@example.com", Some(Role::Admin)).await;
    let res = get(&r, uri, Some(&admin)).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.body);
    assert!(!res.body["items"].as_array().unwrap().is_empty());
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn role_revocation_takes_effect_on_next_request(pool: PgPool) {
    let r = router(&pool);
    let (uid, admin) = signed_in(&pool, &r, "admin@example.com", Some(Role::Admin)).await;
    assert_eq!(
        get(&r, "/api/v1/admin/audit-events", Some(&admin)).await.status,
        StatusCode::OK
    );
    sqlx::query("DELETE FROM user_roles WHERE user_id = $1")
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();
    get(&r, "/api/v1/admin/audit-events", Some(&admin))
        .await
        .assert_problem(StatusCode::FORBIDDEN, "forbidden");
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn body_claims_cannot_escalate(pool: PgPool) {
    let r = router(&pool);
    let res = post(
        &r,
        "/api/v1/auth/register",
        json!({ "email": "e@example.com", "display_name": "E", "password": PASSWORD,
                "roles": ["admin"], "status": "active", "id": "00000000-0000-0000-0000-000000000001" }),
        None,
    )
    .await;
    assert_eq!(res.status, StatusCode::ACCEPTED);
    let (status, roles): (String, i64) = sqlx::query_as(
        "SELECT u.status, (SELECT count(*) FROM user_roles WHERE user_id = u.id) FROM users u WHERE email = 'e@example.com'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((status.as_str(), roles), ("unverified", 0));
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn cursor_pagination_walks_without_gaps_or_duplicates(pool: PgPool) {
    let r = router(&pool);
    let (_, admin) = signed_in(&pool, &r, "admin@example.com", Some(Role::Admin)).await;
    for i in 0..3 {
        signed_in(&pool, &r, &format!("u{i}@example.com"), None).await;
    }
    let total = count(&pool, "SELECT count(*) FROM audit_log").await;
    let mut seen = Vec::new();
    let mut uri = "/api/v1/admin/audit-events?limit=4".to_owned();
    loop {
        let res = get(&r, &uri, Some(&admin)).await;
        assert_eq!(res.status, StatusCode::OK, "{}", res.body);
        seen.extend(
            res.body["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| e["id"].as_str().unwrap().to_owned()),
        );
        match res.body["next_cursor"].as_str() {
            Some(c) => uri = format!("/api/v1/admin/audit-events?limit=4&cursor={c}"),
            None => break,
        }
    }
    let unique: std::collections::HashSet<_> = seen.iter().collect();
    assert_eq!((seen.len() as i64, unique.len() as i64), (total, total));

    get(&r, "/api/v1/admin/audit-events?cursor=garbage", Some(&admin))
        .await
        .assert_problem(StatusCode::BAD_REQUEST, "invalid_cursor");
    get(&r, "/api/v1/admin/audit-events?limit=0", Some(&admin))
        .await
        .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    get(&r, "/api/v1/admin/audit-events?limit=abc", Some(&admin))
        .await
        .assert_problem(StatusCode::BAD_REQUEST, "invalid_query");
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn login_failures_do_not_reveal_account_existence(pool: PgPool) {
    let r = router(&pool);
    register(&r, "f@example.com").await;
    // Correct password but unverified: only now is status revealed.
    post(
        &r,
        "/api/v1/auth/login",
        json!({ "email": "f@example.com", "password": PASSWORD }),
        None,
    )
    .await
    .assert_problem(StatusCode::FORBIDDEN, "email_not_verified");
    let wrong = post(
        &r,
        "/api/v1/auth/login",
        json!({ "email": "f@example.com", "password": "wrong password!!" }),
        None,
    )
    .await;
    let unknown = post(
        &r,
        "/api/v1/auth/login",
        json!({ "email": "nobody@example.com", "password": PASSWORD }),
        None,
    )
    .await;
    wrong.assert_problem(StatusCode::UNAUTHORIZED, "invalid_credentials");
    unknown.assert_problem(StatusCode::UNAUTHORIZED, "invalid_credentials");
    assert_eq!(wrong.body["title"], unknown.body["title"]);
    assert_eq!(count(&pool, "SELECT count(*) FROM sessions").await, 0);
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn duplicate_registration_is_indistinguishable_and_notifies_owner(pool: PgPool) {
    let r = router(&pool);
    assert_eq!(register(&r, "g@example.com").await.status, StatusCode::ACCEPTED);
    assert_eq!(register(&r, "G@Example.com").await.status, StatusCode::ACCEPTED);
    assert_eq!(count(&pool, "SELECT count(*) FROM users").await, 1);
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM outbox WHERE topic = 'email.registration_attempted'"
        )
        .await,
        1
    );
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn concurrent_registrations_create_exactly_one_user(pool: PgPool) {
    let r = router(&pool);
    let tasks: Vec<_> = (0..6)
        .map(|_| {
            let r = r.clone();
            tokio::spawn(async move { register(&r, "race@example.com").await.status })
        })
        .collect();
    for t in tasks {
        assert_eq!(t.await.unwrap(), StatusCode::ACCEPTED);
    }
    assert_eq!(count(&pool, "SELECT count(*) FROM users").await, 1);
    assert_eq!(
        count(&pool, "SELECT count(*) FROM audit_log WHERE action = 'user.registered'").await,
        1
    );
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn verification_token_is_single_use_under_concurrency(pool: PgPool) {
    let r = router(&pool);
    register(&r, "h@example.com").await;
    let token = token_from_outbox(&pool, "email.verify_requested", "h@example.com").await;
    let tasks: Vec<_> = (0..5)
        .map(|_| {
            let (r, t) = (r.clone(), token.clone());
            tokio::spawn(async move { post(&r, "/api/v1/auth/verify-email", json!({ "token": t }), None).await })
        })
        .collect();
    let mut ok = 0;
    for t in tasks {
        let res = t.await.unwrap();
        match res.status {
            StatusCode::NO_CONTENT => ok += 1,
            _ => res.assert_problem(StatusCode::BAD_REQUEST, "invalid_token"),
        }
    }
    assert_eq!(ok, 1);
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM audit_log WHERE action = 'user.email_verified'"
        )
        .await,
        1
    );
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn expired_or_unknown_tokens_are_rejected(pool: PgPool) {
    let r = router(&pool);
    register(&r, "i@example.com").await;
    let token = token_from_outbox(&pool, "email.verify_requested", "i@example.com").await;
    sqlx::query("UPDATE account_tokens SET expires_at = now() - interval '1 second'")
        .execute(&pool)
        .await
        .unwrap();
    post(&r, "/api/v1/auth/verify-email", json!({ "token": token }), None)
        .await
        .assert_problem(StatusCode::BAD_REQUEST, "invalid_token");
    post(&r, "/api/v1/auth/verify-email", json!({ "token": "made-up" }), None)
        .await
        .assert_problem(StatusCode::BAD_REQUEST, "invalid_token");
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn password_reset_rotates_credentials_and_revokes_sessions(pool: PgPool) {
    let r = router(&pool);
    let (_, old_cookie) = signed_in(&pool, &r, "j@example.com", None).await;

    // Unknown address: same response, nothing queued.
    let res = post(
        &r,
        "/api/v1/auth/password-reset",
        json!({ "email": "ghost@example.com" }),
        None,
    )
    .await;
    assert_eq!(res.status, StatusCode::ACCEPTED);
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM outbox WHERE topic = 'email.password_reset_requested'"
        )
        .await,
        0
    );

    assert_eq!(
        post(
            &r,
            "/api/v1/auth/password-reset",
            json!({ "email": "j@example.com", "locale": "ar" }),
            None
        )
        .await
        .status,
        StatusCode::ACCEPTED
    );
    let token = token_from_outbox(&pool, "email.password_reset_requested", "j@example.com").await;
    let link: String =
        sqlx::query_scalar("SELECT payload->>'link' FROM outbox WHERE topic = 'email.password_reset_requested'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        link.starts_with("http://localhost:3000/ar/reset-password#token="),
        "{link}"
    );

    post(
        &r,
        "/api/v1/auth/password-reset/confirm",
        json!({ "token": token, "new_password": "short" }),
        None,
    )
    .await
    .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");

    let new_pw = "an entirely new passphrase";
    let res = post(
        &r,
        "/api/v1/auth/password-reset/confirm",
        json!({ "token": token, "new_password": new_pw }),
        None,
    )
    .await;
    assert_eq!(res.status, StatusCode::NO_CONTENT, "{}", res.body);

    get(&r, "/api/v1/me", Some(&old_cookie))
        .await
        .assert_problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    post(
        &r,
        "/api/v1/auth/login",
        json!({ "email": "j@example.com", "password": PASSWORD }),
        None,
    )
    .await
    .assert_problem(StatusCode::UNAUTHORIZED, "invalid_credentials");
    let res = post(
        &r,
        "/api/v1/auth/login",
        json!({ "email": "j@example.com", "password": new_pw }),
        None,
    )
    .await;
    assert_eq!(res.status, StatusCode::OK);

    post(
        &r,
        "/api/v1/auth/password-reset/confirm",
        json!({ "token": token, "new_password": new_pw }),
        None,
    )
    .await
    .assert_problem(StatusCode::BAD_REQUEST, "invalid_token");
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM audit_log WHERE action = 'user.password_reset'"
        )
        .await,
        1
    );
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn issuing_a_new_reset_token_invalidates_the_previous_one(pool: PgPool) {
    let r = router(&pool);
    signed_in(&pool, &r, "k@example.com", None).await;
    post(
        &r,
        "/api/v1/auth/password-reset",
        json!({ "email": "k@example.com" }),
        None,
    )
    .await;
    let first = token_from_outbox(&pool, "email.password_reset_requested", "k@example.com").await;
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    post(
        &r,
        "/api/v1/auth/password-reset",
        json!({ "email": "k@example.com" }),
        None,
    )
    .await;
    post(
        &r,
        "/api/v1/auth/password-reset/confirm",
        json!({ "token": first, "new_password": "yet another passphrase" }),
        None,
    )
    .await
    .assert_problem(StatusCode::BAD_REQUEST, "invalid_token");
}
