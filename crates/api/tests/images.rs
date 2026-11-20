//! Image upload, review and delivery: stored locally, processed (square JPEG sizes, metadata
//! stripped), moderated like lyrics, and shown with names in browse lists.

mod common;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use common::*;
use domain::Role;
use http_body_util::BodyExt;
use image::{DynamicImage, ImageFormat, RgbImage};
use serde_json::{json, Value};
use sqlx::PgPool;
use std::io::Cursor;
use tower::ServiceExt;
use uuid::Uuid;

fn png(w: u32, h: u32, seed: u8) -> Vec<u8> {
    let img = DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, y| {
        image::Rgb([(x as u8).wrapping_add(seed), y as u8, seed])
    }));
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, ImageFormat::Png).unwrap();
    out.into_inner()
}

struct Part<'a> {
    name: &'a str,
    file: Option<&'a [u8]>,
    text: &'a str,
}

async fn upload(r: &Router, cookie: Option<&str>, parts: &[Part<'_>]) -> Res {
    let boundary = "XBOUNDARYX";
    let mut body: Vec<u8> = Vec::new();
    for p in parts {
        body.extend(format!("--{boundary}\r\n").as_bytes());
        match p.file {
            Some(bytes) => {
                body.extend(
                    format!("Content-Disposition: form-data; name=\"{}\"; filename=\"x.bin\"\r\nContent-Type: application/octet-stream\r\n\r\n", p.name)
                        .as_bytes(),
                );
                body.extend(bytes);
            }
            None => {
                body.extend(format!("Content-Disposition: form-data; name=\"{}\"\r\n\r\n{}", p.name, p.text).as_bytes())
            }
        }
        body.extend(b"\r\n");
    }
    body.extend(format!("--{boundary}--\r\n").as_bytes());
    let mut b = Request::builder()
        .method(Method::POST)
        .uri("/api/v1/images")
        .header("x-requested-with", "test")
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        );
    if let Some(c) = cookie {
        b = b.header(header::COOKIE, c);
    }
    raw(r, b.body(Body::from(body)).unwrap()).await
}

fn form<'a>(file: &'a [u8], entity_type: &'a str, entity_id: &'a str) -> Vec<Part<'a>> {
    vec![
        Part {
            name: "file",
            file: Some(file),
            text: "",
        },
        Part {
            name: "rights",
            file: None,
            text: "true",
        },
        Part {
            name: "credit",
            file: None,
            text: "Photo by the artist",
        },
        Part {
            name: "entity_type",
            file: None,
            text: entity_type,
        },
        Part {
            name: "entity_id",
            file: None,
            text: entity_id,
        },
    ]
}

/// (status, content-type, cache-control, bytes)
async fn media(r: &Router, id: &str, width: u32, cookie: Option<&str>) -> (StatusCode, String, String, Vec<u8>) {
    let mut b = Request::builder().uri(format!("/api/v1/media/{id}/{width}"));
    if let Some(c) = cookie {
        b = b.header(header::COOKIE, c);
    }
    let res = r.clone().oneshot(b.body(Body::empty()).unwrap()).await.unwrap();
    let h = |n| {
        res.headers()
            .get(n)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_owned()
    };
    let (status, ct, cc) = (res.status(), h(header::CONTENT_TYPE), h(header::CACHE_CONTROL));
    (
        status,
        ct,
        cc,
        res.into_body().collect().await.unwrap().to_bytes().to_vec(),
    )
}

struct World {
    pool: PgPool,
    r: Router,
    member: String,
    other: String,
    moderator: String,
}

async fn world(pool: PgPool) -> World {
    let r = router(&pool);
    let (_, member) = signed_in(&pool, &r, "m1@example.com", None).await;
    let (_, other) = signed_in(&pool, &r, "m2@example.com", None).await;
    let (_, moderator) = signed_in(&pool, &r, "mod@example.com", Some(Role::Moderator)).await;
    World {
        pool,
        r,
        member,
        other,
        moderator,
    }
}

async fn artist(w: &World, name: &str) -> String {
    let res = post(
        &w.r,
        "/api/v1/artists",
        json!({ "name": name, "kind": "person" }),
        Some(&w.member),
    )
    .await;
    assert_eq!(res.status, StatusCode::CREATED, "{}", res.body);
    res.body["id"].as_str().unwrap().to_owned()
}

