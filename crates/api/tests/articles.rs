//! Articles: moderators write them, everyone reads the published ones, and a moderator decides
//! whether (and for how long) one is shown on the home page.

mod common;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use common::*;
use domain::Role;
use http_body_util::BodyExt;
use serde_json::json;
use sqlx::PgPool;
use tower::ServiceExt;

fn png() -> Vec<u8> {
    use image::{DynamicImage, ImageFormat, RgbImage};
    let img = DynamicImage::ImageRgb8(RgbImage::from_fn(300, 150, |x, y| {
        image::Rgb([(x % 255) as u8, (y % 255) as u8, 90])
    }));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, ImageFormat::Png).unwrap();
    out.into_inner()
}

fn multipart(bytes: &[u8]) -> (String, Vec<u8>) {
    let b = "XBOUNDARYX";
    let mut body = format!(
        "--{b}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.png\"\r\nContent-Type: image/png\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{b}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={b}"), body)
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn moderators_write_articles_and_choose_what_the_home_page_shows(pool: PgPool) {
    let r = router(&pool);
    let (_, member) = signed_in(&pool, &r, "m@example.com", None).await;
    let (_, moderator) = signed_in(&pool, &r, "mod@example.com", Some(Role::Moderator)).await;
    let body =
        json!({"title": "Onam 2026", "body_md": "# Happy Onam\n\nSpecial songs this week.", "summary": "Celebrate"});

    // Members cannot write.
    post(&r, "/api/v1/articles", body.clone(), Some(&member))
        .await
        .assert_problem(StatusCode::FORBIDDEN, "forbidden");
    post(&r, "/api/v1/articles", body.clone(), None)
        .await
        .assert_problem(StatusCode::UNAUTHORIZED, "unauthenticated");

    // A draft is invisible to the public, visible to moderators.
    let made = post(&r, "/api/v1/articles", body.clone(), Some(&moderator)).await;
    assert_eq!(made.status, StatusCode::CREATED);
    let a = made.body.clone();
    assert_eq!(a["slug"], "onam-2026");
    assert_eq!(a["status"], "draft");
    let id = a["id"].as_str().unwrap().to_owned();
    assert_eq!(
        get(&r, "/api/v1/articles/onam-2026", None).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(&r, "/api/v1/articles/onam-2026", Some(&moderator)).await.status,
        StatusCode::OK
    );
    assert_eq!(
        get(&r, "/api/v1/articles", None).await.body["items"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    // The same title gets another address; an explicit address that is taken is refused.
    let second = post(&r, "/api/v1/articles", body.clone(), Some(&moderator)).await;
    assert_eq!(second.body["slug"], "onam-2026-2");
    post(
        &r,
        "/api/v1/articles",
        json!({"title": "x", "body_md": "y", "slug": "onam-2026"}),
        Some(&moderator),
    )
    .await
    .assert_problem(StatusCode::CONFLICT, "slug_taken");
    post(
        &r,
        "/api/v1/articles",
        json!({"title": "x", "body_md": " "}),
        Some(&moderator),
    )
    .await
    .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");

    // A draft cannot go on the home page.
    let home = |on: bool| json!({"on_home": on});
    send(
        &r,
        Method::PUT,
        &format!("/api/v1/articles/{id}/home"),
        Some(home(true)),
        Some(&moderator),
    )
    .await
    .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");

    // Publishing makes it public; the home page shows nothing until it is switched on.
    let published = send(
        &r,
        Method::PATCH,
        &format!("/api/v1/articles/{id}"),
        Some(json!({"expected_version": 1, "publish": true})),
        Some(&moderator),
    )
    .await;
    assert_eq!(published.status, StatusCode::OK, "{:?}", published.body);
    assert_eq!(published.body["status"], "published");
    assert_eq!(
        get(&r, "/api/v1/articles/onam-2026", None).await.body["body_md"],
        "# Happy Onam\n\nSpecial songs this week."
    );
    assert_eq!(
        get(&r, "/api/v1/articles", None).await.body["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        get(&r, "/api/v1/articles/home", None).await.body["items"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    // A stale version is refused.
    send(
        &r,
        Method::PATCH,
        &format!("/api/v1/articles/{id}"),
        Some(json!({"expected_version": 1, "title": "New"})),
        Some(&moderator),
    )
    .await
    .assert_problem(StatusCode::CONFLICT, "version_conflict");

    // Switch on for a period: shown now; a window in the past or one not started yet hides it.
    let on = send(
        &r,
        Method::PUT,
        &format!("/api/v1/articles/{id}/home"),
        Some(home(true)),
        Some(&moderator),
    )
    .await;
    assert_eq!(on.body["home_now"], true);
    assert_eq!(
        get(&r, "/api/v1/articles/home", None).await.body["items"][0]["slug"],
        "onam-2026"
    );
    let later = (chrono::Utc::now() + chrono::Duration::days(2)).to_rfc3339();
    let notyet = send(
        &r,
        Method::PUT,
        &format!("/api/v1/articles/{id}/home"),
        Some(json!({"on_home": true, "home_from": later})),
        Some(&moderator),
    )
    .await;
    assert_eq!(notyet.body["home_now"], false);
    assert_eq!(
        get(&r, "/api/v1/articles/home", None).await.body["items"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    let past = (chrono::Utc::now() - chrono::Duration::days(1)).to_rfc3339();
    send(
        &r,
        Method::PUT,
        &format!("/api/v1/articles/{id}/home"),
        Some(json!({"on_home": true, "home_until": past})),
        Some(&moderator),
    )
    .await
    .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");

    // A period that has ended hides it again by itself.
    sqlx::query("UPDATE articles SET on_home = true, home_from = now() - interval '3 days', home_until = now() - interval '1 day' WHERE id = $1::uuid")
        .bind(&id).execute(&pool).await.unwrap();
    assert_eq!(
        get(&r, "/api/v1/articles/home", None).await.body["items"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    // Back to a draft takes it off the home page too.
    send(
        &r,
        Method::PUT,
        &format!("/api/v1/articles/{id}/home"),
        Some(home(true)),
        Some(&moderator),
    )
    .await;
    let v = get(&r, "/api/v1/articles/onam-2026", None).await.body["version"]
        .as_i64()
        .unwrap();
    let drafted = send(
        &r,
        Method::PATCH,
        &format!("/api/v1/articles/{id}"),
        Some(json!({"expected_version": v, "publish": false})),
        Some(&moderator),
    )
    .await;
    assert_eq!(drafted.body["on_home"], false);
    assert_eq!(
        get(&r, "/api/v1/articles/onam-2026", None).await.status,
        StatusCode::NOT_FOUND
    );

    // Deleting removes it.
    assert_eq!(
        send(
            &r,
            Method::DELETE,
            &format!("/api/v1/articles/{id}"),
            None,
            Some(&moderator)
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        get(&r, "/api/v1/articles/onam-2026", Some(&moderator)).await.status,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn article_pictures_keep_their_proportions_and_are_public(pool: PgPool) {
    let r = router(&pool);
    let (_, member) = signed_in(&pool, &r, "m@example.com", None).await;
    let (_, moderator) = signed_in(&pool, &r, "mod@example.com", Some(Role::Moderator)).await;
    let a = post(
        &r,
        "/api/v1/articles",
        json!({"title": "Pics", "body_md": "x", "publish": true}),
        Some(&moderator),
    )
    .await
    .body;
    let id = a["id"].as_str().unwrap();

    let (ct, body) = multipart(&png());
    let upload = |cookie: String, ct: String, body: Vec<u8>| {
        let req = Request::builder()
            .method(Method::POST)
            .uri(format!("/api/v1/articles/{id}/images"))
            .header("content-type", ct)
            .header("cookie", cookie)
            .header("x-requested-with", "fetch")
            .body(Body::from(body))
            .unwrap();
        req
    };
    assert_eq!(
        raw(&r, upload(member.clone(), ct.clone(), body.clone())).await.status,
        StatusCode::FORBIDDEN
    );
    let up = raw(&r, upload(moderator.clone(), ct, body)).await;
    assert_eq!(up.status, StatusCode::CREATED, "{:?}", up.body);
    let image = up.body["id"].as_str().unwrap().to_owned();
    assert_eq!(
        (up.body["width"].as_i64(), up.body["height"].as_i64()),
        (Some(300), Some(150))
    );
    assert!(up.body["markdown"].as_str().unwrap().contains(&image));

    // Anyone can fetch it without signing in, and it is a wide JPEG, not a square.
    let res = r
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/media/{image}/300"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let res = (status, bytes);
    assert_eq!(res.0, StatusCode::OK);
    let img = image::load_from_memory(&res.1).unwrap();
    assert_eq!((img.width(), img.height()), (300, 150));

    // It can be the cover; a stranger's picture cannot.
    let v = a["version"].as_i64().unwrap();
    let cover = send(
        &r,
        Method::PATCH,
        &format!("/api/v1/articles/{id}"),
        Some(json!({"expected_version": v, "cover_image_id": image})),
        Some(&moderator),
    )
    .await;
    assert_eq!(cover.body["cover_image_id"], image.as_str());
    let v = cover.body["version"].as_i64().unwrap();
    send(
        &r,
        Method::PATCH,
        &format!("/api/v1/articles/{id}"),
        Some(json!({"expected_version": v, "cover_image_id": uuid::Uuid::new_v4()})),
        Some(&moderator),
    )
    .await
    .assert_problem(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");

    // Removing the picture clears the cover and the file.
    assert_eq!(
        send(
            &r,
            Method::DELETE,
            &format!("/api/v1/articles/{id}/images/{image}"),
            None,
            Some(&moderator)
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        get(&r, &format!("/api/v1/media/{image}/300"), None).await.status,
        StatusCode::NOT_FOUND
    );
    assert!(get(&r, "/api/v1/articles/pics", None).await.body["cover_image_id"].is_null());
}
