//! Public search: tolerant matching, ranking, typed groups, published-only visibility, the outbox
//! indexer, typeahead limits and caching, and swapping the engine.

mod common;

use application::search::{
    Hit, MatchKind, MatchVia, Position, SearchRequest, SearchService, SearchSettings, SearchType,
};
use application::AppError;
use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use common::*;
use domain::Role;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;
use worker::{run_once, LogOnly, SearchIndexer};

fn id(v: &Value) -> String {
    v["id"].as_str().unwrap().to_owned()
}

async fn count(pool: &PgPool, sql: &str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// Runs the real outbox worker until nothing is due.
async fn index(pool: &PgPool) {
    let h = SearchIndexer {
        pool: pool.clone(),
        inner: LogOnly,
    };
    while run_once(pool, &h).await.unwrap() > 0 {}
}

struct World {
    r: Router,
    member: String,
    moderator: String,
    admin: String,
    admin_id: Uuid,
}

async fn world(pool: &PgPool) -> World {
    let r = router(pool);
    let (_, member) = signed_in(pool, &r, "m1@example.com", None).await;
    let (_, moderator) = signed_in(pool, &r, "mod@example.com", Some(Role::Moderator)).await;
    let (admin_id, admin) = signed_in(pool, &r, "admin@example.com", Some(Role::Admin)).await;
    World {
        r,
        member,
        moderator,
        admin,
        admin_id,
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

async fn composition(w: &World, title: &str, credits: Value) -> Value {
    let res = post(
        &w.r,
        "/api/v1/compositions",
        json!({ "title": title, "credits": credits }),
        Some(&w.member),
    )
    .await;
    assert_eq!(res.status, StatusCode::CREATED, "{}", res.body);
    res.body
}

async fn recording(w: &World, comp: &Value, title: &str) -> Value {
    let res = post(
        &w.r,
        "/api/v1/recordings",
        json!({ "composition_id": comp["id"], "title": title }),
        Some(&w.member),
    )
    .await;
    assert_eq!(res.status, StatusCode::CREATED, "{}", res.body);
    res.body
}

async fn search_raw(w: &World, query: &str) -> Res {
    get(&w.r, &format!("/api/v1/search?{query}"), None).await
}

async fn search(w: &World, q: &str) -> Value {
    let res = search_raw(w, &format!("q={}", enc(q))).await;
    assert_eq!(res.status, StatusCode::OK, "{q}: {}", res.body);
    res.body
}

fn group<'a>(body: &'a Value, kind: &str) -> &'a Vec<Value> {
    body["groups"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["type"] == kind)
        .unwrap_or_else(|| panic!("no group {kind}"))["items"]
        .as_array()
        .unwrap()
}

#[allow(clippy::too_many_arguments)]
async fn localize(
    pool: &PgPool,
    kind: &str,
    entity: &str,
    lang: &str,
    script: &str,
    name: &str,
    prov: &str,
    state: &str,
    by: Uuid,
) {
    sqlx::query(
        "INSERT INTO entity_localizations (entity_type, entity_id, language_id, script_id, name, is_preferred, provenance, source,
                                           verification_state, created_by, verified_by, verified_at)
         VALUES ($1, $2::uuid, (SELECT id FROM languages WHERE iso639_3 = $3), (SELECT id FROM scripts WHERE iso15924 = $4),
                 $5, false, $6, 'test', $7, $8, CASE WHEN $7 = 'verified' THEN $8 END, CASE WHEN $7 = 'verified' THEN now() END)",
    )
    .bind(kind).bind(entity).bind(lang).bind(script).bind(name).bind(prov).bind(state).bind(by)
    .execute(pool).await.unwrap();
}

async fn alias(pool: &PgPool, kind: &str, entity: &str, text: &str, state: &str, by: Uuid) {
    sqlx::query(
        "INSERT INTO entity_aliases (entity_type, entity_id, alias, source, verification_state, created_by, verified_by, verified_at)
         VALUES ($1, $2::uuid, $3, 'test', $4, $5, CASE WHEN $4 = 'verified' THEN $5 END, CASE WHEN $4 = 'verified' THEN now() END)",
    )
    .bind(kind).bind(entity).bind(text).bind(state).bind(by)
    .execute(pool).await.unwrap();
}

