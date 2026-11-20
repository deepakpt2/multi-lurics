//! Comparison and community: panels resolve from what is published now, alternatives stay
//! separate, no self-rating, moderation is role-gated and audited, reputation is not a role,
//! and abuse controls hold under concurrency.

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

async fn count(pool: &PgPool, sql: &str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
}

async fn put(r: &Router, uri: &str, body: Value, cookie: Option<&str>) -> Res {
    send(r, Method::PUT, uri, Some(body), cookie).await
}
async fn del(r: &Router, uri: &str, cookie: Option<&str>) -> Res {
    send(r, Method::DELETE, uri, None, cookie).await
}
async fn patch(r: &Router, uri: &str, body: Value, cookie: Option<&str>) -> Res {
    send(r, Method::PATCH, uri, Some(body), cookie).await
}

struct World {
    r: Router,
    pool: PgPool,
    author: String,
    author_id: Uuid,
    reader: String,
    reader_id: Uuid,
    moderator: String,
    moderator_id: Uuid,
    moderator2: String,
    admin: String,
    rec: String,
}

async fn world(pool: &PgPool) -> World {
    let r = router(pool);
    let (author_id, author) = signed_in(pool, &r, "author@example.com", None).await;
    let (reader_id, reader) = signed_in(pool, &r, "reader@example.com", None).await;
    let (moderator_id, moderator) = signed_in(pool, &r, "mod@example.com", Some(Role::Moderator)).await;
    let (_, moderator2) = signed_in(pool, &r, "mod2@example.com", Some(Role::Moderator)).await;
    let (_, admin) = signed_in(pool, &r, "admin@example.com", Some(Role::Admin)).await;
    let w = post(
        &r,
        "/api/v1/compositions",
        json!({ "title": "Kaatrin Mozhi" }),
        Some(&author),
    )
    .await;
    assert_eq!(w.status, StatusCode::CREATED, "{}", w.body);
    let rec = post(
        &r,
        "/api/v1/recordings",
        json!({ "composition_id": id(&w.body), "title": "Take 1" }),
        Some(&author),
    )
    .await;
    assert_eq!(rec.status, StatusCode::CREATED, "{}", rec.body);
    World {
        r,
        pool: pool.clone(),
        author,
        author_id,
        reader,
        reader_id,
        moderator,
        moderator_id,
        moderator2,
        admin,
        rec: id(&rec.body),
    }
}

