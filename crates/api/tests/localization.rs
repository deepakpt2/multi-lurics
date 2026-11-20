//! Localized metadata: reference data, display fallback, Unicode round trips, one artist with
//! many names, credit categories, write authorization, revision history, concurrency.

mod common;

use axum::http::{Method, StatusCode};
use axum::Router;
use common::*;
use domain::Role;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

fn id(v: &Value) -> &str {
    v["id"].as_str().unwrap()
}

async fn make(r: &Router, cookie: &str, path: &str, body: Value) -> Value {
    let res = post(r, &format!("/api/v1/{path}"), body, Some(cookie)).await;
    assert_eq!(res.status, StatusCode::CREATED, "{path}: {}", res.body);
    res.body
}

async fn patch(r: &Router, uri: &str, body: Value, cookie: Option<&str>) -> Res {
    send(r, Method::PATCH, uri, Some(body), cookie).await
}

/// POST /entities/{type}/{id}/localizations.
async fn localize(r: &Router, cookie: &str, entity: &str, eid: &str, body: Value) -> Res {
    post(
        r,
        &format!("/api/v1/entities/{entity}/{eid}/localizations"),
        body,
        Some(cookie),
    )
    .await
}

fn name_in(lang: &str, name: &str) -> Value {
    json!({ "language": lang, "name": name, "provenance": "community" })
}

async fn display(r: &Router, kind: &str, eid: &str, lang: &str) -> Value {
    let res = get(r, &format!("/api/v1/{kind}/{eid}?lang={lang}"), None).await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.body);
    res.body["display"].clone()
}

async fn verify(r: &Router, cookie: &str, loc: &Value) -> Res {
    post(
        r,
        &format!("/api/v1/localizations/{}/verify", id(loc)),
        json!({ "expected_parent_revision_id": loc["current_revision_id"] }),
        Some(cookie),
    )
    .await
}

async fn put_default(r: &Router, who: &str, w: &str, lang: &str, version: i32) -> Res {
    send(
        r,
        Method::PUT,
        &format!("/api/v1/entities/composition/{w}/default-language"),
        Some(json!({ "language": lang, "expected_version": version })),
        Some(who),
    )
    .await
}

async fn decide(r: &Router, who: Option<&str>, loc: &Value, verb: &str) -> Res {
    post(
        r,
        &format!("/api/v1/localizations/{}/{verb}", id(loc)),
        json!({ "expected_parent_revision_id": loc["current_revision_id"] }),
        who,
    )
    .await
}

async fn edit(r: &Router, who: Option<&str>, uri: &str, rev: &Value, name: &str) -> Res {
    patch(r, uri, json!({ "expected_parent_revision_id": rev, "name": name }), who).await
}

