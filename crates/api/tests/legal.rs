//! Legal workflows, abuse limits on credential endpoints, and the metrics endpoint.

mod common;

use axum::http::{Method, StatusCode};
use axum::Router;
use common::*;
use domain::Role;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

fn id(v: &Value) -> String {
    v["id"].as_str().unwrap().to_owned()
}

struct World {
    r: Router,
    author: String,
    moderator: String,
    admin: String,
    admin2: String,
    admin_id: Uuid,
    rec: String,
}

async fn world(pool: &PgPool) -> World {
    let r = router(pool);
    let (_, author) = signed_in(pool, &r, "author@example.com", None).await;
    let (_, moderator) = signed_in(pool, &r, "mod@example.com", Some(Role::Moderator)).await;
    let (admin_id, admin) = signed_in(pool, &r, "admin@example.com", Some(Role::Admin)).await;
    let (_, admin2) = signed_in(pool, &r, "admin2@example.com", Some(Role::Admin)).await;
    let w = post(&r, "/api/v1/compositions", json!({ "title": "Song" }), Some(&author)).await;
    let rec = post(
        &r,
        "/api/v1/recordings",
        json!({ "composition_id": id(&w.body), "title": "Take 1" }),
        Some(&author),
    )
    .await;
    World {
        r,
        author,
        moderator,
        admin,
        admin2,
        admin_id,
        rec: id(&rec.body),
    }
}

async fn published(w: &World, body: &str) -> Value {
    let c = post(
        &w.r,
        &format!("/api/v1/recordings/{}/contributions", w.rec),
        json!({ "type": "original", "language": "ml", "script": "Mlym", "body": body }),
        Some(&w.author),
    )
    .await;
    assert_eq!(c.status, StatusCode::CREATED, "{}", c.body);
    let done = post(
        &w.r,
        &format!("/api/v1/contributions/{}/publish", id(&c.body)),
        json!({ "expected_parent_revision_id": c.body["current_revision_id"] }),
        Some(&w.moderator),
    )
    .await;
    assert_eq!(done.status, StatusCode::OK, "{}", done.body);
    done.body
}

fn notice(target: &str) -> Value {
    json!({ "kind": "copyright", "contribution_id": target, "contact_name": "Rights Holder",
            "contact_email": "owner@example.org", "statement": "These lyrics are my copyrighted work.",
            "good_faith": true })
}