/// Submitted by the author, published by a moderator. Returns the contribution detail.
async fn published(w: &World, kind: &str, lang: &str, script: &str, body: &str) -> Value {
    let c = post(
        &w.r,
        &format!("/api/v1/recordings/{}/contributions", w.rec),
        json!({ "type": kind, "language": lang, "script": script, "body": body }),
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

async fn resolve(w: &World, ids: &[&str]) -> Res {
    get(
        &w.r,
        &format!("/api/v1/comparisons/resolve?ids={}", ids.join(",")),
        None,
    )
    .await
}

async fn audit_count(pool: &PgPool, action: &str, actor: Uuid) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE action = $1 AND actor_id = $2")
        .bind(action)
        .bind(actor)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn set_community(w: &World, f: impl FnOnce(&mut Value)) {
    let mut s = get(&w.r, "/api/v1/admin/community-settings", Some(&w.admin)).await.body;
    f(&mut s);
    let res = put(&w.r, "/api/v1/admin/community-settings", s, Some(&w.admin)).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.body);
}

async fn file_report(w: &World, who: &str, target: &str, reason: &str) -> Res {
    post(
        &w.r,
        "/api/v1/reports",
        json!({ "target_type": "comment", "target_id": target, "reason": reason }),
        Some(who),
    )
    .await
}

async fn rep_sum(w: &World, user: Uuid) -> i64 {
    count(
        &w.pool,
        &format!("SELECT coalesce(sum(points),0) FROM reputation_events WHERE user_id = '{user}'"),
    )
    .await
}

async fn appeal(w: &World, who: &str, target: &str) -> Res {
    post(
        &w.r,
        "/api/v1/appeals",
        json!({ "comment_id": target, "message": "this was fine" }),
        Some(who),
    )
    .await
}

async fn decide(w: &World, appeal_id: &str, who: &str, d: &str) -> Res {
    post(
        &w.r,
        &format!("/api/v1/moderation/appeals/{appeal_id}/decide"),
        json!({ "decision": d, "note": "reviewed" }),
        Some(who),
    )
    .await
}

async fn comment(w: &World, who: &str, c: &str, body: &str) -> Res {
    post(
        &w.r,
        &format!("/api/v1/contributions/{c}/comments"),
        json!({ "body": body }),
        Some(who),
    )
    .await
}

// ---------- comparison ----------

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn four_distinct_published_contributions_resolve_and_alternatives_stay_apart(pool: PgPool) {
    let w = world(&pool).await;
    let original = published(&w, "original", "ml", "Mlym", "ആദ്യ വരി").await;
    let alternative = published(&w, "original", "ml", "Mlym", "ബദൽ വരി").await;
    let translation = published(&w, "translation", "en", "Latn", "first line").await;
    let arabic = published(&w, "translation", "ar", "Arab", "السطر الأول").await;
    assert_eq!(original["is_primary"], true);
    assert_eq!(alternative["is_primary"], false, "second in the slot is an alternative");

    let ids = [id(&original), id(&alternative), id(&translation), id(&arabic)];
    let res = resolve(&w, &ids.iter().map(String::as_str).collect::<Vec<_>>()).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.body);
    let panels = res.body["panels"].as_array().unwrap();
    assert_eq!(panels.len(), 4);
    // Request order is kept and every panel is its own contribution.
    for (p, want) in panels.iter().zip(&ids) {
        assert_eq!(p["id"], *want);
        assert_eq!(p["available"], true);
    }
    let body = |i: usize| panels[i]["content"]["body"].as_str().unwrap().to_owned();
    assert_eq!(body(0), "ആദ്യ വരി");
    assert_eq!(body(1), "ബദൽ വരി", "the alternative is not replaced by the primary");
    assert_eq!(panels[0]["content"]["is_primary"], true);
    assert_eq!(panels[1]["content"]["is_primary"], false);
    assert_eq!(panels[0]["content"]["slot_size"], 2);
    assert_eq!(panels[0]["content"]["language"], panels[1]["content"]["language"]);
    assert_ne!(panels[0]["content"]["revision_id"], panels[1]["content"]["revision_id"]);
    assert_eq!(panels[3]["content"]["dir"], "rtl");
    assert_eq!(panels[2]["content"]["type"], "translation");
    assert_eq!(panels[2]["content"]["script"], "Latn");
    // Public: no session needed, and not cached.
    assert_eq!(res.headers["cache-control"], "private, no-store");
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn resolve_validates_ids_and_never_reveals_unpublished_work(pool: PgPool) {
    let w = world(&pool).await;
    let live = published(&w, "original", "ml", "Mlym", "പരസ്യം").await;
    let pending = post(
        &w.r,
        &format!("/api/v1/recordings/{}/contributions", w.rec),
        json!({ "type": "translation", "language": "en", "script": "Latn", "body": "secret draft" }),
        Some(&w.author),
    )
    .await;
    let missing = Uuid::new_v4().to_string();
    let res = resolve(&w, &[&id(&live), &id(&pending.body), &missing]).await;
    assert_eq!(res.status, StatusCode::OK);
    let p = res.body["panels"].as_array().unwrap();
    assert_eq!(p[0]["available"], true);
    // Pending and unknown look the same, and no text comes back.
    for i in [1, 2] {
        assert_eq!(p[i]["available"], false);
        assert!(p[i]["content"].is_null());
    }
    assert!(!res.body.to_string().contains("secret draft"));

    let too_many: Vec<String> = (0..5).map(|_| Uuid::new_v4().to_string()).collect();
    resolve(&w, &too_many.iter().map(String::as_str).collect::<Vec<_>>())
        .await
        .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    resolve(&w, &["not-a-uuid"])
        .await
        .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    get(&w.r, "/api/v1/comparisons/resolve", None)
        .await
        .assert_problem(StatusCode::BAD_REQUEST, "invalid_query");
    get(&w.r, "/api/v1/comparisons/resolve?ids=", None)
        .await
        .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");

    // Withdrawn work disappears at once.
    sqlx::query("UPDATE contributions SET status = 'rejected', is_primary = false, published_revision_id = NULL WHERE id = $1::uuid")
        .bind(id(&live))
        .execute(&pool)
        .await
        .unwrap();
    let res = resolve(&w, &[&id(&live)]).await;
    assert_eq!(res.body["panels"][0]["available"], false);
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn a_saved_comparison_holds_ids_only_and_resolves_the_new_revision(pool: PgPool) {
    let w = world(&pool).await;
    let c = published(&w, "original", "ml", "Mlym", "ആദ്യ പതിപ്പ്").await;
    let saved = post(
        &w.r,
        "/api/v1/me/comparisons",
        json!({ "name": "My pair", "contribution_ids": [id(&c)] }),
        Some(&w.reader),
    )
    .await;
    assert_eq!(saved.status, StatusCode::CREATED, "{}", saved.body);
    assert_eq!(saved.body["panels"][0]["content"]["body"], "ആദ്യ പതിപ്പ്");
    // The record is ids: no lyric text is stored with it.
    let stored: String = sqlx::query_scalar("SELECT row_to_json(s)::text FROM saved_comparisons s")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(!stored.contains("ആദ്യ"), "{stored}");

    // A new revision is proposed and published; the saved comparison follows.
    let rev = post(
        &w.r,
        &format!("/api/v1/contributions/{}/revisions", id(&c)),
        json!({ "expected_parent_revision_id": c["current_revision_id"], "body": "രണ്ടാം പതിപ്പ്" }),
        Some(&w.author),
    )
    .await;
    assert_eq!(rev.status, StatusCode::CREATED, "{}", rev.body);
    // Until it is published, readers keep seeing the old text.
    let before = get(
        &w.r,
        &format!("/api/v1/me/comparisons/{}", id(&saved.body)),
        Some(&w.reader),
    )
    .await;
    assert_eq!(before.body["panels"][0]["content"]["body"], "ആദ്യ പതിപ്പ്");
    let done = post(
        &w.r,
        &format!("/api/v1/contributions/{}/publish", id(&c)),
        json!({ "expected_parent_revision_id": rev.body["current_revision_id"] }),
        Some(&w.moderator),
    )
    .await;
    assert_eq!(done.status, StatusCode::OK, "{}", done.body);
    let after = get(
        &w.r,
        &format!("/api/v1/me/comparisons/{}", id(&saved.body)),
        Some(&w.reader),
    )
    .await;
    assert_eq!(after.body["panels"][0]["content"]["body"], "രണ്ടാം പതിപ്പ്");
    assert_eq!(after.body["panels"][0]["content"]["revision_no"], 2);
    assert_eq!(after.body["contribution_ids"], json!([id(&c)]));
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn saved_comparisons_are_private_validated_and_limited(pool: PgPool) {
    let w = world(&pool).await;
    let c = published(&w, "original", "ml", "Mlym", "ഒന്ന്").await;
    let draft = post(
        &w.r,
        &format!("/api/v1/recordings/{}/contributions", w.rec),
        json!({ "type": "translation", "language": "en", "script": "Latn", "body": "draft" }),
        Some(&w.author),
    )
    .await;
    let body = |ids: Value| json!({ "name": "x", "contribution_ids": ids });
    post(&w.r, "/api/v1/me/comparisons", body(json!([id(&c)])), None)
        .await
        .assert_problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    // Cannot save what the public cannot see.
    post(
        &w.r,
        "/api/v1/me/comparisons",
        body(json!([id(&draft.body)])),
        Some(&w.reader),
    )
    .await
    .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    post(&w.r, "/api/v1/me/comparisons", body(json!([])), Some(&w.reader))
        .await
        .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    let five: Vec<String> = (0..5).map(|_| id(&c)).collect();
    post(&w.r, "/api/v1/me/comparisons", body(json!(five)), Some(&w.reader))
        .await
        .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    let mine = post(&w.r, "/api/v1/me/comparisons", body(json!([id(&c)])), Some(&w.reader)).await;
    assert_eq!(mine.status, StatusCode::CREATED);
    // Someone else's saved comparison is a 404, and deleting is the owner's alone.
    get(
        &w.r,
        &format!("/api/v1/me/comparisons/{}", id(&mine.body)),
        Some(&w.author),
    )
    .await
    .assert_problem(StatusCode::NOT_FOUND, "not_found");
    del(
        &w.r,
        &format!("/api/v1/me/comparisons/{}", id(&mine.body)),
        Some(&w.author),
    )
    .await
    .assert_problem(StatusCode::NOT_FOUND, "not_found");
    let list = get(&w.r, "/api/v1/me/comparisons", Some(&w.reader)).await;
    assert_eq!(list.body["items"].as_array().unwrap().len(), 1);
    assert!(list.body["items"][0]["panels"].is_null());
    assert_eq!(
        del(
            &w.r,
            &format!("/api/v1/me/comparisons/{}", id(&mine.body)),
            Some(&w.reader)
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(count(&pool, "SELECT count(*) FROM saved_comparisons").await, 0);
}

// ---------- ratings, reactions, bookmarks, reputation ----------

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn nobody_rates_their_own_work_and_ratings_feed_reputation_once(pool: PgPool) {
    let w = world(&pool).await;
    let c = published(&w, "original", "ml", "Mlym", "വരി").await;
    let uri = format!("/api/v1/contributions/{}/rating", id(&c));
    // The author cannot rate; neither can anyone else who wrote a revision.
    let own = put(&w.r, &uri, json!({ "value": 5 }), Some(&w.author)).await;
    own.assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    assert_eq!(own.body["errors"][0]["code"], "self_rating");
    put(&w.r, &uri, json!({ "value": 5 }), None)
        .await
        .assert_problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    for bad in [0, 6, -1] {
        put(&w.r, &uri, json!({ "value": bad }), Some(&w.reader))
            .await
            .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    }
    // The database says no even to a write that bypasses the application.
    let direct = sqlx::query("INSERT INTO ratings (contribution_id, user_id, value) VALUES ($1::uuid, $2, 5)")
        .bind(id(&c))
        .bind(w.author_id)
        .execute(&pool)
        .await;
    assert!(direct.is_err(), "trigger blocks self rating");

    let base = count(
        &pool,
        "SELECT coalesce(sum(points),0) FROM reputation_events WHERE kind = 'rating_received_high'",
    )
    .await;
    assert_eq!(
        put(&w.r, &uri, json!({ "value": 5 }), Some(&w.reader)).await.status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        put(&w.r, &uri, json!({ "value": 4 }), Some(&w.reader)).await.status,
        StatusCode::NO_CONTENT,
        "re-rating replaces"
    );
    assert_eq!(count(&pool, "SELECT count(*) FROM ratings").await, 1);
    let gained = count(
        &pool,
        "SELECT coalesce(sum(points),0) FROM reputation_events WHERE kind = 'rating_received_high'",
    )
    .await;
    assert_eq!(gained - base, 1, "changing a rating does not score again");

    let e = get(
        &w.r,
        &format!("/api/v1/contributions/{}/engagement", id(&c)),
        Some(&w.reader),
    )
    .await;
    assert_eq!(e.body["rating_count"], 1);
    assert_eq!(e.body["rating_average"], 4.0);
    assert_eq!(e.body["mine"]["rating"], 4);
    let anon = get(&w.r, &format!("/api/v1/contributions/{}/engagement", id(&c)), None).await;
    assert!(anon.body["mine"].is_null());
    assert_eq!(del(&w.r, &uri, Some(&w.reader)).await.status, StatusCode::NO_CONTENT);
    assert_eq!(count(&pool, "SELECT count(*) FROM ratings").await, 0);
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn reactions_and_bookmarks_follow_publication(pool: PgPool) {
    let w = world(&pool).await;
    let c = published(&w, "original", "ml", "Mlym", "വരി").await;
    let cid = id(&c);
    assert_eq!(
        put(
            &w.r,
            &format!("/api/v1/contributions/{cid}/reactions/like"),
            json!({}),
            Some(&w.reader)
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        put(
            &w.r,
            &format!("/api/v1/contributions/{cid}/reactions/like"),
            json!({}),
            Some(&w.reader)
        )
        .await
        .status,
        StatusCode::NO_CONTENT,
        "idempotent"
    );
    put(
        &w.r,
        &format!("/api/v1/contributions/{cid}/reactions/boo"),
        json!({}),
        Some(&w.reader),
    )
    .await
    .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    assert_eq!(
        put(
            &w.r,
            &format!("/api/v1/contributions/{cid}/bookmark"),
            json!({}),
            Some(&w.reader)
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    let e = get(
        &w.r,
        &format!("/api/v1/contributions/{cid}/engagement"),
        Some(&w.reader),
    )
    .await;
    assert_eq!(e.body["reactions"], json!([{ "kind": "like", "count": 1 }]));
    assert_eq!(e.body["mine"]["bookmarked"], true);
    assert_eq!(
        get(&w.r, "/api/v1/me/bookmarks", Some(&w.reader)).await.body["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    sqlx::query("UPDATE contributions SET status = 'rejected', is_primary = false, published_revision_id = NULL WHERE id = $1::uuid")
        .bind(&cid)
        .execute(&pool)
        .await
        .unwrap();
    // Unpublished: the page, its comments and the member's bookmark list all stop showing it.
    get(&w.r, &format!("/api/v1/contributions/{cid}/engagement"), None)
        .await
        .assert_problem(StatusCode::NOT_FOUND, "not_found");
    get(&w.r, &format!("/api/v1/contributions/{cid}/comments"), None)
        .await
        .assert_problem(StatusCode::NOT_FOUND, "not_found");
    put(
        &w.r,
        &format!("/api/v1/contributions/{cid}/bookmark"),
        json!({}),
        Some(&w.author),
    )
    .await
    .assert_problem(StatusCode::NOT_FOUND, "not_found");
    assert!(get(&w.r, "/api/v1/me/bookmarks", Some(&w.reader)).await.body["items"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn reputation_is_an_append_only_log_and_not_a_role(pool: PgPool) {
    let w = world(&pool).await;
    published(&w, "original", "ml", "Mlym", "വരി").await;
    // Publishing earned the author points, once, as an event.
    let me = get(&w.r, "/api/v1/me/reputation", Some(&w.author)).await;
    assert_eq!(me.body["score"], 10);
    assert_eq!(me.body["items"][0]["kind"], "contribution_published");
    // The log cannot be rewritten, even by SQL.
    for q in [
        "UPDATE reputation_events SET points = 999",
        "DELETE FROM reputation_events",
        "TRUNCATE reputation_events",
    ] {
        assert!(sqlx::query(q).execute(&pool).await.is_err(), "{q}");
    }
    // A huge score changes no authority: moderation still refuses, and the refusal is audited.
    sqlx::query("INSERT INTO reputation_events (user_id, kind, points, subject_type, subject_id) VALUES ($1, 'test_grant', 1000000, 'user', $1)")
        .bind(w.author_id)
        .execute(&pool)
        .await
        .unwrap();
    let profile = get(&w.r, &format!("/api/v1/users/{}/profile", w.author_id), None).await;
    assert_eq!(profile.body["level"], "trusted");
    assert_eq!(profile.body["roles"], json!([]), "level is a label, not a role");
    get(&w.r, "/api/v1/moderation/reports", Some(&w.author))
        .await
        .assert_problem(StatusCode::FORBIDDEN, "forbidden");
    assert_eq!(audit_count(&pool, "moderation.denied", w.author_id).await, 1);
    let admin = get(&w.r, &format!("/api/v1/users/{}/profile", w.author_id), None).await;
    assert!(
        !admin.body.to_string().contains("author@example.com"),
        "no email on profiles"
    );
}

// ---------- comments, reports, moderation ----------

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn comments_validate_and_conflict_on_stale_edits(pool: PgPool) {
    let w = world(&pool).await;
    let c = published(&w, "original", "ml", "Mlym", "വരി").await;
    let cid = id(&c);
    comment(&w, "", &cid, "hi")
        .await
        .assert_problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    comment(&w, &w.reader, &cid, "   ")
        .await
        .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    let long = "ا".repeat(2001);
    let res = comment(&w, &w.reader, &cid, &long).await;
    res.assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    assert_eq!(res.body["errors"][0]["code"], "too_long");
    comment(&w, &w.reader, &Uuid::new_v4().to_string(), "x")
        .await
        .assert_problem(StatusCode::NOT_FOUND, "not_found");

    let made = comment(&w, &w.reader, &cid, "ما شاء الله، جميل").await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    assert_eq!(made.body["version"], 1);
    let uri = format!("/api/v1/comments/{}", id(&made.body));
    // Only the author edits, and a stale version is a conflict, not an overwrite.
    patch(
        &w.r,
        &uri,
        json!({ "expected_version": 1, "body": "x" }),
        Some(&w.author),
    )
    .await
    .assert_problem(StatusCode::NOT_FOUND, "not_found");
    let ok = patch(
        &w.r,
        &uri,
        json!({ "expected_version": 1, "body": "جميل جدا" }),
        Some(&w.reader),
    )
    .await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.body);
    let stale = patch(
        &w.r,
        &uri,
        json!({ "expected_version": 1, "body": "again" }),
        Some(&w.reader),
    )
    .await;
    stale.assert_problem(StatusCode::CONFLICT, "version_conflict");
    assert_eq!(stale.body["current_version"], 2);
    let list = get(&w.r, &format!("/api/v1/contributions/{cid}/comments"), None).await;
    assert_eq!(list.body["items"][0]["body"], "جميل جدا");
    assert_eq!(del(&w.r, &uri, Some(&w.reader)).await.status, StatusCode::NO_CONTENT);
    assert!(get(&w.r, &format!("/api/v1/contributions/{cid}/comments"), None)
        .await
        .body["items"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn unauthorized_moderation_is_refused_and_audited_while_moderators_act_and_are_audited(pool: PgPool) {
    let w = world(&pool).await;
    let c = published(&w, "original", "ml", "Mlym", "വരി").await;
    let cm = comment(&w, &w.reader, &id(&c), "a comment").await.body;
    let rep = post(
        &w.r,
        "/api/v1/reports",
        json!({ "target_type": "comment", "target_id": id(&cm), "reason": "abuse" }),
        Some(&w.author),
    )
    .await;
    assert_eq!(rep.status, StatusCode::CREATED, "{}", rep.body);

    // Anonymous: 401 (no actor to audit). A member: 403, and each refusal leaves an audit row.
    get(&w.r, "/api/v1/moderation/reports", None)
        .await
        .assert_problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    let attempts: Vec<Res> = vec![
        get(&w.r, "/api/v1/moderation/reports", Some(&w.reader)).await,
        post(
            &w.r,
            &format!("/api/v1/moderation/reports/{}/resolve", id(&rep.body)),
            json!({ "resolution": "upheld" }),
            Some(&w.reader),
        )
        .await,
        post(
            &w.r,
            &format!("/api/v1/moderation/comments/{}/hide", id(&cm)),
            json!({}),
            Some(&w.reader),
        )
        .await,
        post(
            &w.r,
            "/api/v1/moderation/notes",
            json!({ "subject_type": "comment", "subject_id": id(&cm), "body": "n" }),
            Some(&w.reader),
        )
        .await,
        get(
            &w.r,
            &format!("/api/v1/moderation/notes?subject_type=comment&subject_id={}", id(&cm)),
            Some(&w.reader),
        )
        .await,
        get(&w.r, "/api/v1/moderation/appeals", Some(&w.reader)).await,
    ];
    for a in &attempts {
        a.assert_problem(StatusCode::FORBIDDEN, "forbidden");
    }
    assert_eq!(
        audit_count(&pool, "moderation.denied", w.reader_id).await,
        attempts.len() as i64
    );
    // Nothing changed.
    assert_eq!(
        count(&pool, "SELECT count(*) FROM reports WHERE status = 'open'").await,
        1
    );
    assert_eq!(
        count(&pool, "SELECT count(*) FROM comments WHERE status = 'visible'").await,
        1
    );
    // A moderator may not be impersonated by a client header or body.
    let sneaky = post(
        &w.r,
        &format!("/api/v1/moderation/comments/{}/hide", id(&cm)),
        json!({ "role": "moderator" }),
        Some(&w.reader),
    )
    .await;
    assert_eq!(sneaky.status, StatusCode::FORBIDDEN);

    // The moderator works the queue; the decision and the hide are audited.
    let queue = get(&w.r, "/api/v1/moderation/reports", Some(&w.moderator)).await;
    assert_eq!(queue.body["items"].as_array().unwrap().len(), 1);
    let done = post(
        &w.r,
        &format!("/api/v1/moderation/reports/{}/resolve", id(&rep.body)),
        json!({ "resolution": "upheld", "action": "hide_comment", "note": "abusive" }),
        Some(&w.moderator),
    )
    .await;
    assert_eq!(done.status, StatusCode::OK, "{}", done.body);
    assert_eq!(done.body["status"], "upheld");
    assert!(get(&w.r, &format!("/api/v1/contributions/{}/comments", id(&c)), None)
        .await
        .body["items"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM audit_log WHERE action IN ('report.resolved', 'comment.hidden', 'report.created')"
        )
        .await,
        3
    );
    // Consequences land in the log: the reporter gains, the author loses.
    assert_eq!(count(&pool, &format!("SELECT coalesce(sum(points),0) FROM reputation_events WHERE user_id = '{}' AND kind = 'report_upheld_against'", w.reader_id)).await, -5);
    assert_eq!(
        count(
            &pool,
            &format!(
                "SELECT coalesce(sum(points),0) FROM reputation_events WHERE user_id = '{}' AND kind = 'report_upheld'",
                w.author_id
            )
        )
        .await,
        2
    );
    // Notes are append-only and visible to moderators only.
    let n = post(
        &w.r,
        "/api/v1/moderation/notes",
        json!({ "subject_type": "comment", "subject_id": id(&cm), "body": "second strike" }),
        Some(&w.moderator),
    )
    .await;
    assert_eq!(n.status, StatusCode::CREATED, "{}", n.body);
    assert_eq!(
        get(
            &w.r,
            &format!("/api/v1/moderation/notes?subject_type=comment&subject_id={}", id(&cm)),
            Some(&w.moderator)
        )
        .await
        .body["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(sqlx::query("UPDATE moderator_notes SET body = 'x'")
        .execute(&pool)
        .await
        .is_err());
    post(
        &w.r,
        "/api/v1/moderation/notes",
        json!({ "subject_type": "comment", "subject_id": Uuid::new_v4(), "body": "n" }),
        Some(&w.moderator),
    )
    .await
    .assert_problem(StatusCode::NOT_FOUND, "not_found");
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn reports_are_validated_deduplicated_and_decided_once_even_when_racing(pool: PgPool) {
    let w = world(&pool).await;
    let c = published(&w, "original", "ml", "Mlym", "വരി").await;
    let cm = comment(&w, &w.reader, &id(&c), "questionable").await.body;
    file_report(&w, &w.reader, &id(&cm), "spam")
        .await
        .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed"); // own content
    file_report(&w, &w.author, &id(&cm), "weird")
        .await
        .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    file_report(&w, &w.author, &Uuid::new_v4().to_string(), "spam")
        .await
        .assert_problem(StatusCode::NOT_FOUND, "not_found");
    post(
        &w.r,
        "/api/v1/reports",
        json!({ "target_type": "user", "target_id": id(&cm), "reason": "spam" }),
        Some(&w.author),
    )
    .await
    .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    let first = file_report(&w, &w.author, &id(&cm), "spam").await;
    assert_eq!(first.status, StatusCode::CREATED);
    file_report(&w, &w.author, &id(&cm), "spam")
        .await
        .assert_problem(StatusCode::CONFLICT, "report_exists");

    // Two moderators decide the same report at the same moment: exactly one wins.
    let uri = format!("/api/v1/moderation/reports/{}/resolve", id(&first.body));
    let (a, b) = tokio::join!(
        post(&w.r, &uri, json!({ "resolution": "dismissed" }), Some(&w.moderator)),
        post(&w.r, &uri, json!({ "resolution": "upheld" }), Some(&w.moderator2)),
    );
    let mut codes = [a.status.as_u16(), b.status.as_u16()];
    codes.sort_unstable();
    assert_eq!(codes, [200, 409], "{} / {}", a.body, b.body);
    let loser = if a.status == StatusCode::CONFLICT { &a } else { &b };
    assert_eq!(loser.code(), "already_resolved");
    assert_eq!(
        count(&pool, "SELECT count(*) FROM audit_log WHERE action = 'report.resolved'").await,
        1
    );
    // hide_comment only makes sense when upholding a comment report.
    let other = file_report(&w, &w.moderator, &id(&cm), "abuse").await;
    let bad = post(
        &w.r,
        &format!("/api/v1/moderation/reports/{}/resolve", id(&other.body)),
        json!({ "resolution": "dismissed", "action": "hide_comment" }),
        Some(&w.moderator),
    )
    .await;
    bad.assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn an_appeal_goes_to_a_different_moderator_and_restores_the_comment(pool: PgPool) {
    let w = world(&pool).await;
    let c = published(&w, "original", "ml", "Mlym", "വരി").await;
    let cm = comment(&w, &w.reader, &id(&c), "hide me, wrongly").await.body;
    let hide = post(
        &w.r,
        &format!("/api/v1/moderation/comments/{}/hide", id(&cm)),
        json!({ "note": "mistake" }),
        Some(&w.moderator),
    )
    .await;
    assert_eq!(hide.status, StatusCode::OK, "{}", hide.body);
    assert_eq!(hide.body["status"], "hidden");
    assert_eq!(rep_sum(&w, w.reader_id).await, -3, "hiding costs reputation");

    appeal(&w, &w.author, &id(&cm))
        .await
        .assert_problem(StatusCode::NOT_FOUND, "not_found"); // not the author
    let a = appeal(&w, &w.reader, &id(&cm)).await;
    assert_eq!(a.status, StatusCode::CREATED, "{}", a.body);
    // The author can still see their hidden comment (to appeal it); nobody else's list includes it.
    let hidden = get(&w.r, "/api/v1/me/comments", Some(&w.reader)).await;
    assert_eq!(hidden.body["items"][0]["status"], "hidden");
    assert!(get(&w.r, "/api/v1/me/comments", Some(&w.author)).await.body["items"]
        .as_array()
        .unwrap()
        .is_empty());
    appeal(&w, &w.reader, &id(&cm))
        .await
        .assert_problem(StatusCode::CONFLICT, "appeal_pending");
    assert_eq!(
        get(&w.r, "/api/v1/me/appeals", Some(&w.reader)).await.body["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let aid = id(&a.body);
    decide(&w, &aid, &w.reader, "upheld")
        .await
        .assert_problem(StatusCode::FORBIDDEN, "forbidden");
    // The moderator who hid it cannot review the appeal against their own decision.
    decide(&w, &aid, &w.moderator, "upheld")
        .await
        .assert_problem(StatusCode::FORBIDDEN, "own_decision");
    decide(&w, &aid, &w.moderator2, "maybe")
        .await
        .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    let done = decide(&w, &aid, &w.moderator2, "upheld").await;
    assert_eq!(done.status, StatusCode::OK, "{}", done.body);
    assert_eq!(done.body["status"], "upheld");
    decide(&w, &aid, &w.moderator2, "denied")
        .await
        .assert_problem(StatusCode::CONFLICT, "already_resolved");
    assert_eq!(
        get(&w.r, &format!("/api/v1/contributions/{}/comments", id(&c)), None)
            .await
            .body["items"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "restored"
    );
    assert_eq!(
        rep_sum(&w, w.reader_id).await,
        0,
        "the penalty was reversed, by a new event"
    );
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM audit_log WHERE action IN ('appeal.created', 'appeal.decided')"
        )
        .await,
        2
    );
    appeal(&w, &w.reader, &id(&cm))
        .await
        .assert_problem(StatusCode::CONFLICT, "nothing_to_appeal");
}

// ---------- abuse controls ----------

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn limits_spam_and_thresholds_are_configurable_by_admins_only(pool: PgPool) {
    let w = world(&pool).await;
    let c = published(&w, "original", "ml", "Mlym", "വരി").await;
    let cid = id(&c);
    // Only an admin may read or change the rules.
    get(&w.r, "/api/v1/admin/community-settings", Some(&w.moderator))
        .await
        .assert_problem(StatusCode::FORBIDDEN, "forbidden");
    put(&w.r, "/api/v1/admin/community-settings", json!({}), Some(&w.reader))
        .await
        .assert_problem(StatusCode::FORBIDDEN, "forbidden");
    let mut bad = get(&w.r, "/api/v1/admin/community-settings", Some(&w.admin)).await.body;
    bad["levels"] = json!([]);
    put(&w.r, "/api/v1/admin/community-settings", bad, Some(&w.admin))
        .await
        .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");

    // Spam heuristics.
    let spam = comment(
        &w,
        &w.reader,
        &cid,
        "buy at http://a.example http://b.example http://c.example",
    )
    .await;
    spam.assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    assert_eq!(spam.body["errors"][0]["code"], "too_many_links");
    comment(&w, &w.reader, &cid, "aaaaaaaaaaaaaaaaaaaa")
        .await
        .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    set_community(&w, |s| s["spam"]["blocked_terms"] = json!(["Casino"])).await;
    let blocked = comment(&w, &w.reader, &cid, "best CASINO bonus").await;
    assert_eq!(blocked.body["errors"][0]["code"], "blocked_term");
    assert_eq!(
        comment(&w, &w.reader, &cid, "lovely lyric").await.status,
        StatusCode::CREATED
    );
    let dup = comment(&w, &w.reader, &cid, "Lovely lyric").await;
    assert_eq!(dup.body["errors"][0]["code"], "duplicate_content");
    assert_eq!(
        audit_count(&pool, "settings.community_changed", count_admin(&pool).await).await,
        1
    );

    // Length limits come from settings too.
    set_community(&w, |s| s["limits"]["comment_chars"] = json!(10)).await;
    assert_eq!(
        comment(&w, &w.reader, &cid, "this is far too long").await.body["errors"][0]["code"],
        "too_long"
    );

    // A reputation threshold gates the action; a newcomer is refused, a moderator is not, and
    // earning points opens it.
    set_community(&w, |s| {
        s["limits"]["comment_chars"] = json!(2000);
        s["min_reputation"]["comment"] = json!(5);
    })
    .await;
    comment(&w, &w.reader, &cid, "another one")
        .await
        .assert_problem(StatusCode::FORBIDDEN, "reputation_too_low");
    assert_eq!(
        comment(&w, &w.author, &cid, "I wrote this").await.status,
        StatusCode::CREATED,
        "author has 10 points"
    );
    assert_eq!(
        comment(&w, &w.moderator, &cid, "moderator is exempt").await.status,
        StatusCode::CREATED
    );
}

async fn count_admin(pool: &PgPool) -> Uuid {
    sqlx::query_scalar("SELECT id FROM users WHERE email = 'admin@example.com'")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn rate_limits_hold_under_a_concurrent_burst(pool: PgPool) {
    let w = world(&pool).await;
    let c = published(&w, "original", "ml", "Mlym", "വരി").await;
    set_community(&w, |s| s["rate"]["comment"] = json!({ "max": 3, "window_secs": 600 })).await;
    let mut tasks = Vec::new();
    for i in 0..8 {
        let (r, cookie, cid) = (w.r.clone(), w.reader.clone(), id(&c));
        tasks.push(tokio::spawn(async move {
            post(
                &r,
                &format!("/api/v1/contributions/{cid}/comments"),
                json!({ "body": format!("distinct comment number {i}") }),
                Some(&cookie),
            )
            .await
        }));
    }
    let mut results = Vec::new();
    for t in tasks {
        results.push(t.await.unwrap());
    }
    let ok = results.iter().filter(|r| r.status == StatusCode::CREATED).count();
    let limited: Vec<&Res> = results
        .iter()
        .filter(|r| r.status == StatusCode::TOO_MANY_REQUESTS)
        .collect();
    assert_eq!(ok, 3, "exactly the allowed number get through");
    assert_eq!(limited.len(), 5);
    limited[0].assert_problem(StatusCode::TOO_MANY_REQUESTS, "rate_limited");
    assert!(limited[0].body["retry_after_secs"].as_u64().unwrap() > 0);
    assert!(limited[0].headers.contains_key("retry-after"));
    assert_eq!(count(&pool, "SELECT count(*) FROM comments").await, 3);
    // Other members have their own budget.
    assert_eq!(
        comment(&w, &w.author, &id(&c), "my own window").await.status,
        StatusCode::CREATED
    );
    // Reports are throttled separately.
    set_community(&w, |s| s["rate"]["report"] = json!({ "max": 1, "window_secs": 3600 })).await;
    let cm = comment(&w, &w.author, &id(&c), "something else entirely").await.body;
    let extra = comment(&w, &w.moderator, &id(&c), "target for reports").await.body;
    assert_eq!(
        post(
            &w.r,
            "/api/v1/reports",
            json!({ "target_type": "comment", "target_id": id(&extra), "reason": "spam" }),
            Some(&w.reader)
        )
        .await
        .status,
        StatusCode::CREATED
    );
    post(
        &w.r,
        "/api/v1/reports",
        json!({ "target_type": "comment", "target_id": id(&cm), "reason": "spam" }),
        Some(&w.reader),
    )
    .await
    .assert_problem(StatusCode::TOO_MANY_REQUESTS, "rate_limited");
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn profiles_show_public_facts_and_bios_are_checked(pool: PgPool) {
    let w = world(&pool).await;
    published(&w, "original", "ml", "Mlym", "വരി").await;
    let ok = put(
        &w.r,
        "/api/v1/me/profile",
        json!({ "bio": "Lyrics lover, ഹലോ" }),
        Some(&w.author),
    )
    .await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.body);
    let p = get(&w.r, &format!("/api/v1/users/{}/profile", w.author_id), None)
        .await
        .body;
    assert_eq!(p["bio"], "Lyrics lover, ഹലോ");
    assert_eq!(p["published_contributions"], 1);
    assert_eq!(p["reputation"], 10);
    assert_eq!(p["roles"], json!([]));
    let m = get(&w.r, &format!("/api/v1/users/{}/profile", w.moderator_id), None)
        .await
        .body;
    assert_eq!(
        m["roles"],
        json!(["moderator"]),
        "roles are explicit and separate from level"
    );
    put(
        &w.r,
        "/api/v1/me/profile",
        json!({ "bio": "x".repeat(501) }),
        Some(&w.author),
    )
    .await
    .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    put(&w.r, "/api/v1/me/profile", json!({ "bio": "hi" }), None)
        .await
        .assert_problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    get(&w.r, &format!("/api/v1/users/{}/profile", Uuid::new_v4()), None)
        .await
        .assert_problem(StatusCode::NOT_FOUND, "not_found");
}
