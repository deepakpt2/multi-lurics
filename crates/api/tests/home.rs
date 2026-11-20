//! The home page's fixed lists: ranked from views, ratings and comments, rebuilt on a schedule,
//! empty (not broken) on a fresh install, and view counting that cannot be inflated by reloads.

mod common;

use application::App;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use common::*;
use domain::Role;
use serde_json::{json, Value};
use sqlx::PgPool;

struct World {
    app: App,
    r: Router,
    member: String,
    rater: String,
    moderator: String,
}

async fn world(pool: PgPool) -> World {
    let app = app(pool.clone());
    let r = api::router(app.clone());
    let (_, member) = signed_in(&pool, &r, "m1@example.com", None).await;
    let (_, rater) = signed_in(&pool, &r, "m2@example.com", None).await;
    let (_, moderator) = signed_in(&pool, &r, "mod@example.com", Some(Role::Moderator)).await;
    World {
        app,
        r,
        member,
        rater,
        moderator,
    }
}

fn id(v: &Value) -> String {
    v["id"].as_str().unwrap().to_owned()
}

/// A song with one recording and a published lyric. Returns (song id, contribution id, recording id).
async fn song(w: &World, title: &str, singer: &str) -> (String, String, String) {
    let c = post(
        &w.r,
        "/api/v1/compositions",
        json!({ "title": title, "credits": [] }),
        Some(&w.member),
    )
    .await;
    assert_eq!(c.status, StatusCode::CREATED, "{}", c.body);
    let rec = post(
        &w.r,
        "/api/v1/recordings",
        json!({ "composition_id": c.body["id"], "title": title, "credits": [{ "artist_id": singer, "role": "performer" }] }),
        Some(&w.member),
    )
    .await;
    let p = post(
        &w.r,
        &format!("/api/v1/recordings/{}/contributions", id(&rec.body)),
        json!({ "type": "original", "language": "ml", "script": "Mlym", "body": "വരികൾ" }),
        Some(&w.moderator),
    )
    .await;
    assert_eq!(p.body["status"], "published", "{}", p.body);
    (id(&c.body), id(&p.body), id(&rec.body))
}

async fn artist(w: &World, name: &str) -> String {
    id(&post(
        &w.r,
        "/api/v1/artists",
        json!({ "name": name, "kind": "person" }),
        Some(&w.member),
    )
    .await
    .body)
}

/// One view from a distinct visitor (the address is the visitor's identity).
async fn view(w: &World, kind: &str, target: &str, from: &str) -> Res {
    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/v1/views")
        .header("x-requested-with", "test")
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-forwarded-for", from)
        .body(axum::body::Body::from(
            json!({ "kind": kind, "id": target }).to_string(),
        ))
        .unwrap();
    raw(&w.r, req).await
}

async fn views(w: &World, kind: &str, target: &str, n: usize) {
    for i in 0..n {
        let res = view(w, kind, target, &format!("10.0.{}.{}", target.len() % 200, i + 1)).await;
        assert_eq!(res.status, StatusCode::OK, "{}", res.body);
    }
}

async fn section(w: &World, key: &str) -> Vec<Value> {
    let res = get(&w.r, "/api/v1/home", None).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.body);
    res.body["sections"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["key"] == key)
        .unwrap()["items"]
        .as_array()
        .unwrap()
        .clone()
}

