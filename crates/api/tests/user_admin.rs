//! User administration: roles, disabling, listing. Admin only, audited, serialised.

mod common;

use axum::http::{Method, StatusCode};
use common::*;
use domain::Role;
use serde_json::json;
use sqlx::PgPool;

const QUEUE: &str = "/api/v1/moderation/contributions";

async fn count(pool: &PgPool, sql: &str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn only_admins_may_administer_users(pool: PgPool) {
    let r = router(&pool);
    let (member, m) = signed_in(&pool, &r, "m@example.com", None).await;
    let (_, moderator) = signed_in(&pool, &r, "mod@example.com", Some(Role::Moderator)).await;
    let calls = [
        (Method::GET, "/api/v1/admin/users".to_string()),
        (Method::PUT, format!("/api/v1/admin/users/{member}/roles/moderator")),
        (Method::DELETE, format!("/api/v1/admin/users/{member}/roles/moderator")),
        (Method::POST, format!("/api/v1/admin/users/{member}/disable")),
        (Method::POST, format!("/api/v1/admin/users/{member}/enable")),
    ];
    for (method, uri) in calls {
        send(&r, method.clone(), &uri, None, None)
            .await
            .assert_problem(StatusCode::UNAUTHORIZED, "unauthenticated");
        for cookie in [&m, &moderator] {
            assert_eq!(
                send(&r, method.clone(), &uri, None, Some(cookie)).await.status,
                StatusCode::FORBIDDEN,
                "{method} {uri}"
            );
        }
    }
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM user_roles WHERE user_id = (SELECT id FROM users WHERE email='m@example.com')"
        )
        .await,
        0
    );
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn grant_and_revoke_moderator_takes_effect_and_is_audited(pool: PgPool) {
    let r = router(&pool);
    let (_, admin) = signed_in(&pool, &r, "admin@example.com", Some(Role::Admin)).await;
    let (user, cookie) = signed_in(&pool, &r, "u@example.com", None).await;
    assert_eq!(get(&r, QUEUE, Some(&cookie)).await.status, StatusCode::FORBIDDEN);

    let uri = format!("/api/v1/admin/users/{user}/roles/moderator");
    let res = send(&r, Method::PUT, &uri, None, Some(&admin)).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.body);
    assert_eq!(res.body["roles"], json!(["moderator"]));
    // Immediate: the existing session sees the new role.
    assert_eq!(get(&r, QUEUE, Some(&cookie)).await.status, StatusCode::OK);
    // Idempotent, and only one audit entry for the real change.
    assert_eq!(
        send(&r, Method::PUT, &uri, None, Some(&admin)).await.status,
        StatusCode::OK
    );
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM audit_log WHERE action = 'admin.role_granted'"
        )
        .await,
        1
    );

    assert_eq!(
        send(&r, Method::DELETE, &uri, None, Some(&admin)).await.body["roles"],
        json!([])
    );
    assert_eq!(get(&r, QUEUE, Some(&cookie)).await.status, StatusCode::FORBIDDEN);
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM audit_log WHERE action = 'admin.role_revoked'"
        )
        .await,
        1
    );
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn invalid_role_unknown_user_and_own_account_are_refused(pool: PgPool) {
    let r = router(&pool);
    let (admin_id, admin) = signed_in(&pool, &r, "admin@example.com", Some(Role::Admin)).await;
    let (user, _) = signed_in(&pool, &r, "u@example.com", None).await;

    let bad = send(
        &r,
        Method::PUT,
        &format!("/api/v1/admin/users/{user}/roles/owner"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(bad.body["errors"][0]["field"], "role");

    let ghost = uuid::Uuid::new_v4();
    send(
        &r,
        Method::POST,
        &format!("/api/v1/admin/users/{ghost}/disable"),
        None,
        Some(&admin),
    )
    .await
    .assert_problem(StatusCode::NOT_FOUND, "not_found");

    for uri in [
        format!("/api/v1/admin/users/{admin_id}/disable"),
        format!("/api/v1/admin/users/{admin_id}/roles/admin"),
    ] {
        let method = if uri.ends_with("disable") {
            Method::POST
        } else {
            Method::DELETE
        };
        send(&r, method, &uri, None, Some(&admin))
            .await
            .assert_problem(StatusCode::CONFLICT, "own_account");
    }
    assert_eq!(
        count(&pool, "SELECT count(*) FROM audit_log WHERE action LIKE 'admin.%'").await,
        0
    );
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn disabling_ends_sessions_blocks_login_and_can_be_undone(pool: PgPool) {
    let r = router(&pool);
    let (_, admin) = signed_in(&pool, &r, "admin@example.com", Some(Role::Admin)).await;
    let (user, cookie) = signed_in(&pool, &r, "u@example.com", None).await;
    assert_eq!(get(&r, "/api/v1/me", Some(&cookie)).await.status, StatusCode::OK);

    let off = send(
        &r,
        Method::POST,
        &format!("/api/v1/admin/users/{user}/disable"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(off.body["status"], "disabled");
    assert_eq!(
        get(&r, "/api/v1/me", Some(&cookie)).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        count(
            &pool,
            &format!("SELECT count(*) FROM sessions WHERE user_id = '{user}'")
        )
        .await,
        0
    );
    let login = post(
        &r,
        "/api/v1/auth/login",
        json!({ "email": "u@example.com", "password": PASSWORD }),
        None,
    )
    .await;
    assert_ne!(login.status, StatusCode::OK);

    let on = send(
        &r,
        Method::POST,
        &format!("/api/v1/admin/users/{user}/enable"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(on.body["status"], "active");
    let login = post(
        &r,
        "/api/v1/auth/login",
        json!({ "email": "u@example.com", "password": PASSWORD }),
        None,
    )
    .await;
    assert_eq!(login.status, StatusCode::OK);
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM audit_log WHERE action IN ('admin.user_disabled','admin.user_enabled')"
        )
        .await,
        2
    );
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn directory_lists_searches_and_pages(pool: PgPool) {
    let r = router(&pool);
    let (_, admin) = signed_in(&pool, &r, "admin@example.com", Some(Role::Admin)).await;
    signed_in(&pool, &r, "asha@example.com", Some(Role::Moderator)).await;
    signed_in(&pool, &r, "ravi@example.com", None).await;

    let all = get(&r, "/api/v1/admin/users", Some(&admin)).await;
    assert_eq!(all.body["items"].as_array().unwrap().len(), 3);
    assert!(all.body["items"][0].get("password_hash").is_none());

    let found = get(&r, "/api/v1/admin/users?q=ASH", Some(&admin)).await;
    assert_eq!(found.body["items"].as_array().unwrap().len(), 1);
    assert_eq!(found.body["items"][0]["roles"], json!(["moderator"]));
    // LIKE wildcards are literal.
    assert_eq!(
        get(&r, "/api/v1/admin/users?q=%25", Some(&admin)).await.body["items"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    let p1 = get(&r, "/api/v1/admin/users?limit=2", Some(&admin)).await;
    assert_eq!(p1.body["items"].as_array().unwrap().len(), 2);
    let cursor = p1.body["next_cursor"].as_str().unwrap();
    let p2 = get(
        &r,
        &format!("/api/v1/admin/users?limit=2&cursor={cursor}"),
        Some(&admin),
    )
    .await;
    assert_eq!(p2.body["items"].as_array().unwrap().len(), 1);
    assert!(p2.body["next_cursor"].is_null());
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn concurrent_changes_are_serialised(pool: PgPool) {
    let r = router(&pool);
    let (_, a1) = signed_in(&pool, &r, "a1@example.com", Some(Role::Admin)).await;
    let (_, a2) = signed_in(&pool, &r, "a2@example.com", Some(Role::Admin)).await;
    let (user, _) = signed_in(&pool, &r, "u@example.com", None).await;
    let uri = format!("/api/v1/admin/users/{user}/roles/moderator");
    let (x, y) = tokio::join!(
        send(&r, Method::PUT, &uri, None, Some(&a1)),
        send(&r, Method::PUT, &uri, None, Some(&a2)),
    );
    assert_eq!((x.status, y.status), (StatusCode::OK, StatusCode::OK));
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM audit_log WHERE action = 'admin.role_granted'"
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &pool,
            &format!("SELECT count(*) FROM user_roles WHERE user_id = '{user}'")
        )
        .await,
        1
    );
}