async fn count(pool: &PgPool, sql: &str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
}

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
async fn reference_data_is_seeded_by_migration_and_governed(pool: PgPool) {
    let r = router(&pool);
    let all = get(&r, "/api/v1/languages?limit=100", None).await;
    let langs = all.body["items"].as_array().unwrap();
    assert!(langs.len() >= 30 && all.body["next_cursor"].is_null());
    let find = |code: &str| {
        langs
            .iter()
            .find(|l| l["iso639_3"] == code)
            .unwrap_or_else(|| panic!("{code}"))
    };
    assert_eq!(find("mal")["iso639_1"], "ml");
    assert_eq!(find("mal")["endonym"], "മലയാളം");
    assert_eq!(find("mal")["default_script"], "Mlym");
    assert_eq!(find("ara")["is_rtl"], true);
    assert_eq!(find("heb")["is_rtl"], true);
    assert_eq!(find("eng")["is_rtl"], false);
    assert!(langs.iter().all(|l| l["is_extension"] == false));

    let scripts = get(&r, "/api/v1/scripts?limit=100", None).await;
    let scripts = scripts.body["items"].as_array().unwrap();
    let script = |c: &str| scripts.iter().find(|s| s["iso15924"] == c).unwrap();
    assert_eq!(
        (script("Arab")["is_rtl"].clone(), script("Mlym")["numeric_code"].clone()),
        (json!(true), json!(347))
    );
    assert_eq!(script("Latn")["is_rtl"], false);

    // Paging visits each language exactly once.
    let mut seen = std::collections::HashSet::new();
    let mut uri = "/api/v1/languages?limit=10".to_owned();
    loop {
        let page = get(&r, &uri, None).await;
        for l in page.body["items"].as_array().unwrap() {
            assert!(seen.insert(l["id"].as_str().unwrap().to_owned()));
        }
        match page.body["next_cursor"].as_str() {
            Some(c) => uri = format!("/api/v1/languages?limit=10&cursor={c}"),
            None => break,
        }
    }
    assert_eq!(seen.len(), langs.len());

    // Extensions are governed: only private-use codes may be flagged as extensions, and vice versa.
    let ins = |code: &'static str, ext: bool| {
        sqlx::query("INSERT INTO languages (iso639_3, name_en, endonym, is_extension) VALUES ($1, 'X', 'X', $2)")
            .bind(code)
            .bind(ext)
            .execute(&pool)
    };
    assert!(ins("xyz", true).await.is_err());
    assert!(ins("qaa", false).await.is_err());
    assert!(ins("qaa", true).await.is_ok());
    let bad_script = sqlx::query("INSERT INTO scripts (iso15924, name_en, is_extension) VALUES ('Qaaa', 'X', false)")
        .execute(&pool)
        .await;
    assert!(bad_script.is_err());
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn display_fallback_chain_including_missing(pool: PgPool) {
    let r = router(&pool);
    let (_, member) = signed_in(&pool, &r, "member@example.com", None).await;
    let (_, moderator) = signed_in(&pool, &r, "mod@example.com", Some(Role::Moderator)).await;
    let work = make(&r, &member, "compositions", json!({ "title": "Kaatrin Mozhi" })).await;
    let w = id(&work);

    // Nothing localized and no default language: the base name, flagged, language unknown.
    let d = display(&r, "compositions", w, "ml").await;
    assert_eq!(
        (d["text"].clone(), d["resolution"].clone(), d["fallback"].clone()),
        (json!("Kaatrin Mozhi"), json!("base"), json!(true))
    );
    assert_eq!((d["language"].clone(), d["dir"].clone()), (Value::Null, json!("auto")));

    let en = localize(&r, &moderator, "composition", w, name_in("en", "Language of the Wind")).await;
    assert_eq!(en.status, StatusCode::CREATED, "{}", en.body);
    localize(&r, &moderator, "composition", w, name_in("ml", "കാറ്റിൻ മൊഴി")).await;

    // Requested language present.
    let d = display(&r, "compositions", w, "ml").await;
    assert_eq!(
        (d["text"].clone(), d["resolution"].clone(), d["fallback"].clone()),
        (json!("കാറ്റിൻ മൊഴി"), json!("requested"), json!(false))
    );
    assert_eq!((d["language"].clone(), d["dir"].clone()), (json!("ml"), json!("ltr")));
    // Three-letter and region-qualified tags resolve to the same language.
    assert_eq!(display(&r, "compositions", w, "mal").await["text"], "കാറ്റിൻ മൊഴി");
    assert_eq!(display(&r, "compositions", w, "ML-in").await["text"], "കാറ്റിൻ മൊഴി");

    // No default language declared: any available localization, flagged as a fallback.
    let d = display(&r, "compositions", w, "ar").await;
    assert_eq!(
        (d["text"].clone(), d["resolution"].clone(), d["fallback"].clone()),
        (json!("Language of the Wind"), json!("fallback"), json!(true))
    );

    // Default language Malayalam: used before "any".
    assert_eq!(put_default(&r, &member, w, "ml", 1).await.body["version"], 2);
    let d = display(&r, "compositions", w, "ar").await;
    assert_eq!(
        (d["text"].clone(), d["resolution"].clone(), d["language"].clone()),
        (json!("കാറ്റിൻ മൊഴി"), json!("default_language"), json!("ml"))
    );
    assert_eq!(d["fallback"], true);

    // Default language declared but never localized: the base name, which is written in it.
    assert_eq!(put_default(&r, &member, w, "fr", 2).await.body["version"], 3);
    let d = display(&r, "compositions", w, "ar").await;
    assert_eq!(
        (d["text"].clone(), d["resolution"].clone(), d["language"].clone()),
        (json!("Kaatrin Mozhi"), json!("default_language"), json!("fr"))
    );
    put_default(&r, &member, w, "ml", 2)
        .await
        .assert_problem(StatusCode::CONFLICT, "version_conflict");
    assert_eq!(put_default(&r, &member, w, "ml", 3).await.status, StatusCode::OK);

    // Pending and rejected localizations never reach display.
    let ar = localize(&r, &member, "composition", w, name_in("ar", "لغة الريح")).await;
    assert_eq!(ar.body["verification_state"], "pending");
    assert_eq!(
        display(&r, "compositions", w, "ar").await["resolution"],
        "default_language"
    );
    let ta = localize(&r, &member, "composition", w, name_in("ta", "காற்றின் மொழி")).await;
    let rejected = post(
        &r,
        &format!("/api/v1/localizations/{}/reject", id(&ta.body)),
        json!({ "expected_parent_revision_id": ta.body["current_revision_id"] }),
        Some(&moderator),
    )
    .await;
    assert_eq!(rejected.body["verification_state"], "rejected");
    assert_eq!(
        display(&r, "compositions", w, "ta").await["resolution"],
        "default_language"
    );
    // Once verified it is served.
    assert_eq!(verify(&r, &moderator, &ar.body).await.status, StatusCode::OK);
    let d = display(&r, "compositions", w, "ar").await;
    assert_eq!(
        (d["text"].clone(), d["resolution"].clone(), d["dir"].clone()),
        (json!("لغة الريح"), json!("requested"), json!("rtl"))
    );

    // Script selection inside one language.
    let song = make(&r, &member, "compositions", json!({ "title": "Song" })).await;
    let s = id(&song);
    localize(
        &r,
        &moderator,
        "composition",
        s,
        json!({ "language": "sr", "script": "Latn", "name": "Pesma", "provenance": "community" }),
    )
    .await;
    let cyr = localize(
        &r,
        &moderator,
        "composition",
        s,
        json!({ "language": "sr", "script": "Cyrl", "name": "Песма", "provenance": "community" }),
    )
    .await;
    assert_eq!(
        cyr.body["is_preferred"], false,
        "second name in a language is not preferred by default"
    );
    for (tag, text, script) in [
        ("sr-Cyrl", "Песма", "Cyrl"),
        ("sr-Latn", "Pesma", "Latn"),
        ("sr", "Pesma", "Latn"),
    ] {
        let d = display(&r, "compositions", s, tag).await;
        assert_eq!(
            (d["text"].clone(), d["script"].clone(), d["fallback"].clone()),
            (json!(text), json!(script), json!(false)),
            "{tag}"
        );
        assert_eq!(d["language"], format!("sr-{script}"));
    }

    // Every read path uses the same resolver: lists and nested references.
    let list = get(&r, "/api/v1/compositions?lang=ml", None).await;
    let item = list.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == work["id"])
        .unwrap();
    assert_eq!(item["display"]["text"], "കാറ്റിൻ മൊഴി");
    let rec = make(
        &r,
        &member,
        "recordings",
        json!({ "composition_id": w, "title": "Take 1" }),
    )
    .await;
    let detail = get(&r, &format!("/api/v1/recordings/{}?lang=ar", id(&rec)), None).await;
    assert_eq!(detail.body["composition"]["display"]["text"], "لغة الريح");
    assert_eq!(detail.body["display"]["resolution"], "base");

    // Bad tags are field errors, not silent defaults.
    for (tag, code) in [
        ("zz", "unknown_language"),
        ("en-Zzzz", "unknown_script"),
        ("e", "invalid_language_tag"),
        ("en-", "invalid_language_tag"),
    ] {
        let res = get(&r, &format!("/api/v1/compositions/{w}?lang={tag}"), None).await;
        res.assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
        assert_eq!(codes(&res), [("lang".into(), code.into())], "{tag}");
    }
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn rtl_and_mixed_direction_unicode_round_trip(pool: PgPool) {
    let r = router(&pool);
    let (_, moderator) = signed_in(&pool, &r, "mod@example.com", Some(Role::Moderator)).await;
    let work = make(&r, &moderator, "compositions", json!({ "title": "Mixed" })).await;
    let w = id(&work);

    // Arabic with embedded Latin and digits, RLM/LRM marks and an isolate; Hebrew with Latin;
    // Malayalam with ZWNJ/ZWJ (chillu forms). Nothing may be normalized, trimmed inside, or stripped.
    let cases = [
        ("ar", None, "أغنية \u{2067}Love Story 2024\u{2069} الحب\u{200F}", "rtl"),
        ("he", None, "שיר \u{200E}Song\u{200E} 7 אהבה", "rtl"),
        ("ur", None, "محبت کا گیت (Love) ۱۲۳", "rtl"),
        ("ar", Some("Latn"), "Ughniyat al-hubb", "ltr"),
        ("ml", None, "ക\u{200D}ല്\u{200C}മലയാളം ചില്ല് ൻ ർ ൽ ൾ ൺ", "ltr"),
    ];
    for (lang, script, text, _) in &cases {
        let mut body = name_in(lang, text);
        if let Some(s) = script {
            body["script"] = json!(s);
        }
        let res = localize(&r, &moderator, "composition", w, body).await;
        assert_eq!(res.status, StatusCode::CREATED, "{lang}: {}", res.body);
        assert_eq!(res.body["name"], *text, "store→response");
    }
    let list = get(
        &r,
        &format!("/api/v1/entities/composition/{w}/localizations?limit=50"),
        None,
    )
    .await;
    let items = list.body["items"].as_array().unwrap();
    for (lang, script, text, dir) in &cases {
        let item = items
            .iter()
            .find(|i| i["language"].as_str().unwrap() == *lang && i["script"].as_str() == *script)
            .unwrap_or_else(|| panic!("{lang}"));
        assert_eq!(
            item["name"].as_str().unwrap(),
            *text,
            "{lang}: read back byte-identical"
        );
        assert_eq!(item["dir"], *dir, "{lang}: direction follows script, else language");
        let stored: String = sqlx::query_scalar("SELECT name FROM entity_localizations WHERE id = $1::uuid")
            .bind(item["id"].as_str().unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(stored.as_bytes(), text.as_bytes(), "{lang}: stored bytes unchanged");
        let tag = match script {
            Some(s) => format!("{lang}-{s}"),
            None => (*lang).to_owned(),
        };
        let d = display(&r, "compositions", w, &tag).await;
        assert_eq!(
            (d["text"].as_str().unwrap(), d["dir"].as_str().unwrap()),
            (*text, *dir),
            "{tag}: render attributes"
        );
    }
    // Revision snapshots keep the exact text too.
    let first = items
        .iter()
        .find(|i| i["language"] == "ar" && i["script"].is_null())
        .unwrap();
    let revs = get(&r, &format!("/api/v1/localizations/{}/revisions", id(first)), None).await;
    assert_eq!(revs.body["items"][0]["data"]["data"]["name"], cases[0].2);

    // Control characters are refused; formatting characters are not.
    for bad in ["bad\u{0007}name", "nul\u{0000}"] {
        let res = localize(
            &r,
            &moderator,
            "composition",
            w,
            json!({ "language": "fr", "name": bad, "provenance": "community" }),
        )
        .await;
        res.assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
        assert_eq!(codes(&res), [("name".into(), "invalid_characters".into())]);
    }
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn one_artist_one_id_many_names_and_verified_aliases(pool: PgPool) {
    let r = router(&pool);
    let (_, owner) = signed_in(&pool, &r, "owner@example.com", None).await;
    let (_, moderator) = signed_in(&pool, &r, "mod@example.com", Some(Role::Moderator)).await;
    let artist = make(&r, &owner, "artists", json!({ "name": "Yesudas", "kind": "person" })).await;
    let a = id(&artist);

    let names = [
        ("ml", "യേശുദാസ്", "Mlym"),
        ("ar", "يسوداس", "Arab"),
        ("hi", "येसुदास", "Deva"),
        ("en", "K. J. Yesudas", "Latn"),
    ];
    for (lang, name, script) in names {
        let res = localize(
            &r,
            &moderator,
            "artist",
            a,
            json!({ "language": lang, "script": script, "name": name, "provenance": "original", "source": "Artist's official site" }),
        )
        .await;
        assert_eq!(res.status, StatusCode::CREATED, "{}", res.body);
    }
    // One identity: still one artist row, four names, and every language resolves to the same id.
    assert_eq!(count(&pool, "SELECT count(*) FROM artists").await, 1);
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM entity_localizations WHERE entity_id = $1::uuid"
                .replace("$1", &format!("'{a}'"))
                .as_str()
        )
        .await,
        4
    );
    for (lang, name, script) in names {
        let detail = get(&r, &format!("/api/v1/artists/{a}?lang={lang}"), None).await;
        assert_eq!(detail.body["id"], artist["id"]);
        assert_eq!(detail.body["display"]["text"], name);
        assert_eq!(detail.body["display"]["language"], format!("{lang}-{script}"));
        let list = get(&r, &format!("/api/v1/artists?lang={lang}&prefix=yes"), None).await;
        assert_eq!(list.body["items"][0]["id"], artist["id"]);
        assert_eq!(list.body["items"][0]["display"]["text"], name);
    }
    assert_eq!(display(&r, "artists", a, "ja").await["resolution"], "fallback");

    // Credits show the artist in the reader's language; the id is unchanged.
    let work = make(
        &r,
        &owner,
        "compositions",
        json!({ "title": "W", "credits": [{ "artist_id": a, "role": "composer" }] }),
    )
    .await;
    let detail = get(&r, &format!("/api/v1/compositions/{}?lang=ar", id(&work)), None).await;
    assert_eq!(detail.body["credits"][0]["artist_id"], artist["id"]);
    assert_eq!(detail.body["credits"][0]["artist_display"]["text"], "يسوداس");
    assert_eq!(detail.body["credits"][0]["artist_display"]["dir"], "rtl");
    let by_artist = get(&r, &format!("/api/v1/artists/{a}?lang=hi"), None).await;
    assert_eq!(by_artist.body["credits"][0]["title_display"]["resolution"], "base");

    // Personal names: never machine-made, always sourced; the database says the same.
    let machine = localize(
        &r,
        &moderator,
        "artist",
        a,
        json!({ "language": "fr", "name": "Yesudas", "provenance": "machine", "source": "x" }),
    )
    .await;
    machine.assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    assert_eq!(codes(&machine), [("provenance".into(), "machine_not_allowed".into())]);
    let unsourced = localize(
        &r,
        &moderator,
        "artist",
        a,
        json!({ "language": "fr", "name": "Yesudas", "provenance": "original" }),
    )
    .await;
    assert_eq!(codes(&unsourced), [("source".into(), "required".into())]);
    let genre = make(&r, &owner, "taxonomy/genres", json!({ "name": "Ghazal" })).await;
    let ok = localize(
        &r,
        &moderator,
        "genre",
        id(&genre),
        json!({ "language": "fr", "name": "Ghazal", "provenance": "machine" }),
    )
    .await;
    assert_eq!(
        ok.status,
        StatusCode::CREATED,
        "machine output is allowed for non-personal metadata"
    );
    let artist_uuid = Uuid::parse_str(a).unwrap();
    let direct = |sql: &'static str| sqlx::query(sql).bind(artist_uuid).execute(&pool);
    let machine_sql = "INSERT INTO entity_localizations (entity_type, entity_id, language_id, name, is_preferred, provenance, source, created_by)
        SELECT 'artist', $1, (SELECT id FROM languages WHERE iso639_1 = 'de'), 'x', false, 'machine', 'src', id FROM users LIMIT 1";
    assert!(direct(machine_sql)
        .await
        .unwrap_err()
        .to_string()
        .contains("localizations_personal_name_check"));
    let ghost = "INSERT INTO entity_localizations (entity_type, entity_id, language_id, name, is_preferred, provenance, created_by)
        SELECT 'artist', gen_random_uuid(), (SELECT id FROM languages WHERE iso639_1 = 'de'), 'x', false, 'original', id FROM users LIMIT 1";
    assert!(sqlx::query(ghost)
        .execute(&pool)
        .await
        .unwrap_err()
        .to_string()
        .contains("does not exist"));
    localize(
        &r,
        &moderator,
        "artist",
        "00000000-0000-4000-8000-000000000000",
        name_in("en", "x"),
    )
    .await
    .assert_problem(StatusCode::NOT_FOUND, "not_found");
    localize(&r, &moderator, "movie", a, name_in("en", "x"))
        .await
        .assert_problem(StatusCode::NOT_FOUND, "not_found");

    // Aliases: sourced, moderated, case-insensitively unique, public only once verified.
    let alias_uri = format!("/api/v1/entities/artist/{a}/aliases");
    let no_source = post(&r, &alias_uri, json!({ "alias": "Dasettan" }), Some(&owner)).await;
    assert_eq!(codes(&no_source), [("source".into(), "required".into())]);
    let pending = post(
        &r,
        &alias_uri,
        json!({ "alias": "Dasettan", "language": "ml", "source": "Interview, 2019" }),
        Some(&owner),
    )
    .await;
    assert_eq!(pending.status, StatusCode::CREATED, "{}", pending.body);
    assert_eq!(pending.body["verification_state"], "pending");
    assert_eq!(get(&r, &alias_uri, None).await.body["items"], json!([]));
    post(
        &r,
        &alias_uri,
        json!({ "alias": "dasettan", "language": "ml", "source": "Other" }),
        Some(&owner),
    )
    .await
    .assert_problem(StatusCode::CONFLICT, "alias_exists");
    post(
        &r,
        &format!("/api/v1/aliases/{}/verify", id(&pending.body)),
        json!({}),
        Some(&owner),
    )
    .await
    .assert_problem(StatusCode::FORBIDDEN, "forbidden");
    let done = post(
        &r,
        &format!("/api/v1/aliases/{}/verify", id(&pending.body)),
        json!({}),
        Some(&moderator),
    )
    .await;
    assert_eq!(done.body["verification_state"], "verified");
    let public = get(&r, &alias_uri, None).await;
    assert_eq!(public.body["items"][0]["alias"], "Dasettan");
    assert_eq!(public.body["items"][0]["source"], "Interview, 2019");
    post(
        &r,
        &format!("/api/v1/aliases/{}/verify", id(&pending.body)),
        json!({}),
        Some(&moderator),
    )
    .await
    .assert_problem(StatusCode::CONFLICT, "invalid_state_transition");
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn credits_keep_songwriter_translator_and_adapter_separate(pool: PgPool) {
    let r = router(&pool);
    let (_, owner) = signed_in(&pool, &r, "owner@example.com", None).await;
    let (_, moderator) = signed_in(&pool, &r, "mod@example.com", Some(Role::Moderator)).await;
    let mut ids = vec![];
    for n in ["Writer", "Translator Person", "Adapter Person"] {
        ids.push(id(&make(&r, &owner, "artists", json!({ "name": n, "kind": "person" })).await).to_owned());
    }
    let work = make(
        &r,
        &owner,
        "compositions",
        json!({ "title": "Song", "credits": [
            { "artist_id": ids[0], "role": "composer" }, { "artist_id": ids[0], "role": "lyricist" },
            { "artist_id": ids[1], "role": "translator" }, { "artist_id": ids[2], "role": "adapter" } ] }),
    )
    .await;
    let credits = work["credits"].as_array().unwrap();
    let cat = |role: &str| credits.iter().find(|c| c["role"] == role).unwrap()["role_category"].clone();
    assert_eq!(
        (cat("composer"), cat("lyricist"), cat("translator"), cat("adapter")),
        (
            json!("original_songwriter"),
            json!("original_songwriter"),
            json!("translator"),
            json!("adapter")
        )
    );
    // The three groups are disjoint and complete.
    let group = |c: &str| credits.iter().filter(|k| k["role_category"] == c).count();
    assert_eq!(
        (group("original_songwriter"), group("translator"), group("adapter")),
        (2, 1, 1)
    );
    assert_eq!(credits.len(), 4);

    // A translator credit is a work-level credit: refused on a recording.
    let rec = post(&r, "/api/v1/recordings", json!({ "composition_id": id(&work), "title": "R", "credits": [{ "artist_id": ids[1], "role": "translator" }] }), Some(&owner)).await;
    assert_eq!(codes(&rec), [("credits".into(), "role_not_applicable".into())]);

    // Role labels are localizable entities; the category does not change with language.
    let role_id = credits.iter().find(|c| c["role"] == "translator").unwrap()["role_id"]
        .as_str()
        .unwrap();
    let loc = localize(
        &r,
        &moderator,
        "credit_role",
        role_id,
        json!({ "language": "ar", "name": "مترجم", "provenance": "human_translation" }),
    )
    .await;
    assert_eq!(loc.status, StatusCode::CREATED, "{}", loc.body);
    let ar = get(&r, &format!("/api/v1/compositions/{}?lang=ar", id(&work)), None).await;
    let t = ar.body["credits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["role"] == "translator")
        .unwrap()
        .clone();
    assert_eq!(
        (
            t["role_display"]["text"].clone(),
            t["role_display"]["dir"].clone(),
            t["role_category"].clone()
        ),
        (json!("مترجم"), json!("rtl"), json!("translator"))
    );
    let en = get(&r, &format!("/api/v1/compositions/{}?lang=en", id(&work)), None).await;
    let t = en.body["credits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["role"] == "translator")
        .unwrap()
        .clone();
    assert_eq!(
        (t["role_display"]["text"].clone(), t["role_display"]["language"].clone()),
        (json!("Translator"), json!("en"))
    );

    // Artist page groups by the same categories.
    let page = get(&r, &format!("/api/v1/artists/{}", ids[1]), None).await;
    assert_eq!(page.body["credits"][0]["role_category"], "translator");
    assert_eq!(count(&pool, "SELECT count(DISTINCT category) FROM credit_roles WHERE category IN ('original_songwriter','translator','adapter')").await, 3);
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn metadata_write_authorization(pool: PgPool) {
    let r = router(&pool);
    let (_, author) = signed_in(&pool, &r, "author@example.com", None).await;
    let (_, other) = signed_in(&pool, &r, "other@example.com", None).await;
    let (_, moderator) = signed_in(&pool, &r, "mod@example.com", Some(Role::Moderator)).await;
    let work = make(&r, &author, "compositions", json!({ "title": "W" })).await;
    let w = id(&work);
    let uri = format!("/api/v1/entities/composition/{w}/localizations");

    post(&r, &uri, name_in("en", "Anon"), None)
        .await
        .assert_problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    post(&r, &uri, name_in("en", "Forged"), Some("lyrics_session=forged"))
        .await
        .assert_problem(StatusCode::UNAUTHORIZED, "unauthenticated");

    // A member's submission is pending and invisible to the public and to other members.
    let sub = localize(&r, &author, "composition", w, name_in("en", "Draft name")).await;
    assert_eq!(sub.status, StatusCode::CREATED);
    assert_eq!(
        (sub.body["verification_state"].clone(), sub.body["verified_by"].clone()),
        (json!("pending"), Value::Null)
    );
    let loc = sub.body.clone();
    let one = format!("/api/v1/localizations/{}", id(&loc));
    assert_eq!(get(&r, &uri, None).await.body["items"], json!([]));
    get(&r, &one, None)
        .await
        .assert_problem(StatusCode::NOT_FOUND, "not_found");
    get(&r, &one, Some(&other))
        .await
        .assert_problem(StatusCode::NOT_FOUND, "not_found");
    get(&r, &format!("{one}/revisions"), Some(&other))
        .await
        .assert_problem(StatusCode::NOT_FOUND, "not_found");
    assert_eq!(get(&r, &one, Some(&author)).await.status, StatusCode::OK);
    assert_eq!(get(&r, &one, Some(&moderator)).await.status, StatusCode::OK);
    get(&r, &format!("{uri}?state=pending"), None)
        .await
        .assert_problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    get(&r, &format!("{uri}?state=pending"), Some(&other))
        .await
        .assert_problem(StatusCode::FORBIDDEN, "forbidden");
    assert_eq!(
        get(&r, &format!("{uri}?state=pending"), Some(&moderator)).await.body["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    // Only moderators decide, and only moderators see the queue.
    decide(&r, None, &loc, "verify")
        .await
        .assert_problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    decide(&r, Some(&author), &loc, "verify")
        .await
        .assert_problem(StatusCode::FORBIDDEN, "forbidden");
    decide(&r, Some(&other), &loc, "reject")
        .await
        .assert_problem(StatusCode::FORBIDDEN, "forbidden");
    get(&r, "/api/v1/moderation/localizations", Some(&author))
        .await
        .assert_problem(StatusCode::FORBIDDEN, "forbidden");
    get(&r, "/api/v1/moderation/localizations", None)
        .await
        .assert_problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    let queue = get(&r, "/api/v1/moderation/localizations", Some(&moderator)).await;
    assert_eq!(queue.body["items"][0]["id"], loc["id"]);

    // Editing: the author may revise an unpublished row; others may not.
    edit(&r, None, &one, &loc["current_revision_id"], "x")
        .await
        .assert_problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    edit(&r, Some(&other), &one, &loc["current_revision_id"], "Hijack")
        .await
        .assert_problem(StatusCode::NOT_FOUND, "not_found");
    let edited = edit(&r, Some(&author), &one, &loc["current_revision_id"], "Better draft").await;
    assert_eq!(edited.status, StatusCode::OK, "{}", edited.body);
    assert_eq!(edited.body["verification_state"], "pending");

    // A moderator publishes; afterwards the author can no longer change it, the public can read it.
    let published = post(
        &r,
        &format!("/api/v1/localizations/{}/verify", id(&loc)),
        json!({ "expected_parent_revision_id": edited.body["current_revision_id"] }),
        Some(&moderator),
    )
    .await;
    assert_eq!(published.status, StatusCode::OK, "{}", published.body);
    assert_ne!(published.body["verified_by"], Value::Null);
    assert_eq!(get(&r, &one, None).await.body["name"], "Better draft");
    edit(
        &r,
        Some(&author),
        &one,
        &published.body["current_revision_id"],
        "Sneaky",
    )
    .await
    .assert_problem(StatusCode::FORBIDDEN, "forbidden");
    edit(&r, Some(&other), &one, &published.body["current_revision_id"], "Sneaky")
        .await
        .assert_problem(StatusCode::FORBIDDEN, "forbidden");
    assert_eq!(get(&r, &one, None).await.body["name"], "Better draft");
    let fixed = edit(
        &r,
        Some(&moderator),
        &one,
        &published.body["current_revision_id"],
        "Moderator fix",
    )
    .await;
    assert_eq!(
        (fixed.status, fixed.body["verification_state"].clone()),
        (StatusCode::OK, json!("verified"))
    );

    // A moderator's own submission is published directly; the actor comes from the session, never the body.
    let direct = post(&r, &uri, json!({ "language": "ml", "name": "നേരിട്ട്", "provenance": "original", "created_by": "00000000-0000-4000-8000-000000000000", "verification_state": "pending" }), Some(&moderator)).await;
    assert_eq!(
        (
            direct.body["verification_state"].clone(),
            direct.body["created_by"].clone()
        ),
        (json!("verified"), published.body["verified_by"].clone())
    );

    // Default language: creator or moderator only, versioned.
    put_default(&r, &other, w, "ml", 1)
        .await
        .assert_problem(StatusCode::FORBIDDEN, "forbidden");
    assert_eq!(put_default(&r, &author, w, "ml", 1).await.status, StatusCode::OK);
    send(
        &r,
        Method::PUT,
        &format!("/api/v1/entities/composition/{w}/default-language"),
        Some(json!({ "language": "ml" })),
        Some(&author),
    )
    .await
    .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");

    // Nothing the denied callers tried changed any state.
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM entity_localizations WHERE name IN ('Hijack','Sneaky','Anon','Forged')"
        )
        .await,
        0
    );
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM audit_log WHERE action = 'localization.created'"
        )
        .await,
        2
    );
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn revision_history_is_immutable_linear_and_audited(pool: PgPool) {
    let r = router(&pool);
    let (_, moderator) = signed_in(&pool, &r, "mod@example.com", Some(Role::Moderator)).await;
    let work = make(&r, &moderator, "compositions", json!({ "title": "W" })).await;
    let w = id(&work);
    let created = localize(
        &r,
        &moderator,
        "composition",
        w,
        json!({ "language": "en", "name": "First", "description": "v1", "provenance": "community" }),
    )
    .await;
    let loc = created.body.clone();
    let uri = format!("/api/v1/localizations/{}", id(&loc));
    let rev1 = loc["current_revision_id"].clone();

    let v2 = patch(
        &r,
        &uri,
        json!({ "expected_parent_revision_id": rev1, "name": "Second" }),
        Some(&moderator),
    )
    .await;
    assert_eq!(v2.status, StatusCode::OK, "{}", v2.body);
    let rev2 = v2.body["current_revision_id"].clone();
    assert_ne!(rev1, rev2);

    // Stale write: 409 carrying the revision to rebase onto; nothing changes.
    let stale = patch(
        &r,
        &uri,
        json!({ "expected_parent_revision_id": rev1, "name": "Lost" }),
        Some(&moderator),
    )
    .await;
    stale.assert_problem(StatusCode::CONFLICT, "stale_revision");
    assert_eq!(stale.body["current_revision_id"], rev2);
    assert_eq!(get(&r, &uri, None).await.body["name"], "Second");
    let missing = patch(&r, &uri, json!({ "name": "No parent" }), Some(&moderator)).await;
    assert_eq!(
        codes(&missing),
        [("expected_parent_revision_id".into(), "required".into())]
    );

    // A patch that changes nothing creates no revision.
    let same = patch(
        &r,
        &uri,
        json!({ "expected_parent_revision_id": rev2, "name": "Second" }),
        Some(&moderator),
    )
    .await;
    assert_eq!(same.body["current_revision_id"], rev2);

    // Reject frees the preferred slot; verify restores the row. Both are revisions.
    let rejected = post(
        &r,
        &format!("{uri}/reject"),
        json!({ "expected_parent_revision_id": rev2 }),
        Some(&moderator),
    )
    .await;
    assert_eq!(
        (
            rejected.body["verification_state"].clone(),
            rejected.body["is_preferred"].clone()
        ),
        (json!("rejected"), json!(false))
    );
    assert_eq!(
        get(&r, &format!("/api/v1/entities/composition/{w}/localizations"), None)
            .await
            .body["items"],
        json!([])
    );
    post(
        &r,
        &format!("{uri}/reject"),
        json!({ "expected_parent_revision_id": rejected.body["current_revision_id"] }),
        Some(&moderator),
    )
    .await
    .assert_problem(StatusCode::CONFLICT, "invalid_state_transition");
    let back = post(
        &r,
        &format!("{uri}/verify"),
        json!({ "expected_parent_revision_id": rejected.body["current_revision_id"] }),
        Some(&moderator),
    )
    .await;
    assert_eq!(back.body["verification_state"], "verified");

    // History: newest first, linear parents, immutable snapshots; paged by cursor.
    let revs = get(&r, &format!("{uri}/revisions"), None).await;
    let items = revs.body["items"].as_array().unwrap();
    let summary: Vec<_> = items
        .iter()
        .map(|i| (i["revision_no"].as_i64().unwrap(), i["action"].as_str().unwrap()))
        .collect();
    assert_eq!(
        summary,
        [(4, "verified"), (3, "rejected"), (2, "edited"), (1, "created")]
    );
    for w in items.windows(2) {
        assert_eq!(
            w[0]["parent_revision_id"], w[1]["id"],
            "each revision's parent is the previous one"
        );
    }
    assert_eq!(items[3]["parent_revision_id"], Value::Null);
    assert_eq!(items[3]["data"]["data"]["name"], "First");
    assert_eq!(items[2]["data"]["data"]["name"], "Second");
    assert_eq!(back.body["current_revision_id"], items[0]["id"]);
    let page1 = get(&r, &format!("{uri}/revisions?limit=3"), None).await;
    assert_eq!(page1.body["items"].as_array().unwrap().len(), 3);
    let page2 = get(
        &r,
        &format!(
            "{uri}/revisions?limit=3&cursor={}",
            page1.body["next_cursor"].as_str().unwrap()
        ),
        None,
    )
    .await;
    assert_eq!(page2.body["items"][0]["revision_no"], 1);
    assert!(page2.body["next_cursor"].is_null());

    // Immutable at the database level.
    let lid = Uuid::parse_str(id(&loc)).unwrap();
    for sql in [
        "UPDATE localization_revisions SET action = 'created' WHERE localization_id = $1",
        "DELETE FROM localization_revisions WHERE localization_id = $1",
    ] {
        let err = sqlx::query(sql).bind(lid).execute(&pool).await.unwrap_err();
        assert!(err.to_string().contains("append-only"), "{err}");
    }
    assert!(sqlx::query("TRUNCATE localization_revisions")
        .execute(&pool)
        .await
        .is_err());
    // Linear history is a constraint, not a convention: a second child of the same parent is refused.
    let dup = sqlx::query(
        "INSERT INTO localization_revisions (localization_id, revision_no, parent_revision_id, action, data, created_by)
         SELECT localization_id, 99, parent_revision_id, 'edited', '{}'::jsonb, created_by FROM localization_revisions
         WHERE localization_id = $1 AND revision_no = 2",
    )
    .bind(lid)
    .execute(&pool)
    .await;
    assert!(dup
        .unwrap_err()
        .to_string()
        .contains("localization_id_parent_revision_id"));

    // Every consequential change is audited with before/after.
    let actions: Vec<String> =
        sqlx::query_scalar("SELECT action FROM audit_log WHERE target_id = $1 ORDER BY occurred_at, id")
            .bind(lid)
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        actions,
        [
            "localization.created",
            "localization.edited",
            "localization.rejected",
            "localization.verified"
        ]
    );
    let (before, after): (Value, Value) =
        sqlx::query_as("SELECT before, after FROM audit_log WHERE action = 'localization.edited' AND target_id = $1")
            .bind(lid)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        (before["name"].clone(), after["name"].clone()),
        (json!("First"), json!("Second"))
    );

    // Preferred slot: never swapped silently. The rejected row gave up its slot and re-verifying does
    // not take it back, so the next name in that language becomes preferred; further claims conflict.
    assert_eq!(back.body["is_preferred"], false);
    let en2 = localize(
        &r,
        &moderator,
        "composition",
        w,
        json!({ "language": "en", "script": "Latn", "name": "Alt", "provenance": "community" }),
    )
    .await;
    assert_eq!(en2.body["is_preferred"], true, "the slot was free");
    let grab = localize(
        &r,
        &moderator,
        "composition",
        w,
        json!({ "language": "en", "script": "Cyrl", "name": "Grab", "provenance": "community", "is_preferred": true }),
    )
    .await;
    grab.assert_problem(StatusCode::CONFLICT, "preferred_slot_taken");
    assert_eq!(grab.body["existing_id"], en2.body["id"]);
    let dupe_slot = localize(&r, &moderator, "composition", w, name_in("en", "Duplicate slot")).await;
    dupe_slot.assert_problem(StatusCode::CONFLICT, "localization_slot_taken");
    assert_eq!(dupe_slot.body["existing_id"], loc["id"]);
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn concurrent_metadata_writes_never_fork_history_or_duplicate_slots(pool: PgPool) {
    let r = router(&pool);
    let (_, a) = signed_in(&pool, &r, "a@example.com", None).await;
    let (_, b) = signed_in(&pool, &r, "b@example.com", None).await;
    let (_, m1) = signed_in(&pool, &r, "m1@example.com", Some(Role::Moderator)).await;
    let (_, m2) = signed_in(&pool, &r, "m2@example.com", Some(Role::Moderator)).await;
    let work = make(&r, &a, "compositions", json!({ "title": "W" })).await;
    let w = id(&work);

    // Same slot from two members at once: exactly one wins, the other is told who holds it.
    let (x, y) = tokio::join!(
        localize(&r, &a, "composition", w, name_in("ta", "முதல்")),
        localize(&r, &b, "composition", w, name_in("ta", "இரண்டாவது"))
    );
    let (won, lost) = if x.status == StatusCode::CREATED {
        (x, y)
    } else {
        (y, x)
    };
    assert_eq!(won.status, StatusCode::CREATED, "{}", won.body);
    lost.assert_problem(StatusCode::CONFLICT, "localization_slot_taken");
    assert_eq!(count(&pool, "SELECT count(*) FROM entity_localizations WHERE language_id = (SELECT id FROM languages WHERE iso639_1 = 'ta')").await, 1);

    // Two edits from the same parent: one wins, the other gets 409 with the winner's revision.
    let uri = format!("/api/v1/localizations/{}", id(&won.body));
    let parent = won.body["current_revision_id"].clone();
    let owner = if won.body["created_by"] == lost_user(&pool, "a@example.com").await {
        &a
    } else {
        &b
    };
    let (e1, e2) = tokio::join!(
        patch(
            &r,
            &uri,
            json!({ "expected_parent_revision_id": parent, "name": "ஒன்று" }),
            Some(owner)
        ),
        patch(
            &r,
            &uri,
            json!({ "expected_parent_revision_id": parent, "name": "இரண்டு" }),
            Some(owner)
        ),
    );
    let (ok, stale) = if e1.status == StatusCode::OK {
        (e1, e2)
    } else {
        (e2, e1)
    };
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.body);
    stale.assert_problem(StatusCode::CONFLICT, "stale_revision");
    assert_eq!(stale.body["current_revision_id"], ok.body["current_revision_id"]);
    assert_eq!(get(&r, &uri, Some(owner)).await.body["name"], ok.body["name"]);

    // Two moderators deciding from the same parent: one decision is recorded, the other is stale.
    let parent = ok.body["current_revision_id"].clone();
    let decide = |who: String, verb: &'static str| {
        let (r, uri, parent) = (r.clone(), uri.clone(), parent.clone());
        async move {
            post(
                &r,
                &format!("{uri}/{verb}"),
                json!({ "expected_parent_revision_id": parent }),
                Some(&who),
            )
            .await
        }
    };
    let (d1, d2) = tokio::join!(decide(m1.clone(), "verify"), decide(m2.clone(), "reject"));
    let statuses = [d1.status, d2.status];
    assert!(
        statuses.contains(&StatusCode::OK) && statuses.contains(&StatusCode::CONFLICT),
        "{statuses:?}"
    );

    // History is one unbroken line: revisions 1..n, each parent is the previous revision.
    let lid = Uuid::parse_str(id(&won.body)).unwrap();
    let rows: Vec<(i32, Option<Uuid>, Uuid)> = sqlx::query_as("SELECT revision_no, parent_revision_id, id FROM localization_revisions WHERE localization_id = $1 ORDER BY revision_no")
        .bind(lid)
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(rows.iter().map(|r| r.0).collect::<Vec<_>>(), [1, 2, 3]);
    assert_eq!(rows[0].1, None);
    for w in rows.windows(2) {
        assert_eq!(w[1].1, Some(w[0].2));
    }
    let current: Option<Uuid> =
        sqlx::query_scalar("SELECT current_revision_id FROM entity_localizations WHERE id = $1")
            .bind(lid)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(current, Some(rows[2].2));
}