// ---------- matching ----------

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn punctuation_spacing_case_and_typos_are_tolerated_and_display_is_preserved(pool: PgPool) {
    let w = world(&pool).await;
    let a = artist(&w, "A. R. Rahman").await;
    let c = composition(&w, "Kaatrin Mozhi", json!([])).await;
    index(&pool).await;

    for q in [
        "a r rahman",
        "A.R. RAHMAN",
        "  a-r   rahman ",
        "arrahman",
        "Á. R. Rahmán",
    ] {
        let b = search(&w, q).await;
        let hits = group(&b, "artists");
        assert_eq!(hits[0]["id"], a["id"], "{q}");
        assert_ne!(hits[0]["match"]["kind"], "fuzzy", "{q}");
        assert_eq!(hits[0]["display"]["text"], "A. R. Rahman");
    }
    for q in [
        "kaatrin mozi",
        "katrin mozhi",
        "kaatrinmozhi",
        "KAATRIN  MOZHI!",
        "kaatrin mozh",
    ] {
        let b = search(&w, q).await;
        assert_eq!(group(&b, "compositions")[0]["id"], c["id"], "{q}");
    }
    // Nothing searchable in the text: an empty answer, not an error. No text at all: a field error.
    let punct = search(&w, "!!! ...").await;
    assert!(punct["groups"]
        .as_array()
        .unwrap()
        .iter()
        .all(|g| g["items"].as_array().unwrap().is_empty()));
    search_raw(&w, "q=")
        .await
        .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    // The contract describes matches, never the engine.
    let text = search(&w, "rahman").await.to_string();
    for leak in ["score", "similarity", "trgm", "tsvector", "bm25", "postgres"] {
        assert!(!text.contains(leak), "{leak} leaked: {text}");
    }
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn malayalam_script_and_transliteration_queries(pool: PgPool) {
    let w = world(&pool).await;
    let c = composition(&w, "കാറ്റിൻ മൊഴി", json!([])).await;
    let cid = id(&c);
    localize(
        &pool,
        "composition",
        &cid,
        "mal",
        "Latn",
        "Kaatrin Mozhi",
        "transliteration",
        "verified",
        w.admin_id,
    )
    .await;
    alias(&pool, "composition", &cid, "Kattrin Mozhi", "verified", w.admin_id).await;
    index(&pool).await;

    let native = search(&w, "കാറ്റിൻ മൊഴി").await;
    let hit = &group(&native, "compositions")[0];
    assert_eq!(
        (hit["id"].as_str().unwrap(), hit["match"]["kind"].as_str().unwrap()),
        (cid.as_str(), "exact")
    );
    let prefix = search(&w, "കാറ്റിൻ").await;
    assert_eq!(group(&prefix, "compositions")[0]["match"]["kind"], "prefix");

    // A Latin-script transliteration finds the Malayalam work, and says how.
    let latin = search(&w, "kaatrin mozhi").await;
    let hit = &group(&latin, "compositions")[0];
    assert_eq!(hit["id"], c["id"]);
    assert_eq!(
        (hit["match"]["via"].clone(), hit["match"]["text"].clone()),
        (json!("transliteration"), json!("Kaatrin Mozhi"))
    );
    // ...also with a different spelling or a typo, through the alias or fuzzy matching.
    let spelled = search(&w, "KATTRIN-MOZHI").await;
    assert_eq!(group(&spelled, "compositions")[0]["match"]["via"], "alias");
    assert_eq!(
        group(&search(&w, "kaatrin mozi").await, "compositions")[0]["id"],
        c["id"]
    );
    // The title shown follows the reader's language but stays verbatim.
    let shown = get(
        &w.r,
        &format!("/api/v1/search?q={}&lang=ml", enc("kaatrin mozhi")),
        None,
    )
    .await;
    assert!(!group(&shown.body, "compositions")[0]["display"]["text"]
        .as_str()
        .unwrap()
        .is_empty());
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn exact_outranks_prefix_outranks_fuzzy_and_popularity_cannot_override(pool: PgPool) {
    let w = world(&pool).await;
    let fuzzy = artist(&w, "Rahmen").await;
    let prefix = artist(&w, "Rahman Khan").await;
    let exact = artist(&w, "Rahman").await;
    index(&pool).await;
    // The fuzzy artist is overwhelmingly popular; the bounded boost must not lift it a class.
    sqlx::query("UPDATE search_documents SET popularity = 1000000 WHERE entity_id = $1::uuid")
        .bind(id(&fuzzy))
        .execute(&pool)
        .await
        .unwrap();

    let order = |b: &Value| {
        group(b, "artists")
            .iter()
            .map(|i| (i["id"].clone(), i["match"]["kind"].clone()))
            .collect::<Vec<_>>()
    };
    let b = search(&w, "rahman").await;
    assert_eq!(
        order(&b),
        vec![
            (exact["id"].clone(), json!("exact")),
            (prefix["id"].clone(), json!("prefix")),
            (fuzzy["id"].clone(), json!("fuzzy"))
        ]
    );

    // Weights are data, not code: lowering the prefix weight below fuzzy reorders without a deploy.
    sqlx::query(
        "UPDATE settings SET value = jsonb_set(value, '{class_weights,prefix}', '10') WHERE key = 'search_ranking'",
    )
    .execute(&pool)
    .await
    .unwrap();
    let b = search(&w, "rahman").await;
    assert_eq!(
        order(&b).iter().map(|o| o.1.clone()).collect::<Vec<_>>(),
        vec![json!("exact"), json!("fuzzy"), json!("prefix")]
    );
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn popularity_breaks_ties_between_equal_matches(pool: PgPool) {
    let w = world(&pool).await;
    let a = composition(&w, "Moonlight", json!([])).await;
    let b = composition(&w, "Moonlight", json!([])).await;
    // Make the popular one the one that would otherwise sort last on the id tie-break.
    let (popular, other) = if id(&a) < id(&b) { (a, b) } else { (b, a) };
    let rec = recording(&w, &popular, "Take 1").await;
    let lyric = post(
        &w.r,
        &format!("/api/v1/recordings/{}/contributions", id(&rec)),
        json!({ "type": "original", "language": "ml", "script": "Mlym", "body": "വരികൾ" }),
        Some(&w.moderator),
    )
    .await;
    assert_eq!(lyric.body["status"], "published", "{}", lyric.body);
    index(&pool).await;
    let got = search(&w, "moonlight").await;
    let ids: Vec<_> = group(&got, "compositions").iter().map(|i| i["id"].clone()).collect();
    assert_eq!(ids, vec![popular["id"].clone(), other["id"].clone()]);
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn results_are_typed_groups_with_contributors_and_cursors(pool: PgPool) {
    let w = world(&pool).await;
    let writer = artist(&w, "Sagar Mozhi").await;
    let singer = artist(&w, "Mozhi Singer").await;
    let comp = composition(&w, "Mozhi", json!([{ "artist_id": writer["id"], "role": "lyricist" }])).await;
    let rec = recording(&w, &comp, "Mozhi (Live)").await;
    sqlx::query(
        "WITH a AS (INSERT INTO albums (title, kind, created_by) VALUES ('Mozhi Hits', 'album', $1) RETURNING id)
         INSERT INTO album_tracks (album_id, position, recording_id) SELECT id, 1, $2::uuid FROM a",
    )
    .bind(w.admin_id)
    .bind(id(&rec))
    .execute(&pool)
    .await
    .unwrap();
    index(&pool).await;

    let b = search(&w, "mozhi").await;
    let kinds: Vec<_> = b["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["type"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds,
        ["compositions", "recordings", "artists", "albums", "contributors"]
    );
    assert_eq!(group(&b, "compositions")[0]["id"], comp["id"]);
    let recs = group(&b, "recordings");
    assert_eq!(
        (recs[0]["id"].clone(), recs[0]["parent"]["id"].clone()),
        (rec["id"].clone(), comp["id"].clone())
    );
    assert_eq!(group(&b, "artists").len(), 2);
    assert_eq!(group(&b, "albums")[0]["sub_kind"], "album");
    // Only the credited writer is a contributor; the singer is not.
    let writers: Vec<_> = group(&b, "contributors").iter().map(|i| i["id"].clone()).collect();
    assert_eq!(writers, vec![writer["id"].clone()]);
    assert!(!writers.contains(&singer["id"]));

    // A group pages with its own cursor; a cursor without a type is refused.
    let p1 = search_raw(&w, "q=mozhi&type=artists&limit=1").await;
    assert_eq!(p1.body["groups"].as_array().unwrap().len(), 1);
    let cursor = p1.body["groups"][0]["next_cursor"].as_str().unwrap().to_owned();
    let p2 = search_raw(&w, &format!("q=mozhi&type=artists&limit=1&cursor={cursor}")).await;
    assert_ne!(group(&p1.body, "artists")[0]["id"], group(&p2.body, "artists")[0]["id"]);
    assert!(p2.body["groups"][0]["next_cursor"].is_null());
    search_raw(&w, &format!("q=mozhi&cursor={cursor}"))
        .await
        .assert_problem(StatusCode::BAD_REQUEST, "cursor_requires_type");
    search_raw(&w, "q=mozhi&type=artists&cursor=garbage")
        .await
        .assert_problem(StatusCode::BAD_REQUEST, "invalid_cursor");
    search_raw(&w, "q=mozhi&type=nope")
        .await
        .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    search_raw(&w, "q=mozhi&limit=0")
        .await
        .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    search_raw(&w, "q=mozhi&lang=zz-nope")
        .await
        .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
}

// ---------- visibility ----------

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn pending_private_and_unpublished_content_is_never_returned(pool: PgPool) {
    let w = world(&pool).await;
    let a = artist(&w, "Visible Artist").await;
    let aid = id(&a);
    alias(&pool, "artist", &aid, "Pending Secret Alias", "pending", w.admin_id).await;
    alias(&pool, "artist", &aid, "Rejected Secret Alias", "rejected", w.admin_id).await;
    localize(
        &pool,
        "artist",
        &aid,
        "tam",
        "Latn",
        "Pending Secret Name",
        "original",
        "pending",
        w.admin_id,
    )
    .await;
    // A lyric awaiting review: neither its text nor its existence is searchable.
    let rec = recording(&w, &composition(&w, "Draft Song", json!([])).await, "Draft Take").await;
    let draft = post(
        &w.r,
        &format!("/api/v1/recordings/{}/contributions", id(&rec)),
        json!({ "type": "original", "language": "ml", "script": "Mlym", "body": "uniquelyprivatelyrics zebra" }),
        Some(&w.member),
    )
    .await;
    assert_eq!(draft.body["status"], "pending");
    index(&pool).await;

    for q in [
        "Pending Secret Alias",
        "Rejected Secret Alias",
        "Pending Secret Name",
        "uniquelyprivatelyrics",
        "zebra",
    ] {
        let b = search(&w, q).await;
        assert!(
            b["groups"]
                .as_array()
                .unwrap()
                .iter()
                .all(|g| g["items"].as_array().unwrap().is_empty()),
            "{q}: {b}"
        );
    }
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM search_documents WHERE display ILIKE '%secret%'"
        )
        .await,
        0
    );
    assert_eq!(
        count(&pool, "SELECT count(*) FROM search_documents WHERE popularity > 0").await,
        0
    );

    // Verified, then withdrawn: the name stops matching immediately, before the worker has caught up.
    alias(&pool, "artist", &aid, "Briefly Verified", "verified", w.admin_id).await;
    index(&pool).await;
    assert_eq!(
        group(&search(&w, "briefly verified").await, "artists")[0]["id"],
        a["id"]
    );
    sqlx::query("UPDATE entity_aliases SET verification_state = 'rejected' WHERE alias = 'Briefly Verified'")
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        group(&search(&w, "briefly verified").await, "artists").is_empty(),
        "stale index row served"
    );
    // The same guard holds for suggest and for an entity that was merged away.
    let s = get(&w.r, "/api/v1/search/suggest?q=briefly", None).await;
    assert!(s.body["items"].as_array().unwrap().is_empty());
}

// ---------- the outbox indexer ----------

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn every_change_path_enqueues_a_reindex_and_the_worker_applies_it(pool: PgPool) {
    let w = world(&pool).await;
    index(&pool).await;
    let pending = || {
        count(
            &pool,
            "SELECT count(*) FROM outbox WHERE topic = 'search.reindex' AND processed_at IS NULL",
        )
    };

    // Create: not searchable until the worker runs, in the same transaction as the change.
    let a = artist(&w, "Ilaiyaraaja").await;
    assert!(pending().await >= 1);
    assert!(group(&search(&w, "ilaiyaraaja").await, "artists").is_empty());
    index(&pool).await;
    assert_eq!(group(&search(&w, "ilaiyaraaja").await, "artists")[0]["id"], a["id"]);
    assert_eq!(pending().await, 0);

    // Metadata edit: the new name is found, the old one is gone.
    let patched = send(
        &w.r,
        Method::PATCH,
        &format!("/api/v1/artists/{}", id(&a)),
        Some(json!({ "name": "Ilayaraaja", "kind": "person", "expected_version": a["version"] })),
        Some(&w.member),
    )
    .await;
    assert_eq!(patched.status, StatusCode::OK, "{}", patched.body);
    index(&pool).await;
    assert_eq!(group(&search(&w, "ilayaraaja").await, "artists")[0]["id"], a["id"]);
    assert!(group(&search(&w, "ilaiyaraaja").await, "artists")
        .iter()
        .all(|i| i["match"]["kind"] == "fuzzy"));

    // Moderation: verifying an alias publishes it to search.
    alias(&pool, "artist", &id(&a), "Maestro", "pending", w.admin_id).await;
    index(&pool).await;
    assert!(group(&search(&w, "maestro").await, "artists").is_empty());
    sqlx::query("UPDATE entity_aliases SET verification_state = 'verified', verified_by = $1, verified_at = now() WHERE alias = 'Maestro'")
        .bind(w.admin_id).execute(&pool).await.unwrap();
    index(&pool).await;
    assert_eq!(
        group(&search(&w, "maestro").await, "artists")[0]["match"]["via"],
        "alias"
    );

    // Publishing lyrics moves popularity: queued for the recording, its work and its credited artist.
    let comp = composition(
        &w,
        "Maestro Song",
        json!([{ "artist_id": a["id"], "role": "composer" }]),
    )
    .await;
    let rec = recording(&w, &comp, "Take 1").await;
    index(&pool).await;
    let before = count(
        &pool,
        "SELECT coalesce(max(popularity), 0)::bigint FROM search_documents",
    )
    .await;
    let lyric = post(
        &w.r,
        &format!("/api/v1/recordings/{}/contributions", id(&rec)),
        json!({ "type": "original", "language": "ml", "script": "Mlym", "body": "വരി" }),
        Some(&w.moderator),
    )
    .await;
    assert_eq!(lyric.body["status"], "published");
    assert!(pending().await >= 3, "recording, composition and artist are queued");
    index(&pool).await;
    assert_eq!(before, 0);
    for (t, e) in [("recording", id(&rec)), ("composition", id(&comp)), ("artist", id(&a))] {
        assert_eq!(count(&pool, &format!("SELECT min(popularity)::bigint FROM search_documents WHERE entity_type = '{t}' AND entity_id = '{e}'")).await, 1, "{t}");
    }

    // Merge: the loser disappears from search, the survivor answers to the loser's name.
    let b = artist(&w, "Ilaiyaraja").await;
    index(&pool).await;
    let from = send(&w.r, Method::GET, &format!("/api/v1/artists/{}", id(&b)), None, None)
        .await
        .body;
    let into = send(&w.r, Method::GET, &format!("/api/v1/artists/{}", id(&a)), None, None)
        .await
        .body;
    let merged = post(
        &w.r,
        "/api/v1/entities/merges",
        json!({ "entity_type": "artist", "from_id": from["id"], "into_id": into["id"],
                "expected_from_version": from["version"], "expected_into_version": into["version"] }),
        Some(&w.admin),
    )
    .await;
    assert_eq!(merged.status, StatusCode::OK, "{}", merged.body);
    // Even before the worker runs, the dead id cannot surface.
    assert!(group(&search(&w, "ilaiyaraja").await, "artists")
        .iter()
        .all(|i| i["id"] != b["id"]));
    index(&pool).await;
    assert_eq!(
        count(
            &pool,
            &format!("SELECT count(*) FROM search_documents WHERE entity_id = '{}'", id(&b))
        )
        .await,
        0
    );
    let after = search(&w, "ilaiyaraja").await;
    assert_eq!(group(&after, "artists")[0]["id"], a["id"]);
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn reindexing_is_idempotent_and_queued_events_are_coalesced(pool: PgPool) {
    let w = world(&pool).await;
    let a = artist(&w, "Coalesce Me").await;
    for n in 0..5 {
        alias(&pool, "artist", &id(&a), &format!("Alias {n}"), "verified", w.admin_id).await;
    }
    assert_eq!(
        count(
            &pool,
            &format!(
                "SELECT count(*) FROM outbox WHERE topic = 'search.reindex' AND aggregate_id = '{}'",
                id(&a)
            )
        )
        .await,
        1,
        "one pending reindex per entity, however many changes"
    );
    index(&pool).await;
    let rows = || count(&pool, "SELECT count(*) FROM search_documents");
    let first = rows().await;
    assert_eq!(first, 6);
    // Redelivery and concurrent workers leave the same rows.
    for _ in 0..3 {
        sqlx::query("SELECT search_documents_refresh('artist', $1::uuid)")
            .bind(id(&a))
            .execute(&pool)
            .await
            .unwrap();
    }
    let (r1, r2) = tokio::join!(
        sqlx::query("SELECT search_documents_refresh('artist', $1::uuid)")
            .bind(id(&a))
            .execute(&pool),
        sqlx::query("SELECT search_documents_refresh('artist', $1::uuid)")
            .bind(id(&a))
            .execute(&pool)
    );
    assert!(r1.is_ok() && r2.is_ok(), "{r1:?} {r2:?}");
    sqlx::query("SELECT search_documents_refresh('artist', $1::uuid)")
        .bind(id(&a))
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(rows().await, first);
}

// ---------- typeahead ----------

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn suggest_has_a_minimum_length_mixed_types_and_short_public_caching(pool: PgPool) {
    let w = world(&pool).await;
    artist(&w, "Mozhi Singer").await;
    composition(&w, "Mozhi", json!([])).await;
    index(&pool).await;

    get(&w.r, "/api/v1/search/suggest?q=m", None)
        .await
        .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    get(&w.r, "/api/v1/search/suggest", None)
        .await
        .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    get(&w.r, "/api/v1/search/suggest?q=mozhi&limit=99", None)
        .await
        .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");

    let res = get(&w.r, "/api/v1/search/suggest?q=mozhi", None).await;
    assert_eq!(res.status, StatusCode::OK);
    let types: Vec<_> = res.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["type"].as_str().unwrap())
        .collect();
    assert!(types.contains(&"composition") && types.contains(&"artist"), "{types:?}");
    let cache = res.headers[header::CACHE_CONTROL].to_str().unwrap();
    assert!(cache.starts_with("public, max-age=30"), "{cache}");

    // Revalidation: an unchanged answer is a 304 with no body; a changed one is not.
    let etag = res.headers[header::ETAG].to_str().unwrap().to_owned();
    let revalidate = |tag: String| {
        let req = Request::builder()
            .uri("/api/v1/search/suggest?q=mozhi")
            .header(header::IF_NONE_MATCH, tag)
            .body(Body::empty())
            .unwrap();
        raw(&w.r, req)
    };
    assert_eq!(revalidate(etag.clone()).await.status, StatusCode::NOT_MODIFIED);
    artist(&w, "Mozhi Newcomer").await;
    index(&pool).await;
    assert_eq!(revalidate(etag).await.status, StatusCode::OK);
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn suggest_is_rate_limited_per_client_and_errors_are_not_cached(pool: PgPool) {
    let mut app = app(pool.clone());
    let mut cfg = (*app.config).clone();
    cfg.search_rate_per_min = 3;
    app.config = Arc::new(cfg);
    let r = api::router(app);
    let from = |ip: &'static str| {
        let req = Request::builder()
            .uri("/api/v1/search/suggest?q=abc")
            .header("x-forwarded-for", ip)
            .body(Body::empty())
            .unwrap();
        raw(&r, req)
    };
    for _ in 0..3 {
        assert_eq!(from("203.0.113.7").await.status, StatusCode::OK);
    }
    let limited = from("203.0.113.7").await;
    limited.assert_problem(StatusCode::TOO_MANY_REQUESTS, "rate_limited");
    assert!(
        limited.headers[header::RETRY_AFTER]
            .to_str()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            >= 1
    );
    assert_eq!(limited.headers[header::CACHE_CONTROL], "no-store");
    // Another client is unaffected, and the full search is not behind this limiter.
    assert_eq!(from("198.51.100.9").await.status, StatusCode::OK);
    assert_eq!(get(&r, "/api/v1/search?q=abc", None).await.status, StatusCode::OK);
    assert!(get(&r, "/api/v1/search/suggest?q=a", None)
        .await
        .headers
        .get(header::CACHE_CONTROL)
        .is_none_or(|c| !c.to_str().unwrap().contains("public")));
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn metrics_count_latency_empty_results_and_acceptance_for_admins_only(pool: PgPool) {
    let w = world(&pool).await;
    artist(&w, "Metric Artist").await;
    index(&pool).await;
    search(&w, "metric artist").await;
    search(&w, "zzzz nothing here").await;
    get(&w.r, "/api/v1/search/suggest?q=metric", None).await;
    // Accepting a suggestion is a CSRF-protected write that carries nothing but the fact.
    let accepted = post(&w.r, "/api/v1/search/suggest/accepted", json!({}), None).await;
    assert_eq!(accepted.status, StatusCode::NO_CONTENT);
    let no_csrf = raw(
        &w.r,
        Request::builder()
            .method(Method::POST)
            .uri("/api/v1/search/suggest/accepted")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(no_csrf.status, StatusCode::FORBIDDEN);
    // Counters are written off the request path.
    for _ in 0..50 {
        if count(
            &pool,
            "SELECT coalesce(sum(requests + accepted), 0)::bigint FROM search_metrics",
        )
        .await
            >= 4
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    let m = get(&w.r, "/api/v1/admin/search/metrics", Some(&w.admin)).await;
    assert_eq!(m.status, StatusCode::OK, "{}", m.body);
    let rows = m.body["items"].as_array().unwrap();
    let of = |e: &str| rows.iter().find(|r| r["endpoint"] == e).unwrap().clone();
    assert_eq!(
        (of("search")["requests"].clone(), of("search")["empty_results"].clone()),
        (json!(2), json!(1))
    );
    assert_eq!(of("search")["empty_rate"], 0.5);
    assert!(of("search")["avg_latency_ms"].as_f64().unwrap() >= 0.0);
    assert_eq!(
        (
            of("suggest")["requests"].clone(),
            of("suggest")["accepted"].clone(),
            of("suggest")["acceptance_rate"].clone()
        ),
        (json!(1), json!(1), json!(1.0))
    );
    // Aggregates only: nothing in the table can identify a query, a user or an address.
    assert_eq!(count(&pool, "SELECT count(*) FROM information_schema.columns WHERE table_name = 'search_metrics' AND column_name NOT IN ('day','endpoint','shard','requests','empty','latency_ms','accepted')").await, 0);

    get(&w.r, "/api/v1/admin/search/metrics", None)
        .await
        .assert_problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    get(&w.r, "/api/v1/admin/search/metrics", Some(&w.moderator))
        .await
        .assert_problem(StatusCode::FORBIDDEN, "forbidden");
    get(&w.r, "/api/v1/admin/search/metrics?days=0", Some(&w.admin))
        .await
        .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
}

#[test]
fn every_operation_id_is_unique_so_generated_clients_keep_each_endpoint() {
    let spec: Value = serde_json::from_str(&api::openapi_json()).unwrap();
    let mut seen = std::collections::HashMap::new();
    for (path, item) in spec["paths"].as_object().unwrap() {
        for (method, op) in item.as_object().unwrap() {
            let op_id = op["operationId"]
                .as_str()
                .unwrap_or_else(|| panic!("{method} {path} has no operationId"));
            if let Some(first) = seen.insert(op_id.to_owned(), format!("{method} {path}")) {
                panic!("operationId {op_id} used by both {first} and {method} {path}");
            }
        }
    }
}

// ---------- the engine is swappable ----------

struct Fake;

#[async_trait::async_trait]
impl SearchService for Fake {
    async fn search(&self, req: &SearchRequest) -> Result<Vec<Hit>, AppError> {
        let id = Uuid::parse_str("00000000-0000-0000-0000-00000000f00d").unwrap();
        Ok(req
            .types
            .iter()
            .filter(|t| **t == SearchType::Artist && req.q == "fake query")
            .map(|_| Hit {
                entity: SearchType::Artist,
                id,
                text: "Fake Hit".into(),
                kind: MatchKind::Exact,
                via: MatchVia::Alias,
                position: Position(1, id),
            })
            .collect())
    }
    async fn settings(&self) -> Result<SearchSettings, AppError> {
        Ok(SearchSettings {
            min_suggest_chars: 3,
            cache_max_age_secs: 7,
        })
    }
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn the_search_service_is_swappable_and_the_api_contract_does_not_change(pool: PgPool) {
    let real = world(&pool).await;
    let a = artist(&real, "Fake Hit").await; // the catalogue still supplies display and facts
    let fake_id = "00000000-0000-0000-0000-00000000f00d";
    // The fake names an id the catalogue lacks: it is dropped rather than shown half-formed.
    let r = api::router(app(pool.clone()).with_search(Arc::new(Fake)));
    let none = get(&r, "/api/v1/search?q=fake%20query&type=artists", None).await;
    assert_eq!(none.status, StatusCode::OK);
    assert!(group(&none.body, "artists").is_empty());
    // Point the fake at a real row to see the unchanged response shape and the fake's settings.
    sqlx::query("UPDATE artists SET id = $1 WHERE id = $2")
        .bind(Uuid::parse_str(fake_id).unwrap())
        .bind(Uuid::parse_str(&id(&a)).unwrap())
        .execute(&pool)
        .await
        .unwrap();
    let res = get(&r, "/api/v1/search?q=fake%20query&type=artists", None).await;
    let hit = &group(&res.body, "artists")[0];
    assert_eq!(
        (
            hit["id"].as_str().unwrap(),
            hit["match"]["via"].clone(),
            hit["match"]["text"].clone()
        ),
        (fake_id, json!("alias"), json!("Fake Hit"))
    );
    assert_eq!(hit["display"]["text"], "Fake Hit");
    assert!(res.headers[header::CACHE_CONTROL]
        .to_str()
        .unwrap()
        .starts_with("public, max-age=7"));
    // Settings come from the engine too: its minimum length applies to suggest.
    get(&r, "/api/v1/search/suggest?q=ab", None)
        .await
        .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    assert_eq!(
        get(&r, "/api/v1/search/suggest?q=fake%20query", None).await.body["items"][0]["id"],
        fake_id
    );
}
