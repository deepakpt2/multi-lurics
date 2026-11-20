//! Browsing songs by metadata: filters that combine with AND, typed suggestions that respect the
//! filters already chosen, localized names and aliases, slugs that never show an id, and paging.

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
    moderator: String,
    admin_id: Uuid,
}

async fn world(pool: &PgPool) -> World {
    let r = router(pool);
    let (_, member) = signed_in(pool, &r, "m1@example.com", None).await;
    let (_, moderator) = signed_in(pool, &r, "mod@example.com", Some(Role::Moderator)).await;
    let (admin_id, _) = signed_in(pool, &r, "admin@example.com", Some(Role::Admin)).await;
    World {
        r,
        member,
        moderator,
        admin_id,
    }
}

async fn create(w: &World, path: &str, body: Value) -> Value {
    let res = post(&w.r, path, body, Some(&w.member)).await;
    assert_eq!(res.status, StatusCode::CREATED, "{}", res.body);
    res.body
}

async fn artist(w: &World, name: &str) -> String {
    id(&create(w, "/api/v1/artists", json!({ "name": name, "kind": "person" })).await)
}

/// A song with one recording and a published lyric. `singers` perform it; `album` holds it.
async fn song(
    w: &World,
    title: &str,
    singers: &[&str],
    writers: Value,
    extra: Value,
    album: Option<&str>,
) -> (String, String) {
    let mut body = json!({ "title": title, "credits": writers });
    body.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    let c = create(w, "/api/v1/compositions", body).await;
    let credits: Vec<Value> = singers
        .iter()
        .map(|a| json!({ "artist_id": a, "role": "performer" }))
        .collect();
    let rec = create(
        w,
        "/api/v1/recordings",
        json!({ "composition_id": c["id"], "title": title, "credits": credits }),
    )
    .await;
    let res = post(
        &w.r,
        &format!("/api/v1/recordings/{}/contributions", id(&rec)),
        json!({ "type": "original", "language": "ml", "script": "Mlym", "body": "വരികൾ" }),
        Some(&w.moderator),
    )
    .await;
    assert_eq!(res.body["status"], "published", "{}", res.body);
    if let Some(a) = album {
        let res = send(
            &w.r,
            axum::http::Method::PATCH,
            &format!("/api/v1/albums/{a}"),
            Some(json!({ "tracks": [rec["id"]] })),
            Some(&w.member),
        )
        .await;
        // Adding to an album that already has tracks must keep them.
        assert!(res.status.is_success(), "{}", res.body);
    }
    (id(&c), id(&rec))
}

async fn album(w: &World, title: &str, tracks: &[&str]) -> String {
    id(&create(
        w,
        "/api/v1/albums",
        json!({ "title": title, "kind": "album", "tracks": tracks }),
    )
    .await)
}

async fn ok(w: &World, uri: &str) -> Value {
    let res = get(&w.r, uri, None).await;
    assert_eq!(res.status, StatusCode::OK, "{uri}: {}", res.body);
    res.body
}

