//! Critical authorization and isolation suite. The first test walks the OpenAPI contract itself, so a
//! new endpoint that forgets authentication fails here without anyone remembering to add a case.
//! The second proves pending text never reaches any public surface or shared cache.

mod common;

use axum::http::{Method, StatusCode};
use common::*;
use domain::Role;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::collections::BTreeSet;
use uuid::Uuid;

/// Writes that are deliberately open to anonymous callers. Anything else must require a session.
const PUBLIC_WRITES: &[(&str, &str)] = &[
    ("post", "/api/v1/auth/register"),
    ("post", "/api/v1/auth/verify-email"),
    ("post", "/api/v1/auth/login"),
    ("post", "/api/v1/auth/password-reset"),
    ("post", "/api/v1/auth/password-reset/confirm"),
    ("post", "/api/v1/legal/notices"),
    ("post", "/api/v1/search/suggest/accepted"),
];

fn fill(path: &str) -> String {
    let mut out = String::new();
    let mut rest = path;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let end = rest[i..].find('}').unwrap() + i;
        let name = &rest[i + 1..end];
        // Enumerated path segments need a valid value; ids get a random uuid.
        out.push_str(match name {
            "kind" => "reputation",
            "entity_type" => "artist",
            "scope" => "song",
            "slug" => "x",
            _ => "",
        });
        if !matches!(name, "kind" | "entity_type" | "scope" | "slug") {
            out.push_str(&Uuid::new_v4().to_string());
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

fn operations() -> Vec<(Method, String, String, bool)> {
    let spec: Value = serde_json::from_str(&api::openapi_json()).unwrap();
    let mut ops = vec![];
    for (path, item) in spec["paths"].as_object().unwrap() {
        for (m, op) in item.as_object().unwrap() {
            let Ok(method) = m.to_uppercase().parse::<Method>() else {
                continue;
            };
            let secured = op.get("security").is_some_and(|s| !s.as_array().unwrap().is_empty());
            ops.push((method, path.clone(), fill(path), secured));
        }
    }
    ops
}

/// Generated clients are keyed by operationId: a duplicate silently merges two endpoints' types.
#[test]
fn operation_ids_are_unique() {
    let spec: Value = serde_json::from_str(&api::openapi_json()).unwrap();
    let mut seen = BTreeSet::new();
    for (path, item) in spec["paths"].as_object().unwrap() {
        for (method, op) in item.as_object().unwrap() {
            let id = op["operationId"]
                .as_str()
                .unwrap_or_else(|| panic!("{method} {path} has no operationId"));
            assert!(
                seen.insert(id.to_owned()),
                "operationId {id} is used twice (second: {method} {path})"
            );
        }
    }
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn every_secured_operation_refuses_anonymous_and_members_cannot_reach_staff_endpoints(pool: PgPool) {
    let r = router(&pool);
    let (_, member) = signed_in(&pool, &r, "member@example.com", None).await;
    let ops = operations();
    assert!(ops.len() > 100, "contract looks truncated: {}", ops.len());

    let mut open_writes = BTreeSet::new();
    let mut checked = 0;
    let mut forbidden = 0;
    for (method, template, path, secured) in &ops {
        let body = (*method != Method::GET).then(|| json!({}));
        if !secured {
            if matches!(*method, Method::POST | Method::PUT | Method::PATCH | Method::DELETE) {
                open_writes.insert((method.as_str().to_lowercase(), template.clone()));
            }
            continue;
        }
        let res = send(&r, method.clone(), path, body.clone(), None).await;
        assert_eq!(
            res.status,
            StatusCode::UNAUTHORIZED,
            "{method} {template} answered {} to an anonymous caller",
            res.status
        );
        checked += 1;

        // A signed-in member with no role must never succeed on staff endpoints.
        if template.contains("/moderation/") || template.contains("/admin/") {
            let res = send(&r, method.clone(), path, body, Some(&member)).await;
            // Missing query parameters or a malformed body may be rejected before the role check;
            // what must never happen is success.
            let refused = matches!(res.status.as_u16(), 400 | 403 | 404 | 422);
            forbidden += usize::from(res.status == StatusCode::FORBIDDEN);
            assert!(refused, "{method} {template} gave a plain member {}", res.status);
        }
    }
    assert!(forbidden >= 10, "staff endpoints answered 403 only {forbidden} times");
    assert!(checked > 60, "only {checked} secured operations were exercised");

    let allowed: BTreeSet<_> = PUBLIC_WRITES
        .iter()
        .map(|(m, p)| ((*m).to_owned(), (*p).to_owned()))
        .collect();
    assert_eq!(
        open_writes, allowed,
        "the set of anonymous-writable endpoints changed; review it"
    );
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn pending_text_never_reaches_a_public_surface_or_a_shared_cache(pool: PgPool) {
    let r = router(&pool);
    let (_, author) = signed_in(&pool, &r, "author@example.com", None).await;
    let (_, other) = signed_in(&pool, &r, "other@example.com", None).await;
    let (_, moderator) = signed_in(&pool, &r, "mod@example.com", Some(Role::Moderator)).await;
    let work = post(
        &r,
        "/api/v1/compositions",
        json!({ "title": "Isolation Song" }),
        Some(&author),
    )
    .await;
    let wid = work.body["id"].as_str().unwrap().to_owned();
    let rec = post(
        &r,
        "/api/v1/recordings",
        json!({ "composition_id": wid, "title": "Take 1" }),
        Some(&author),
    )
    .await;
    let rid = rec.body["id"].as_str().unwrap().to_owned();
    let token = "zxqpendingtoken";
    let c = post(
        &r,
        &format!("/api/v1/recordings/{rid}/contributions"),
        json!({ "type": "original", "language": "ml", "script": "Mlym", "body": format!("line {token} one\nline two") }),
        Some(&author),
    )
    .await;
    assert_eq!(c.status, StatusCode::CREATED, "{}", c.body);
    let cid = c.body["id"].as_str().unwrap().to_owned();
    let rev = c.body["current_revision_id"].as_str().unwrap().to_owned();

    // Every public read, as anonymous and as another member: neither the text nor the id leaks.
    let surfaces = [
        format!("/api/v1/recordings/{rid}/contributions"),
        format!("/api/v1/contributions/{cid}"),
        format!("/api/v1/contributions/{cid}/revisions"),
        format!("/api/v1/revisions/{rev}"),
        format!("/api/v1/comparisons/resolve?ids={cid}"),
        format!("/api/v1/contributions/{cid}/engagement"),
        format!("/api/v1/contributions/{cid}/comments"),
        format!("/api/v1/search?q={token}"),
        "/api/v1/search?q=Isolation".to_owned(),
        "/api/v1/search/suggest?q=Isola".to_owned(),
        "/api/v1/sitemap".to_owned(),
    ];
    for who in [None, Some(other.as_str())] {
        for uri in &surfaces {
            let res = get(&r, uri, who).await;
            let text = res.body.to_string();
            assert!(!text.contains(token), "{uri} leaked pending text to {who:?}");
            assert!(
                !text.contains(&rev) || res.status != StatusCode::OK,
                "{uri} leaked a revision id"
            );
            if res.status == StatusCode::OK && uri.contains(&cid) && !uri.contains("resolve") {
                // Only available through the privileged endpoints below.
                assert!(!text.contains("line"), "{uri}: {text}");
            }
            // Anything that can differ per caller must forbid shared caches.
            if uri.contains("/contributions") || uri.contains("/comparisons") || uri.contains("/revisions") {
                let cc = res
                    .headers
                    .get("cache-control")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                assert!(cc.contains("no-store"), "{uri} is cacheable: {cc:?}");
            }
        }
    }
    // The comparison resolver cannot tell pending from unknown.
    let pending = get(&r, &format!("/api/v1/comparisons/resolve?ids={cid}"), None).await;
    let unknown = get(&r, &format!("/api/v1/comparisons/resolve?ids={}", Uuid::new_v4()), None).await;
    assert_eq!(pending.body["panels"][0]["available"], false);
    assert_eq!(
        pending.body["panels"][0].get("content"),
        unknown.body["panels"][0].get("content")
    );

    // The author and staff can still see it, so the above is isolation, not breakage.
    let own = get(&r, &format!("/api/v1/contributions/{cid}"), Some(&author)).await;
    assert_eq!(own.status, StatusCode::OK);
    assert!(own.body.to_string().contains(token));
    let staff = get(&r, &format!("/api/v1/contributions/{cid}"), Some(&moderator)).await;
    assert!(staff.body.to_string().contains(token));

    // Once published the same reads show it, and unpublishing removes it from search again.
    let done = post(
        &r,
        &format!("/api/v1/contributions/{cid}/publish"),
        json!({ "expected_parent_revision_id": rev }),
        Some(&moderator),
    )
    .await;
    assert_eq!(done.status, StatusCode::OK, "{}", done.body);
    let live = get(&r, &format!("/api/v1/contributions/{cid}"), None).await;
    assert!(live.body.to_string().contains(token));
    sqlx::query("UPDATE contributions SET status = 'rejected', is_primary = false, published_revision_id = NULL WHERE id = $1::uuid")
        .bind(&cid)
        .execute(&pool)
        .await
        .unwrap();
    let gone = get(&r, &format!("/api/v1/contributions/{cid}"), None).await;
    assert_eq!(gone.status, StatusCode::NOT_FOUND);
}