async fn lost_user(pool: &PgPool, email: &str) -> Value {
    let id: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE email = $1")
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap();
    json!(id.to_string())
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn taxonomy_entities_are_localizable(pool: PgPool) {
    let r = router(&pool);
    let (_, member) = signed_in(&pool, &r, "m@example.com", None).await;
    let (_, moderator) = signed_in(&pool, &r, "mod@example.com", Some(Role::Moderator)).await;
    for kind in ["labels", "publishers", "genres"] {
        let item = make(&r, &member, &format!("taxonomy/{kind}"), json!({ "name": "Base name" })).await;
        let et = kind.trim_end_matches('s');
        localize(&r, &moderator, et, id(&item), name_in("ml", "മലയാളം പേര്")).await;
        let one = get(&r, &format!("/api/v1/taxonomy/{kind}/{}?lang=ml", id(&item)), None).await;
        assert_eq!(one.body["display"]["text"], "മലയാളം പേര്", "{kind}");
        let list = get(&r, &format!("/api/v1/taxonomy/{kind}?lang=en"), None).await;
        assert_eq!(list.body["items"][0]["display"]["resolution"], "fallback", "{kind}");
        post(&r, &format!("/api/v1/taxonomy/{kind}"), json!({ "name": "x" }), None)
            .await
            .assert_problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    }
    get(&r, "/api/v1/taxonomy/movies", None)
        .await
        .assert_problem(StatusCode::NOT_FOUND, "not_found");
    post(&r, "/api/v1/taxonomy/labels", json!({ "name": " " }), Some(&member))
        .await
        .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
}

/// Language pickers search as you type: a code, or part of the English name or the name in the
/// language's own script; best matches first; the whole list is reachable (no 100-row cut-off).
#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn languages_are_searched_by_what_is_typed(pool: PgPool) {
    let r = router(&pool);
    let names = |v: &serde_json::Value| -> Vec<String> {
        v["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["name_en"].as_str().unwrap().to_owned())
            .collect()
    };
    let by = |q: &str| {
        let r = r.clone();
        let uri = format!("/api/v1/languages?q={q}");
        async move { get(&r, &uri, None).await.body }
    };
    let sans = by("sans").await;
    assert_eq!(names(&sans)[0], "Sanskrit");
    assert_eq!(sans["items"][0]["iso639_3"], "san");
    assert_eq!(sans["items"][0]["default_script"], "Deva");
    assert!(sans["next_cursor"].is_null());
    // A code finds its language first; a name in its own script works too.
    assert_eq!(names(&by("ml").await)[0], "Malayalam");
    assert_eq!(
        names(&by("%E0%B4%AE%E0%B4%B2%E0%B4%AF%E0%B4%BE%E0%B4%B3%E0%B4%82").await)[0],
        "Malayalam"
    );
    // Names that merely contain the text come after those that start with it; wildcards are plain text.
    assert!(
        names(&by("hin").await).iter().position(|n| n == "Hindi")
            < names(&by("hin").await)
                .iter()
                .position(|n| n == "Marathi")
                .or(Some(usize::MAX))
    );
    assert!(by("%25").await["items"].as_array().unwrap().is_empty());
    assert!(by("zzzzzz").await["items"].as_array().unwrap().is_empty());
    // Empty text lists by name, limited.
    let all = get(&r, "/api/v1/languages?q=&limit=5", None).await.body;
    assert_eq!(all["items"].as_array().unwrap().len(), 5);
    // The newer Indian and international languages are all there to find.
    for q in ["odia", "assamese", "pali", "vietnamese", "tulu", "santali"] {
        assert!(!by(q).await["items"].as_array().unwrap().is_empty(), "{q}");
    }
}
