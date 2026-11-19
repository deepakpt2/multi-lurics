//! Entity suggestion, proposals and merges: one normalized search, no silent duplicates,
//! no automatic merges, merges that keep every reference.

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

struct World {
    r: Router,
    member: String,
    member2: String,
    moderator: String,
    admin: String,
    admin_id: Uuid,
    member_id: Uuid,
}

async fn world(pool: &PgPool) -> World {
    let r = router(pool);
    let (member_id, member) = signed_in(pool, &r, "m1@example.com", None).await;
    let (_, member2) = signed_in(pool, &r, "m2@example.com", None).await;
    let (_, moderator) = signed_in(pool, &r, "mod@example.com", Some(Role::Moderator)).await;
    let (admin_id, admin) = signed_in(pool, &r, "admin@example.com", Some(Role::Admin)).await;
    World {
        r,
        member,
        member2,
        moderator,
        admin,
        admin_id,
        member_id,
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

async fn suggest(w: &World, kind: &str, q: &str) -> Vec<Value> {
    let uri = format!("/api/v1/entities/suggest?type={kind}&q={}", urlencode(q));
    let res = get(&w.r, &uri, None).await;
    assert_eq!(res.status, StatusCode::OK, "{} for {q}", res.body);
    res.body["items"].as_array().unwrap().clone()
}

fn urlencode(s: &str) -> String {
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

async fn verified_alias(pool: &PgPool, kind: &str, entity: &str, alias: &str, by: Uuid, state: &str) {
    sqlx::query(
        "INSERT INTO entity_aliases (entity_type, entity_id, alias, source, verification_state, created_by, verified_by, verified_at)
         VALUES ($1, $2::uuid, $3, 'test', $4, $5, CASE WHEN $4 = 'verified' THEN $5 END, CASE WHEN $4 = 'verified' THEN now() END)",
    )
    .bind(kind)
    .bind(entity)
    .bind(alias)
    .bind(state)
    .bind(by)
    .execute(pool)
    .await
    .unwrap();
}

async fn propose(w: &World, cookie: &str, kind: &str, name: &str, extra: Value) -> Res {
    let mut b = json!({ "entity_type": kind, "name": name });
    for (k, v) in extra.as_object().unwrap() {
        b[k] = v.clone();
    }
    post(&w.r, "/api/v1/entities/proposals", b, Some(cookie)).await
}

fn merge_body(kind: &str, from: &Value, into: &Value) -> Value {
    json!({ "entity_type": kind, "from_id": from["id"], "into_id": into["id"],
            "expected_from_version": from["version"], "expected_into_version": into["version"] })
}

// ---------- suggest ----------

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn punctuation_spacing_case_and_diacritic_variants_resolve_to_one_id(pool: PgPool) {
    let w = world(&pool).await;
    let a = artist(&w, "A. R. Rahman").await;
    for q in [
        "a r rahman",
        "A.R. RAHMAN",
        "  a-r   rahman ",
        "Á. R. Rahmán",
        "arrahman",
        "a.r.rahman!",
    ] {
        let hits = suggest(&w, "artist", q).await;
        assert_eq!(hits.len(), 1, "{q}: {hits:?}");
        assert_eq!(hits[0]["id"], a["id"], "{q}");
        assert_eq!(hits[0]["score"], 1.0, "{q}");
        // The display value is preserved exactly; only the search key is normalized.
        assert_eq!(hits[0]["display"]["text"], "A. R. Rahman");
    }
    // Arabic marks and Malayalam signs: harakat are insensitive, vowel signs and viramas are not.
    let f = artist(&w, "فيروز").await;
    assert_eq!(suggest(&w, "artist", "فَيْرُوز").await[0]["id"], f["id"]);
    let m = artist(&w, "കാറ്റിൻ").await;
    assert_eq!(suggest(&w, "artist", "കാറ്റിൻ").await[0]["score"], 1.0);
    let near = suggest(&w, "artist", "കാറ്റിന").await;
    assert!(near.iter().all(|h| h["score"].as_f64().unwrap() < 1.0), "{near:?}");
    assert_eq!(near[0]["id"], m["id"]);

    // Renaming re-keys; the old spelling stops matching.
    sqlx::query("UPDATE artists SET name = 'Someone Else' WHERE id = $1::uuid")
        .bind(id(&a))
        .execute(&pool)
        .await
        .unwrap();
    assert!(suggest(&w, "artist", "a r rahman").await.is_empty());
    assert_eq!(suggest(&w, "artist", "someone else").await[0]["id"], a["id"]);
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn aliases_and_localizations_match_but_pending_ones_never_do(pool: PgPool) {
    let w = world(&pool).await;
    let a = artist(&w, "Ilaiyaraaja").await;
    verified_alias(&pool, "artist", &id(&a), "Ilayaraja", w.admin_id, "verified").await;
    verified_alias(&pool, "artist", &id(&a), "Secret Pending Name", w.member_id, "pending").await;
    sqlx::query(
        "INSERT INTO entity_localizations (entity_type, entity_id, language_id, name, is_preferred, provenance, source, verification_state, created_by, verified_by, verified_at)
         VALUES ('artist', $1::uuid, (SELECT id FROM languages WHERE iso639_3 = 'mal'), 'ഇളയരാജ', true, 'original', 'test', 'verified', $2, $2, now()),
                ('artist', $1::uuid, (SELECT id FROM languages WHERE iso639_3 = 'tam'), 'Hidden Pending', true, 'original', 'test', 'pending', $2, NULL, NULL)",
    )
    .bind(id(&a))
    .bind(w.admin_id)
    .execute(&pool)
    .await
    .unwrap();

    let by_alias = suggest(&w, "artist", "ilayaraja").await;
    assert_eq!(
        (by_alias[0]["id"].clone(), by_alias[0]["matched_via"].clone()),
        (a["id"].clone(), json!("alias"))
    );
    let by_local = suggest(&w, "artist", "ഇളയരാജ").await;
    assert_eq!(by_local[0]["id"], a["id"]);
    assert_eq!(by_local[0]["matched_via"], "localization");
    assert!(suggest(&w, "artist", "Secret Pending Name").await.is_empty());
    assert!(suggest(&w, "artist", "Hidden Pending").await.is_empty());
    // Context: verified localized names only, with their language.
    let names = by_alias[0]["localized_names"].as_array().unwrap();
    assert_eq!(names.len(), 1);
    assert_eq!(names[0], json!({ "language": "ml", "name": "ഇളയരാജ" }));
    // A pending alias that is verified later becomes searchable.
    sqlx::query("UPDATE entity_aliases SET verification_state = 'verified', verified_by = $1, verified_at = now() WHERE alias = 'Secret Pending Name'")
        .bind(w.admin_id)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(suggest(&w, "artist", "secret pending name").await.len(), 1);
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn selection_stores_the_id_and_same_names_are_never_merged(pool: PgPool) {
    let w = world(&pool).await;
    let one = artist(&w, "John Williams").await;
    let two = post(
        &w.r,
        "/api/v1/artists",
        json!({ "name": "John Williams", "kind": "person", "disambiguation": "guitarist" }),
        Some(&w.member),
    )
    .await
    .body;
    let hits = suggest(&w, "artist", "john williams").await;
    assert_eq!(hits.len(), 2, "same name, two people: {hits:?}");
    let guitarist = hits.iter().find(|h| h["detail"] == "guitarist").unwrap();
    assert_eq!(guitarist["id"], two["id"]);

    let comp = post(
        &w.r,
        "/api/v1/compositions",
        json!({ "title": "Theme", "credits": [{ "artist_id": guitarist["id"], "role": "composer" }] }),
        Some(&w.member),
    )
    .await;
    assert_eq!(comp.status, StatusCode::CREATED, "{}", comp.body);
    assert_eq!(comp.body["credits"][0]["artist_id"], two["id"]);
    // Related works disambiguate: the guitarist is now credited on "Theme", the other is not.
    let after = suggest(&w, "artist", "john williams").await;
    let with_work = after.iter().find(|h| h["id"] == two["id"]).unwrap();
    assert_eq!(with_work["related"][0]["title"], "Theme");
    assert!(after.iter().find(|h| h["id"] == one["id"]).unwrap()["related"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(count(&pool, "SELECT count(*) FROM artists").await, 2);
    assert_eq!(count(&pool, "SELECT count(*) FROM entity_merges").await, 0);
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn suggest_validates_and_covers_every_type(pool: PgPool) {
    let w = world(&pool).await;
    for (uri, code) in [
        ("/api/v1/entities/suggest?type=composition&q=x", "invalid_choice"),
        ("/api/v1/entities/suggest?type=artist&q=", "required"),
        ("/api/v1/entities/suggest?type=artist&q=x&limit=0", "out_of_range"),
        ("/api/v1/entities/suggest?type=artist&q=x&limit=21", "out_of_range"),
    ] {
        let res = get(&w.r, uri, None).await;
        assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY, "{uri}");
        assert!(res.body["errors"].to_string().contains(code), "{uri}: {}", res.body);
    }
    // Punctuation alone has nothing to search for: an empty list, not an error.
    assert!(suggest(&w, "artist", "?!.").await.is_empty());
    // Labels, genres, publishers, albums, languages and scripts go through the same endpoint.
    for kind in ["label", "publisher", "genre"] {
        let res = post(
            &w.r,
            &format!("/api/v1/taxonomy/{kind}"),
            json!({ "name": "Sony Music" }),
            Some(&w.member),
        )
        .await;
        assert_eq!(res.status, StatusCode::CREATED, "{}", res.body);
        let hits = suggest(&w, kind, "SONY-music").await;
        assert_eq!(hits[0]["id"], res.body["id"], "{kind}");
    }
    let album = post(
        &w.r,
        "/api/v1/albums",
        json!({ "title": "Thriller", "kind": "album" }),
        Some(&w.member),
    )
    .await;
    assert_eq!(suggest(&w, "album", "thriller").await[0]["id"], album.body["id"]);
    let lang = suggest(&w, "language", "malayalam").await;
    assert_eq!(lang[0]["code"], "mal");
    assert_eq!(suggest(&w, "language", "മലയാളം").await[0]["code"], "mal");
    assert_eq!(suggest(&w, "script", "mlym").await[0]["code"], "Mlym");
    // An artist is not a label.
    assert!(suggest(&w, "label", "thriller").await.is_empty());
}

// ---------- proposals ----------

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn proposals_show_matches_first_then_create_entity_and_aliases_on_approval(pool: PgPool) {
    let w = world(&pool).await;
    let existing = artist(&w, "A. R. Rahman").await;

    // The likely match must be looked at before anything is queued.
    let first = propose(&w, &w.member2, "artist", "AR Rahman", json!({ "kind": "person" })).await;
    first.assert_problem(StatusCode::CONFLICT, "likely_duplicates");
    assert_eq!(first.body["candidate_ids"], json!([existing["id"]]));
    assert_eq!(count(&pool, "SELECT count(*) FROM entity_proposals").await, 0);

    // The proposer says it is someone else.
    let extra = json!({ "kind": "person", "disambiguation": "drummer", "aliases": ["Drum Man", "drum man"], "confirm_distinct": true });
    let made = propose(&w, &w.member2, "artist", "AR Rahman", extra).await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    assert_eq!(made.body["status"], "pending");
    assert_eq!(made.body["shown_matches"], json!([existing["id"]]));
    assert_eq!(
        made.body["aliases"],
        json!(["Drum Man"]),
        "case-insensitive duplicates collapse"
    );
    // Nothing is created or searchable before approval.
    assert_eq!(count(&pool, "SELECT count(*) FROM artists").await, 1);
    assert_eq!(suggest(&w, "artist", "drum man").await.len(), 0);
    // The same name cannot be queued twice.
    let dup = propose(
        &w,
        &w.member,
        "artist",
        "a.r. rahman",
        json!({ "kind": "person", "confirm_distinct": true }),
    )
    .await;
    dup.assert_problem(StatusCode::CONFLICT, "proposal_pending");
    assert_eq!(dup.body["existing_id"], made.body["id"]);

    let pid = id(&made.body);
    let approve = format!("/api/v1/entities/proposals/{pid}/approve");
    // Moderators are shown the same matches and must confirm too.
    post(&w.r, &approve, json!({}), Some(&w.moderator))
        .await
        .assert_problem(StatusCode::CONFLICT, "likely_duplicates");
    let done = post(
        &w.r,
        &approve,
        json!({ "confirm_distinct": true, "note": "different person" }),
        Some(&w.moderator),
    )
    .await;
    assert_eq!(done.status, StatusCode::OK, "{}", done.body);
    assert_eq!(done.body["status"], "approved");
    let new_id = done.body["entity_id"].as_str().unwrap().to_owned();
    assert_ne!(new_id, id(&existing));
    let created = get(&w.r, &format!("/api/v1/artists/{new_id}"), None).await.body;
    assert_eq!(created["name"], "AR Rahman");
    assert_eq!(created["disambiguation"], "drummer");
    let creator: Uuid = sqlx::query_scalar("SELECT created_by FROM artists WHERE id = $1::uuid")
        .bind(&new_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_ne!(creator, w.admin_id);
    // The alias is verified and findable; both entities remain distinct.
    let by_alias = suggest(&w, "artist", "drum man").await;
    assert_eq!(
        (by_alias.len(), by_alias[0]["id"].as_str().unwrap()),
        (1, new_id.as_str())
    );
    // Approving twice is an invalid transition.
    post(&w.r, &approve, json!({ "confirm_distinct": true }), Some(&w.moderator))
        .await
        .assert_problem(StatusCode::CONFLICT, "invalid_state_transition");
    for action in ["entity_proposal.created", "entity_proposal.approved", "artist.created"] {
        assert!(
            count(
                &pool,
                &format!("SELECT count(*) FROM audit_log WHERE action = '{action}'")
            )
            .await
                >= 1,
            "{action}"
        );
    }

    // A genuinely new name needs no confirmation; rejection can point at the existing entity.
    let fresh = propose(&w, &w.member, "genre", "Qawwali", json!({})).await;
    assert_eq!(fresh.status, StatusCode::CREATED, "{}", fresh.body);
    let rej = post(
        &w.r,
        &format!("/api/v1/entities/proposals/{}/reject", id(&fresh.body)),
        json!({ "duplicate_of_id": Uuid::new_v4() }),
        Some(&w.moderator),
    )
    .await;
    assert_eq!(
        rej.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "unknown duplicate_of_id: {}",
        rej.body
    );
    let rej = post(
        &w.r,
        &format!("/api/v1/entities/proposals/{}/reject", id(&fresh.body)),
        json!({ "note": "not a genre" }),
        Some(&w.moderator),
    )
    .await;
    assert_eq!(rej.body["status"], "rejected");
    assert_eq!(count(&pool, "SELECT count(*) FROM genres").await, 0);
    // A rejected name can be proposed again.
    assert_eq!(
        propose(&w, &w.member, "genre", "Qawwali", json!({})).await.status,
        StatusCode::CREATED
    );
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn proposal_authorization_and_validation(pool: PgPool) {
    let w = world(&pool).await;
    let anon = post(
        &w.r,
        "/api/v1/entities/proposals",
        json!({ "entity_type": "genre", "name": "Ghazal" }),
        None,
    )
    .await;
    anon.assert_problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    let p = propose(&w, &w.member, "genre", "Ghazal", json!({})).await;
    let pid = id(&p.body);
    for verb in ["approve", "reject"] {
        let uri = format!("/api/v1/entities/proposals/{pid}/{verb}");
        post(&w.r, &uri, json!({}), None)
            .await
            .assert_problem(StatusCode::UNAUTHORIZED, "unauthenticated");
        post(&w.r, &uri, json!({}), Some(&w.member))
            .await
            .assert_problem(StatusCode::FORBIDDEN, "forbidden");
    }
    get(&w.r, "/api/v1/moderation/entity-proposals", None)
        .await
        .assert_problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    get(&w.r, "/api/v1/moderation/entity-proposals", Some(&w.member))
        .await
        .assert_problem(StatusCode::FORBIDDEN, "forbidden");
    let queue = get(&w.r, "/api/v1/moderation/entity-proposals", Some(&w.moderator)).await;
    assert_eq!(queue.body["items"][0]["id"], p.body["id"]);
    assert_eq!(
        count(&pool, "SELECT count(*) FROM genres").await,
        0,
        "denied calls created nothing"
    );
    post(
        &w.r,
        &format!("/api/v1/entities/proposals/{}/approve", Uuid::new_v4()),
        json!({}),
        Some(&w.moderator),
    )
    .await
    .assert_problem(StatusCode::NOT_FOUND, "not_found");

    for (b, field, code) in [
        (
            json!({ "entity_type": "language", "name": "Klingon" }),
            "entity_type",
            "not_proposable",
        ),
        (
            json!({ "entity_type": "script", "name": "Tengwar" }),
            "entity_type",
            "not_proposable",
        ),
        (
            json!({ "entity_type": "artist", "name": "No Kind" }),
            "kind",
            "invalid_choice",
        ),
        (
            json!({ "entity_type": "genre", "name": "X", "kind": "person" }),
            "kind",
            "not_applicable",
        ),
        (
            json!({ "entity_type": "genre", "name": "Y", "disambiguation": "d" }),
            "disambiguation",
            "not_applicable",
        ),
        (json!({ "entity_type": "genre", "name": "   " }), "name", "required"),
        (json!({ "entity_type": "genre", "name": "?!…" }), "name", "no_letters"),
    ] {
        let res = post(&w.r, "/api/v1/entities/proposals", b.clone(), Some(&w.member)).await;
        assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY, "{b}: {}", res.body);
        assert!(
            res.body["errors"].to_string().contains(code) && res.body["errors"].to_string().contains(field),
            "{b}: {}",
            res.body
        );
    }
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn concurrent_identical_proposals_queue_once_and_concurrent_approvals_create_once(pool: PgPool) {
    let w = world(&pool).await;
    for round in 0..4 {
        let name = format!("Fusion {round}");
        let lower = name.to_lowercase();
        let (a, b) = tokio::join!(
            propose(&w, &w.member, "genre", &name, json!({})),
            propose(&w, &w.member2, "genre", &lower, json!({}))
        );
        let mut s = [a.status, b.status];
        s.sort();
        assert_eq!(
            s,
            [StatusCode::CREATED, StatusCode::CONFLICT],
            "round {round}: {} / {}",
            a.body,
            b.body
        );
        let loser = if a.status == StatusCode::CONFLICT { &a } else { &b };
        assert_eq!(loser.code(), "proposal_pending");
    }
    let p = propose(&w, &w.member, "genre", "Folk", json!({})).await;
    let uri = format!("/api/v1/entities/proposals/{}/approve", id(&p.body));
    let (x, y) = tokio::join!(
        post(&w.r, &uri, json!({}), Some(&w.moderator)),
        post(&w.r, &uri, json!({}), Some(&w.admin))
    );
    let mut s = [x.status, y.status];
    s.sort();
    assert_eq!(s, [StatusCode::OK, StatusCode::CONFLICT], "{} / {}", x.body, y.body);
    assert_eq!(count(&pool, "SELECT count(*) FROM genres WHERE name = 'Folk'").await, 1);
}

// ---------- merge ----------

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

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn merge_repoints_references_keeps_names_redirects_and_audits(pool: PgPool) {
    let w = world(&pool).await;
    let keep = artist(&w, "A. R. Rahman").await;
    let dup = artist(&w, "AR Rahmaan").await;
    let (k, d) = (id(&keep), id(&dup));
    // dup shares a credit with keep (collapses), has one of its own (moves), and has names.
    let shared = composition(&w, "Shared", json!([{ "artist_id": k, "role": "composer" }])).await;
    sqlx::query(
        "INSERT INTO credits (artist_id, role, composition_id, position) VALUES ($1::uuid, 'composer', $2::uuid, 5)",
    )
    .bind(&d)
    .bind(id(&shared))
    .execute(&pool)
    .await
    .unwrap();
    let own = composition(&w, "Only dup", json!([{ "artist_id": d, "role": "lyricist" }])).await;
    verified_alias(&pool, "artist", &d, "Dilip", w.admin_id, "verified").await;
    verified_alias(&pool, "artist", &d, "Pending Alias", w.member_id, "pending").await;
    sqlx::query(
        "INSERT INTO entity_localizations (entity_type, entity_id, language_id, name, is_preferred, provenance, source, verification_state, created_by, verified_by, verified_at)
         SELECT 'artist', e, (SELECT id FROM languages WHERE iso639_3 = 'eng'), n, true, 'original', 'test', 'verified', $3, $3, now()
         FROM (VALUES ($1::uuid, 'Kept English'), ($2::uuid, 'Retired English')) v(e, n)",
    )
    .bind(&k)
    .bind(&d)
    .bind(w.admin_id)
    .execute(&pool)
    .await
    .unwrap();

    // Unauthorized and invalid requests change nothing.
    let body = merge_body("artist", &dup, &keep);
    post(&w.r, "/api/v1/entities/merges", body.clone(), None)
        .await
        .assert_problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    for c in [&w.member, &w.moderator] {
        post(&w.r, "/api/v1/entities/merges", body.clone(), Some(c))
            .await
            .assert_problem(StatusCode::FORBIDDEN, "forbidden");
    }
    let bad = |b: Value| post(&w.r, "/api/v1/entities/merges", b, Some(&w.admin));
    assert_eq!(bad(json!({ "entity_type": "language", "from_id": d, "into_id": k, "expected_from_version": 1, "expected_into_version": 1 })).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(bad(json!({ "entity_type": "artist", "from_id": d, "into_id": d, "expected_from_version": 1, "expected_into_version": 1 })).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        bad(json!({ "entity_type": "artist", "from_id": d, "into_id": k }))
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let stale = bad(json!({ "entity_type": "artist", "from_id": d, "into_id": k, "expected_from_version": 9, "expected_into_version": 1 })).await;
    stale.assert_problem(StatusCode::CONFLICT, "version_conflict");
    assert_eq!(stale.body["current_version"], 1);
    assert_eq!(count(&pool, "SELECT count(*) FROM artists").await, 2);

    let res = post(&w.r, "/api/v1/entities/merges", body.clone(), Some(&w.admin)).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.body);
    let m = &res.body["moved"];
    assert_eq!(
        (m["credits"].as_u64(), m["duplicate_credits_dropped"].as_u64()),
        (Some(1), Some(1)),
        "{m}"
    );
    assert_eq!(
        (m["localizations_retired"].as_u64(), m["aliases"].as_u64()),
        (Some(1), Some(2)),
        "{m}"
    );

    // References: one credit on Shared, the other moved; nothing points at the old id.
    let shared_now = get(&w.r, &format!("/api/v1/compositions/{}", id(&shared)), None)
        .await
        .body;
    assert_eq!(shared_now["credits"].as_array().unwrap().len(), 1);
    assert_eq!(shared_now["credits"][0]["artist_id"], keep["id"]);
    let own_now = get(&w.r, &format!("/api/v1/compositions/{}", id(&own)), None)
        .await
        .body;
    assert_eq!(own_now["credits"][0]["artist_id"], keep["id"]);
    assert_eq!(
        count(&pool, &format!("SELECT count(*) FROM credits WHERE artist_id = '{d}'")).await,
        0
    );
    assert_eq!(
        count(&pool, &format!("SELECT count(*) FROM artists WHERE id = '{d}'")).await,
        0
    );
    // Survivor is versioned forward; its own fields are untouched.
    let kept = get(&w.r, &format!("/api/v1/artists/{k}"), None).await.body;
    assert_eq!(
        (kept["version"].as_i64(), kept["name"].as_str()),
        (Some(2), Some("A. R. Rahman"))
    );

    // Names survive: the old name and aliases find the survivor; pending stays hidden.
    for q in ["AR Rahmaan", "Dilip", "Retired English"] {
        let hits = suggest(&w, "artist", q).await;
        assert_eq!((hits.len(), hits[0]["id"].clone()), (1, keep["id"].clone()), "{q}");
    }
    assert!(suggest(&w, "artist", "Pending Alias").await.is_empty());
    assert_eq!(count(&pool, &format!("SELECT count(*) FROM entity_localizations WHERE entity_id = '{k}' AND merged_from_id IS NOT NULL AND verification_state = 'rejected'")).await, 1);
    // Exactly one preferred English name remains.
    assert_eq!(
        count(
            &pool,
            &format!("SELECT count(*) FROM entity_localizations WHERE entity_id = '{k}' AND is_preferred")
        )
        .await,
        1
    );

    // The old route redirects; history is append-only; the merge is audited with the old record.
    let gone = get(&w.r, &format!("/api/v1/artists/{d}?lang=ml"), None).await;
    assert_eq!(gone.status, StatusCode::MOVED_PERMANENTLY);
    let loc = send(&w.r, Method::GET, &format!("/api/v1/artists/{d}"), None, None).await;
    assert_eq!(loc.status, StatusCode::MOVED_PERMANENTLY);
    assert_eq!(count(&pool, "SELECT count(*) FROM entity_merges").await, 1);
    assert!(sqlx::query("UPDATE entity_merges SET note = 'x'")
        .execute(&pool)
        .await
        .is_err());
    assert!(sqlx::query("DELETE FROM entity_merges").execute(&pool).await.is_err());
    let audit: Value = sqlx::query_scalar(
        "SELECT jsonb_build_object('before', before, 'after', after) FROM audit_log WHERE action = 'artist.merged'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audit["before"]["name"], "AR Rahmaan");
    assert_eq!(audit["after"]["into_id"], keep["id"]);
    // A second attempt on the same record says so; unknown ids are 404.
    post(&w.r, "/api/v1/entities/merges", body, Some(&w.admin))
        .await
        .assert_problem(StatusCode::CONFLICT, "already_merged");
    post(
        &w.r,
        "/api/v1/entities/merges",
        merge_body("artist", &json!({ "id": Uuid::new_v4(), "version": 1 }), &kept),
        Some(&w.admin),
    )
    .await
    .assert_problem(StatusCode::NOT_FOUND, "not_found");
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn ambiguous_records_are_never_merged(pool: PgPool) {
    let w = world(&pool).await;
    let mk = |name: &str, extra: Value| {
        let mut b = json!({ "name": name, "kind": "person" });
        for (k, v) in extra.as_object().unwrap() {
            b[k] = v.clone();
        }
        post(&w.r, "/api/v1/artists", b, Some(&w.member))
    };
    let a = mk("Same Name", json!({ "isni": "0000000121032683" })).await.body;
    let b = mk("Same Name", json!({ "isni": "0000000121032675" })).await.body;
    let c = mk("Same Name", json!({ "kind": "group" })).await.body;
    let admin = Some(w.admin.as_str());
    post(&w.r, "/api/v1/entities/merges", merge_body("artist", &a, &b), admin)
        .await
        .assert_problem(StatusCode::CONFLICT, "identifier_mismatch");
    post(&w.r, "/api/v1/entities/merges", merge_body("artist", &c, &a), admin)
        .await
        .assert_problem(StatusCode::CONFLICT, "kind_mismatch");

    // Albums that both list tracks at the same position are ambiguous too.
    let rec = {
        let comp = composition(&w, "Song", json!([])).await;
        let r = post(
            &w.r,
            "/api/v1/recordings",
            json!({ "composition_id": id(&comp), "title": "T" }),
            Some(&w.member),
        )
        .await;
        let r2 = post(
            &w.r,
            "/api/v1/recordings",
            json!({ "composition_id": id(&comp), "title": "T2" }),
            Some(&w.member),
        )
        .await;
        (id(&r.body), id(&r2.body))
    };
    let a1 = post(
        &w.r,
        "/api/v1/albums",
        json!({ "title": "LP", "kind": "album", "tracks": [rec.0] }),
        Some(&w.member),
    )
    .await
    .body;
    let a2 = post(
        &w.r,
        "/api/v1/albums",
        json!({ "title": "LP", "kind": "album", "tracks": [rec.1] }),
        Some(&w.member),
    )
    .await
    .body;
    post(&w.r, "/api/v1/entities/merges", merge_body("album", &a1, &a2), admin)
        .await
        .assert_problem(StatusCode::CONFLICT, "track_conflict");

    // Nothing moved, nothing deleted, nothing recorded; suggestions and proposals never merge either.
    assert_eq!(count(&pool, "SELECT count(*) FROM artists").await, 3);
    assert_eq!(count(&pool, "SELECT count(*) FROM albums").await, 2);
    assert_eq!(count(&pool, "SELECT count(*) FROM entity_merges").await, 0);
    assert_eq!(
        count(&pool, "SELECT count(*) FROM audit_log WHERE action LIKE '%.merged'").await,
        0
    );
    assert_eq!(suggest(&w, "artist", "same name").await.len(), 3);
    let p = propose(
        &w,
        &w.member2,
        "artist",
        "Same Name",
        json!({ "kind": "person", "confirm_distinct": true }),
    )
    .await;
    assert_eq!(p.status, StatusCode::CREATED);
    post(
        &w.r,
        &format!("/api/v1/entities/proposals/{}/approve", id(&p.body)),
        json!({ "confirm_distinct": true }),
        Some(&w.admin),
    )
    .await;
    assert_eq!(
        count(&pool, "SELECT count(*) FROM artists").await,
        4,
        "approval adds a fourth, never folds into one"
    );
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn merges_chain_redirects_and_opposite_merges_do_not_deadlock(pool: PgPool) {
    let w = world(&pool).await;
    let a = artist(&w, "Alpha One").await;
    let b = artist(&w, "Beta Two").await;
    let c = artist(&w, "Gamma Three").await;
    let admin = Some(w.admin.as_str());
    assert_eq!(
        post(&w.r, "/api/v1/entities/merges", merge_body("artist", &a, &b), admin)
            .await
            .status,
        StatusCode::OK
    );
    let b2 = get(&w.r, &format!("/api/v1/artists/{}", id(&b)), None).await.body;
    assert_eq!(
        post(&w.r, "/api/v1/entities/merges", merge_body("artist", &b2, &c), admin)
            .await
            .status,
        StatusCode::OK
    );
    // A's route reaches the final survivor in one hop.
    let res = get(&w.r, &format!("/api/v1/artists/{}", id(&a)), None).await;
    assert_eq!(res.status, StatusCode::MOVED_PERMANENTLY);
    assert_eq!(count(&pool, "SELECT count(*) FROM entity_merges").await, 2);
    assert_eq!(suggest(&w, "artist", "alpha one").await[0]["id"], c["id"]);

    // Opposite merges at once: one wins, the other fails cleanly.
    let x = artist(&w, "Xray Four").await;
    let y = artist(&w, "Yankee Five").await;
    let (r1, r2) = tokio::join!(
        post(&w.r, "/api/v1/entities/merges", merge_body("artist", &x, &y), admin),
        post(&w.r, "/api/v1/entities/merges", merge_body("artist", &y, &x), admin)
    );
    let ok = [&r1, &r2].iter().filter(|r| r.status == StatusCode::OK).count();
    assert_eq!(ok, 1, "{} / {}", r1.body, r2.body);
    let other = if r1.status == StatusCode::OK { &r2 } else { &r1 };
    assert!(
        matches!(other.status, StatusCode::CONFLICT | StatusCode::NOT_FOUND),
        "{} {}",
        other.status,
        other.body
    );
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM artists WHERE name IN ('Xray Four','Yankee Five')"
        )
        .await,
        1
    );
}
