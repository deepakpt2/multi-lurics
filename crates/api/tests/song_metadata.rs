//! Indian-song metadata on a composition: category (mainstream/devotional), raga, religion, deity, tags.
//! Title, year, lyricist, music director (composer credit), singers and album are existing fields.

mod common;

use axum::http::{Method, StatusCode};
use common::*;
use domain::Role;
use serde_json::json;
use sqlx::PgPool;

fn codes(res: &Res) -> Vec<(String, String)> {
    res.body["errors"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|e| (e["field"].as_str().unwrap().into(), e["code"].as_str().unwrap().into()))
                .collect()
        })
        .unwrap_or_default()
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn devotional_metadata_round_trips_and_filters(pool: PgPool) {
    let r = router(&pool);
    let (_, c) = signed_in(&pool, &r, "ed@example.com", None).await;
    let made = post(
        &r,
        "/api/v1/compositions",
        json!({
            "title": "Hanuman Chalisa", "year": 1990, "category": "devotional", "raga": "  Mohanam ",
            "religion": "hindu", "deity": "Hanuman", "tags": ["Bhajan", " bhajan ", "Morning", ""]
        }),
        Some(&c),
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    let s = &made.body;
    assert_eq!(s["category"], "devotional");
    assert_eq!(s["raga"], "Mohanam");
    assert_eq!(s["religion"], "hindu");
    assert_eq!(s["deity"], "Hanuman");
    assert_eq!(s["tags"], json!(["Bhajan", "Morning"]));

    // A plain song defaults to mainstream with no devotional fields.
    let plain = post(&r, "/api/v1/compositions", json!({ "title": "Film song" }), Some(&c)).await;
    assert_eq!(plain.status, StatusCode::CREATED);
    assert_eq!(plain.body["category"], "mainstream");
    assert_eq!(plain.body["tags"], json!([]));

    for (q, want) in [
        ("category=devotional", 1),
        ("category=mainstream", 1),
        ("raga=MOHANAM", 1),
        ("deity=hanuman", 1),
        ("tag=MORNING", 1),
        ("tag=nothing", 0),
    ] {
        let l = get(&r, &format!("/api/v1/compositions?{q}"), None).await;
        assert_eq!(l.status, StatusCode::OK);
        assert_eq!(l.body["items"].as_array().unwrap().len(), want, "{q}");
    }
    let bad = get(&r, "/api/v1/compositions?category=film", None).await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn invalid_metadata_is_rejected_with_field_codes(pool: PgPool) {
    let r = router(&pool);
    let (_, c) = signed_in(&pool, &r, "ed@example.com", None).await;
    let cases = [
        (
            json!({ "title": "T", "category": "film" }),
            ("category", "invalid_value"),
        ),
        (
            json!({ "title": "T", "category": "devotional", "religion": "pagan" }),
            ("religion", "invalid_value"),
        ),
        (
            json!({ "title": "T", "religion": "hindu" }),
            ("religion", "devotional_only"),
        ),
        (
            json!({ "title": "T", "category": "devotional", "deity": "x".repeat(101) }),
            ("deity", "too_long"),
        ),
        (
            json!({ "title": "T", "tags": vec!["x".repeat(41)] }),
            ("tags", "too_long"),
        ),
        (
            json!({ "title": "T", "tags": (0..21).map(|i| format!("t{i}")).collect::<Vec<_>>() }),
            ("tags", "too_many"),
        ),
    ];
    for (body, (f, code)) in cases {
        let res = post(&r, "/api/v1/compositions", body.clone(), Some(&c)).await;
        assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert!(
            codes(&res).contains(&(f.into(), code.into())),
            "{body} -> {:?}",
            codes(&res)
        );
    }
    let n = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM compositions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn patch_updates_clears_and_leaving_devotional_drops_religion(pool: PgPool) {
    let r = router(&pool);
    let (_, c) = signed_in(&pool, &r, "ed@example.com", None).await;
    let made = post(
        &r,
        "/api/v1/compositions",
        json!({ "title": "T", "category": "devotional", "religion": "sikh", "deity": "Waheguru", "raga": "Bhairavi", "tags": ["a"] }),
        Some(&c),
    )
    .await;
    let uri = format!("/api/v1/compositions/{}", made.body["id"].as_str().unwrap());
    let v = made.body["version"].as_i64().unwrap();

    // Untouched fields stay; null clears; tags replace the whole list.
    let p = send(
        &r,
        Method::PATCH,
        &uri,
        Some(json!({ "expected_version": v, "raga": null, "tags": ["b", "c"] })),
        Some(&c),
    )
    .await;
    assert_eq!(p.status, StatusCode::OK, "{}", p.body);
    assert_eq!(p.body["raga"], json!(null));
    assert_eq!(p.body["deity"], "Waheguru");
    assert_eq!(p.body["tags"], json!(["b", "c"]));

    // Switching to mainstream clears religion and deity.
    let v = p.body["version"].as_i64().unwrap();
    let p = send(
        &r,
        Method::PATCH,
        &uri,
        Some(json!({ "expected_version": v, "category": "mainstream" })),
        Some(&c),
    )
    .await;
    assert_eq!(p.status, StatusCode::OK, "{}", p.body);
    assert_eq!(p.body["religion"], json!(null));
    assert_eq!(p.body["deity"], json!(null));

    let audited = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM audit_log WHERE action = 'composition.updated'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(audited, 2);
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn only_editors_may_change_metadata(pool: PgPool) {
    let r = router(&pool);
    let (_, owner) = signed_in(&pool, &r, "owner@example.com", None).await;
    let (_, stranger) = signed_in(&pool, &r, "stranger@example.com", None).await;
    let (_, moderator) = signed_in(&pool, &r, "mod@example.com", Some(Role::Moderator)).await;
    let made = post(&r, "/api/v1/compositions", json!({ "title": "T" }), Some(&owner)).await;
    let uri = format!("/api/v1/compositions/{}", made.body["id"].as_str().unwrap());
    let body = json!({ "expected_version": 1, "category": "devotional", "deity": "Murugan" });

    assert_eq!(
        post(
            &r,
            "/api/v1/compositions",
            json!({ "title": "x", "category": "devotional" }),
            None
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(&r, Method::PATCH, &uri, Some(body.clone()), None).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(&r, Method::PATCH, &uri, Some(body.clone()), Some(&stranger))
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        send(&r, Method::PATCH, &uri, Some(body), Some(&moderator)).await.status,
        StatusCode::OK
    );
    let deity: Option<String> = sqlx::query_scalar("SELECT deity FROM compositions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(deity.as_deref(), Some("Murugan"));
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn concurrent_edits_with_the_same_version_conflict(pool: PgPool) {
    let r = router(&pool);
    let (_, c) = signed_in(&pool, &r, "ed@example.com", None).await;
    let made = post(&r, "/api/v1/compositions", json!({ "title": "T" }), Some(&c)).await;
    let uri = format!("/api/v1/compositions/{}", made.body["id"].as_str().unwrap());
    let one = json!({ "expected_version": 1, "raga": "Kalyani" });
    let two = json!({ "expected_version": 1, "raga": "Todi" });
    let (a, b) = tokio::join!(
        send(&r, Method::PATCH, &uri, Some(one), Some(&c)),
        send(&r, Method::PATCH, &uri, Some(two), Some(&c)),
    );
    let mut s = [a.status, b.status];
    s.sort();
    assert_eq!(s, [StatusCode::OK, StatusCode::CONFLICT], "{} / {}", a.body, b.body);
    let (version, raga): (i32, Option<String>) = sqlx::query_as("SELECT version, raga FROM compositions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(version, 2);
    assert!(matches!(raga.as_deref(), Some("Kalyani" | "Todi")));
}

async fn index(pool: &PgPool) {
    let h = worker::SearchIndexer {
        pool: pool.clone(),
        inner: worker::LogOnly,
        revalidate: None,
    };
    while worker::run_once(pool, &h).await.unwrap() > 0 {}
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn songs_are_found_by_raga_deity_and_tag_and_follow_edits(pool: PgPool) {
    let r = router(&pool);
    let (_, c) = signed_in(&pool, &r, "ed@example.com", None).await;
    let made = post(
        &r,
        "/api/v1/compositions",
        json!({ "title": "Aarti", "category": "devotional", "religion": "hindu", "deity": "Hanuman",
                "raga": "Mohanam", "tags": ["bhajan"] }),
        Some(&c),
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    index(&pool).await;

    let found = |body: &serde_json::Value| -> Vec<(String, String)> {
        body["groups"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|g| g["items"].as_array().unwrap().clone())
            .map(|i| {
                (
                    i["id"].as_str().unwrap().to_owned(),
                    i["match"]["via"].as_str().unwrap().to_owned(),
                )
            })
            .collect()
    };
    let id = made.body["id"].as_str().unwrap().to_owned();
    for q in ["hanuman", "MOHANAM", "bhajan"] {
        let res = get(&r, &format!("/api/v1/search?q={q}"), None).await;
        assert_eq!(res.status, StatusCode::OK);
        assert_eq!(found(&res.body), vec![(id.clone(), "metadata".to_owned())], "{q}");
    }

    // Editing the deity moves the song: the old value stops matching once the worker has run.
    let uri = format!("/api/v1/compositions/{id}");
    let p = send(
        &r,
        Method::PATCH,
        &uri,
        Some(json!({ "expected_version": 1, "deity": "Murugan" })),
        Some(&c),
    )
    .await;
    assert_eq!(p.status, StatusCode::OK);
    index(&pool).await;
    assert!(found(&get(&r, "/api/v1/search?q=hanuman", None).await.body).is_empty());
    assert_eq!(found(&get(&r, "/api/v1/search?q=murugan", None).await.body).len(), 1);
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn raga_and_deity_reuse_the_spelling_already_in_use(pool: PgPool) {
    let r = router(&pool);
    let (_, c) = signed_in(&pool, &r, "ed@example.com", None).await;
    post(
        &r,
        "/api/v1/compositions",
        json!({ "title": "A", "raga": "Mohanam" }),
        Some(&c),
    )
    .await;
    let b = post(
        &r,
        "/api/v1/compositions",
        json!({ "title": "B", "raga": " mohanam " }),
        Some(&c),
    )
    .await;
    assert_eq!(b.body["raga"], "Mohanam");
    let n: i64 = sqlx::query_scalar("SELECT count(DISTINCT raga) FROM compositions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn the_database_refuses_religion_on_a_mainstream_song(pool: PgPool) {
    let r = router(&pool);
    let (_, c) = signed_in(&pool, &r, "ed@example.com", None).await;
    post(&r, "/api/v1/compositions", json!({ "title": "A" }), Some(&c)).await;
    let direct = sqlx::query("UPDATE compositions SET religion = 'hindu'")
        .execute(&pool)
        .await;
    assert!(
        direct.is_err(),
        "constraint compositions_devotional_check must hold below the API too"
    );
}
