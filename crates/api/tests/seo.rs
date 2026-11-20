//! Public pages: readable Unicode slugs, history and redirects, canonical/hreflang, published-only
//! visibility, the sitemap, and the worker's cache invalidation.

mod common;

use axum::http::{header, Method, StatusCode};
use axum::Router;
use common::*;
use domain::Role;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;
use worker::{run_once, LogOnly, Revalidate, SearchIndexer};

fn id(v: &Value) -> String {
    v["id"].as_str().unwrap().to_owned()
}

async fn count(pool: &PgPool, sql: &str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
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

async fn index_with(pool: &PgPool, revalidate: Option<Revalidate>) -> anyhow::Result<()> {
    let h = SearchIndexer {
        pool: pool.clone(),
        inner: LogOnly,
        revalidate,
    };
    while run_once(pool, &h).await? > 0 {}
    Ok(())
}

async fn index(pool: &PgPool) {
    index_with(pool, None).await.unwrap();
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

async fn song(w: &World, title: &str, credits: Value) -> Value {
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

/// A moderator's lyric publishes at once (the default trust policy).
async fn publish(w: &World, rec: &Value, body: &str) -> Value {
    let res = post(
        &w.r,
        &format!("/api/v1/recordings/{}/contributions", id(rec)),
        json!({ "type": "original", "language": "ml", "script": "Mlym", "body": body }),
        Some(&w.moderator),
    )
    .await;
    assert_eq!(res.body["status"], "published", "{}", res.body);
    res.body
}

/// A published song with one recording.
async fn published_song(w: &World, pool: &PgPool, title: &str, credits: Value) -> (Value, Value) {
    let c = song(w, title, credits).await;
    let r = recording(w, &c, "Take 1").await;
    publish(w, &r, "വരികൾ").await;
    index(pool).await;
    (c, r)
}

async fn page(w: &World, scope: &str, locale: &str, slug: &str) -> Res {
    get(
        &w.r,
        &format!("/api/v1/pages/{scope}/{}?locale={locale}", enc(slug)),
        None,
    )
    .await
}

async fn slug_of(pool: &PgPool, entity: &str, lang: &str) -> Option<String> {
    sqlx::query_scalar(
        "SELECT slug FROM localized_slugs s JOIN languages l ON l.id = s.language_id
         WHERE s.entity_id = $1::uuid AND l.iso639_1 = $2 AND s.is_current",
    )
    .bind(entity)
    .bind(lang)
    .fetch_optional(pool)
    .await
    .unwrap()
}

/// The operator's opt-in: addresses in the script of each title instead of Latin letters only.
async fn native_script_addresses(pool: &PgPool) {
    sqlx::query("UPDATE settings SET value = jsonb_set(value, '{slug,preserve_native_script}', 'true'::jsonb) WHERE key = 'seo'")
        .execute(pool)
        .await
        .unwrap();
}

#[allow(clippy::too_many_arguments)]
async fn localize(
    pool: &PgPool,
    kind: &str,
    entity: &str,
    lang: &str,
    script: &str,
    name: &str,
    state: &str,
    by: Uuid,
) {
    sqlx::query(
        "INSERT INTO entity_localizations (entity_type, entity_id, language_id, script_id, name, is_preferred, provenance, source,
                                           verification_state, created_by, verified_by, verified_at)
         VALUES ($1, $2::uuid, (SELECT id FROM languages WHERE iso639_3 = $3), (SELECT id FROM scripts WHERE iso15924 = $4),
                 $5, true, 'original', 'test', $6, $7, CASE WHEN $6 = 'verified' THEN $7 END, CASE WHEN $6 = 'verified' THEN now() END)",
    )
    .bind(kind).bind(entity).bind(lang).bind(script).bind(name).bind(state).bind(by)
    .execute(pool).await.unwrap();
}

// ---------- slugs and resolution ----------

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn a_published_song_gets_a_readable_title_url_that_resolves_to_its_stable_id(pool: PgPool) {
    let w = world(&pool).await;
    let singer = artist(&w, "A. R. Rahman").await;
    let (c, r) = published_song(
        &w,
        &pool,
        "Kaatrin Mozhi (Live)",
        json!([{ "artist_id": singer["id"], "role": "composer" }]),
    )
    .await;
    assert_eq!(
        slug_of(&pool, &id(&c), "en").await.as_deref(),
        Some("kaatrin-mozhi-live")
    );
    assert_eq!(slug_of(&pool, &id(&singer), "en").await.as_deref(), Some("a-r-rahman"));

    let res = page(&w, "song", "en", "kaatrin-mozhi-live").await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.body);
    assert_eq!(
        (res.body["id"].clone(), res.body["scope"].clone()),
        (c["id"].clone(), json!("song"))
    );
    assert_eq!(res.body["title"]["text"], "Kaatrin Mozhi (Live)");
    assert_eq!(
        res.body["canonical"],
        json!({ "locale": "en", "slug": "kaatrin-mozhi-live" })
    );
    // Related pages link by slug, never by raw id.
    assert_eq!(
        res.body["credits"][0]["artist"]["page"],
        json!({ "locale": "en", "slug": "a-r-rahman" })
    );
    assert_eq!(res.body["recordings"][0]["id"], r["id"]);
    assert_eq!(res.body["lyrics"][0]["body"], "വരികൾ");
    assert_eq!(res.body["lyrics"][0]["dir"], "ltr");
    assert!(res.headers[header::CACHE_CONTROL]
        .to_str()
        .unwrap()
        .starts_with("public"));

    let art = page(&w, "artist", "en", "a-r-rahman").await;
    assert_eq!(art.body["id"], singer["id"]);
    assert_eq!(art.body["songs"][0]["page"]["slug"], "kaatrin-mozhi-live");
    let rec = page(&w, "recording", "en", "take-1").await;
    assert_eq!(
        (rec.body["id"].clone(), rec.body["parent"]["id"].clone()),
        (r["id"].clone(), c["id"].clone())
    );
    // A scope is a namespace: an artist slug is not a song.
    page(&w, "song", "en", "a-r-rahman")
        .await
        .assert_problem(StatusCode::NOT_FOUND, "not_found");
    page(&w, "album", "en", "x")
        .await
        .assert_problem(StatusCode::NOT_FOUND, "not_found");
    page(&w, "song", "fr", "kaatrin-mozhi-live")
        .await
        .assert_problem(StatusCode::NOT_FOUND, "not_found");
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn unpublished_content_has_no_url_and_a_404_is_never_cacheable(pool: PgPool) {
    let w = world(&pool).await;
    let c = song(&w, "Hidden Draft", json!([])).await;
    let r = recording(&w, &c, "Draft Take").await;
    // A member's lyric is pending: no page, no slug.
    let draft = post(
        &w.r,
        &format!("/api/v1/recordings/{}/contributions", id(&r)),
        json!({ "type": "original", "language": "ml", "script": "Mlym", "body": "സ്വകാര്യം" }),
        Some(&w.member),
    )
    .await;
    assert_eq!(draft.body["status"], "pending");
    index(&pool).await;
    assert_eq!(count(&pool, "SELECT count(*) FROM localized_slugs").await, 0);
    let res = page(&w, "song", "en", "hidden-draft").await;
    res.assert_problem(StatusCode::NOT_FOUND, "not_found");
    assert_eq!(res.headers[header::CACHE_CONTROL], "no-store");
    let sm = get(&w.r, "/api/v1/sitemap", None).await;
    assert_eq!(
        (sm.body["total"].clone(), sm.body["items"].as_array().unwrap().len()),
        (json!(0), 0)
    );

    // Published: it appears. Withdrawn again: it disappears at once, history stays for later.
    let lyric = publish(&w, &r, "പ്രസിദ്ധീകരിച്ചത്").await;
    index(&pool).await;
    assert_eq!(page(&w, "song", "en", "hidden-draft").await.status, StatusCode::OK);
    assert!(!page(&w, "song", "en", "hidden-draft")
        .await
        .body
        .to_string()
        .contains("സ്വകാര്യം"));
    sqlx::query("UPDATE contributions SET status = 'rejected', is_primary = false, published_revision_id = NULL WHERE id = $1::uuid")
        .bind(id(&lyric)).execute(&pool).await.unwrap();
    // The page is gone even before the worker has run: visibility is checked live.
    page(&w, "song", "en", "hidden-draft")
        .await
        .assert_problem(StatusCode::NOT_FOUND, "not_found");
    page(&w, "artist", "en", "anyone")
        .await
        .assert_problem(StatusCode::NOT_FOUND, "not_found");
    index(&pool).await;
    page(&w, "recording", "en", "draft-take")
        .await
        .assert_problem(StatusCode::NOT_FOUND, "not_found");
    assert!(
        count(&pool, "SELECT count(*) FROM localized_slugs").await > 0,
        "history is kept"
    );
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn editing_a_title_keeps_the_old_url_alive_with_a_301_and_history_is_retained(pool: PgPool) {
    let w = world(&pool).await;
    let (c, _) = published_song(&w, &pool, "Old Title", json!([])).await;
    let cid = id(&c);
    let uri = format!("/api/v1/compositions/{cid}");
    let edit = |title: &'static str, version: i64| {
        send(
            &w.r,
            Method::PATCH,
            &uri,
            Some(json!({ "title": title, "credits": [], "expected_version": version })),
            Some(&w.member),
        )
    };
    assert_eq!(edit("New Title", 1).await.status, StatusCode::OK);
    index(&pool).await;
    assert_eq!(slug_of(&pool, &cid, "en").await.as_deref(), Some("new-title"));

    let old = page(&w, "song", "en", "old-title").await;
    assert_eq!(old.status, StatusCode::MOVED_PERMANENTLY);
    assert_eq!(old.body, json!({ "locale": "en", "slug": "new-title" }));
    assert_eq!(old.headers[header::LOCATION], "/api/v1/pages/song/new-title?locale=en");
    assert_eq!(page(&w, "song", "en", "new-title").await.status, StatusCode::OK);

    // Back to the first title: the old slug becomes current again, no duplicate row, nothing lost.
    assert_eq!(edit("Old Title", 2).await.status, StatusCode::OK);
    index(&pool).await;
    assert_eq!(slug_of(&pool, &cid, "en").await.as_deref(), Some("old-title"));
    assert_eq!(
        page(&w, "song", "en", "new-title").await.status,
        StatusCode::MOVED_PERMANENTLY
    );
    assert_eq!(
        count(
            &pool,
            &format!("SELECT count(*) FROM localized_slugs WHERE entity_id = '{cid}'")
        )
        .await,
        2
    );
    assert_eq!(
        count(
            &pool,
            &format!("SELECT count(*) FROM localized_slugs WHERE entity_id = '{cid}' AND is_current")
        )
        .await,
        1
    );

    // History is append-only: slugs cannot be deleted or rewritten, only retired.
    assert!(sqlx::query("DELETE FROM localized_slugs").execute(&pool).await.is_err());
    assert!(sqlx::query("UPDATE localized_slugs SET slug = 'hijack'")
        .execute(&pool)
        .await
        .is_err());
    assert!(sqlx::query("TRUNCATE localized_slugs").execute(&pool).await.is_err());
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn collisions_are_resolved_the_same_way_every_time(pool: PgPool) {
    let w = world(&pool).await;
    let (a, _) = published_song(&w, &pool, "Moonlight", json!([])).await;
    let (b, _) = published_song(&w, &pool, "Moonlight", json!([])).await;
    let (sa, sb) = (
        slug_of(&pool, &id(&a), "en").await.unwrap(),
        slug_of(&pool, &id(&b), "en").await.unwrap(),
    );
    assert_eq!(sa, "moonlight", "the first to be published keeps the plain slug");
    assert_eq!(sb, format!("moonlight-{}", &id(&b).replace('-', "")[..8]));
    // Re-running the sync (redelivery, another worker) never flips them.
    for _ in 0..3 {
        sqlx::query("INSERT INTO outbox (topic, aggregate_type, aggregate_id, payload) SELECT 'search.reindex', 'composition', id, '{}' FROM compositions").execute(&pool).await.unwrap();
        index(&pool).await;
    }
    assert_eq!(slug_of(&pool, &id(&a), "en").await.unwrap(), sa);
    assert_eq!(slug_of(&pool, &id(&b), "en").await.unwrap(), sb);
    assert_eq!(page(&w, "song", "en", &sb).await.body["id"], b["id"]);

    // Reserved words and slug-less titles never take a bare route name or an empty slug.
    let (s, _) = published_song(&w, &pool, "Search", json!([])).await;
    assert!(slug_of(&pool, &id(&s), "en").await.unwrap().starts_with("search-"));
    let (p, _) = published_song(&w, &pool, "?!?", json!([])).await;
    assert!(slug_of(&pool, &id(&p), "en").await.unwrap().starts_with("id-"));

    // Two syncs of equal titles racing each other: exactly one plain slug, no error, no duplicate.
    let x = song(&w, "Racing Title", json!([])).await;
    let y = song(&w, "Racing Title", json!([])).await;
    for c in [&x, &y] {
        let r = recording(&w, c, "Take").await;
        publish(&w, &r, "വരി").await;
    }
    let sync = |e: String| {
        let pool = pool.clone();
        async move {
            let mut tx = pool.begin().await.unwrap();
            infrastructure::seo::sync(&mut tx, "composition", Uuid::parse_str(&e).unwrap()).await?;
            tx.commit().await
        }
    };
    // The loser waits on the unique index, then either fails (and would be retried by the outbox) or
    // takes the suffixed slug: never two plain slugs, never a hang.
    let (rx, ry) = tokio::join!(sync(id(&x)), sync(id(&y)));
    assert!(rx.is_ok() || ry.is_ok());
    index(&pool).await;
    let racing = count(
        &pool,
        "SELECT count(*) FROM localized_slugs WHERE slug = 'racing-title'",
    )
    .await;
    assert_eq!(racing, 1);
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM localized_slugs WHERE slug LIKE 'racing-title-%'"
        )
        .await,
        1
    );
}

// ---------- locales, canonical, hreflang ----------

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn canonical_and_hreflang_cover_only_locales_that_have_content_including_non_latin(pool: PgPool) {
    native_script_addresses(&pool).await;
    let w = world(&pool).await;
    let (c, _) = published_song(&w, &pool, "Kaatrin Mozhi", json!([])).await;
    let cid = id(&c);
    // English only so far: Arabic has no content, so there is no Arabic alternate.
    let en = page(&w, "song", "en", "kaatrin-mozhi").await;
    assert_eq!(
        en.body["alternates"],
        json!([{ "locale": "en", "slug": "kaatrin-mozhi" }])
    );
    // A visitor asking for Arabic is sent to the English page (one canonical, no duplicate).
    let ar = page(&w, "song", "ar", "kaatrin-mozhi").await;
    assert_eq!(ar.status, StatusCode::MOVED_PERMANENTLY);
    assert_eq!(ar.body, json!({ "locale": "en", "slug": "kaatrin-mozhi" }));

    // A pending Arabic title is not content; a verified one is.
    localize(
        &pool,
        "composition",
        &cid,
        "ara",
        "Arab",
        "كاترين موزي",
        "pending",
        w.admin_id,
    )
    .await;
    index(&pool).await;
    assert_eq!(slug_of(&pool, &cid, "ar").await, None);
    sqlx::query("UPDATE entity_localizations SET verification_state = 'verified', verified_by = $1, verified_at = now() WHERE name = 'كاترين موزي'")
        .bind(w.admin_id).execute(&pool).await.unwrap();
    index(&pool).await;
    assert_eq!(slug_of(&pool, &cid, "ar").await.as_deref(), Some("كاترين-موزي"));

    let en = page(&w, "song", "en", "kaatrin-mozhi").await;
    let both = json!([{ "locale": "en", "slug": "kaatrin-mozhi" }, { "locale": "ar", "slug": "كاترين-موزي" }]);
    assert_eq!(en.body["alternates"], both);
    assert_eq!(en.body["canonical"]["locale"], "en");
    // Unicode URL round-trip: the percent-encoded Arabic slug resolves, and names itself canonical.
    let ar = page(&w, "song", "ar", "كاترين-موزي").await;
    assert_eq!(ar.status, StatusCode::OK, "{}", ar.body);
    assert_eq!(ar.body["id"], c["id"]);
    assert_eq!(ar.body["canonical"], json!({ "locale": "ar", "slug": "كاترين-موزي" }));
    assert_eq!(ar.body["alternates"], both);
    assert_eq!(ar.body["x_default"], json!({ "locale": "en", "slug": "kaatrin-mozhi" }));
    assert_eq!(ar.body["title"]["text"], "كاترين موزي");
    assert_eq!(ar.body["title"]["dir"], "rtl");
    // Asking for the English page under the Arabic locale now lands on the Arabic canonical.
    let moved = page(&w, "song", "ar", "kaatrin-mozhi").await;
    assert_eq!(
        (moved.status, moved.body),
        (
            StatusCode::MOVED_PERMANENTLY,
            json!({ "locale": "ar", "slug": "كاترين-موزي" })
        )
    );
    assert_eq!(
        moved.headers[header::LOCATION],
        format!("/api/v1/pages/song/{}?locale=ar", enc("كاترين-موزي"))
    );

    // Withdrawing the Arabic title removes the alternate again.
    sqlx::query("UPDATE entity_localizations SET verification_state = 'rejected' WHERE name = 'كاترين موزي'")
        .execute(&pool)
        .await
        .unwrap();
    index(&pool).await;
    assert_eq!(
        page(&w, "song", "en", "kaatrin-mozhi").await.body["alternates"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn native_scripts_and_spelling_variants_have_one_url(pool: PgPool) {
    native_script_addresses(&pool).await;
    let w = world(&pool).await;
    let (c, _) = published_song(&w, &pool, "കാറ്റിൻ മൊഴി", json!([])).await;
    let slug = slug_of(&pool, &id(&c), "en").await.unwrap();
    assert_eq!(
        slug, "കാറ്റിൻ-മൊഴി",
        "the native script is kept, virama and vowel signs intact"
    );
    assert_eq!(page(&w, "song", "en", &slug).await.body["id"], c["id"]);
    // Decomposed Unicode or upper case are variants of the same slug and redirect to the exact one.
    let decomposed: String = {
        use unicode_normalization::UnicodeNormalization;
        slug.nfd().collect()
    };
    if decomposed != slug {
        assert_eq!(
            page(&w, "song", "en", &decomposed).await.status,
            StatusCode::MOVED_PERMANENTLY
        );
    }
    let (l, _) = published_song(&w, &pool, "Latin Name", json!([])).await;
    let _ = l;
    let upper = page(&w, "song", "en", "LATIN-Name").await;
    assert_eq!(
        (upper.status, upper.body),
        (
            StatusCode::MOVED_PERMANENTLY,
            json!({ "locale": "en", "slug": "latin-name" })
        )
    );
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn a_merged_artist_url_redirects_to_the_survivor(pool: PgPool) {
    let w = world(&pool).await;
    let keep = artist(&w, "Ilaiyaraaja").await;
    let dup = artist(&w, "Ilayaraja").await;
    for a in [&keep, &dup] {
        let (_, _) = published_song(
            &w,
            &pool,
            &format!("Song of {}", a["name"].as_str().unwrap()),
            json!([{ "artist_id": a["id"], "role": "composer" }]),
        )
        .await;
    }
    assert_eq!(page(&w, "artist", "en", "ilayaraja").await.status, StatusCode::OK);
    let merged = post(
        &w.r,
        "/api/v1/entities/merges",
        json!({ "entity_type": "artist", "from_id": dup["id"], "into_id": keep["id"], "expected_from_version": dup["version"], "expected_into_version": keep["version"] }),
        Some(&w.admin),
    )
    .await;
    assert_eq!(merged.status, StatusCode::OK, "{}", merged.body);
    index(&pool).await;
    let moved = page(&w, "artist", "en", "ilayaraja").await;
    assert_eq!(
        (moved.status, moved.body),
        (
            StatusCode::MOVED_PERMANENTLY,
            json!({ "locale": "en", "slug": "ilaiyaraaja" })
        )
    );
    let survivor = page(&w, "artist", "en", "ilaiyaraaja").await;
    assert_eq!(
        survivor.body["songs"].as_array().unwrap().len(),
        2,
        "credits moved with the merge"
    );
}

// ---------- sitemap ----------

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn the_sitemap_lists_published_pages_once_with_content_locale_alternates(pool: PgPool) {
    let w = world(&pool).await;
    let (c, _) = published_song(&w, &pool, "First Song", json!([])).await;
    published_song(&w, &pool, "Second Song", json!([])).await;
    song(&w, "Never Published", json!([])).await;
    localize(
        &pool,
        "composition",
        &id(&c),
        "ara",
        "Arab",
        "الأغنية الأولى",
        "verified",
        w.admin_id,
    )
    .await;
    index(&pool).await;

    let sm = get(&w.r, "/api/v1/sitemap", None).await;
    assert_eq!(sm.status, StatusCode::OK, "{}", sm.body);
    // 2 songs and 2 recordings; the unpublished song is absent.
    assert_eq!(
        (sm.body["total"].clone(), sm.body["pages"].clone()),
        (json!(4), json!(1))
    );
    let items = sm.body["items"].as_array().unwrap();
    assert_eq!(items.len(), 4);
    let first = items.iter().find(|i| i["id"] == c["id"]).unwrap();
    assert_eq!(first["canonical"], json!({ "locale": "en", "slug": "first-song" }));
    assert_eq!(first["alternates"].as_array().unwrap().len(), 2);
    let second = items
        .iter()
        .find(|i| i["scope"] == "song" && i["id"] != c["id"])
        .unwrap();
    assert_eq!(second["alternates"].as_array().unwrap().len(), 1);
    assert!(items.iter().all(|i| !i.to_string().contains("never-published")));

    // Pages of a configurable size cover every entry exactly once.
    sqlx::query("UPDATE settings SET value = jsonb_set(value, '{sitemap_page_size}', '3') WHERE key = 'seo'")
        .execute(&pool)
        .await
        .unwrap();
    let (p0, p1) = (
        get(&w.r, "/api/v1/sitemap?page=0", None).await,
        get(&w.r, "/api/v1/sitemap?page=1", None).await,
    );
    assert_eq!(p0.body["pages"], 2);
    let mut seen: Vec<String> = [p0, p1]
        .iter()
        .flat_map(|p| p.body["items"].as_array().unwrap().iter().map(id))
        .collect();
    let n = seen.len();
    seen.sort();
    seen.dedup();
    assert_eq!((n, seen.len()), (4, 4));
    get(&w.r, "/api/v1/sitemap?page=-1", None)
        .await
        .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
}

// ---------- cache invalidation ----------

async fn mock_web(status: &'static str) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/api/revalidate", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    tokio::spawn(async move {
        loop {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 16384];
            let mut got = 0;
            // Read until the body is complete.
            loop {
                let n = s.read(&mut buf[got..]).await.unwrap_or(0);
                got += n;
                let text = String::from_utf8_lossy(&buf[..got]).to_string();
                if n == 0 || text.split("\r\n\r\n").nth(1).is_some_and(|b| b.ends_with('}')) {
                    log.lock().unwrap().push(text);
                    break;
                }
            }
            let _ = s
                .write_all(format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes())
                .await;
        }
    });
    (url, seen)
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn publishing_and_slug_changes_invalidate_exactly_the_affected_pages(pool: PgPool) {
    let w = world(&pool).await;
    let (url, seen) = mock_web("204 No Content").await;
    let rv = Revalidate {
        url,
        secret: "s3cret-s3cret-s3cret".into(),
    };
    let c = song(&w, "Fresh Song", json!([])).await;
    let r = recording(&w, &c, "Take 1").await;
    index_with(&pool, Some(rv.clone())).await.unwrap();
    // Nothing is published yet: the worker still reports, so a cached 404 can never outlive a publish.
    seen.lock().unwrap().clear();
    publish(&w, &r, "വരി").await;
    index_with(&pool, Some(rv.clone())).await.unwrap();
    let calls = seen.lock().unwrap().clone();
    assert!(!calls.is_empty());
    let all = calls.join("\n");
    assert!(all.starts_with("POST /api/revalidate HTTP/1.1"), "{all}");
    assert!(all.to_lowercase().contains("x-revalidate-secret: s3cret-s3cret-s3cret"));
    for tag in [
        format!("entity:{}", id(&c)),
        "slug:song:en:fresh-song".into(),
        "slug:song:ar:fresh-song".into(),
        "slug:recording:en:take-1".into(),
        "sitemap".into(),
    ] {
        assert!(all.contains(&tag), "missing {tag} in {all}");
    }

    // A rename invalidates the old address too, so the cached page is replaced by the redirect.
    seen.lock().unwrap().clear();
    send(
        &w.r,
        Method::PATCH,
        &format!("/api/v1/compositions/{}", id(&c)),
        Some(json!({ "title": "Renamed Song", "credits": [], "expected_version": 1 })),
        Some(&w.member),
    )
    .await;
    index_with(&pool, Some(rv.clone())).await.unwrap();
    let all = seen.lock().unwrap().join("\n");
    assert!(
        all.contains("slug:song:en:fresh-song") && all.contains("slug:song:en:renamed-song"),
        "{all}"
    );

    // Unpublishing invalidates too (the page must turn into a 404 for everyone).
    seen.lock().unwrap().clear();
    sqlx::query("UPDATE contributions SET status = 'rejected', is_primary = false, published_revision_id = NULL")
        .execute(&pool)
        .await
        .unwrap();
    index_with(&pool, Some(rv)).await.unwrap();
    assert!(seen.lock().unwrap().join("\n").contains("slug:song:en:renamed-song"));
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn a_failed_invalidation_is_retried_not_dropped(pool: PgPool) {
    let w = world(&pool).await;
    let (url, _) = mock_web("500 Internal Server Error").await;
    let (c, _) = published_song(&w, &pool, "Retry Me", json!([])).await;
    sqlx::query("UPDATE outbox SET processed_at = now()")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO outbox (topic, aggregate_type, aggregate_id, payload) VALUES ('search.reindex', 'composition', $1::uuid, '{}')").bind(id(&c)).execute(&pool).await.unwrap();
    index_with(
        &pool,
        Some(Revalidate {
            url,
            secret: "s3cret-s3cret-s3cret".into(),
        }),
    )
    .await
    .unwrap();
    let (processed, error): (bool, Option<String>) = sqlx::query_as("SELECT processed_at IS NOT NULL, last_error FROM outbox WHERE processed_at IS NULL OR last_error IS NOT NULL ORDER BY created_at DESC LIMIT 1").fetch_one(&pool).await.unwrap();
    assert!(!processed);
    assert!(error.unwrap().contains("500"));
    // The data itself was committed before the failed notification.
    assert_eq!(slug_of(&pool, &id(&c), "en").await.as_deref(), Some("retry-me"));
}

/// The default: addresses are Latin letters, made from the English title, whatever other titles exist.
#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn addresses_are_always_latin_and_come_from_the_english_title(pool: PgPool) {
    let w = world(&pool).await;
    let (c, _) = published_song(&w, &pool, "Kaatrin Mozhi", json!([])).await;
    let cid = id(&c);
    localize(
        &pool,
        "composition",
        &cid,
        "ara",
        "Arab",
        "كاترين موزي",
        "verified",
        w.admin_id,
    )
    .await;
    index(&pool).await;
    // The Arabic page exists, with the same Latin address; its title stays Arabic.
    assert_eq!(slug_of(&pool, &cid, "en").await.as_deref(), Some("kaatrin-mozhi"));
    assert_eq!(slug_of(&pool, &cid, "ar").await.as_deref(), Some("kaatrin-mozhi"));

    // A song whose main title is not Latin gets an id address until it has an English title.
    let (m, _) = published_song(&w, &pool, "കാറ്റിൻ മൊഴി", json!([])).await;
    let mid = id(&m);
    let before = slug_of(&pool, &mid, "en").await.unwrap();
    assert!(before.starts_with("id-"), "{before}");
    localize(
        &pool,
        "composition",
        &mid,
        "eng",
        "Latn",
        "Kaattin Mozhi",
        "verified",
        w.admin_id,
    )
    .await;
    index(&pool).await;
    assert_eq!(slug_of(&pool, &mid, "en").await.as_deref(), Some("kaattin-mozhi"));
    // The old address redirects to the new one.
    assert_eq!(
        page(&w, "song", "en", &before).await.status,
        StatusCode::MOVED_PERMANENTLY
    );
}