async fn denied(pool: &PgPool, actor: Uuid) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE action = 'moderation.denied' AND actor_id = $1")
        .bind(actor)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn notice_takedown_appeal_and_reinstatement(pool: PgPool) {
    let w = world(&pool).await;
    let first = published(&w, "ഒന്നാം വരി").await;
    let (fid, frev) = (id(&first), first["current_revision_id"].as_str().unwrap().to_owned());
    // A second published contribution in the same slot is an alternative.
    let second = published(&w, "രണ്ടാം വരി").await;
    let sid = id(&second);
    assert_eq!(first["is_primary"], true);
    assert_eq!(second["is_primary"], false);

    // Filing needs no account, and the receipt carries no contact details.
    let filed = post(&w.r, "/api/v1/legal/notices", notice(&fid), None).await;
    assert_eq!(filed.status, StatusCode::CREATED, "{}", filed.body);
    assert!(filed.body["reference"].as_str().unwrap().starts_with("LN-"));
    assert!(!filed.body.to_string().contains("owner@example.org"));
    let nid: String = sqlx::query_scalar("SELECT id::text FROM legal_notices")
        .fetch_one(&pool)
        .await
        .unwrap();

    // Unauthorised staff actions are refused and audited; anonymous gets 401.
    let (member_id, member) = signed_in(&pool, &w.r, "member@example.com", None).await;
    let q = get(&w.r, "/api/v1/moderation/legal/notices", Some(&member)).await;
    q.assert_problem(StatusCode::FORBIDDEN, "forbidden");
    let d = post(
        &w.r,
        &format!("/api/v1/moderation/legal/notices/{nid}/decide"),
        json!({ "action": "takedown" }),
        Some(&member),
    )
    .await;
    d.assert_problem(StatusCode::FORBIDDEN, "forbidden");
    assert_eq!(denied(&pool, member_id).await, 2);
    get(&w.r, "/api/v1/moderation/legal/notices", None)
        .await
        .assert_problem(StatusCode::UNAUTHORIZED, "unauthenticated");

    // The queue shows the filer's contact details to staff only.
    let queue = get(&w.r, "/api/v1/moderation/legal/notices", Some(&w.moderator)).await;
    assert_eq!(queue.body["items"][0]["contact_email"], "owner@example.org");

    // Two concurrent decisions: exactly one wins, the other is told it is already resolved.
    let uri = format!("/api/v1/moderation/legal/notices/{nid}/decide");
    let (a, b) = tokio::join!(
        post(
            &w.r,
            &uri,
            json!({ "action": "takedown", "note": "valid" }),
            Some(&w.admin)
        ),
        post(&w.r, &uri, json!({ "action": "takedown" }), Some(&w.moderator)),
    );
    let mut statuses = [a.status.as_u16(), b.status.as_u16()];
    statuses.sort_unstable();
    assert_eq!(statuses, [200, 409]);

    // The taken-down text is gone from every public read, and the alternative became primary.
    let pub_get = get(&w.r, &format!("/api/v1/contributions/{fid}"), None).await;
    pub_get.assert_problem(StatusCode::NOT_FOUND, "not_found");
    let resolved = get(&w.r, &format!("/api/v1/comparisons/resolve?ids={fid},{sid}"), None).await;
    assert_eq!(resolved.body["panels"][0]["available"], false);
    assert_eq!(resolved.body["panels"][1]["available"], true);
    assert_eq!(resolved.body["panels"][1]["content"]["is_primary"], true);
    // The revision itself is untouched (immutable history).
    let rev_exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM lyric_revisions WHERE id = $1::uuid)")
        .bind(&frev)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(rev_exists);
    let outbox: i64 = sqlx::query_scalar("SELECT count(*) FROM outbox WHERE topic = 'lyrics.unpublished'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(outbox, 1);

    // Only the contributor can appeal; everyone else sees nothing.
    let appeal_uri = format!("/api/v1/legal/notices/{nid}/appeal");
    let long = json!({ "statement": "I wrote these lyrics myself; here is the evidence." });
    post(&w.r, &appeal_uri, long.clone(), Some(&member))
        .await
        .assert_problem(StatusCode::NOT_FOUND, "not_found");
    post(&w.r, &appeal_uri, json!({ "statement": "short" }), Some(&w.author))
        .await
        .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    let ap = post(&w.r, &appeal_uri, long.clone(), Some(&w.author)).await;
    assert_eq!(ap.status, StatusCode::CREATED, "{}", ap.body);
    post(&w.r, &appeal_uri, long, Some(&w.author))
        .await
        .assert_problem(StatusCode::CONFLICT, "appeal_pending");
    let aid = id(&ap.body);

    // Appeals are an admin decision; a moderator is refused and audited.
    let dec = format!("/api/v1/admin/legal/appeals/{aid}/decide");
    let (mod_id,): (Uuid,) = sqlx::query_as("SELECT id FROM users WHERE email = 'mod@example.com'")
        .fetch_one(&pool)
        .await
        .unwrap();
    get(&w.r, "/api/v1/admin/legal/appeals", Some(&w.moderator))
        .await
        .assert_problem(StatusCode::FORBIDDEN, "forbidden");
    post(&w.r, &dec, json!({ "uphold": true }), Some(&w.moderator))
        .await
        .assert_problem(StatusCode::FORBIDDEN, "forbidden");
    assert_eq!(denied(&pool, mod_id).await, 2);
    // Nobody reviews their own takedown.
    let decider: Uuid = sqlx::query_scalar("SELECT decided_by FROM legal_notices")
        .fetch_one(&pool)
        .await
        .unwrap();
    let (own, other) = if decider == w.admin_id {
        (&w.admin, &w.admin2)
    } else {
        (&w.admin2, &w.admin)
    };
    post(&w.r, &dec, json!({ "uphold": true }), Some(own))
        .await
        .assert_problem(StatusCode::FORBIDDEN, "own_decision");
    let up = post(
        &w.r,
        &dec,
        json!({ "uphold": true, "note": "evidence accepted" }),
        Some(other),
    )
    .await;
    assert_eq!(up.status, StatusCode::OK, "{}", up.body);
    post(&w.r, &dec, json!({ "uphold": false }), Some(other))
        .await
        .assert_problem(StatusCode::CONFLICT, "already_resolved");

    // Reinstated: the very same revision is public again, as an alternative (the slot has a primary).
    let back = get(&w.r, &format!("/api/v1/contributions/{fid}"), None).await;
    assert_eq!(back.status, StatusCode::OK, "{}", back.body);
    let live_rev: String =
        sqlx::query_scalar("SELECT published_revision_id::text FROM contributions WHERE id = $1::uuid")
            .bind(&fid)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(live_rev, frev);
    assert_eq!(back.body["is_primary"], false);
    let primaries: i64 = sqlx::query_scalar("SELECT count(*) FROM contributions WHERE is_primary")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(primaries, 1);
    for action in [
        "legal.notice_filed",
        "legal.notice_decided",
        "contribution.taken_down",
        "legal.appeal_filed",
        "contribution.reinstated",
        "legal.appeal_decided",
    ] {
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE action = $1")
            .bind(action)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(n >= 1, "missing audit for {action}");
    }
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn notices_are_validated_and_cannot_probe_private_work(pool: PgPool) {
    let w = world(&pool).await;
    // A pending contribution looks exactly like an unknown id.
    let c = post(
        &w.r,
        &format!("/api/v1/recordings/{}/contributions", w.rec),
        json!({ "type": "original", "language": "ml", "script": "Mlym", "body": "draft" }),
        Some(&w.author),
    )
    .await;
    let pending = id(&c.body);
    for target in [pending, Uuid::new_v4().to_string()] {
        post(&w.r, "/api/v1/legal/notices", notice(&target), None)
            .await
            .assert_problem(StatusCode::NOT_FOUND, "not_found");
    }
    let live = id(&published(&w, "text").await);
    for (patch, field) in [
        (json!({ "good_faith": false }), "good_faith"),
        (json!({ "contact_email": "not an email" }), "contact_email"),
        (json!({ "statement": "too short" }), "statement"),
        (json!({ "kind": "nonsense" }), "kind"),
        (json!({ "contribution_id": null }), "contribution_id"),
    ] {
        let mut b = notice(&live);
        b.as_object_mut().unwrap().extend(patch.as_object().unwrap().clone());
        let res = post(&w.r, "/api/v1/legal/notices", b, None).await;
        res.assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
        assert!(res.body.to_string().contains(field), "{}", res.body);
    }
    // A privacy request needs no target and takedown is not available for it.
    let p = post(
        &w.r,
        "/api/v1/legal/notices",
        json!({ "kind": "privacy", "contact_name": "Person", "contact_email": "p@example.org",
                "statement": "Please erase the data held about me.", "good_faith": true }),
        None,
    )
    .await;
    assert_eq!(p.status, StatusCode::CREATED, "{}", p.body);
    let nid: String = sqlx::query_scalar("SELECT id::text FROM legal_notices WHERE kind = 'privacy'")
        .fetch_one(&pool)
        .await
        .unwrap();
    post(
        &w.r,
        &format!("/api/v1/moderation/legal/notices/{nid}/decide"),
        json!({ "action": "takedown" }),
        Some(&w.moderator),
    )
    .await
    .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    let ok = post(
        &w.r,
        &format!("/api/v1/moderation/legal/notices/{nid}/decide"),
        json!({ "action": "close", "note": "handled" }),
        Some(&w.moderator),
    )
    .await;
    assert_eq!(ok.status, StatusCode::OK);
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn credential_endpoints_are_rate_limited_and_metrics_are_private(pool: PgPool) {
    let r = router_with(&pool, |c| c.auth_rate_per_min = 3);
    let mut codes = vec![];
    for _ in 0..5 {
        let res = send(
            &r,
            Method::POST,
            "/api/v1/auth/login",
            Some(json!({ "email": "x@example.com", "password": "wrong password here" })),
            None,
        )
        .await;
        codes.push(res.status.as_u16());
        if res.status == StatusCode::TOO_MANY_REQUESTS {
            res.assert_problem(StatusCode::TOO_MANY_REQUESTS, "rate_limited");
            assert!(res.headers.contains_key("retry-after"));
        }
    }
    assert_eq!(&codes[..3], &[401, 401, 401], "{codes:?}");
    assert_eq!(&codes[3..], &[429, 429]);

    // /metrics: 401 without the token, text with it, and no raw paths or ids in labels.
    let m = r.clone();
    assert_eq!(get(&m, "/metrics", None).await.status, StatusCode::UNAUTHORIZED);
    let id = Uuid::new_v4();
    let _ = get(&m, &format!("/api/v1/contributions/{id}"), None).await;
    let req = axum::http::Request::builder()
        .uri("/metrics")
        .header("authorization", "Bearer test-metrics-token-0123")
        .body(axum::body::Body::empty())
        .unwrap();
    let res = raw(&m, req).await;
    assert_eq!(res.status, StatusCode::OK);
    let text = res.body.as_str().unwrap().to_owned();
    assert!(text.contains("lyrics_http_request_duration_seconds_bucket"));
    assert!(text.contains("lyrics_outbox_pending"));
    assert!(!text.contains(&id.to_string()));
    // Disabled entirely when no token is configured.
    let off = router_with(&pool, |c| c.metrics_token = None);
    assert_eq!(get(&off, "/metrics", None).await.status, StatusCode::NOT_FOUND);
}
