//! Catalogue invariants: composition ⇄ recordings, covers without lyric reuse, distinct credit
//! roles, referential integrity under concurrent relationship updates, write authorization.

mod common;

use axum::http::{Method, StatusCode};
use axum::Router;
use common::*;
use domain::Role;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::collections::HashSet;

async fn create(r: &Router, cookie: &str, path: &str, body: Value) -> Value {
    let res = post(r, &format!("/api/v1/{path}"), body, Some(cookie)).await;
    assert_eq!(res.status, StatusCode::CREATED, "{}", res.body);
    let id = res.body["id"].as_str().unwrap();
    assert_eq!(res.headers["location"], format!("/api/v1/{path}/{id}"));
    res.body
}

async fn patch(r: &Router, uri: &str, body: Value, cookie: Option<&str>) -> Res {
    send(r, Method::PATCH, uri, Some(body), cookie).await
}

fn field_codes(res: &Res) -> Vec<(String, String)> {
    res.body["errors"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|e| (e["field"].as_str().unwrap().into(), e["code"].as_str().unwrap().into()))
                .collect()
        })
        .unwrap_or_default()
}

fn id(v: &Value) -> &str {
    v["id"].as_str().unwrap()
}

async fn count(pool: &PgPool, sql: &str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn composition_with_n_recordings(pool: PgPool) {
    let r = router(&pool);
    let (_, c) = signed_in(&pool, &r, "ed@example.com", None).await;
    let work = create(&r, &c, "compositions", json!({ "title": "Kaatrin Mozhi" })).await;
    let other = create(&r, &c, "compositions", json!({ "title": "Other" })).await;
    let mut made = HashSet::new();
    for n in 0..5 {
        let rec = create(
            &r,
            &c,
            "recordings",
            json!({ "composition_id": id(&work), "title": format!("Take {n}") }),
        )
        .await;
        assert_eq!(rec["composition"]["id"], work["id"]);
        made.insert(id(&rec).to_owned());
    }
    create(
        &r,
        &c,
        "recordings",
        json!({ "composition_id": id(&other), "title": "Elsewhere" }),
    )
    .await;

    let detail = get(&r, &format!("/api/v1/compositions/{}", id(&work)), None).await;
    assert_eq!(detail.status, StatusCode::OK);
    assert_eq!(detail.body["recording_count"], 5);
    assert_eq!(detail.body["recordings"].as_array().unwrap().len(), 5);

    // Cursor pagination over the composition filter visits each recording exactly once.
    let mut seen = HashSet::new();
    let mut uri = format!("/api/v1/recordings?composition_id={}&limit=2", id(&work));
    loop {
        let page = get(&r, &uri, None).await;
        assert_eq!(page.status, StatusCode::OK, "{}", page.body);
        for item in page.body["items"].as_array().unwrap() {
            assert!(seen.insert(id(item).to_owned()), "duplicate across pages");
        }
        match page.body["next_cursor"].as_str() {
            Some(cur) => uri = format!("/api/v1/recordings?composition_id={}&limit=2&cursor={cur}", id(&work)),
            None => break,
        }
    }
    assert_eq!(seen, made);

    // A recording cannot exist without an existing composition.
    let res = post(&r, "/api/v1/recordings", json!({ "title": "Orphan" }), Some(&c)).await;
    res.assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    assert!(field_codes(&res).contains(&("composition_id".into(), "required".into())));
    let res = post(
        &r,
        "/api/v1/recordings",
        json!({ "title": "Orphan", "composition_id": "00000000-0000-4000-8000-000000000000" }),
        Some(&c),
    )
    .await;
    assert!(field_codes(&res).contains(&("composition_id".into(), "not_found".into())));
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn cover_links_composition_without_lyric_reuse(pool: PgPool) {
    let r = router(&pool);
    let (_, c) = signed_in(&pool, &r, "ed@example.com", None).await;
    let singer = create(
        &r,
        &c,
        "artists",
        json!({ "name": "Original Singer", "kind": "person" }),
    )
    .await;
    let work = create(&r, &c, "compositions", json!({ "title": "Song" })).await;
    let original = create(
        &r,
        &c,
        "recordings",
        json!({
            "composition_id": id(&work), "title": "Song", "isrc": "US-RC1-76-07839", "duration_ms": 245000,
            "credits": [{ "artist_id": id(&singer), "role": "performer" }]
        }),
    )
    .await;

    let cover = create(
        &r,
        &c,
        "recordings",
        json!({
            "composition_id": id(&work), "title": "Song (cover)",
            "relationships": [{ "kind": "cover", "related_recording_id": id(&original) }]
        }),
    )
    .await;
    // Same work, nothing recording-specific inherited from the original.
    assert_eq!(cover["composition"]["id"], work["id"]);
    assert_eq!(cover["credits"], json!([]));
    assert_eq!(cover["isrc"], Value::Null);
    assert_eq!(cover["duration_ms"], Value::Null);
    assert_eq!(cover["relationships"][0]["kind"], "cover");
    assert_eq!(cover["relationships"][0]["recording_id"], original["id"]);

    let orig = get(&r, &format!("/api/v1/recordings/{}", id(&original)), None).await;
    assert_eq!(orig.body["related_from"][0]["recording_id"], cover["id"]);
    assert_eq!(orig.body["credits"][0]["artist_name"], "Original Singer");

    // A relationship is metadata only: no column can point at lyrics or contributions.
    let cols: Vec<String> = sqlx::query_scalar(
        "SELECT column_name::text FROM information_schema.columns
         WHERE table_name = 'recording_relationships' ORDER BY column_name",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        cols,
        [
            "composition_id",
            "created_at",
            "created_by",
            "id",
            "kind",
            "recording_id",
            "related_recording_id"
        ]
    );

    // A cover of a different work is refused; a loosely related performance is not.
    let other = create(&r, &c, "compositions", json!({ "title": "Other" })).await;
    let res = post(
        &r,
        "/api/v1/recordings",
        json!({
            "composition_id": id(&other), "title": "Not a cover",
            "relationships": [{ "kind": "cover", "related_recording_id": id(&original) }]
        }),
        Some(&c),
    )
    .await;
    res.assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    assert_eq!(
        field_codes(&res),
        [("relationships".into(), "composition_mismatch".into())]
    );
    create(
        &r,
        &c,
        "recordings",
        json!({
            "composition_id": id(&other), "title": "Medley",
            "relationships": [{ "kind": "related_performance", "related_recording_id": id(&original) }]
        }),
    )
    .await;

    // The database refuses a cross-composition cover even if the application check is bypassed.
    let err = sqlx::query(
        "INSERT INTO recording_relationships (kind, recording_id, related_recording_id, composition_id, created_by)
         SELECT 'live', $1, $2, $3, created_by FROM recordings WHERE id = $1",
    )
    .bind(uuid::Uuid::parse_str(id(&cover)).unwrap())
    .bind(uuid::Uuid::parse_str(id(&original)).unwrap())
    .bind(uuid::Uuid::parse_str(id(&other)).unwrap())
    .execute(&pool)
    .await
    .unwrap_err();
    assert!(err.to_string().contains("recording_relationships_from_fk"), "{err}");

    // Self-links and unknown kinds are field errors.
    let res = patch(
        &r,
        &format!("/api/v1/recordings/{}", id(&cover)),
        json!({ "expected_version": cover["version"], "relationships": [
            { "kind": "cover", "related_recording_id": id(&cover) },
            { "kind": "sample", "related_recording_id": id(&original) }
        ]}),
        Some(&c),
    )
    .await;
    assert_eq!(
        field_codes(&res),
        [
            ("relationships".into(), "self_reference".into()),
            ("relationships".into(), "invalid_kind".into())
        ]
    );
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn credit_roles_are_distinct(pool: PgPool) {
    let r = router(&pool);
    let (_, c) = signed_in(&pool, &r, "ed@example.com", None).await;
    let a = create(&r, &c, "artists", json!({ "name": "இளையராஜா", "kind": "person" })).await;
    let b = create(&r, &c, "artists", json!({ "name": "Vaali", "kind": "person" })).await;

    // Composer and lyricist are two credits, even for the same person.
    let work = create(
        &r,
        &c,
        "compositions",
        json!({ "title": "Work", "credits": [
            { "artist_id": id(&a), "role": "composer" },
            { "artist_id": id(&a), "role": "lyricist" },
            { "artist_id": id(&b), "role": "lyricist" }
        ]}),
    )
    .await;
    let roles: Vec<(String, String)> = work["credits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|k| {
            (
                k["artist_name"].as_str().unwrap().into(),
                k["role"].as_str().unwrap().into(),
            )
        })
        .collect();
    assert_eq!(
        roles,
        [
            ("இளையராஜா".into(), "composer".into()),
            ("இளையராஜா".into(), "lyricist".into()),
            ("Vaali".into(), "lyricist".into())
        ]
    );

    let bad = |credits: Value| json!({ "title": "X", "credits": credits });
    let cases = [
        (json!([{ "artist_id": id(&a), "role": "author" }]), "invalid_role"),
        (
            json!([{ "artist_id": id(&a), "role": "composer_lyricist" }]),
            "invalid_role",
        ),
        (
            json!([{ "artist_id": id(&a), "role": "performer" }]),
            "role_not_applicable",
        ),
        (
            json!([{ "artist_id": id(&a), "role": "composer" }, { "artist_id": id(&a), "role": "composer" }]),
            "duplicate_credit",
        ),
        (
            json!([{ "artist_id": "00000000-0000-4000-8000-000000000000", "role": "composer" }]),
            "unknown_artist",
        ),
    ];
    for (credits, code) in cases {
        let res = post(&r, "/api/v1/compositions", bad(credits), Some(&c)).await;
        res.assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
        assert_eq!(field_codes(&res), [("credits".into(), code.into())], "{code}");
    }
    assert_eq!(count(&pool, "SELECT count(*) FROM compositions").await, 1);

    // The schema enforces the same: role scoped to entity, one row per (entity, artist, role).
    let wid = uuid::Uuid::parse_str(id(&work)).unwrap();
    let aid = uuid::Uuid::parse_str(id(&a)).unwrap();
    for (role, constraint) in [("performer", "credits_check"), ("composer", "credits_composition_key")] {
        let err = sqlx::query("INSERT INTO credits (artist_id, role, composition_id, position) VALUES ($1, $2, $3, 9)")
            .bind(aid)
            .bind(role)
            .bind(wid)
            .execute(&pool)
            .await
            .unwrap_err();
        assert!(err.to_string().contains(constraint), "{role}: {err}");
    }

    // Replacing credits is audited with before/after.
    let res = patch(
        &r,
        &format!("/api/v1/compositions/{}", id(&work)),
        json!({ "expected_version": 1, "credits": [{ "artist_id": id(&b), "role": "composer" }] }),
        Some(&c),
    )
    .await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.body);
    assert_eq!(res.body["version"], 2);
    let (before, after): (Value, Value) =
        sqlx::query_as("SELECT before, after FROM audit_log WHERE action = 'credits.changed' AND before IS NOT NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(before.as_array().unwrap().len(), 3);
    assert_eq!(after, json!([{ "artist_id": id(&b), "role": "composer" }]));
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn concurrent_relationship_updates_keep_referential_integrity(pool: PgPool) {
    let r = router(&pool);
    let (_, c) = signed_in(&pool, &r, "ed@example.com", None).await;
    let a = create(&r, &c, "compositions", json!({ "title": "A" })).await;
    let b = create(&r, &c, "compositions", json!({ "title": "B" })).await;

    // Race: move R1 to composition B while someone records a cover of R1 under composition A.
    let mut outcomes = HashSet::new();
    for n in 0..12 {
        let r1 = create(
            &r,
            &c,
            "recordings",
            json!({ "composition_id": id(&a), "title": format!("R{n}") }),
        )
        .await;
        let r1_uri = format!("/api/v1/recordings/{}", id(&r1));
        let (moved, cover) = tokio::join!(
            patch(
                &r,
                &r1_uri,
                json!({ "expected_version": 1, "composition_id": id(&b) }),
                Some(&c)
            ),
            post(
                &r,
                "/api/v1/recordings",
                json!({ "composition_id": id(&a), "title": "Cover",
                        "relationships": [{ "kind": "cover", "related_recording_id": id(&r1) }] }),
                Some(&c),
            ),
        );
        let outcome = (
            moved.status,
            moved.code().to_owned(),
            cover.status,
            cover.code().to_owned(),
        );
        assert!(
            matches!(
                (outcome.0, outcome.1.as_str(), outcome.2, outcome.3.as_str()),
                (
                    StatusCode::OK,
                    "",
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "validation_failed"
                ) | (
                    StatusCode::CONFLICT,
                    "recording_has_relationships",
                    StatusCode::CREATED,
                    ""
                )
            ),
            "unexpected outcome {outcome:?}"
        );
        outcomes.insert(outcome.0);
    }
    // Whatever the interleaving, no relationship spans two compositions.
    let broken = count(
        &pool,
        "SELECT count(*) FROM recording_relationships rel
         JOIN recordings x ON x.id = rel.recording_id JOIN recordings y ON y.id = rel.related_recording_id
         WHERE rel.composition_id IS NOT NULL
           AND (x.composition_id <> rel.composition_id OR y.composition_id <> rel.composition_id)",
    )
    .await;
    assert_eq!(broken, 0);
    assert!(!outcomes.is_empty());

    // Two editors replace the same recording's relationships from the same version: one wins,
    // the other gets 409 with the current version; the stored set is exactly the winner's.
    let base = create(
        &r,
        &c,
        "recordings",
        json!({ "composition_id": id(&a), "title": "Base" }),
    )
    .await;
    let x = create(&r, &c, "recordings", json!({ "composition_id": id(&a), "title": "X" })).await;
    let y = create(&r, &c, "recordings", json!({ "composition_id": id(&a), "title": "Y" })).await;
    let uri = format!("/api/v1/recordings/{}", id(&base));
    let body = |target: &Value, kind: &str| json!({ "expected_version": 1, "relationships": [{ "kind": kind, "related_recording_id": id(target) }] });
    let (p1, p2) = tokio::join!(
        patch(&r, &uri, body(&x, "live"), Some(&c)),
        patch(&r, &uri, body(&y, "remake"), Some(&c))
    );
    let (winner, loser) = if p1.status == StatusCode::OK {
        (p1, p2)
    } else {
        (p2, p1)
    };
    assert_eq!(winner.status, StatusCode::OK, "{}", winner.body);
    loser.assert_problem(StatusCode::CONFLICT, "version_conflict");
    assert_eq!(loser.body["current_version"], 2);
    let stored = get(&r, &uri, None).await;
    assert_eq!(stored.body["relationships"], winner.body["relationships"]);
    assert_eq!(stored.body["relationships"].as_array().unwrap().len(), 1);
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM audit_log WHERE action = 'recording.relationships_changed' AND before IS NOT NULL"
        )
        .await,
        1
    );

    // Moving a recording that anchors a cover is refused until the link is removed.
    let anchored = create(
        &r,
        &c,
        "recordings",
        json!({ "composition_id": id(&a), "title": "Anchor" }),
    )
    .await;
    create(
        &r,
        &c,
        "recordings",
        json!({ "composition_id": id(&a), "title": "Its cover",
                "relationships": [{ "kind": "cover", "related_recording_id": id(&anchored) }] }),
    )
    .await;
    patch(
        &r,
        &format!("/api/v1/recordings/{}", id(&anchored)),
        json!({ "expected_version": 1, "composition_id": id(&b) }),
        Some(&c),
    )
    .await
    .assert_problem(StatusCode::CONFLICT, "recording_has_relationships");
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn unauthorized_writes_are_rejected(pool: PgPool) {
    let r = router(&pool);
    let (_, owner) = signed_in(&pool, &r, "owner@example.com", None).await;
    let (_, stranger) = signed_in(&pool, &r, "stranger@example.com", None).await;
    let (_, moderator) = signed_in(&pool, &r, "mod@example.com", Some(Role::Moderator)).await;

    let artist = create(&r, &owner, "artists", json!({ "name": "A", "kind": "group" })).await;
    let work = create(&r, &owner, "compositions", json!({ "title": "W" })).await;
    let rec = create(
        &r,
        &owner,
        "recordings",
        json!({ "composition_id": id(&work), "title": "R" }),
    )
    .await;
    let album = create(&r, &owner, "albums", json!({ "title": "L", "kind": "album" })).await;
    let audit_before = count(&pool, "SELECT count(*) FROM audit_log").await;

    // Anonymous: every write is 401, before the body is even considered.
    for path in ["artists", "compositions", "recordings", "albums"] {
        post(&r, &format!("/api/v1/{path}"), json!({}), None)
            .await
            .assert_problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    }
    let targets = [
        (
            format!("/api/v1/artists/{}", id(&artist)),
            json!({ "expected_version": 1, "name": "Hijacked" }),
        ),
        (
            format!("/api/v1/compositions/{}", id(&work)),
            json!({ "expected_version": 1, "title": "Hijacked" }),
        ),
        (
            format!("/api/v1/recordings/{}", id(&rec)),
            json!({ "expected_version": 1, "title": "Hijacked" }),
        ),
        (
            format!("/api/v1/albums/{}", id(&album)),
            json!({ "expected_version": 1, "title": "Hijacked" }),
        ),
    ];
    for (uri, body) in &targets {
        patch(&r, uri, body.clone(), None)
            .await
            .assert_problem(StatusCode::UNAUTHORIZED, "unauthenticated");
        // Another member may not edit someone else's record.
        patch(&r, uri, body.clone(), Some(&stranger))
            .await
            .assert_problem(StatusCode::FORBIDDEN, "forbidden");
        // A forged cookie is just anonymous.
        patch(&r, uri, body.clone(), Some("lyrics_session=forged"))
            .await
            .assert_problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    }
    // Missing CSRF header is refused even with a valid session.
    let mut req = axum::http::Request::builder()
        .method(Method::POST)
        .uri("/api/v1/artists")
        .header("content-type", "application/json")
        .header("cookie", &owner);
    req = req.header("accept", "application/json");
    raw(
        &r,
        req.body(axum::body::Body::from(r#"{"name":"N","kind":"person"}"#))
            .unwrap(),
    )
    .await
    .assert_problem(StatusCode::FORBIDDEN, "csrf_header_missing");

    assert_eq!(count(&pool, "SELECT count(*) FROM audit_log").await, audit_before);
    assert_eq!(
        count(&pool, "SELECT count(*) FROM artists WHERE name = 'Hijacked'").await,
        0
    );
    let unchanged = get(&r, &format!("/api/v1/artists/{}", id(&artist)), None).await;
    assert_eq!(unchanged.body["version"], 1);

    // The creator and a moderator are allowed; the actor is taken from the session, not the body.
    for (uri, body) in &targets {
        let res = patch(&r, uri, body.clone(), Some(&moderator)).await;
        assert_eq!(res.status, StatusCode::OK, "{uri}: {}", res.body);
    }
    let res = patch(
        &r,
        &format!("/api/v1/artists/{}", id(&artist)),
        json!({ "expected_version": 2, "name": "Renamed", "created_by": "00000000-0000-4000-8000-000000000000" }),
        Some(&owner),
    )
    .await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.body);
    assert_eq!(res.body["created_by"], artist["created_by"]);
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn identifiers_are_unique_and_versions_guard_updates(pool: PgPool) {
    let r = router(&pool);
    let (_, c) = signed_in(&pool, &r, "ed@example.com", None).await;
    let work = create(
        &r,
        &c,
        "compositions",
        json!({ "title": "W", "iswc": "T-034.524.680-1" }),
    )
    .await;
    assert_eq!(work["iswc"], "T0345246801");

    // Same identifier in another spelling: 409 naming the existing record; never merged.
    let res = post(
        &r,
        "/api/v1/compositions",
        json!({ "title": "W2", "iswc": "t0345246801" }),
        Some(&c),
    )
    .await;
    res.assert_problem(StatusCode::CONFLICT, "duplicate_identifier");
    assert_eq!(res.body["existing_id"], work["id"]);
    assert_eq!(field_codes(&res), [("iswc".into(), "duplicate".into())]);
    let res = post(
        &r,
        "/api/v1/compositions",
        json!({ "title": "W3", "iswc": "T-034.524.680-2" }),
        Some(&c),
    )
    .await;
    assert_eq!(field_codes(&res), [("iswc".into(), "invalid_identifier".into())]);

    let rec = create(
        &r,
        &c,
        "recordings",
        json!({ "composition_id": id(&work), "title": "R", "isrc": "USRC17607839" }),
    )
    .await;
    let res = post(
        &r,
        "/api/v1/recordings",
        json!({ "composition_id": id(&work), "title": "R2", "isrc": "us-rc1-76-07839" }),
        Some(&c),
    )
    .await;
    res.assert_problem(StatusCode::CONFLICT, "duplicate_identifier");
    assert_eq!(res.body["existing_id"], rec["id"]);

    // Names are never unique: two artists may share one.
    create(
        &r,
        &c,
        "artists",
        json!({ "name": "Same", "kind": "person", "isni": "0000 0001 2103 2683" }),
    )
    .await;
    create(
        &r,
        &c,
        "artists",
        json!({ "name": "Same", "kind": "person", "disambiguation": "the drummer" }),
    )
    .await;
    let res = post(
        &r,
        "/api/v1/artists",
        json!({ "name": "Third", "kind": "person", "isni": "0000000121032683" }),
        Some(&c),
    )
    .await;
    res.assert_problem(StatusCode::CONFLICT, "duplicate_identifier");

    // Album with tracks; barcode duplicate guarded.
    let album = create(
        &r,
        &c,
        "albums",
        json!({ "title": "LP", "kind": "soundtrack", "barcode": "4006381333931", "tracks": [id(&rec)] }),
    )
    .await;
    assert_eq!(album["tracks"][0]["recording_id"], rec["id"]);
    assert_eq!(album["tracks"][0]["position"], 1);
    let res = post(
        &r,
        "/api/v1/albums",
        json!({ "title": "LP2", "kind": "album", "barcode": "4006381333931" }),
        Some(&c),
    )
    .await;
    assert_eq!(res.body["existing_id"], album["id"]);
    let rec_detail = get(&r, &format!("/api/v1/recordings/{}", id(&rec)), None).await;
    assert_eq!(rec_detail.body["albums"][0]["album_id"], album["id"]);

    // Version guard, missing version, null clears, absent leaves unchanged.
    let uri = format!("/api/v1/compositions/{}", id(&work));
    let res = patch(&r, &uri, json!({ "title": "W!" }), Some(&c)).await;
    assert_eq!(field_codes(&res), [("expected_version".into(), "required".into())]);
    let res = patch(
        &r,
        &uri,
        json!({ "expected_version": 1, "iswc": null, "year": 1985 }),
        Some(&c),
    )
    .await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.body);
    assert_eq!(
        (
            res.body["iswc"].clone(),
            res.body["year"].clone(),
            res.body["title"].clone()
        ),
        (Value::Null, json!(1985), json!("W"))
    );
    let res = patch(&r, &uri, json!({ "expected_version": 1, "title": "Stale" }), Some(&c)).await;
    res.assert_problem(StatusCode::CONFLICT, "version_conflict");
    assert_eq!(res.body["current_version"], 2);
    // A no-op patch changes nothing and audits nothing.
    let audits = count(&pool, "SELECT count(*) FROM audit_log").await;
    let res = patch(&r, &uri, json!({ "expected_version": 2, "title": "W" }), Some(&c)).await;
    assert_eq!(res.body["version"], 2);
    assert_eq!(count(&pool, "SELECT count(*) FROM audit_log").await, audits);

    // Every consequential change was audited.
    let actions: Vec<String> =
        sqlx::query_scalar("SELECT DISTINCT action FROM audit_log WHERE action LIKE '%.%' ORDER BY 1")
            .fetch_all(&pool)
            .await
            .unwrap();
    for a in [
        "album.created",
        "album.tracks_changed",
        "artist.created",
        "composition.created",
        "composition.updated",
        "recording.created",
    ] {
        assert!(actions.contains(&a.to_owned()), "{a} missing from {actions:?}");
    }
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn reads_are_public_and_errors_are_problems(pool: PgPool) {
    let r = router(&pool);
    let (_, c) = signed_in(&pool, &r, "ed@example.com", None).await;
    let artist = create(&r, &c, "artists", json!({ "name": "Fairuz", "kind": "person" })).await;
    create(&r, &c, "artists", json!({ "name": "100% Pure_", "kind": "group" })).await;

    let list = get(&r, "/api/v1/artists?prefix=FAIR", None).await;
    assert_eq!(list.body["items"].as_array().unwrap().len(), 1);
    assert_eq!(list.body["items"][0]["id"], artist["id"]);
    // LIKE metacharacters in the prefix are literal.
    assert_eq!(
        get(&r, "/api/v1/artists?prefix=%25", None).await.body["items"],
        json!([])
    );
    assert_eq!(
        get(&r, "/api/v1/artists?prefix=100%25%20P", None).await.body["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        get(&r, "/api/v1/artists?kind=group", None).await.body["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    get(&r, "/api/v1/artists?kind=band", None)
        .await
        .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    get(&r, "/api/v1/artists?limit=0", None)
        .await
        .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    get(&r, "/api/v1/artists?cursor=nope", None)
        .await
        .assert_problem(StatusCode::BAD_REQUEST, "invalid_cursor");
    get(&r, "/api/v1/artists/not-a-uuid", None)
        .await
        .assert_problem(StatusCode::NOT_FOUND, "not_found");
    get(&r, "/api/v1/albums/00000000-0000-4000-8000-000000000000", None)
        .await
        .assert_problem(StatusCode::NOT_FOUND, "not_found");

    let res = post(
        &r,
        "/api/v1/artists",
        json!({ "name": " ", "kind": "band", "isni": "123" }),
        Some(&c),
    )
    .await;
    res.assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    assert_eq!(
        field_codes(&res),
        [
            ("name".into(), "required".into()),
            ("kind".into(), "invalid_choice".into()),
            ("isni".into(), "invalid_identifier".into())
        ]
    );
    let detail = get(&r, &format!("/api/v1/artists/{}", id(&artist)), None).await;
    assert_eq!(detail.status, StatusCode::OK);
    assert_eq!(detail.body["credits"], json!([]));
}

/// Waits until some backend is blocked on a row lock (the request under test is queued behind us).
async fn wait_for_lock_waiter(pool: &PgPool) {
    for _ in 0..200 {
        let waiting = count(
            pool,
            "SELECT count(*) FROM pg_stat_activity WHERE wait_event_type = 'Lock' AND datname = current_database()",
        )
        .await;
        if waiting > 0 {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("request never blocked on the competing lock");
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn relationship_races_resolve_deterministically_in_both_orders(pool: PgPool) {
    let r = router(&pool);
    let (user_id, c) = signed_in(&pool, &r, "ed@example.com", None).await;
    let a = create(&r, &c, "compositions", json!({ "title": "A" })).await;
    let b = create(&r, &c, "compositions", json!({ "title": "B" })).await;
    let uid = |v: &Value| uuid::Uuid::parse_str(id(v)).unwrap();

    // Order 1: a cover insert is in flight (holds KEY SHARE on R1) when the move arrives.
    let r1 = create(&r, &c, "recordings", json!({ "composition_id": id(&a), "title": "R1" })).await;
    let r2 = create(&r, &c, "recordings", json!({ "composition_id": id(&a), "title": "R2" })).await;
    let mut tx = pool.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO recording_relationships (kind, recording_id, related_recording_id, composition_id, created_by)
         VALUES ('cover', $1, $2, $3, $4)",
    )
    .bind(uid(&r2))
    .bind(uid(&r1))
    .bind(uid(&a))
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    let (rr, uri, cc, to) = (
        r.clone(),
        format!("/api/v1/recordings/{}", id(&r1)),
        c.clone(),
        id(&b).to_owned(),
    );
    let mv = tokio::spawn(async move {
        patch(
            &rr,
            &uri,
            json!({ "expected_version": 1, "composition_id": to }),
            Some(&cc),
        )
        .await
    });
    wait_for_lock_waiter(&pool).await;
    tx.commit().await.unwrap();
    mv.await
        .unwrap()
        .assert_problem(StatusCode::CONFLICT, "recording_has_relationships");

    // Order 2: a move is in flight (holds the row lock, composition already changed) when the cover arrives.
    let r3 = create(&r, &c, "recordings", json!({ "composition_id": id(&a), "title": "R3" })).await;
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("UPDATE recordings SET composition_id = $2, version = version + 1 WHERE id = $1")
        .bind(uid(&r3))
        .bind(uid(&b))
        .execute(&mut *tx)
        .await
        .unwrap();
    let (rr, cc, comp, target) = (r.clone(), c.clone(), id(&a).to_owned(), id(&r3).to_owned());
    let cover = tokio::spawn(async move {
        post(
            &rr,
            "/api/v1/recordings",
            json!({ "composition_id": comp, "title": "Late cover",
                    "relationships": [{ "kind": "cover", "related_recording_id": target }] }),
            Some(&cc),
        )
        .await
    });
    wait_for_lock_waiter(&pool).await;
    tx.commit().await.unwrap();
    let res = cover.await.unwrap();
    res.assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    assert_eq!(
        field_codes(&res),
        [("relationships".into(), "composition_mismatch".into())]
    );
    assert_eq!(
        count(&pool, "SELECT count(*) FROM recordings WHERE title = 'Late cover'").await,
        0
    );
}
