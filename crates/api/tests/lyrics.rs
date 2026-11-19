//! Lyric contributions: slots and primaries, immutable revisions, stale writes, trust policy,
//! moderation, idempotency and what the public can and cannot see.

mod common;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use common::*;
use domain::Role;
use serde_json::{json, Value};
use sqlx::PgPool;

fn id(v: &Value) -> String {
    v["id"].as_str().unwrap().to_owned()
}

async fn count(pool: &PgPool, sql: &str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
}

/// A composition with one recording; returns the recording id.
async fn recording(r: &Router, cookie: &str) -> String {
    let w = post(
        r,
        "/api/v1/compositions",
        json!({ "title": "Kaatrin Mozhi" }),
        Some(cookie),
    )
    .await;
    assert_eq!(w.status, StatusCode::CREATED, "{}", w.body);
    let rec = post(
        r,
        "/api/v1/recordings",
        json!({ "composition_id": id(&w.body), "title": "Take 1" }),
        Some(cookie),
    )
    .await;
    assert_eq!(rec.status, StatusCode::CREATED, "{}", rec.body);
    id(&rec.body)
}

fn lyric(kind: &str, lang: &str, script: &str, body: &str) -> Value {
    json!({ "type": kind, "language": lang, "script": script, "body": body })
}

async fn contribute(r: &Router, cookie: &str, rec: &str, b: Value) -> Res {
    post(r, &format!("/api/v1/recordings/{rec}/contributions"), b, Some(cookie)).await
}

async fn with_key(r: &Router, uri: &str, body: Value, cookie: &str, key: &str) -> Res {
    let req = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header("x-requested-with", "test")
        .header("idempotency-key", key)
        .header(header::COOKIE, cookie)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    raw(r, req).await
}

async fn revise(r: &Router, cookie: Option<&str>, c: &Value, body: &str) -> Res {
    post(
        r,
        &format!("/api/v1/contributions/{}/revisions", id(c)),
        json!({ "expected_parent_revision_id": c["current_revision_id"], "body": body }),
        cookie,
    )
    .await
}

async fn decide(r: &Router, cookie: Option<&str>, c: &Value, verb: &str) -> Res {
    post(
        r,
        &format!("/api/v1/contributions/{}/{verb}", id(c)),
        json!({ "expected_parent_revision_id": c["current_revision_id"] }),
        cookie,
    )
    .await
}

async fn fetch(r: &Router, cookie: Option<&str>, c: &Value) -> Value {
    let res = get(r, &format!("/api/v1/contributions/{}", id(c)), cookie).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.body);
    res.body
}

struct World {
    r: Router,
    member: String,
    member2: String,
    moderator: String,
    admin: String,
    rec: String,
}

