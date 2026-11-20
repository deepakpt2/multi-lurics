//! Reading and discovery features: YouTube links, devotional structure, whole-text lyric search,
//! related songs, filling in missing details, metadata pages and the bulk import.

mod common;

use axum::http::StatusCode;
use axum::Router;
use common::*;
use domain::Role;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;
use worker::{run_once, LogOnly, SearchIndexer};

fn id(v: &Value) -> String {
    v["id"].as_str().unwrap().to_owned()
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

async fn index(pool: &PgPool) {
    let h = SearchIndexer {
        pool: pool.clone(),
        inner: LogOnly,
        revalidate: None,
    };
    while run_once(pool, &h).await.unwrap() > 0 {}
}

struct World {
    r: Router,
    member: String,
    other: String,
    moderator: String,
}

async fn world(pool: &PgPool) -> World {
    let r = router(pool);
    let (_, member) = signed_in(pool, &r, "m1@example.com", None).await;
    let (_, other) = signed_in(pool, &r, "m2@example.com", None).await;
    let (_, moderator) = signed_in(pool, &r, "mod@example.com", Some(Role::Moderator)).await;
    World {
        r,
        member,
        other,
        moderator,
    }
}

async fn artist(w: &World, name: &str) -> Value {
    let res = post(
        &w.r,
        "/api/v1/artists",
        json!({ "name": name, "kind": "person" }),
        Some(&w.member),
    )
    .await;
    assert_eq!(res.status, StatusCode::CREATED, "{}", res.body);
    res.body
}

async fn song(w: &World, body: Value) -> Value {
    let res = post(&w.r, "/api/v1/compositions", body, Some(&w.member)).await;
    assert_eq!(res.status, StatusCode::CREATED, "{}", res.body);
    res.body
}

async fn recording(w: &World, comp: &Value, credits: Value) -> Value {
    let res = post(
        &w.r,
        "/api/v1/recordings",
        json!({ "composition_id": comp["id"], "title": comp["title"], "credits": credits }),
        Some(&w.member),
    )
    .await;
    assert_eq!(res.status, StatusCode::CREATED, "{}", res.body);
    res.body
}

async fn publish(w: &World, rec: &Value, kind: &str, lang: &str, script: &str, body: &str) -> Value {
    let res = post(
        &w.r,
        &format!("/api/v1/recordings/{}/contributions", id(rec)),
        json!({ "type": kind, "language": lang, "script": script, "body": body }),
        Some(&w.moderator),
    )
    .await;
    assert_eq!(res.body["status"], "published", "{}", res.body);
    res.body
}

async fn album_with(w: &World, title: &str, recs: &[&Value]) -> Value {
    let a = post(
        &w.r,
        "/api/v1/albums",
        json!({ "title": title, "kind": "soundtrack", "release_year": 1988 }),
        Some(&w.member),
    )
    .await;
    assert_eq!(a.status, StatusCode::CREATED, "{}", a.body);
    let ids: Vec<String> = recs.iter().map(|r| id(r)).collect();
    let up = send(
        &w.r,
        axum::http::Method::PATCH,
        &format!("/api/v1/albums/{}", id(&a.body)),
        Some(json!({ "tracks": ids, "expected_version": a.body["version"] })),
        Some(&w.member),
    )
    .await;
    assert_eq!(up.status, StatusCode::OK, "{}", up.body);
    a.body
}

async fn page_slug(pool: &PgPool, comp: &Value) -> String {
    sqlx::query_scalar(
        "SELECT slug FROM localized_slugs WHERE entity_type = 'composition' AND entity_id = $1::uuid AND is_current LIMIT 1",
    )
    .bind(id(comp))
    .fetch_one(pool)
    .await
    .unwrap()
}

fn problems(res: &Res) -> Vec<(String, String)> {
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
async fn a_youtube_link_in_any_form_is_stored_as_the_video_id(pool: PgPool) {
    let w = world(&pool).await;
    for link in [
        "https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=42",
        "https://youtu.be/dQw4w9WgXcQ?si=x",
        "https://www.youtube.com/shorts/dQw4w9WgXcQ",
        "dQw4w9WgXcQ",
    ] {
        let s = song(&w, json!({ "title": "Video", "youtube": link })).await;
        assert_eq!(s["youtube_id"], "dQw4w9WgXcQ", "{link}");
    }
    let bad = post(
        &w.r,
        "/api/v1/compositions",
        json!({ "title": "Video", "youtube": "https://vimeo.com/12345678" }),
        Some(&w.member),
    )
    .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        problems(&bad).contains(&("youtube".into(), "invalid_video_link".into())),
        "{}",
        bad.body
    );

    // Cleared by sending null; the public page carries only the id.
    let s = song(
        &w,
        json!({ "title": "Clip", "youtube": "https://youtu.be/dQw4w9WgXcQ" }),
    )
    .await;
    let rec = recording(&w, &s, json!([])).await;
    publish(&w, &rec, "original", "ml", "Mlym", "വരികൾ").await;
    index(&pool).await;
    let page = get(
        &w.r,
        &format!("/api/v1/pages/song/{}?locale=en", enc(&page_slug(&pool, &s).await)),
        None,
    )
    .await;
    assert_eq!(page.status, StatusCode::OK, "{}", page.body);
    assert_eq!(page.body["youtube_id"], "dQw4w9WgXcQ");
    let cleared = send(
        &w.r,
        axum::http::Method::PATCH,
        &format!("/api/v1/compositions/{}", id(&s)),
        Some(json!({ "youtube": null, "expected_version": s["version"] })),
        Some(&w.member),
    )
    .await;
    assert_eq!(cleared.status, StatusCode::OK);
    assert!(cleared.body["youtube_id"].is_null());
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn devotional_structure_belongs_to_devotional_songs_and_is_browsable(pool: PgPool) {
    let w = world(&pool).await;
    let s = song(
        &w,
        json!({
            "title": "Suprabhatam", "category": "devotional", "religion": "hindu", "deity": "Venkateswara",
            "devotional_form": "suprabhatam", "source_work": " Venkateswara Suprabhatam ",
            "occasions": ["Dhanurmasa", "dhanurmasa", "Morning"]
        }),
    )
    .await;
    assert_eq!(s["devotional_form"], "suprabhatam");
    assert_eq!(s["source_work"], "Venkateswara Suprabhatam");
    assert_eq!(s["occasions"], json!(["Dhanurmasa", "Morning"]));

    let bad = post(
        &w.r,
        "/api/v1/compositions",
        json!({ "title": "Pop", "devotional_form": "bhajan" }),
        Some(&w.member),
    )
    .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        problems(&bad).contains(&("devotional_form".into(), "devotional_only".into())),
        "{}",
        bad.body
    );
    let bad = post(
        &w.r,
        "/api/v1/compositions",
        json!({ "title": "X", "category": "devotional", "devotional_form": "polka" }),
        Some(&w.member),
    )
    .await;
    assert!(
        problems(&bad).contains(&("devotional_form".into(), "invalid_value".into())),
        "{}",
        bad.body
    );

    // Leaving devotional clears the structure.
    let flipped = send(
        &w.r,
        axum::http::Method::PATCH,
        &format!("/api/v1/compositions/{}", id(&s)),
        Some(json!({ "category": "mainstream", "expected_version": s["version"] })),
        Some(&w.member),
    )
    .await;
    assert_eq!(flipped.status, StatusCode::OK, "{}", flipped.body);
    assert!(flipped.body["devotional_form"].is_null());
    assert_eq!(flipped.body["occasions"], json!([]));

    // Published devotional songs can be browsed by form.
    let b = song(
        &w,
        json!({ "title": "Aarti", "category": "devotional", "devotional_form": "aarti" }),
    )
    .await;
    let rec = recording(&w, &b, json!([])).await;
    publish(&w, &rec, "original", "hi", "Deva", "आरती").await;
    let list = get(&w.r, "/api/v1/browse/songs?form=aarti", None).await;
    assert_eq!(list.body["total"], 1, "{}", list.body);
    assert_eq!(list.body["filters"][0]["facet"], "form");
    assert_eq!(list.body["filters"][0]["resolved"], true);
    let none = get(&w.r, "/api/v1/browse/songs?form=bhajan", None).await;
    assert_eq!(none.body["total"], 0);
    let values = get(&w.r, "/api/v1/browse/facets/form", None).await;
    assert_eq!(values.body["items"][0]["slug"], "aarti", "{}", values.body);
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn words_from_anywhere_in_a_lyric_are_found_and_openings_rank_first(pool: PgPool) {
    let w = world(&pool).await;
    for (title, body) in [
        ("Beta", "Someone said the moon river\nflows to the sea"),
        ("Gamma", "hello again\nMoon  River, flows on"),
        ("Alpha", "Moon river, wider than a mile\nI'm crossing you in style"),
        ("Unrelated", "nothing to see\nhere at all"),
    ] {
        let s = song(&w, json!({ "title": title })).await;
        let rec = recording(&w, &s, json!([])).await;
        publish(&w, &rec, "original", "en", "Latn", body).await;
    }
    index(&pool).await;

    let res = get(&w.r, "/api/v1/search?q=moon%20river&types=composition", None).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.body);
    let items = res.body["groups"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["type"] == "compositions")
        .unwrap()["items"]
        .as_array()
        .unwrap()
        .clone();
    let titles: Vec<&str> = items.iter().map(|i| i["display"]["text"].as_str().unwrap()).collect();
    assert_eq!(
        titles,
        ["Alpha", "Gamma", "Beta"],
        "opening, then line start, then inside: {}",
        res.body
    );
    assert_eq!(items[0]["match"]["via"], "lyrics");
    assert_eq!(items[0]["match"]["text"], "Moon river, wider than a mile");
    assert_eq!(
        items[1]["match"]["text"], "Moon  River, flows on",
        "the line as written, not normalized"
    );
    assert_eq!(items[2]["match"]["text"], "Someone said the moon river");

    // A title match still comes before words inside a lyric.
    let s = song(&w, json!({ "title": "Moon River" })).await;
    let rec = recording(&w, &s, json!([])).await;
    publish(&w, &rec, "original", "en", "Latn", "completely different words").await;
    index(&pool).await;
    let res = get(&w.r, "/api/v1/search?q=moon%20river&types=composition", None).await;
    let first = &res.body["groups"][0]["items"][0];
    assert_eq!(first["display"]["text"], "Moon River");
    assert_eq!(first["match"]["via"], "name");

    // Words from a word in the middle of a Malayalam line, and unpublishing removes the lyric from search.
    let s = song(&w, json!({ "title": "Ml" })).await;
    let rec = recording(&w, &s, json!([])).await;
    let lyric = publish(&w, &rec, "original", "ml", "Mlym", "ആദ്യ വരി\nഇത് ഒരു മധുര ഗാനം").await;
    let res = get(
        &w.r,
        &format!("/api/v1/search?q={}&types=composition", enc("മധുര ഗാനം")),
        None,
    )
    .await;
    assert_eq!(
        res.body["groups"][0]["items"][0]["display"]["text"], "Ml",
        "{}",
        res.body
    );
    let gone: i64 = sqlx::query_scalar("SELECT count(*) FROM lyric_search WHERE contribution_id = $1::uuid")
        .bind(id(&lyric))
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(gone, 1);
    sqlx::query("UPDATE contributions SET published_revision_id = NULL, is_primary = false, status = 'approved' WHERE id = $1::uuid").bind(id(&lyric)).execute(&pool).await.unwrap();
    let gone: i64 = sqlx::query_scalar("SELECT count(*) FROM lyric_search WHERE contribution_id = $1::uuid")
        .bind(id(&lyric))
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(gone, 0, "an unpublished lyric leaves the search index at once");
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn a_song_page_lists_related_songs_and_why(pool: PgPool) {
    let w = world(&pool).await;
    let singer = artist(&w, "Yesudas").await;
    let credit = json!([{ "artist_id": singer["id"], "role": "performer" }]);
    let a = song(
        &w,
        json!({ "title": "Song A", "category": "devotional", "deity": "Krishna" }),
    )
    .await;
    let b = song(
        &w,
        json!({ "title": "Song B", "category": "devotional", "deity": "Krishna" }),
    )
    .await;
    let c = song(&w, json!({ "title": "Song C" })).await;
    let d = song(&w, json!({ "title": "Song D" })).await;
    let e = song(&w, json!({ "title": "Song E" })).await;
    let ra = recording(&w, &a, credit.clone()).await;
    let rb = recording(&w, &b, json!([])).await;
    let rc = recording(&w, &c, credit.clone()).await;
    let rd = recording(&w, &d, json!([])).await;
    let re = recording(&w, &e, json!([])).await;
    album_with(&w, "Gokulam", &[&ra, &rd]).await;
    for (rec, body) in [(&ra, "a"), (&rb, "b"), (&rc, "c"), (&rd, "d")] {
        publish(&w, rec, "original", "en", "Latn", body).await;
    }
    // E has no published lyric: it is never listed.
    let _ = re;
    index(&pool).await;

    let page = get(
        &w.r,
        &format!("/api/v1/pages/song/{}?locale=en", enc(&page_slug(&pool, &a).await)),
        None,
    )
    .await;
    assert_eq!(page.status, StatusCode::OK, "{}", page.body);
    let related = page.body["related"].as_array().unwrap();
    let names: Vec<(&str, Vec<&str>)> = related
        .iter()
        .map(|r| {
            (
                r["song"]["label"]["text"].as_str().unwrap(),
                r["reasons"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| x.as_str().unwrap())
                    .collect(),
            )
        })
        .collect();
    assert_eq!(
        names,
        [
            ("Song D", vec!["album"]),
            ("Song B", vec!["deity"]),
            ("Song C", vec!["singer"])
        ],
        "album (4) before deity (3) before singer (3, by title); unpublished and the song itself never: {}",
        page.body
    );
    assert!(related[0]["song"]["page"]["slug"].is_string());
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn anyone_signed_in_may_fill_in_what_is_blank_but_not_change_what_is_there(pool: PgPool) {
    let w = world(&pool).await;
    let s = song(
        &w,
        json!({ "title": "Raga song", "category": "devotional", "deity": "Shiva" }),
    )
    .await;
    let uri = format!("/api/v1/compositions/{}", id(&s));
    async fn patch_as(w: &World, uri: &str, mut body: Value, who: Option<&str>) -> Res {
        let current = get(&w.r, uri, None).await;
        body["expected_version"] = current.body["version"].clone();
        send(&w.r, axum::http::Method::PATCH, uri, Some(body), who).await
    }
    let patch = |body: Value, who: Option<&str>| {
        let (uri, who) = (uri.clone(), who.map(str::to_owned));
        let w = &w;
        async move { patch_as(w, &uri, body, who.as_deref()).await }
    };

    assert_eq!(
        patch(json!({ "raga": "Mohanam" }), None).await.status,
        StatusCode::UNAUTHORIZED
    );
    // Someone else fills in blanks.
    let ok = patch(json!({ "raga": "Mohanam", "year": 1975, "devotional_form": "stotra", "youtube": "dQw4w9WgXcQ", "tags": ["night"] }), Some(&w.other)).await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.body);
    assert_eq!(ok.body["raga"], "Mohanam");
    assert_eq!(ok.body["year"], 1975);
    assert_eq!(ok.body["youtube_id"], "dQw4w9WgXcQ");
    // They may not change what has a value, or the title, or the category.
    for body in [
        json!({ "raga": "Kalyani" }),
        json!({ "year": 1980 }),
        json!({ "deity": "Vishnu" }),
        json!({ "title": "Other" }),
        json!({ "category": "mainstream" }),
        json!({ "tags": ["day"] }),
        json!({ "youtube": "aaaaaaaaaaa" }),
    ] {
        let res = patch(body.clone(), Some(&w.other)).await;
        assert_eq!(res.status, StatusCode::FORBIDDEN, "{body}: {}", res.body);
    }
    // The same value again is not a change.
    assert_eq!(
        patch(json!({ "raga": "Mohanam" }), Some(&w.other)).await.status,
        StatusCode::OK
    );
    // The creator and moderators still change anything.
    assert_eq!(
        patch(json!({ "raga": "Kalyani", "title": "Renamed" }), Some(&w.member))
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(
        patch(json!({ "year": 1990 }), Some(&w.moderator)).await.status,
        StatusCode::OK
    );

    // Credits may be added to a song that has none, not replaced.
    let artist = artist(&w, "Ilaiyaraaja").await;
    let credit = json!({ "credits": [{ "artist_id": artist["id"], "role": "composer" }] });
    assert_eq!(patch(credit.clone(), Some(&w.other)).await.status, StatusCode::OK);
    let second = artist_named(&w, "Someone Else").await;
    let swap = json!({ "credits": [{ "artist_id": second["id"], "role": "composer" }] });
    assert_eq!(patch(swap, Some(&w.other)).await.status, StatusCode::FORBIDDEN);

    // Artists and albums follow the same rule.
    let a = artist_named(&w, "No Details").await;
    let uri = format!("/api/v1/artists/{}", id(&a));
    let fill = patch_as_any(&w, &uri, json!({ "disambiguation": "singer, b. 1940" }), &w.other).await;
    assert_eq!(fill.status, StatusCode::OK, "{}", fill.body);
    let change = patch_as_any(&w, &uri, json!({ "disambiguation": "other" }), &w.other).await;
    assert_eq!(change.status, StatusCode::FORBIDDEN);
}

async fn patch_as_any(w: &World, uri: &str, mut body: Value, who: &str) -> Res {
    let current = get(&w.r, uri, None).await;
    body["expected_version"] = current.body["version"].clone();
    send(&w.r, axum::http::Method::PATCH, uri, Some(body), Some(who)).await
}

async fn artist_named(w: &World, name: &str) -> Value {
    artist(w, name).await
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn a_metadata_page_has_its_picture_facts_and_songs_grouped_by_album(pool: PgPool) {
    let w = world(&pool).await;
    let singer = artist(&w, "Chithra").await;
    let credit = json!([{ "artist_id": singer["id"], "role": "performer" }]);
    let mut recs = Vec::new();
    for t in ["Zebra", "Apple", "Mango", "Solo"] {
        let s = song(&w, json!({ "title": t })).await;
        let r = recording(&w, &s, credit.clone()).await;
        publish(&w, &r, "original", "ml", "Mlym", t).await;
        recs.push(r);
    }
    album_with(&w, "B Album", &[&recs[0], &recs[1]]).await;
    album_with(&w, "A Album", &[&recs[2]]).await;
    // Browse addresses (slugs) are made when a value is first listed, as the app does when it shows a card.
    assert_eq!(get(&w.r, "/api/v1/browse/songs", None).await.body["total"], 4);
    let res = get(
        &w.r,
        "/api/v1/browse/songs?singer=chithra&sort=album&page_size=50",
        None,
    )
    .await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.body);
    let titles: Vec<&str> = res.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["title"]["text"].as_str().unwrap())
        .collect();
    assert_eq!(
        titles,
        ["Mango", "Zebra", "Apple", "Solo"],
        "albums A then B (track order inside), singles last: {}",
        res.body
    );
    let f = &res.body["filters"][0];
    assert_eq!(f["id"], singer["id"]);
    assert_eq!(f["kind"], "person");
    assert!(f["image_id"].is_null());
    let album = get(&w.r, "/api/v1/browse/songs?album=a-album", None).await;
    assert_eq!(album.body["filters"][0]["kind"], "soundtrack");
    assert_eq!(album.body["filters"][0]["detail"], "1988");
    let bad = get(&w.r, "/api/v1/browse/songs?sort=sideways", None).await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
}

fn import_row(title: &str) -> Value {
    json!({
        "title": title, "language": "ml", "lyrics": format!("{title} വരികൾ\nരണ്ടാം വരി"), "album": "Kireedam",
        "album_kind": "soundtrack", "year": 1989, "singers": ["M. G. Sreekumar", "K. S. Chithra"],
        "composers": ["Johnson"], "lyricists": ["Kaithapram"], "tags": ["film"]
    })
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn moderators_import_songs_in_bulk_and_a_second_run_changes_nothing(pool: PgPool) {
    let w = world(&pool).await;
    let body = json!({ "rows": [
        import_row("Kannum Kannum"),
        import_row("Pavizham"),
        { "title": "Hanuman Chalisa", "language": "hi", "lyrics": "श्रीगुरु चरन सरोज रज", "category": "devotional", "religion": "hindu", "deity": "Hanuman", "devotional_form": "chalisa", "source_work": "Tulsidas", "occasions": ["Tuesday"], "youtube": "https://youtu.be/dQw4w9WgXcQ" },
        { "title": "", "language": "ml" },
        { "title": "Bad", "language": "klingon", "lyrics": "x" },
        { "title": "No Lyrics Yet", "album": "Kireedam" },
    ], "dry_run": true });

    let denied = post(&w.r, "/api/v1/import/songs", body.clone(), Some(&w.member)).await;
    assert_eq!(denied.status, StatusCode::FORBIDDEN);
    let anon = post(&w.r, "/api/v1/import/songs", body.clone(), None).await;
    assert_eq!(anon.status, StatusCode::UNAUTHORIZED);

    let dry = post(&w.r, "/api/v1/import/songs", body.clone(), Some(&w.moderator)).await;
    assert_eq!(dry.status, StatusCode::OK, "{}", dry.body);
    let statuses: Vec<&str> = dry.body["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["status"].as_str().unwrap())
        .collect();
    assert_eq!(
        statuses,
        [
            "would_create",
            "would_create",
            "would_create",
            "failed",
            "failed",
            "would_create"
        ]
    );
    assert_eq!(dry.body["created"], 4);
    assert_eq!(dry.body["failed"], 2);
    assert_eq!(dry.body["results"][3]["problems"][0]["field"], "title");
    assert_eq!(dry.body["results"][4]["problems"][0]["field"], "language");
    let written: i64 =
        sqlx::query_scalar("SELECT (SELECT count(*) FROM compositions) + (SELECT count(*) FROM artists)")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(written, 0, "a dry run writes nothing");

    let mut real = body.clone();
    real["dry_run"] = json!(false);
    let done = post(&w.r, "/api/v1/import/songs", real.clone(), Some(&w.moderator)).await;
    assert_eq!(done.status, StatusCode::OK, "{}", done.body);
    assert_eq!(done.body["created"], 4, "{}", done.body);
    assert_eq!(done.body["failed"], 2);

    let c = |sql: &'static str| {
        let pool = pool.clone();
        async move { sqlx::query_scalar::<_, i64>(sql).fetch_one(&pool).await.unwrap() }
    };
    assert_eq!(c("SELECT count(*) FROM compositions").await, 4);
    assert_eq!(
        c("SELECT count(*) FROM artists").await,
        4,
        "people are created once and reused"
    );
    assert_eq!(c("SELECT count(*) FROM albums").await, 1);
    assert_eq!(c("SELECT count(*) FROM album_tracks").await, 3);
    assert_eq!(c("SELECT count(*) FROM credits WHERE role = 'performer'").await, 4);
    assert_eq!(
        c("SELECT count(*) FROM credits WHERE role IN ('composer', 'lyricist')").await,
        4
    );
    assert_eq!(
        c("SELECT count(*) FROM contributions WHERE published_revision_id IS NOT NULL").await,
        3
    );
    let (form, source, yt): (String, String, String) = sqlx::query_as(
        "SELECT devotional_form, source_work, youtube_id FROM compositions WHERE title = 'Hanuman Chalisa'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        (form.as_str(), source.as_str(), yt.as_str()),
        ("chalisa", "Tulsidas", "dQw4w9WgXcQ")
    );
    let positions: Vec<i32> = sqlx::query_scalar("SELECT position FROM album_tracks ORDER BY position")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(positions, [1, 2, 3]);

    // Imported lyrics are public and searchable at once.
    let found = get(
        &w.r,
        &format!("/api/v1/search?q={}&types=composition", enc("രണ്ടാം വരി")),
        None,
    )
    .await;
    assert_eq!(
        found.body["groups"][0]["items"].as_array().unwrap().len(),
        2,
        "{}",
        found.body
    );

    // Running the same file again skips what is there.
    let again = post(&w.r, "/api/v1/import/songs", real, Some(&w.moderator)).await;
    assert_eq!(again.body["created"], 0, "{}", again.body);
    assert_eq!(again.body["skipped"], 4);
    assert_eq!(again.body["results"][0]["reason"], "already_exists");
    assert_eq!(c("SELECT count(*) FROM compositions").await, 4);

    // Limits.
    let empty = post(&w.r, "/api/v1/import/songs", json!({ "rows": [] }), Some(&w.moderator)).await;
    assert_eq!(empty.status, StatusCode::UNPROCESSABLE_ENTITY);
    let many = post(
        &w.r,
        "/api/v1/import/songs",
        json!({ "rows": (0..101).map(|i| json!({ "title": format!("t{i}") })).collect::<Vec<_>>(), "dry_run": true }),
        Some(&w.moderator),
    )
    .await;
    assert_eq!(many.status, StatusCode::UNPROCESSABLE_ENTITY);
    let _: Uuid = Uuid::nil();
}