fn img_id(res: &Res) -> String {
    res.body["id"].as_str().unwrap().to_owned()
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn a_members_image_waits_for_review_and_is_private_until_approved(pool: PgPool) {
    let w = world(pool).await;
    let a = artist(&w, "Yesudas").await;
    let up = upload(&w.r, Some(&w.member), &form(&png(300, 200, 1), "artist", &a)).await;
    assert_eq!(up.status, StatusCode::CREATED, "{}", up.body);
    assert_eq!(up.body["status"], "pending");
    let id = img_id(&up);

    // Not public yet: only the uploader and moderators can see the file.
    assert_eq!(media(&w.r, &id, 64, None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(media(&w.r, &id, 64, Some(&w.other)).await.0, StatusCode::NOT_FOUND);
    let (st, ct, cc, bytes) = media(&w.r, &id, 64, Some(&w.member)).await;
    assert_eq!(
        (st, ct.as_str(), cc.as_str()),
        (StatusCode::OK, "image/jpeg", "private, no-store")
    );
    assert_eq!(image::guess_format(&bytes).unwrap(), ImageFormat::Jpeg);
    assert_eq!(media(&w.r, &id, 64, Some(&w.moderator)).await.0, StatusCode::OK);

    let state = get(&w.r, &format!("/api/v1/entities/artist/{a}/images"), Some(&w.member)).await;
    assert!(state.body["current"].is_null());
    assert_eq!(state.body["mine_pending"]["id"], id.as_str());

    // Members cannot review; moderators can.
    let denied = post(
        &w.r,
        &format!("/api/v1/images/{id}/approve"),
        json!({}),
        Some(&w.member),
    )
    .await;
    assert_eq!(denied.status, StatusCode::FORBIDDEN);
    let queue = get(&w.r, "/api/v1/moderation/images", Some(&w.moderator)).await;
    let item = &queue.body["items"][0];
    assert_eq!(
        (item["subject"].as_str(), item["image"]["id"].as_str()),
        (Some("Yesudas"), Some(id.as_str()))
    );
    assert_eq!(
        get(&w.r, "/api/v1/moderation/images", Some(&w.member)).await.status,
        StatusCode::FORBIDDEN
    );

    let ok = post(
        &w.r,
        &format!("/api/v1/images/{id}/approve"),
        json!({}),
        Some(&w.moderator),
    )
    .await;
    assert_eq!(ok.body["status"], "approved", "{}", ok.body);
    let (st, _, cc, _) = media(&w.r, &id, 64, None).await;
    assert_eq!(st, StatusCode::OK);
    assert!(cc.contains("immutable"), "{cc}");
    let state = get(&w.r, &format!("/api/v1/entities/artist/{a}/images"), None).await;
    assert_eq!(state.body["current"], id.as_str());
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn sizes_are_square_jpegs_and_a_request_gets_the_smallest_that_fits(pool: PgPool) {
    let w = world(pool).await;
    let a = artist(&w, "Chitra").await;
    let id = img_id(&upload(&w.r, Some(&w.moderator), &form(&png(400, 300, 2), "artist", &a)).await);
    for (asked, want) in [(1, 32), (32, 32), (50, 64), (100, 128), (5000, 128)] {
        let (st, _, _, bytes) = media(&w.r, &id, asked, None).await;
        assert_eq!(st, StatusCode::OK);
        let img = image::load_from_memory(&bytes).unwrap();
        assert_eq!((img.width(), img.height()), (want, want), "asked {asked}");
    }
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn a_moderators_upload_is_published_and_replaces_the_old_image(pool: PgPool) {
    let w = world(pool).await;
    let a = artist(&w, "Yesudas").await;
    let first = upload(&w.r, Some(&w.moderator), &form(&png(200, 200, 3), "artist", &a)).await;
    assert_eq!(first.body["status"], "approved", "{}", first.body);
    let first = img_id(&first);
    let second = upload(&w.r, Some(&w.moderator), &form(&png(200, 200, 4), "artist", &a)).await;
    let second = img_id(&second);
    assert_eq!(
        media(&w.r, &first, 32, None).await.0,
        StatusCode::NOT_FOUND,
        "the old file is gone"
    );
    assert_eq!(media(&w.r, &second, 32, None).await.0, StatusCode::OK);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM entity_images WHERE status = 'approved'")
        .fetch_one(&w.pool)
        .await
        .unwrap();
    assert_eq!(n, 1, "one public image per entity");
    // The same picture again is refused rather than queued.
    let again = upload(&w.r, Some(&w.member), &form(&png(200, 200, 4), "artist", &a)).await;
    again.assert_problem(StatusCode::CONFLICT, "image_already_current");
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn rejection_needs_a_reason_and_deletes_the_file(pool: PgPool) {
    let w = world(pool).await;
    let a = artist(&w, "Devarajan").await;
    let id = img_id(&upload(&w.r, Some(&w.member), &form(&png(200, 200, 5), "artist", &a)).await);
    let dup = upload(&w.r, Some(&w.member), &form(&png(200, 200, 6), "artist", &a)).await;
    dup.assert_problem(StatusCode::CONFLICT, "image_pending");
    let no_note = post(
        &w.r,
        &format!("/api/v1/images/{id}/reject"),
        json!({}),
        Some(&w.moderator),
    )
    .await;
    no_note.assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    let ok = post(
        &w.r,
        &format!("/api/v1/images/{id}/reject"),
        json!({ "note": "blurry" }),
        Some(&w.moderator),
    )
    .await;
    assert_eq!(ok.body["status"], "rejected");
    assert_eq!(media(&w.r, &id, 32, Some(&w.member)).await.0, StatusCode::NOT_FOUND);
    // After a rejection the person may try again.
    let retry = upload(&w.r, Some(&w.member), &form(&png(200, 200, 7), "artist", &a)).await;
    assert_eq!(retry.status, StatusCode::CREATED);
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn uploads_are_checked_by_content_size_and_consent(pool: PgPool) {
    let w = world(pool).await;
    let a = artist(&w, "Yesudas").await;
    let field_code = |res: &Res| res.body["errors"][0].clone();

    let svg = upload(
        &w.r,
        Some(&w.member),
        &form(
            b"<svg xmlns='http://www.w3.org/2000/svg' onload='alert(1)'/>",
            "artist",
            &a,
        ),
    )
    .await;
    svg.assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    assert_eq!(field_code(&svg), json!({ "field": "file", "code": "unsupported_type" }));

    let renamed = upload(&w.r, Some(&w.member), &form(b"GIF89a-not-really", "artist", &a)).await;
    assert_eq!(field_code(&renamed)["field"], "file");

    let tiny = upload(&w.r, Some(&w.member), &form(&png(20, 20, 1), "artist", &a)).await;
    assert_eq!(
        field_code(&tiny),
        json!({ "field": "file", "code": "dimensions_too_small" })
    );

    let huge = upload(&w.r, Some(&w.member), &form(&png(4100, 70, 1), "artist", &a)).await;
    assert_eq!(
        field_code(&huge),
        json!({ "field": "file", "code": "dimensions_too_large" })
    );

    let big = vec![0u8; 1024 * 1024 + 10];
    let too_big = upload(&w.r, Some(&w.member), &form(&big, "artist", &a)).await;
    assert!(
        too_big.status == StatusCode::PAYLOAD_TOO_LARGE || field_code(&too_big)["code"] == "too_large",
        "{} {}",
        too_big.status,
        too_big.body
    );

    let ok_png = png(200, 200, 1);
    let mut no_rights = form(&ok_png, "artist", &a);
    no_rights[1].text = "false";
    let r = upload(&w.r, Some(&w.member), &no_rights).await;
    assert_eq!(field_code(&r), json!({ "field": "rights", "code": "must_confirm" }));

    let anon = upload(&w.r, None, &form(&png(200, 200, 1), "artist", &a)).await;
    assert_eq!(anon.status, StatusCode::UNAUTHORIZED);
    let unknown = upload(
        &w.r,
        Some(&w.member),
        &form(&png(200, 200, 1), "artist", &Uuid::new_v4().to_string()),
    )
    .await;
    assert_eq!(unknown.status, StatusCode::NOT_FOUND);
    let tag = upload(&w.r, Some(&w.member), &form(&png(200, 200, 1), "tag", &a)).await;
    assert_eq!(field_code(&tag)["field"], "entity_type");
    let files: i64 = sqlx::query_scalar("SELECT count(*) FROM entity_images")
        .fetch_one(&w.pool)
        .await
        .unwrap();
    assert_eq!(files, 0, "nothing was stored for any refused upload");
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn metadata_is_stripped_by_re_encoding(pool: PgPool) {
    let w = world(pool).await;
    let a = artist(&w, "Yesudas").await;
    // A JPEG carrying a recognisable comment segment (stands in for EXIF/GPS).
    let mut jpg = Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(RgbImage::new(120, 120))
        .write_to(&mut jpg, ImageFormat::Jpeg)
        .unwrap();
    let mut jpg = jpg.into_inner();
    let marker = b"SECRET-GPS-12.34N";
    let mut seg = vec![0xFF, 0xFE, 0x00, (marker.len() + 2) as u8];
    seg.extend(marker);
    jpg.splice(2..2, seg);
    let id = img_id(&upload(&w.r, Some(&w.moderator), &form(&jpg, "artist", &a)).await);
    let (_, _, _, out) = media(&w.r, &id, 64, None).await;
    assert!(!out.windows(marker.len()).any(|x| x == marker));
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn a_new_entity_proposal_can_carry_an_image_decided_with_it(pool: PgPool) {
    let w = world(pool).await;
    let draft = upload(
        &w.r,
        Some(&w.member),
        &[
            Part {
                name: "file",
                file: Some(&png(200, 200, 8)),
                text: "",
            },
            Part {
                name: "rights",
                file: None,
                text: "true",
            },
        ],
    )
    .await;
    assert_eq!(draft.body["status"], "draft", "{}", draft.body);
    let id = img_id(&draft);
    // Someone else cannot attach it.
    let stolen = post(
        &w.r,
        "/api/v1/entities/proposals",
        json!({ "entity_type": "artist", "name": "K J Yesudas", "kind": "person", "image_id": id }),
        Some(&w.other),
    )
    .await;
    assert_eq!(stolen.status, StatusCode::UNPROCESSABLE_ENTITY);
    let p = post(
        &w.r,
        "/api/v1/entities/proposals",
        json!({ "entity_type": "artist", "name": "K J Yesudas", "kind": "person", "image_id": id }),
        Some(&w.member),
    )
    .await;
    assert_eq!(p.status, StatusCode::CREATED, "{}", p.body);
    let pid = p.body["id"].as_str().unwrap().to_owned();

    let q = get(&w.r, "/api/v1/moderation/entity-proposals", Some(&w.moderator)).await;
    assert_eq!(q.body["items"][0]["image_id"], id.as_str());
    let q = get(&w.r, "/api/v1/moderation/images", Some(&w.moderator)).await;
    assert_eq!(q.body["items"][0]["subject"], "K J Yesudas");
    // The image alone cannot be approved: it goes with its proposal.
    let alone = post(
        &w.r,
        &format!("/api/v1/images/{id}/approve"),
        json!({}),
        Some(&w.moderator),
    )
    .await;
    alone.assert_problem(StatusCode::CONFLICT, "decided_with_proposal");

    let ok = post(
        &w.r,
        &format!("/api/v1/entities/proposals/{pid}/approve"),
        json!({}),
        Some(&w.moderator),
    )
    .await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.body);
    let entity = ok.body["entity_id"].as_str().unwrap();
    let state = get(&w.r, &format!("/api/v1/entities/artist/{entity}/images"), None).await;
    assert_eq!(state.body["current"], id.as_str());
    assert_eq!(media(&w.r, &id, 32, None).await.0, StatusCode::OK);
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn rejecting_a_proposal_discards_its_image(pool: PgPool) {
    let w = world(pool).await;
    let draft = upload(
        &w.r,
        Some(&w.member),
        &[
            Part {
                name: "file",
                file: Some(&png(200, 200, 9)),
                text: "",
            },
            Part {
                name: "rights",
                file: None,
                text: "true",
            },
        ],
    )
    .await;
    let id = img_id(&draft);
    let p = post(
        &w.r,
        "/api/v1/entities/proposals",
        json!({ "entity_type": "album", "name": "Some Album", "kind": "album", "image_id": id }),
        Some(&w.member),
    )
    .await;
    let pid = p.body["id"].as_str().unwrap().to_owned();
    let no = post(
        &w.r,
        &format!("/api/v1/entities/proposals/{pid}/reject"),
        json!({ "note": "no" }),
        Some(&w.moderator),
    )
    .await;
    assert_eq!(no.status, StatusCode::OK, "{}", no.body);
    assert_eq!(media(&w.r, &id, 32, Some(&w.member)).await.0, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn an_unsent_or_waiting_image_can_be_withdrawn_by_its_owner_only(pool: PgPool) {
    let w = world(pool).await;
    let a = artist(&w, "Yesudas").await;
    let id = img_id(&upload(&w.r, Some(&w.member), &form(&png(200, 200, 1), "artist", &a)).await);
    let other = send(
        &w.r,
        Method::DELETE,
        &format!("/api/v1/images/{id}"),
        None,
        Some(&w.other),
    )
    .await;
    assert_eq!(other.status, StatusCode::NOT_FOUND);
    let own = send(
        &w.r,
        Method::DELETE,
        &format!("/api/v1/images/{id}"),
        None,
        Some(&w.member),
    )
    .await;
    assert_eq!(own.status, StatusCode::NO_CONTENT);
    assert_eq!(media(&w.r, &id, 32, Some(&w.member)).await.0, StatusCode::NOT_FOUND);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM entity_images")
        .fetch_one(&w.pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn a_merged_entity_hands_its_image_to_the_survivor_when_it_has_none(pool: PgPool) {
    let w = world(pool).await;
    let (_, admin) = signed_in(&w.pool, &w.r, "admin@example.com", Some(Role::Admin)).await;
    let from = artist(&w, "K J Yesudas").await;
    let into = artist(&w, "Yesudas Kattassery").await;
    let id = img_id(&upload(&w.r, Some(&w.moderator), &form(&png(200, 200, 1), "artist", &from)).await);
    let m = post(&w.r, "/api/v1/entities/merges", json!({ "entity_type": "artist", "from_id": from, "into_id": into, "expected_from_version": 1, "expected_into_version": 1 }), Some(&admin)).await;
    assert!(m.status.is_success(), "{}", m.body);
    let state = get(&w.r, &format!("/api/v1/entities/artist/{into}/images"), None).await;
    assert_eq!(state.body["current"], id.as_str());
    assert_eq!(media(&w.r, &id, 32, None).await.0, StatusCode::OK);
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn browse_lists_carry_the_public_image_of_each_entity(pool: PgPool) {
    let w = world(pool).await;
    let with = artist(&w, "Yesudas").await;
    let without = artist(&w, "Chitra").await;
    for (name, who) in [("Song A", &with), ("Song B", &without)] {
        let c = post(
            &w.r,
            "/api/v1/compositions",
            json!({ "title": name, "credits": [] }),
            Some(&w.member),
        )
        .await;
        let rec = post(&w.r, "/api/v1/recordings", json!({ "composition_id": c.body["id"], "title": name, "credits": [{ "artist_id": who, "role": "performer" }] }), Some(&w.member)).await;
        let p = post(
            &w.r,
            &format!("/api/v1/recordings/{}/contributions", rec.body["id"].as_str().unwrap()),
            json!({ "type": "original", "language": "ml", "script": "Mlym", "body": "വരികൾ" }),
            Some(&w.moderator),
        )
        .await;
        assert_eq!(p.body["status"], "published", "{}", p.body);
    }
    let id = img_id(&upload(&w.r, Some(&w.moderator), &form(&png(200, 200, 1), "artist", &with)).await);
    let facets = get(&w.r, "/api/v1/browse/facets/artist", None).await;
    assert_eq!(facets.status, StatusCode::OK, "{}", facets.body);
    let by_name = |n: &str| -> Value {
        facets.body["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["label"]["text"] == n)
            .unwrap()
            .clone()
    };
    assert_eq!(by_name("Yesudas")["image_id"], id.as_str());
    assert_eq!(by_name("Yesudas")["id"], with.as_str());
    assert!(by_name("Chitra")["image_id"].is_null());
    let songs = get(&w.r, "/api/v1/browse/songs?singer=yesudas", None).await;
    assert_eq!(
        songs.body["items"][0]["singers"][0]["image_id"],
        id.as_str(),
        "{}",
        songs.body
    );
}