async fn world(pool: &PgPool) -> World {
    let r = router(pool);
    let (_, member) = signed_in(pool, &r, "m1@example.com", None).await;
    let (_, member2) = signed_in(pool, &r, "m2@example.com", None).await;
    let (_, moderator) = signed_in(pool, &r, "mod@example.com", Some(Role::Moderator)).await;
    let (_, admin) = signed_in(pool, &r, "admin@example.com", Some(Role::Admin)).await;
    let rec = recording(&r, &member).await;
    World {
        r,
        member,
        member2,
        moderator,
        admin,
        rec,
    }
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn translation_can_exist_before_any_original(pool: PgPool) {
    let w = world(&pool).await;
    let ml = lyric("translation", "ml", "Mlym", "കാറ്റിൻ മൊഴി\u{200d}");
    let t = contribute(&w.r, &w.member, &w.rec, ml).await;
    assert_eq!(t.status, StatusCode::CREATED, "{}", t.body);
    assert_eq!(t.body["status"], "pending", "members go through review by default");
    assert_eq!(t.body["is_primary"], false);
    assert_eq!(t.body["dir"], "ltr");
    assert_eq!(
        count(&pool, "SELECT count(*) FROM contributions WHERE type = 'original'").await,
        0
    );

    // Moderator publishes it: first of its slot, so it becomes primary — no original needed.
    let t = fetch(&w.r, Some(&w.member), &t.body).await;
    let done = decide(&w.r, Some(&w.moderator), &t, "publish").await;
    assert_eq!(done.status, StatusCode::OK, "{}", done.body);
    assert_eq!(done.body["status"], "published");
    assert_eq!(done.body["is_primary"], true);

    // An original arriving later is its own slot and does not disturb the translation.
    let o = contribute(&w.r, &w.moderator, &w.rec, lyric("original", "ml", "Mlym", "ഒറിജിനൽ")).await;
    assert_eq!(o.body["is_primary"], true);
    let list = get(&w.r, &format!("/api/v1/recordings/{}/contributions", w.rec), None).await;
    assert_eq!(list.body["items"].as_array().unwrap().len(), 2);
    // The web client adds `lang` to every read; lyric endpoints must accept and ignore it.
    let with_lang = get(
        &w.r,
        &format!("/api/v1/recordings/{}/contributions?lang=ar&type=original", w.rec),
        None,
    )
    .await;
    assert_eq!(with_lang.status, StatusCode::OK, "{}", with_lang.body);
    assert_eq!(with_lang.body["items"].as_array().unwrap().len(), 1);
    // Malayalam joiners survive byte for byte, and the source pointer is optional.
    let public = fetch(&w.r, None, &t).await;
    assert_eq!(public["published"]["body"], "കാറ്റിൻ മൊഴി\u{200d}");
    assert!(public["source_contribution_id"].is_null());
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn two_concurrent_first_submissions_cannot_both_become_primary(pool: PgPool) {
    let w = world(&pool).await;
    let (_, mod2) = signed_in(&pool, &w.r, "mod2@example.com", Some(Role::Moderator)).await;
    for round in 0..8 {
        let rec = recording(&w.r, &w.member).await;
        // Both are moderators, so both auto-publish into the same empty slot at once.
        let (a, b) = tokio::join!(
            contribute(
                &w.r,
                &w.moderator,
                &rec,
                lyric("original", "ml", "Mlym", &format!("a{round}"))
            ),
            contribute(&w.r, &mod2, &rec, lyric("original", "ml", "Mlym", &format!("b{round}"))),
        );
        assert_eq!(
            (a.status, b.status),
            (StatusCode::CREATED, StatusCode::CREATED),
            "{} {}",
            a.body,
            b.body
        );
        let primaries = [&a.body, &b.body].iter().filter(|c| c["is_primary"] == true).count();
        assert_eq!(primaries, 1, "round {round}: exactly one primary");
        let in_db = count(
            &pool,
            &format!("SELECT count(*) FROM contributions WHERE recording_id = '{rec}' AND is_primary"),
        )
        .await;
        assert_eq!(in_db, 1);
        let published = count(
            &pool,
            &format!("SELECT count(*) FROM contributions WHERE recording_id = '{rec}' AND status = 'published'"),
        )
        .await;
        assert_eq!(published, 2, "the loser is a published alternative, not dropped");
    }

    // Reviewed path: two pending submissions published by two moderators at the same moment.
    let rec = recording(&w.r, &w.member).await;
    let a = contribute(&w.r, &w.member, &rec, lyric("translation", "ar", "Arab", "واحد")).await;
    let b = contribute(&w.r, &w.member2, &rec, lyric("translation", "ar", "Arab", "اثنان")).await;
    let (x, y) = tokio::join!(
        decide(&w.r, Some(&w.moderator), &a.body, "publish"),
        decide(&w.r, Some(&mod2), &b.body, "publish")
    );
    assert_eq!((x.status, y.status), (StatusCode::OK, StatusCode::OK));
    assert_eq!(
        count(
            &pool,
            &format!("SELECT count(*) FROM contributions WHERE recording_id = '{rec}' AND is_primary")
        )
        .await,
        1
    );

    // The database refuses a second primary even if the application were bypassed.
    let err = sqlx::query(&format!(
        "UPDATE contributions SET is_primary = true WHERE recording_id = '{rec}' AND NOT is_primary"
    ))
    .execute(&pool)
    .await
    .unwrap_err();
    assert!(err.to_string().contains("contributions_primary_key"), "{err}");
    // ...and a primary must be published.
    let err = sqlx::query("UPDATE contributions SET published_revision_id = NULL, status = 'pending' WHERE is_primary")
        .execute(&pool)
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("contributions_primary_published_check"),
        "{err}"
    );
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn alternatives_are_kept_apart_and_never_merged(pool: PgPool) {
    let w = world(&pool).await;
    let first = contribute(
        &w.r,
        &w.moderator,
        &w.rec,
        lyric("translation", "en", "Latn", "Wind\nsong"),
    )
    .await;
    let alt = contribute(
        &w.r,
        &w.member2,
        &w.rec,
        lyric("translation", "en", "Latn", "Song of\nthe wind"),
    )
    .await;
    assert_eq!(first.body["is_primary"], true);
    let alt = decide(
        &w.r,
        Some(&w.moderator),
        &fetch(&w.r, Some(&w.member2), &alt.body).await,
        "publish",
    )
    .await;
    assert_eq!(alt.body["is_primary"], false, "a later one is an alternative");
    assert_eq!(alt.body["published"]["body"], "Song of\nthe wind");
    assert_ne!(id(&first.body), id(&alt.body));
    assert_eq!(fetch(&w.r, None, &first.body).await["published"]["body"], "Wind\nsong");

    // Every contribution keeps its own history; comparing across them is refused.
    let (ra, rb) = (
        first.body["published"]["id"].as_str().unwrap(),
        alt.body["published"]["id"].as_str().unwrap(),
    );
    let d = get(&w.r, &format!("/api/v1/revisions/{ra}/diff/{rb}"), None).await;
    d.assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    assert_eq!(d.body["errors"][0]["code"], "different_contribution");
    // A different member cannot edit someone's published version: they add an alternative instead.
    let c = fetch(&w.r, Some(&w.moderator), &first.body).await;
    revise(&w.r, Some(&w.member2), &c, "hijack")
        .await
        .assert_problem(StatusCode::FORBIDDEN, "forbidden");
    assert_eq!(count(&pool, "SELECT count(*) FROM contributions").await, 2);
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn stale_parent_gives_409_with_current_revision(pool: PgPool) {
    let w = world(&pool).await;
    let c = contribute(&w.r, &w.member, &w.rec, lyric("original", "ml", "Mlym", "v1"))
        .await
        .body;
    let v1 = c["current_revision_id"].clone();
    assert_eq!(
        revise(&w.r, Some(&w.member), &c, "v2").await.status,
        StatusCode::CREATED
    );
    let latest = fetch(&w.r, Some(&w.member), &c).await["current_revision_id"].clone();
    assert_ne!(latest, v1);

    let stale = post(
        &w.r,
        &format!("/api/v1/contributions/{}/revisions", id(&c)),
        json!({ "expected_parent_revision_id": v1, "body": "late" }),
        Some(&w.member),
    )
    .await;
    stale.assert_problem(StatusCode::CONFLICT, "stale_revision");
    assert_eq!(stale.body["current_revision_id"], latest);
    // Missing parent is a validation error, not a blind write.
    let none = post(
        &w.r,
        &format!("/api/v1/contributions/{}/revisions", id(&c)),
        json!({ "body": "x" }),
        Some(&w.member),
    )
    .await;
    assert_eq!(none.body["errors"][0]["code"], "required");
    // A moderator who reviewed v1 cannot approve after the author moved on: nothing changes.
    let stale_review = post(
        &w.r,
        &format!("/api/v1/contributions/{}/publish", id(&c)),
        json!({ "expected_parent_revision_id": v1 }),
        Some(&w.moderator),
    )
    .await;
    stale_review.assert_problem(StatusCode::CONFLICT, "stale_revision");
    assert_eq!(stale_review.body["current_revision_id"], latest);
    assert_eq!(fetch(&w.r, Some(&w.member), &c).await["status"], "pending");

    // Two edits from the same parent at the same time: one wins, one is stale; history stays linear.
    let c = fetch(&w.r, Some(&w.member), &c).await;
    let (a, b) = tokio::join!(
        revise(&w.r, Some(&w.member), &c, "left"),
        revise(&w.r, Some(&w.moderator), &c, "right")
    );
    let mut codes = [a.status, b.status];
    codes.sort();
    assert_eq!(
        codes,
        [StatusCode::CREATED, StatusCode::CONFLICT],
        "{} / {}",
        a.body,
        b.body
    );
    let loser = if a.status == StatusCode::CONFLICT { &a } else { &b };
    assert_eq!(loser.code(), "stale_revision");
    let chain: Vec<(i32, Option<uuid::Uuid>)> = sqlx::query_as(
        "SELECT revision_no, parent_revision_id FROM lyric_revisions WHERE contribution_id = $1 ORDER BY revision_no",
    )
    .bind(uuid::Uuid::parse_str(&id(&c)).unwrap())
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(chain.iter().map(|c| c.0).collect::<Vec<_>>(), vec![1, 2, 3]);
    assert!(chain[0].1.is_none() && chain[1].1.is_some() && chain[2].1.is_some());
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn history_is_immutable_and_restore_adds_a_revision(pool: PgPool) {
    let w = world(&pool).await;
    let c = contribute(&w.r, &w.moderator, &w.rec, lyric("original", "ar", "Arab", "الأول\nسطر"))
        .await
        .body;
    let r1 = c["published"]["id"].as_str().unwrap().to_owned();
    let c2 = revise(&w.r, Some(&w.moderator), &c, "الثاني\nسطر").await.body;
    let r2 = c2["current_revision_id"].as_str().unwrap().to_owned();

    // Restore revision 1: a *new* revision with the old text; nothing is rewritten.
    let restored = post(
        &w.r,
        &format!("/api/v1/contributions/{}/revisions", id(&c)),
        json!({ "expected_parent_revision_id": r2, "restore_from_revision_id": r1, "note": "back" }),
        Some(&w.moderator),
    )
    .await;
    assert_eq!(restored.status, StatusCode::CREATED, "{}", restored.body);
    let r3 = restored.body["head"].clone();
    assert_eq!(r3["revision_no"], 3);
    assert_eq!(r3["body"], "الأول\nسطر");
    assert_eq!(r3["restored_from_revision_id"], r1);
    assert_eq!(r3["parent_revision_id"], r2);
    let again = restored.body["current_revision_id"].clone();
    let unchanged = post(
        &w.r,
        &format!("/api/v1/contributions/{}/revisions", id(&c)),
        json!({ "expected_parent_revision_id": again, "restore_from_revision_id": r3["id"] }),
        Some(&w.moderator),
    )
    .await;
    assert_eq!(
        unchanged.body["errors"][0]["code"], "unchanged",
        "restoring what is already current is a no-op, refused"
    );

    let hist = get(
        &w.r,
        &format!("/api/v1/contributions/{}/revisions", id(&c)),
        Some(&w.moderator),
    )
    .await;
    let nos: Vec<i64> = hist.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["revision_no"].as_i64().unwrap())
        .collect();
    assert_eq!(nos, vec![3, 2, 1], "newest first");
    let one = get(&w.r, &format!("/api/v1/revisions/{r1}"), None).await;
    assert_eq!(one.body["body"], "الأول\nسطر", "the original revision is untouched");

    let diff = get(&w.r, &format!("/api/v1/revisions/{r1}/diff/{r2}"), Some(&w.moderator)).await;
    assert_eq!(diff.status, StatusCode::OK, "{}", diff.body);
    let ops: Vec<_> = diff.body["hunks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["op"].as_str().unwrap())
        .collect();
    assert_eq!(ops, vec!["delete", "insert", "equal"]);
    // A restore from another contribution is refused.
    let other = contribute(&w.r, &w.moderator, &w.rec, lyric("original", "en", "Latn", "x"))
        .await
        .body;
    let foreign = post(
        &w.r,
        &format!("/api/v1/contributions/{}/revisions", id(&other)),
        json!({ "expected_parent_revision_id": other["current_revision_id"], "restore_from_revision_id": r1 }),
        Some(&w.moderator),
    )
    .await;
    assert_eq!(foreign.body["errors"][0]["code"], "invalid_revision");

    // The database itself refuses to alter or remove history, for any caller.
    for sql in [
        "UPDATE lyric_revisions SET body = 'x'",
        "DELETE FROM lyric_revisions",
        "TRUNCATE lyric_revisions CASCADE",
        "UPDATE publication_events SET actor_id = actor_id",
        "DELETE FROM publication_events",
    ] {
        let e = sqlx::query(sql).execute(&pool).await.unwrap_err();
        assert!(e.to_string().contains("append-only"), "{sql}: {e}");
    }
    assert_eq!(count(&pool, "SELECT count(*) FROM lyric_revisions").await, 4);
    // Audited with before and after.
    assert_eq!(count(&pool, "SELECT count(*) FROM audit_log WHERE action = 'contribution.restored' AND before IS NOT NULL AND after IS NOT NULL").await, 1);
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn unpublished_work_is_invisible_to_the_public(pool: PgPool) {
    let w = world(&pool).await;
    let pending = contribute(&w.r, &w.member, &w.rec, lyric("original", "ml", "Mlym", "secret draft")).await;
    assert_eq!(pending.status, StatusCode::CREATED);
    let c = pending.body;
    let uri = format!("/api/v1/contributions/{}", id(&c));
    let head = c["current_revision_id"].as_str().unwrap().to_owned();

    // Nothing leaks to anonymous callers or other members, through any path.
    for who in [None, Some(w.member2.as_str())] {
        get(&w.r, &uri, who)
            .await
            .assert_problem(StatusCode::NOT_FOUND, "not_found");
        get(&w.r, &format!("{uri}/revisions"), who)
            .await
            .assert_problem(StatusCode::NOT_FOUND, "not_found");
        get(&w.r, &format!("/api/v1/revisions/{head}"), who)
            .await
            .assert_problem(StatusCode::NOT_FOUND, "not_found");
        get(&w.r, &format!("/api/v1/revisions/{head}/diff/{head}"), who)
            .await
            .assert_problem(StatusCode::NOT_FOUND, "not_found");
        let list = get(&w.r, &format!("/api/v1/recordings/{}/contributions", w.rec), who).await;
        assert_eq!(list.body["items"].as_array().unwrap().len(), 0);
    }
    assert_eq!(
        count(&pool, "SELECT count(*) FROM published_lyrics").await,
        0,
        "the read model for search/caches is empty"
    );
    assert_eq!(
        count(&pool, "SELECT count(*) FROM outbox WHERE topic = 'lyrics.published'").await,
        0
    );
    // The author and moderators see it, and the author sees it in their own list.
    let own = fetch(&w.r, Some(&w.member), &c).await;
    assert_eq!(own["head"]["body"], "secret draft");
    assert!(own["published"].is_null());
    assert_eq!(fetch(&w.r, Some(&w.moderator), &c).await["status"], "pending");
    let mine = get(&w.r, "/api/v1/me/contributions", Some(&w.member)).await;
    assert_eq!(mine.body["items"].as_array().unwrap().len(), 1);
    let queue = get(&w.r, "/api/v1/moderation/contributions", Some(&w.moderator)).await;
    assert_eq!(queue.body["items"].as_array().unwrap().len(), 1);

    // Approved is still not public; only publishing is.
    let approved = decide(&w.r, Some(&w.moderator), &own, "approve").await;
    assert_eq!(approved.body["status"], "approved");
    get(&w.r, &uri, None)
        .await
        .assert_problem(StatusCode::NOT_FOUND, "not_found");
    let published = decide(&w.r, Some(&w.moderator), &approved.body, "publish").await;
    assert_eq!(published.status, StatusCode::OK, "{}", published.body);
    let public = fetch(&w.r, None, &c).await;
    assert_eq!(public["published"]["body"], "secret draft");
    assert!(public["head"].is_null() && public["current_revision_id"].is_null() && public["review_note"].is_null());
    assert_eq!(count(&pool, "SELECT count(*) FROM published_lyrics").await, 1);
    assert_eq!(
        count(&pool, "SELECT count(*) FROM outbox WHERE topic = 'lyrics.published'").await,
        1
    );

    // A later edit under review stays private: the public keeps the published text, and the
    // intermediate revision does not appear in the public history either.
    let own = fetch(&w.r, Some(&w.member), &c).await;
    let edited = revise(&w.r, Some(&w.member), &own, "unreviewed edit").await;
    assert_eq!(edited.status, StatusCode::CREATED);
    assert_eq!(edited.body["has_unpublished_changes"], true);
    let public = fetch(&w.r, None, &c).await;
    assert_eq!(public["published"]["body"], "secret draft");
    assert_eq!(public["status"], "published");
    let hist = get(&w.r, &format!("{uri}/revisions"), None).await;
    assert_eq!(hist.body["items"].as_array().unwrap().len(), 1);
    let new_head = edited.body["current_revision_id"].as_str().unwrap();
    get(&w.r, &format!("/api/v1/revisions/{new_head}"), None)
        .await
        .assert_problem(StatusCode::NOT_FOUND, "not_found");
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM published_lyrics WHERE body LIKE '%unreviewed%'"
        )
        .await,
        0
    );

    // Rejecting the edit leaves the public text alone.
    let rej = decide(&w.r, Some(&w.moderator), &edited.body, "reject").await;
    assert_eq!(rej.body["status"], "rejected");
    assert_eq!(fetch(&w.r, None, &c).await["published"]["body"], "secret draft");

    // Responses that depend on the caller are never stored by shared caches.
    let res = get(&w.r, &uri, None).await;
    assert_eq!(res.headers[header::CACHE_CONTROL], "private, no-store");
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn trust_policy_decides_between_publish_and_review(pool: PgPool) {
    let w = world(&pool).await;
    // Default: members are reviewed, moderators publish directly.
    let m = contribute(&w.r, &w.member, &w.rec, lyric("original", "ml", "Mlym", "m")).await;
    assert_eq!(m.body["status"], "pending");
    let md = contribute(&w.r, &w.moderator, &w.rec, lyric("original", "en", "Latn", "mod")).await;
    assert_eq!(md.body["status"], "published");

    get(&w.r, "/api/v1/admin/trust-policy", Some(&w.moderator))
        .await
        .assert_problem(StatusCode::FORBIDDEN, "forbidden");
    get(&w.r, "/api/v1/admin/trust-policy", None)
        .await
        .assert_problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    let bad = send(
        &w.r,
        Method::PUT,
        "/api/v1/admin/trust-policy",
        Some(json!({ "auto_publish_roles": ["root"], "auto_publish_min_published": null })),
        Some(&w.admin),
    )
    .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);

    // Configured, not hardcoded: members with one published contribution skip review; moderators no longer do.
    let put = send(
        &w.r,
        Method::PUT,
        "/api/v1/admin/trust-policy",
        Some(json!({ "auto_publish_roles": ["admin"], "auto_publish_min_published": 1 })),
        Some(&w.admin),
    )
    .await;
    assert_eq!(put.status, StatusCode::OK, "{}", put.body);
    let m_state = fetch(&w.r, Some(&w.member), &m.body).await;
    decide(&w.r, Some(&w.admin), &m_state, "publish").await;
    let again = contribute(
        &w.r,
        &w.member,
        &w.rec,
        lyric("translation", "en", "Latn", "trusted now"),
    )
    .await;
    assert_eq!(again.body["status"], "published");
    // Moderators are no longer listed, and without a published track record they are reviewed too.
    let (_, fresh_mod) = signed_in(&pool, &w.r, "mod3@example.com", Some(Role::Moderator)).await;
    let mod_again = contribute(&w.r, &fresh_mod, &w.rec, lyric("translation", "ar", "Arab", "x")).await;
    assert_eq!(mod_again.body["status"], "pending");
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM audit_log WHERE action = 'settings.trust_changed' AND before IS NOT NULL"
        )
        .await,
        1
    );
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn moderation_and_authorization(pool: PgPool) {
    let w = world(&pool).await;
    let uri = format!("/api/v1/recordings/{}/contributions", w.rec);
    post(&w.r, &uri, lyric("original", "ml", "Mlym", "x"), None)
        .await
        .assert_problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    let c = contribute(&w.r, &w.member, &w.rec, lyric("original", "ml", "Mlym", "x"))
        .await
        .body;
    let before = count(&pool, "SELECT count(*) FROM audit_log").await;
    for verb in ["approve", "publish", "reject"] {
        decide(&w.r, None, &c, verb)
            .await
            .assert_problem(StatusCode::UNAUTHORIZED, "unauthenticated");
        decide(&w.r, Some(&w.member), &c, verb)
            .await
            .assert_problem(StatusCode::FORBIDDEN, "forbidden");
        decide(&w.r, Some(&w.member2), &c, verb)
            .await
            .assert_problem(StatusCode::FORBIDDEN, "forbidden");
    }
    get(&w.r, "/api/v1/moderation/contributions", Some(&w.member))
        .await
        .assert_problem(StatusCode::FORBIDDEN, "forbidden");
    assert_eq!(
        count(&pool, "SELECT count(*) FROM audit_log").await,
        before,
        "denied calls leave no audit rows"
    );
    assert_eq!(
        count(&pool, "SELECT count(*) FROM contributions WHERE status <> 'pending'").await,
        0
    );
    // Other members get 404 (not 403) for someone's unpublished work, so its existence is not revealed.
    revise(&w.r, Some(&w.member2), &c, "y")
        .await
        .assert_problem(StatusCode::NOT_FOUND, "not_found");

    // State machine: approve once; a second approve, or approving after rejection, is a typed conflict.
    let ok = decide(&w.r, Some(&w.moderator), &c, "approve").await;
    assert_eq!(ok.status, StatusCode::OK);
    decide(&w.r, Some(&w.moderator), &c, "approve")
        .await
        .assert_problem(StatusCode::CONFLICT, "invalid_state_transition");
    let rej = post(
        &w.r,
        &format!("/api/v1/contributions/{}/reject", id(&c)),
        json!({ "expected_parent_revision_id": c["current_revision_id"], "note": "off topic" }),
        Some(&w.moderator),
    )
    .await;
    assert_eq!(rej.body["review_note"], "off topic");
    decide(&w.r, Some(&w.moderator), &c, "publish")
        .await
        .assert_problem(StatusCode::CONFLICT, "invalid_state_transition");
    let own = fetch(&w.r, Some(&w.member), &c).await;
    assert_eq!(own["status"], "rejected");
    assert_eq!(own["review_note"], "off topic");
    // The author revises a rejected contribution: it goes back to the queue.
    assert_eq!(
        revise(&w.r, Some(&w.member), &own, "better").await.body["status"],
        "pending"
    );
    for action in [
        "contribution.created",
        "contribution.approved",
        "contribution.rejected",
        "contribution.revised",
    ] {
        assert_eq!(
            count(
                &pool,
                &format!("SELECT count(*) FROM audit_log WHERE action = '{action}'")
            )
            .await,
            1,
            "{action}"
        );
    }
    // Queue pagination uses the cursor envelope.
    for i in 0..3 {
        contribute(
            &w.r,
            &w.member2,
            &w.rec,
            lyric("original", "hi", "Deva", &format!("l{i}")),
        )
        .await;
    }
    let p1 = get(&w.r, "/api/v1/moderation/contributions?limit=2", Some(&w.moderator)).await;
    assert_eq!(p1.body["items"].as_array().unwrap().len(), 2);
    let cur = p1.body["next_cursor"].as_str().unwrap();
    let p2 = get(
        &w.r,
        &format!("/api/v1/moderation/contributions?limit=2&cursor={cur}"),
        Some(&w.moderator),
    )
    .await;
    assert_eq!(p2.body["items"].as_array().unwrap().len(), 2);
    assert!(p2.body["next_cursor"].is_null());
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn validation_is_server_side(pool: PgPool) {
    let w = world(&pool).await;
    let bad = contribute(
        &w.r,
        &w.member,
        &w.rec,
        json!({ "type": "remix", "language": "xx", "script": "Zzzz", "body": "  \n" }),
    )
    .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    let errs: Vec<(String, String)> = bad.body["errors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| (e["field"].as_str().unwrap().into(), e["code"].as_str().unwrap().into()))
        .collect();
    for want in [
        ("type", "invalid_choice"),
        ("language", "unknown_language"),
        ("script", "unknown_script"),
        ("body", "required"),
    ] {
        assert!(errs.contains(&(want.0.into(), want.1.into())), "{errs:?}");
    }
    let long = contribute(
        &w.r,
        &w.member,
        &w.rec,
        lyric("original", "en", "Latn", &"x".repeat(100_001)),
    )
    .await;
    assert_eq!(long.body["errors"][0]["code"], "too_long");
    let nul = contribute(&w.r, &w.member, &w.rec, lyric("original", "en", "Latn", "a\u{0}b")).await;
    assert_eq!(nul.body["errors"][0]["code"], "invalid_characters");
    contribute(
        &w.r,
        &w.member,
        "00000000-0000-0000-0000-000000000000",
        lyric("original", "en", "Latn", "x"),
    )
    .await
    .assert_problem(StatusCode::NOT_FOUND, "not_found");
    // Line endings are normalised; bidi marks and joiners are kept.
    let ok = contribute(
        &w.r,
        &w.member,
        &w.rec,
        lyric("original", "ar", "Arab", "سطر\r\nline\u{200f}2"),
    )
    .await;
    assert_eq!(ok.body["head"]["body"], "سطر\nline\u{200f}2");
    assert_eq!(ok.body["dir"], "rtl");
    // `source_contribution_id` must point at a contribution of this recording that you may see.
    let other_rec = recording(&w.r, &w.member).await;
    let foreign = contribute(&w.r, &w.member, &other_rec, json!({ "type": "translation", "language": "en", "script": "Latn", "body": "t", "source_contribution_id": ok.body["id"] })).await;
    assert_eq!(foreign.body["errors"][0]["code"], "invalid_source");
    let hidden = contribute(&w.r, &w.member2, &w.rec, json!({ "type": "translation", "language": "en", "script": "Latn", "body": "t", "source_contribution_id": ok.body["id"] })).await;
    assert_eq!(
        hidden.body["errors"][0]["code"], "invalid_source",
        "another member's pending work cannot be referenced"
    );
    let mine = contribute(&w.r, &w.member, &w.rec, json!({ "type": "translation", "language": "en", "script": "Latn", "body": "t", "source_contribution_id": ok.body["id"] })).await;
    assert_eq!(mine.status, StatusCode::CREATED);
    // The public does not get a pointer to a hidden source.
    let fm = fetch(&w.r, Some(&w.moderator), &mine.body).await;
    decide(&w.r, Some(&w.moderator), &fm, "publish").await;
    assert!(fetch(&w.r, None, &mine.body).await["source_contribution_id"].is_null());
    assert_eq!(
        fetch(&w.r, Some(&w.member), &mine.body).await["source_contribution_id"],
        ok.body["id"]
    );
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn idempotency_keys_make_retries_safe(pool: PgPool) {
    let w = world(&pool).await;
    let uri = format!("/api/v1/recordings/{}/contributions", w.rec);
    let body = lyric("original", "ml", "Mlym", "once");
    let a = with_key(&w.r, &uri, body.clone(), &w.member, "key-aaaa-0001").await;
    let b = with_key(&w.r, &uri, body.clone(), &w.member, "key-aaaa-0001").await;
    assert_eq!((a.status, b.status), (StatusCode::CREATED, StatusCode::CREATED));
    assert_eq!(a.body["id"], b.body["id"]);
    assert!(a.headers.get("idempotent-replayed").is_none());
    assert_eq!(b.headers["idempotent-replayed"], "true");
    assert_eq!(count(&pool, "SELECT count(*) FROM contributions").await, 1);
    assert_eq!(count(&pool, "SELECT count(*) FROM lyric_revisions").await, 1);
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM audit_log WHERE action = 'contribution.created'"
        )
        .await,
        1
    );

    // Same key, different request: refused, never silently reused.
    let c = with_key(
        &w.r,
        &uri,
        lyric("original", "ml", "Mlym", "other"),
        &w.member,
        "key-aaaa-0001",
    )
    .await;
    assert_eq!(c.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(c.body["errors"][0]["code"], "reused_with_different_request");
    // Keys are per user, and malformed keys are rejected.
    let d = with_key(&w.r, &uri, body.clone(), &w.member2, "key-aaaa-0001").await;
    assert_ne!(d.body["id"], a.body["id"]);
    assert_eq!(
        with_key(&w.r, &uri, body.clone(), &w.member, "short").await.body["errors"][0]["code"],
        "invalid"
    );

    // Two simultaneous retries with one key create exactly one contribution.
    let (x, y) = tokio::join!(
        with_key(
            &w.r,
            &uri,
            lyric("original", "hi", "Deva", "race"),
            &w.member,
            "key-race-0002"
        ),
        with_key(
            &w.r,
            &uri,
            lyric("original", "hi", "Deva", "race"),
            &w.member,
            "key-race-0002"
        ),
    );
    assert_eq!(
        (x.status, y.status),
        (StatusCode::CREATED, StatusCode::CREATED),
        "{} {}",
        x.body,
        y.body
    );
    assert_eq!(x.body["id"], y.body["id"]);
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM contributions WHERE language_id = (SELECT id FROM languages WHERE iso639_1 = 'hi')"
        )
        .await,
        1
    );

    // Revisions and decisions are retryable too.
    let c = fetch(&w.r, Some(&w.member), &a.body).await;
    let ruri = format!("/api/v1/contributions/{}/revisions", id(&c));
    let rb = json!({ "expected_parent_revision_id": c["current_revision_id"], "body": "second" });
    let r1 = with_key(&w.r, &ruri, rb.clone(), &w.member, "key-rev-00003").await;
    let r2 = with_key(&w.r, &ruri, rb, &w.member, "key-rev-00003").await;
    assert_eq!(
        (r1.status, r2.status),
        (StatusCode::CREATED, StatusCode::CREATED),
        "{}",
        r2.body
    );
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM lyric_revisions WHERE contribution_id = $$"
                .to_owned()
                .as_str()
                .replace("$$", &format!("'{}'", id(&c)))
                .as_str()
        )
        .await,
        2
    );
    let c = fetch(&w.r, Some(&w.member), &c).await;
    let duri = format!("/api/v1/contributions/{}/publish", id(&c));
    let db = json!({ "expected_parent_revision_id": c["current_revision_id"] });
    let p1 = with_key(&w.r, &duri, db.clone(), &w.moderator, "key-pub-00004").await;
    let p2 = with_key(&w.r, &duri, db, &w.moderator, "key-pub-00004").await;
    assert_eq!((p1.status, p2.status), (StatusCode::OK, StatusCode::OK), "{}", p2.body);
    assert_eq!(count(&pool, "SELECT count(*) FROM publication_events").await, 1);
}