fn titles(b: &Value) -> Vec<String> {
    let mut t: Vec<String> = b["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["title"]["text"].as_str().unwrap().to_owned())
        .collect();
    t.sort();
    t
}

fn kinds(b: &Value) -> Vec<(String, String)> {
    b["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            (
                s["kind"].as_str().unwrap().to_owned(),
                s["label"]["text"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

struct Data {
    yesudas: String,
    chitra: String,
    song1: String,
}

/// Yesudas sings "Yesuvinte Namam" and "Karunamayane" (album "Yesu Mahathwam"); Chitra sings
/// "Chitra Gaanam" (album "Yesu Unrelated"). Devarajan composes the first.
async fn seed(w: &World, pool: &PgPool) -> Data {
    let yesudas = artist(w, "Yesudas").await;
    let chitra = artist(w, "Chitra").await;
    let devarajan = artist(w, "Devarajan").await;
    let (song1, rec1) = song(
        w,
        "Yesuvinte Namam",
        &[&yesudas],
        json!([{ "artist_id": devarajan, "role": "composer" }]),
        json!({ "category": "devotional", "religion": "christian", "deity": "Jesus", "raga": "Mohanam" }),
        None,
    )
    .await;
    let (_, rec2) = song(w, "Karunamayane", &[&yesudas, &chitra], json!([]), json!({}), None).await;
    let (_, rec3) = song(w, "Chitra Gaanam", &[&chitra], json!([]), json!({}), None).await;
    album(w, "Yesu Mahathwam", &[&rec1, &rec2]).await;
    album(w, "Yesu Unrelated", &[&rec3]).await;
    // A song nobody has published a lyric for never shows.
    let hidden = create(
        w,
        "/api/v1/compositions",
        json!({ "title": "Yesu Draft", "credits": [] }),
    )
    .await;
    create(
        w,
        "/api/v1/recordings",
        json!({ "composition_id": hidden["id"], "title": "Yesu Draft" }),
    )
    .await;
    index(pool).await;
    Data { yesudas, chitra, song1 }
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn typing_offers_songs_and_typed_metadata_and_never_unpublished_songs(pool: PgPool) {
    let w = world(&pool).await;
    seed(&w, &pool).await;
    let b = ok(&w, "/api/v1/browse/suggest?q=yesu").await;
    let got = kinds(&b);
    for expected in [
        ("singer", "Yesudas"),
        ("song", "Yesuvinte Namam"),
        ("album", "Yesu Mahathwam"),
        ("album", "Yesu Unrelated"),
    ] {
        assert!(
            got.contains(&(expected.0.into(), expected.1.into())),
            "{expected:?} in {got:?}"
        );
    }
    assert!(
        !got.iter().any(|(_, l)| l == "Yesu Draft"),
        "unpublished song is not offered: {got:?}"
    );
    // The person is offered in the roles they actually have: no composer entry for a singer.
    assert!(!got.contains(&("composer".into(), "Yesudas".into())));
    // Other roles are found where they exist.
    let c = ok(&w, "/api/v1/browse/suggest?q=devar").await;
    assert_eq!(kinds(&c), vec![("composer".to_owned(), "Devarajan".to_owned())]);
    // Words from other kinds of metadata.
    assert!(kinds(&ok(&w, "/api/v1/browse/suggest?q=mohan").await).contains(&("raga".into(), "Mohanam".into())));
    assert!(kinds(&ok(&w, "/api/v1/browse/suggest?q=jes").await).contains(&("deity".into(), "Jesus".into())));
    assert!(kinds(&ok(&w, "/api/v1/browse/suggest?q=christ").await).contains(&("religion".into(), "christian".into())));
    let lang = ok(&w, "/api/v1/browse/suggest?q=malay").await;
    assert!(
        lang["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["kind"] == "language" && x["slug"] == "mal"),
        "{lang}"
    );
    // Small typos still land.
    assert!(kinds(&ok(&w, "/api/v1/browse/suggest?q=yesudaas").await).contains(&("singer".into(), "Yesudas".into())));
    get(&w.r, "/api/v1/browse/suggest?q=y", None)
        .await
        .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn suggestions_narrow_to_what_exists_with_the_active_filters(pool: PgPool) {
    let w = world(&pool).await;
    seed(&w, &pool).await;
    let b = ok(&w, "/api/v1/browse/suggest?q=yesu&singer=yesudas").await;
    let got = kinds(&b);
    assert!(got.contains(&("album".into(), "Yesu Mahathwam".into())), "{got:?}");
    assert!(
        !got.contains(&("album".into(), "Yesu Unrelated".into())),
        "an album without this singer is not offered: {got:?}"
    );
    assert!(got.contains(&("song".into(), "Yesuvinte Namam".into())));
    // The chosen singer is not offered again; a co-singer on the same songs still is.
    assert!(!got.contains(&("singer".into(), "Yesudas".into())));
    let co = kinds(&ok(&w, "/api/v1/browse/suggest?q=chit&singer=yesudas").await);
    assert!(co.contains(&("singer".into(), "Chitra".into())), "{co:?}");
    assert!(
        !co.contains(&("song".into(), "Chitra Gaanam".into())),
        "Chitra's solo song is not Yesudas's: {co:?}"
    );
    // Two filters narrow further; an impossible combination offers nothing.
    let none = kinds(&ok(&w, "/api/v1/browse/suggest?q=yesu&singer=yesudas&album=yesu-unrelated").await);
    assert!(none.is_empty(), "{none:?}");
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn filters_combine_with_and_and_can_be_removed(pool: PgPool) {
    let w = world(&pool).await;
    seed(&w, &pool).await;
    let all = ok(&w, "/api/v1/browse/songs").await;
    assert_eq!(
        (all["total"].as_i64(), titles(&all)),
        (
            Some(3),
            vec!["Chitra Gaanam".into(), "Karunamayane".into(), "Yesuvinte Namam".into()]
        )
    );

    let singer = ok(&w, "/api/v1/browse/songs?singer=yesudas").await;
    assert_eq!(titles(&singer), vec!["Karunamayane", "Yesuvinte Namam"]);
    assert_eq!(singer["filters"][0]["facet"], "singer");
    assert_eq!(singer["filters"][0]["label"]["text"], "Yesudas");

    let both = ok(&w, "/api/v1/browse/songs?singer=yesudas&album=yesu-mahathwam").await;
    assert_eq!(titles(&both), vec!["Karunamayane", "Yesuvinte Namam"]);
    let none = ok(&w, "/api/v1/browse/songs?singer=yesudas&album=yesu-unrelated").await;
    assert_eq!(none["total"], 0);
    // Two of one kind also AND: only the song both sing.
    let duet = ok(&w, "/api/v1/browse/songs?singer=yesudas&singer=chitra").await;
    assert_eq!(titles(&duet), vec!["Karunamayane"]);
    // Removing the album leaves the singer's list; removing everything restores the general list.
    assert_eq!(ok(&w, "/api/v1/browse/songs?singer=yesudas").await["total"], 2);
    assert_eq!(ok(&w, "/api/v1/browse/songs").await["total"], 3);

    // Other kinds of metadata, combined with a person.
    assert_eq!(
        titles(&ok(&w, "/api/v1/browse/songs?composer=devarajan").await),
        vec!["Yesuvinte Namam"]
    );
    assert_eq!(
        titles(
            &ok(
                &w,
                "/api/v1/browse/songs?raga=mohanam&religion=christian&category=devotional&deity=jesus"
            )
            .await
        ),
        vec!["Yesuvinte Namam"]
    );
    assert_eq!(
        ok(&w, "/api/v1/browse/songs?raga=mohanam&singer=chitra").await["total"],
        0
    );
    assert_eq!(ok(&w, "/api/v1/browse/songs?language=mal").await["total"], 3);
    assert_eq!(ok(&w, "/api/v1/browse/songs?language=ara").await["total"], 0);
    // "Artist" is anyone credited in any role.
    assert_eq!(
        titles(&ok(&w, "/api/v1/browse/songs?artist=devarajan").await),
        vec!["Yesuvinte Namam"]
    );
    assert_eq!(ok(&w, "/api/v1/browse/songs?artist=chitra").await["total"], 2);
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn an_unknown_filter_matches_nothing_and_says_so(pool: PgPool) {
    let w = world(&pool).await;
    seed(&w, &pool).await;
    let b = ok(&w, "/api/v1/browse/songs?singer=nobody").await;
    assert_eq!(b["total"], 0);
    assert_eq!(
        (b["filters"][0]["resolved"].clone(), b["filters"][0]["slug"].clone()),
        (json!(false), json!("nobody"))
    );
    let b = ok(&w, "/api/v1/browse/songs?singer=yesudas&album=nope").await;
    assert_eq!(b["total"], 0);
    assert_eq!(b["filters"][0]["resolved"], true);
    // A parameter that is not a filter is ignored; bad paging is a field error.
    assert_eq!(ok(&w, "/api/v1/browse/songs?utm_source=x").await["total"], 3);
    for q in ["page=0", "page_size=0", "page_size=51", "page=x", "sort=bogus"] {
        get(&w.r, &format!("/api/v1/browse/songs?{q}"), None)
            .await
            .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    }
    get(&w.r, "/api/v1/browse/facets/nonsense", None)
        .await
        .assert_problem(StatusCode::NOT_FOUND, "not_found");
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn songs_carry_what_a_card_shows_and_a_link_that_resolves(pool: PgPool) {
    let w = world(&pool).await;
    seed(&w, &pool).await;
    let b = ok(
        &w,
        "/api/v1/browse/songs?singer=yesudas&album=yesu-mahathwam&sort=title",
    )
    .await;
    let s = &b["items"][0];
    assert_eq!(s["title"]["text"], "Karunamayane");
    let names: Vec<&str> = s["singers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["name"]["text"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["Yesudas", "Chitra"]);
    assert_eq!(s["albums"][0]["slug"], "yesu-mahathwam");
    assert_eq!(
        (s["original_language"]["code"].clone(), s["translations"].clone()),
        (json!("ml"), json!([]))
    );
    // The song link is a real, published page.
    let page = &s["page"];
    let res = get(
        &w.r,
        &format!(
            "/api/v1/pages/song/{}?locale={}",
            enc(page["slug"].as_str().unwrap()),
            page["locale"].as_str().unwrap()
        ),
        None,
    )
    .await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.body);
    assert_eq!(res.body["title"]["text"], "Karunamayane");
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn pagination_works_with_and_without_filters(pool: PgPool) {
    let w = world(&pool).await;
    seed(&w, &pool).await;
    let p1 = ok(&w, "/api/v1/browse/songs?page_size=1&sort=title").await;
    assert_eq!(
        (p1["total"].clone(), p1["total_pages"].clone(), p1["page"].clone()),
        (json!(3), json!(3), json!(1))
    );
    let seen: Vec<String> = {
        let mut v = Vec::new();
        for p in 1..=3 {
            v.extend(titles(
                &ok(&w, &format!("/api/v1/browse/songs?page_size=1&page={p}&sort=title")).await,
            ));
        }
        v
    };
    assert_eq!(seen, vec!["Chitra Gaanam", "Karunamayane", "Yesuvinte Namam"]);
    // With a filter: the totals describe the filtered list, and the last page is partial.
    let f2 = ok(&w, "/api/v1/browse/songs?singer=yesudas&page_size=1&page=2&sort=title").await;
    assert_eq!(
        (f2["total"].clone(), f2["total_pages"].clone(), titles(&f2)),
        (json!(2), json!(2), vec!["Yesuvinte Namam".to_owned()])
    );
    // Past the end: empty items, a truthful total.
    let beyond = ok(&w, "/api/v1/browse/songs?singer=yesudas&page_size=1&page=9").await;
    assert_eq!(
        (beyond["items"].as_array().unwrap().len(), beyond["total"].clone()),
        (0, json!(2))
    );
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn metadata_pages_list_values_in_context_with_song_counts(pool: PgPool) {
    let w = world(&pool).await;
    seed(&w, &pool).await;
    let b = ok(&w, "/api/v1/browse/facets/singer").await;
    let got: Vec<(String, i64)> = b["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| (x["slug"].as_str().unwrap().to_owned(), x["songs"].as_i64().unwrap()))
        .collect();
    assert_eq!(got, vec![("chitra".to_owned(), 2), ("yesudas".to_owned(), 2)]);
    assert_eq!(b["total"], 2);
    // In the context of an album, only its people.
    let ctx = ok(&w, "/api/v1/browse/facets/singer?album=yesu-unrelated").await;
    assert_eq!(ctx["items"].as_array().unwrap().len(), 1);
    assert_eq!(ctx["items"][0]["slug"], "chitra");
    // Search inside a page of values, and paging of values.
    assert_eq!(
        ok(&w, "/api/v1/browse/facets/singer?q=yesu").await["items"][0]["slug"],
        "yesudas"
    );
    let p = ok(&w, "/api/v1/browse/facets/album?page_size=1&page=2").await;
    assert_eq!(
        (
            p["total"].clone(),
            p["total_pages"].clone(),
            p["items"].as_array().unwrap().len()
        ),
        (json!(2), json!(2), 1)
    );
    // Composers, lyricists, text-valued kinds.
    assert_eq!(
        ok(&w, "/api/v1/browse/facets/composer").await["items"][0]["slug"],
        "devarajan"
    );
    assert_eq!(ok(&w, "/api/v1/browse/facets/lyricist").await["total"], 0);
    let ragas = ok(&w, "/api/v1/browse/facets/raga").await;
    assert_eq!(
        (
            ragas["items"][0]["slug"].clone(),
            ragas["items"][0]["label"]["text"].clone()
        ),
        (json!("mohanam"), json!("Mohanam"))
    );
    assert_eq!(
        ok(&w, "/api/v1/browse/facets/language").await["items"][0]["slug"],
        "mal"
    );
    assert_eq!(
        ok(&w, "/api/v1/browse/facets/religion").await["items"][0]["slug"],
        "christian"
    );
    // Selecting an item shows its songs: the slug of a value is the filter for the list.
    for facet in ["singer", "album", "composer", "raga", "language", "religion"] {
        let v = ok(&w, &format!("/api/v1/browse/facets/{facet}")).await;
        let slug = v["items"][0]["slug"].as_str().unwrap();
        assert!(
            ok(&w, &format!("/api/v1/browse/songs?{facet}={}", enc(slug))).await["total"]
                .as_i64()
                .unwrap()
                > 0,
            "{facet}"
        );
    }
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn localized_names_and_aliases_find_the_same_entity_and_slugs_hide_ids(pool: PgPool) {
    let w = world(&pool).await;
    let d = seed(&w, &pool).await;
    for (sql_kind, name) in [("localization", "യേശുദാസ്"), ("alias", "K. J. Jesudas")] {
        if sql_kind == "localization" {
            sqlx::query(
                "INSERT INTO entity_localizations (entity_type, entity_id, language_id, script_id, name, is_preferred, provenance, source,
                                                   verification_state, created_by, verified_by, verified_at)
                 VALUES ('artist', $1::uuid, (SELECT id FROM languages WHERE iso639_3 = 'mal'), (SELECT id FROM scripts WHERE iso15924 = 'Mlym'),
                         $2, false, 'community', 'test', 'verified', $3, $3, now())",
            )
            .bind(&d.yesudas).bind(name).bind(w.admin_id).execute(&pool).await.unwrap();
        } else {
            sqlx::query(
                "INSERT INTO entity_aliases (entity_type, entity_id, alias, source, verification_state, created_by, verified_by, verified_at)
                 VALUES ('artist', $1::uuid, $2, 'test', 'verified', $3, $3, now())",
            )
            .bind(&d.yesudas).bind(name).bind(w.admin_id).execute(&pool).await.unwrap();
        }
    }
    index(&pool).await;
    for q in ["യേശു", "jesud", "k j jesudas"] {
        let b = ok(&w, &format!("/api/v1/browse/suggest?q={}", enc(q))).await;
        let hit = b["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["kind"] == "singer")
            .unwrap_or_else(|| panic!("{q}: {b}"));
        // Whatever name matched, the suggestion resolves to the one underlying person, by one slug.
        assert_eq!(hit["slug"], "yesudas", "{q}");
        assert!(
            hit["matched"].as_str().is_some_and(|m| !m.is_empty()),
            "{q}: says what matched"
        );
    }
    // The label follows the reader's language.
    let ml = ok(&w, "/api/v1/browse/songs?singer=yesudas&lang=ml").await;
    assert_eq!(ml["filters"][0]["label"]["text"], "യേശുദാസ്");

    // Renaming keeps the old address working and reports the new one.
    sqlx::query("UPDATE artists SET name = 'K J Yesudas' WHERE id = $1::uuid")
        .bind(&d.yesudas)
        .execute(&pool)
        .await
        .unwrap();
    index(&pool).await;
    let moved = ok(&w, "/api/v1/browse/songs?singer=yesudas").await;
    assert_eq!(moved["total"], 2);
    assert_eq!(moved["filters"][0]["slug"], "k-j-yesudas");
    assert_eq!(ok(&w, "/api/v1/browse/songs?singer=k-j-yesudas").await["total"], 2);
    // A second person with the same name gets a distinct readable slug, never an id.
    let twin = artist(&w, "K J Yesudas").await;
    let (_, rec) = song(&w, "Another", &[&twin], json!([]), json!({}), None).await;
    let _ = rec;
    index(&pool).await;
    let ids = ok(&w, "/api/v1/browse/facets/singer?q=yesudas").await;
    let slugs: Vec<&str> = ids["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["slug"].as_str().unwrap())
        .collect();
    assert!(
        slugs.contains(&"k-j-yesudas") && slugs.contains(&"k-j-yesudas-2"),
        "{slugs:?}"
    );
    for s in &slugs {
        assert!(!s.contains(&d.yesudas[..8]) && !s.contains(&twin[..8]));
    }
    let _ = d.chitra;
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn browse_is_public_cacheable_and_suggested_songs_open_their_page(pool: PgPool) {
    let w = world(&pool).await;
    let d = seed(&w, &pool).await;
    // Anonymous reads; the answer carries an ETag and short public caching.
    let res = get(&w.r, "/api/v1/browse/songs?singer=yesudas", None).await;
    assert_eq!(res.status, StatusCode::OK);
    assert!(res.headers.get("etag").is_some());
    assert!(res.headers["cache-control"].to_str().unwrap().starts_with("public"));
    // The song a suggestion points at opens its page.
    let s = ok(&w, "/api/v1/browse/suggest?q=yesuvinte").await;
    let song = s["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["kind"] == "song")
        .unwrap();
    let page = get(
        &w.r,
        &format!(
            "/api/v1/pages/song/{}?locale={}",
            enc(song["slug"].as_str().unwrap()),
            song["locale"].as_str().unwrap()
        ),
        None,
    )
    .await;
    assert_eq!(page.status, StatusCode::OK);
    assert_eq!(page.body["id"], d.song1);
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn raga_deity_and_tags_are_one_entity_across_scripts_in_suggestions_and_filters(pool: PgPool) {
    let w = world(&pool).await;
    let yesudas = artist(&w, "Yesudas").await;
    song(
        &w,
        "Latin Song",
        &[&yesudas],
        json!([]),
        json!({ "category": "devotional", "deity": "Krishna", "raga": "Mohanam", "tags": ["Bhajan"] }),
        None,
    )
    .await;
    song(
        &w,
        "Malayalam Song",
        &[&yesudas],
        json!([]),
        json!({ "category": "devotional", "deity": "കൃഷ്ണ", "raga": "മോഹനം", "tags": ["ഭജൻ"] }),
        None,
    )
    .await;
    index(&pool).await;
    // Typing in either script offers the one raga, deity and tag.
    for q in ["mohan", "മോഹ"] {
        let b = ok(&w, &format!("/api/v1/browse/suggest?q={}", enc(q))).await;
        let ragas: Vec<&Value> = b["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|x| x["kind"] == "raga")
            .collect();
        assert_eq!(ragas.len(), 1, "{q}: {b}");
        assert_eq!(ragas[0]["slug"], "mohanam");
    }
    let b = ok(&w, "/api/v1/browse/suggest?q=krish").await;
    assert!(kinds(&b).contains(&("deity".into(), "Krishna".into())), "{b}");
    // One filter finds the songs of both spellings.
    let both = ok(&w, "/api/v1/browse/songs?raga=mohanam&deity=krishna&tag=bhajan").await;
    assert_eq!(titles(&both), vec!["Latin Song", "Malayalam Song"]);
    assert_eq!(both["filters"][0]["label"]["text"], "Mohanam");
    assert_eq!(ok(&w, "/api/v1/browse/facets/raga").await["total"], 1);
}