fn titles(items: &[Value]) -> Vec<&str> {
    items.iter().map(|i| i["title"]["text"].as_str().unwrap()).collect()
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn a_fresh_install_has_six_empty_sections_not_an_error(pool: PgPool) {
    let w = world(pool).await;
    let res = get(&w.r, "/api/v1/home", None).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.body);
    let keys: Vec<&str> = res.body["sections"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["key"].as_str().unwrap())
        .collect();
    assert_eq!(
        keys,
        [
            "editors_picks",
            "popular_songs",
            "new_songs",
            "popular_albums",
            "new_albums",
            "popular_artists"
        ]
    );
    assert!(res.body["sections"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["items"].as_array().unwrap().is_empty()));
    assert!(res.body["generated_at"].is_string());
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn views_count_once_per_visitor_and_day_and_only_for_public_pages(pool: PgPool) {
    let w = world(pool).await;
    let a = artist(&w, "Yesudas").await;
    let (s, ..) = song(&w, "Karunamayane", &a).await;
    let first = view(&w, "song", &s, "1.1.1.1").await;
    assert_eq!(first.body["views"], 1, "{}", first.body);
    assert_eq!(
        view(&w, "song", &s, "1.1.1.1").await.body["views"],
        1,
        "a reload is not a new view"
    );
    assert_eq!(view(&w, "song", &s, "2.2.2.2").await.body["views"], 2);
    let signed = send(
        &w.r,
        Method::POST,
        "/api/v1/views",
        Some(json!({ "kind": "song", "id": s })),
        Some(&w.rater),
    )
    .await;
    assert_eq!(signed.body["views"], 3);
    let again = send(
        &w.r,
        Method::POST,
        "/api/v1/views",
        Some(json!({ "kind": "song", "id": s })),
        Some(&w.rater),
    )
    .await;
    assert_eq!(again.body["views"], 3);
    // Nothing is kept that identifies the viewer.
    let leaked: i64 = sqlx::query_scalar("SELECT count(*) FROM view_seen WHERE seen_hash LIKE '%1.1.1.1%'")
        .fetch_one(&w.app.pool)
        .await
        .unwrap();
    assert_eq!(leaked, 0);

    // A song nobody has published lyrics for, and an unknown id, are not countable.
    let hidden = post(
        &w.r,
        "/api/v1/compositions",
        json!({ "title": "Draft", "credits": [] }),
        Some(&w.member),
    )
    .await;
    view(&w, "song", &id(&hidden.body), "3.3.3.3")
        .await
        .assert_problem(StatusCode::NOT_FOUND, "not_found");
    view(&w, "song", &uuid::Uuid::new_v4().to_string(), "3.3.3.3")
        .await
        .assert_problem(StatusCode::NOT_FOUND, "not_found");
    let bad = send(
        &w.r,
        Method::POST,
        "/api/v1/views",
        Some(json!({ "kind": "recording", "id": s })),
        None,
    )
    .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn popular_songs_rank_by_views_ratings_and_comments_and_stay_fixed_until_the_next_run(pool: PgPool) {
    let w = world(pool).await;
    let a = artist(&w, "Yesudas").await;
    let (quiet, ..) = song(&w, "Quiet", &a).await;
    let (viewed, ..) = song(&w, "Viewed", &a).await;
    let (loved, loved_c, _) = song(&w, "Loved", &a).await;
    let (_unseen, ..) = song(&w, "Unseen", &a).await;
    views(&w, "song", &quiet, 1).await;
    views(&w, "song", &viewed, 6).await;
    views(&w, "song", &loved, 3).await;
    let rate = send(
        &w.r,
        Method::PUT,
        &format!("/api/v1/contributions/{loved_c}/rating"),
        Some(json!({ "value": 5 })),
        Some(&w.rater),
    )
    .await;
    assert!(rate.status.is_success(), "{}", rate.body);
    let comment = post(
        &w.r,
        &format!("/api/v1/contributions/{loved_c}/comments"),
        json!({ "body": "Beautiful" }),
        Some(&w.rater),
    )
    .await;
    assert_eq!(comment.status, StatusCode::CREATED, "{}", comment.body);

    application::home::refresh(&w.app).await.unwrap();
    let list = section(&w, "popular_songs").await;
    // Loved: 3 views + a 5-star rating + a comment outranks Viewed's 6 plain views; Unseen has no activity so is absent.
    assert_eq!(titles(&list), ["Loved", "Viewed", "Quiet"], "{list:?}");
    assert_eq!(list[0]["rank"], 1);
    assert_eq!(list[0]["views"], 3);
    assert_eq!(list[0]["rating_count"], 1);
    assert_eq!(list[0]["comments"], 1);
    assert_eq!(list[0]["singers"][0]["name"]["text"], "Yesudas");
    assert!(
        list[0]["page"]["slug"].is_string(),
        "links to the public song page: {}",
        list[0]
    );

    // More views change nothing until the next run: the list is a fixed snapshot.
    views(&w, "song", &quiet, 40).await;
    assert_eq!(
        titles(&section(&w, "popular_songs").await),
        ["Loved", "Viewed", "Quiet"]
    );
    application::home::refresh(&w.app).await.unwrap();
    assert_eq!(titles(&section(&w, "popular_songs").await)[0], "Quiet");
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn new_songs_albums_and_most_viewed_artists(pool: PgPool) {
    let w = world(pool).await;
    let big = artist(&w, "Yesudas").await;
    let small = artist(&w, "Chitra").await;
    let (s1, _, r1) = song(&w, "First", &big).await;
    let (s2, _, r2) = song(&w, "Second", &small).await;
    let (_s3, ..) = song(&w, "Third", &small).await;
    let a1 = id(&post(
        &w.r,
        "/api/v1/albums",
        json!({ "title": "Old Album", "kind": "album", "tracks": [r1] }),
        Some(&w.member),
    )
    .await
    .body);
    let a2 = id(&post(
        &w.r,
        "/api/v1/albums",
        json!({ "title": "New Album", "kind": "album", "tracks": [r2] }),
        Some(&w.member),
    )
    .await
    .body);
    // An album with no public song never shows.
    post(
        &w.r,
        "/api/v1/albums",
        json!({ "title": "Empty", "kind": "album", "tracks": [] }),
        Some(&w.member),
    )
    .await;
    views(&w, "song", &s1, 5).await;
    views(&w, "song", &s2, 1).await;
    views(&w, "album", &a1, 2).await;
    views(&w, "artist", &small, 1).await;
    application::home::refresh(&w.app).await.unwrap();

    assert_eq!(
        titles(&section(&w, "new_songs").await),
        ["Third", "Second", "First"],
        "newest first"
    );
    assert_eq!(titles(&section(&w, "new_albums").await), ["New Album", "Old Album"]);
    let popular_albums = section(&w, "popular_albums").await;
    assert_eq!(
        titles(&popular_albums),
        ["Old Album", "New Album"],
        "the album of the viewed song, plus its own views"
    );
    assert!(popular_albums[0]["slug"].is_string(), "albums link to browse by slug");
    // Artists: page views plus views of their songs. Yesudas 5; Chitra 1 (page) + 1 (song).
    let artists = section(&w, "popular_artists").await;
    assert_eq!(titles(&artists), ["Yesudas", "Chitra"], "{artists:?}");
    assert_eq!(
        (artists[0]["views"].as_i64(), artists[1]["views"].as_i64()),
        (Some(5), Some(2))
    );
    let _ = a2;
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn lists_are_capped_and_the_hourly_job_clears_expired_view_markers(pool: PgPool) {
    let w = world(pool).await;
    let a = artist(&w, "Yesudas").await;
    for i in 0..12 {
        let (s, ..) = song(&w, &format!("Song {i:02}"), &a).await;
        views(&w, "song", &s, 1).await;
    }
    application::home::refresh(&w.app).await.unwrap();
    assert_eq!(section(&w, "new_songs").await.len(), 10);
    assert_eq!(section(&w, "popular_songs").await.len(), 10);
    // The hourly job's entry point does the same and also drops expired view-dedupe rows.
    sqlx::query("INSERT INTO view_seen (seen_hash, day) VALUES (repeat('a', 64), current_date - 5)")
        .execute(&w.app.pool)
        .await
        .unwrap();
    worker::refresh_home(&w.app.pool).await.unwrap();
    let old: i64 = sqlx::query_scalar("SELECT count(*) FROM view_seen WHERE day < current_date - 1")
        .fetch_one(&w.app.pool)
        .await
        .unwrap();
    assert_eq!(old, 0);
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn moderators_feature_songs_as_editors_picks_immediately(pool: PgPool) {
    let w = world(pool).await;
    let a = artist(&w, "Yesudas").await;
    let (s1, ..) = song(&w, "First", &a).await;
    let (s2, ..) = song(&w, "Second", &a).await;
    assert!(section(&w, "editors_picks").await.is_empty());

    let uri = |s: &str| format!("/api/v1/editors-picks/{s}");
    send(&w.r, Method::PUT, &uri(&s1), None, Some(&w.member))
        .await
        .assert_problem(StatusCode::FORBIDDEN, "forbidden");
    send(&w.r, Method::PUT, &uri(&s1), None, None)
        .await
        .assert_problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    assert_eq!(
        send(&w.r, Method::PUT, &uri(&s1), None, Some(&w.moderator))
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(
        send(&w.r, Method::PUT, &uri(&s1), None, Some(&w.moderator))
            .await
            .status,
        StatusCode::OK,
        "idempotent"
    );
    assert_eq!(
        send(&w.r, Method::PUT, &uri(&s2), None, Some(&w.moderator))
            .await
            .status,
        StatusCode::OK
    );
    // No worker run needed: curated picks show at once, newest pick first.
    assert_eq!(titles(&section(&w, "editors_picks").await), ["Second", "First"]);
    assert_eq!(get(&w.r, &uri(&s1), None).await.body["picked"], true);

    send(&w.r, Method::DELETE, &uri(&s2), None, Some(&w.moderator)).await;
    assert_eq!(titles(&section(&w, "editors_picks").await), ["First"]);
    let draft = post(
        &w.r,
        "/api/v1/compositions",
        json!({ "title": "Draft", "credits": [] }),
        Some(&w.member),
    )
    .await;
    send(&w.r, Method::PUT, &uri(&id(&draft.body)), None, Some(&w.moderator))
        .await
        .assert_problem(StatusCode::NOT_FOUND, "not_found");
}
