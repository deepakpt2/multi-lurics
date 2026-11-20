//! A reader suggests a fix to a published lyric; a moderator applies or rejects it.

mod common;

use axum::http::StatusCode;
use common::*;
use domain::Role;
use serde_json::{json, Value};
use sqlx::PgPool;

fn id(v: &Value) -> String {
    v["id"].as_str().unwrap().to_owned()
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn readers_suggest_and_moderators_decide(pool: PgPool) {
    let r = router(&pool);
    let (_, author) = signed_in(&pool, &r, "a@example.com", Some(Role::Moderator)).await;
    let (_, reader) = signed_in(&pool, &r, "r@example.com", None).await;
    let song = post(&r, "/api/v1/compositions", json!({"title": "Kaatrin Mozhi"}), Some(&author)).await;
    let lyric = post(
        &r,
        &format!("/api/v1/compositions/{}/lyrics", id(&song.body)),
        json!({"type": "original", "language": "ml", "script": "Mlym", "body": "old line"}),
        Some(&author),
    )
    .await;
    assert_eq!(lyric.status, StatusCode::CREATED, "{}", lyric.body);
    let rec: String = sqlx::query_scalar("SELECT id::text FROM recordings LIMIT 1").fetch_one(&pool).await.unwrap();
    let c: String = sqlx::query_scalar("SELECT id::text FROM contributions WHERE recording_id = $1::uuid")
        .bind(&rec)
        .fetch_one(&pool)
        .await
        .unwrap();
    let uri = format!("/api/v1/contributions/{c}/corrections");

    post(&r, &uri, json!({"body": "new line"}), None).await.assert_problem(StatusCode::UNAUTHORIZED, "unauthenticated");
    let same = post(&r, &uri, json!({"body": "old line"}), Some(&reader)).await;
    assert_eq!(same.status, StatusCode::UNPROCESSABLE_ENTITY, "{}", same.body);
    let made = post(&r, &uri, json!({"body": "new line", "note": "typo"}), Some(&reader)).await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    let again = post(&r, &uri, json!({"body": "newer line"}), Some(&reader)).await;
    assert_eq!(again.status, StatusCode::UNPROCESSABLE_ENTITY, "one open suggestion each");

    // Nothing is public yet, and only moderators see the queue.
    let public = get(&r, &format!("/api/v1/contributions/{c}"), None).await;
    assert_eq!(public.body["published"]["body"], "old line");
    get(&r, "/api/v1/moderation/corrections", Some(&reader)).await.assert_problem(StatusCode::FORBIDDEN, "forbidden");
    let q = get(&r, "/api/v1/moderation/corrections", Some(&author)).await;
    assert_eq!(q.body["items"].as_array().unwrap().len(), 1);
    assert_eq!(q.body["items"][0]["current_body"], "old line");
    let k = id(&q.body["items"][0]);

    let done = post(&r, &format!("/api/v1/moderation/corrections/{k}/apply"), json!({}), Some(&author)).await;
    assert_eq!(done.status, StatusCode::NO_CONTENT, "{}", done.body);
    let public = get(&r, &format!("/api/v1/contributions/{c}"), None).await;
    assert_eq!(public.body["published"]["body"], "new line");
    assert_eq!(get(&r, "/api/v1/moderation/corrections", Some(&author)).await.body["items"].as_array().unwrap().len(), 0);

    // A rejected one leaves the lyric alone, and the reader may try again.
    post(&r, &uri, json!({"body": "wrong"}), Some(&reader)).await;
    let k2 = id(&get(&r, "/api/v1/moderation/corrections", Some(&author)).await.body["items"][0]);
    let rej = post(&r, &format!("/api/v1/moderation/corrections/{k2}/reject"), json!({"note": "no"}), Some(&author)).await;
    assert_eq!(rej.status, StatusCode::NO_CONTENT);
    let public = get(&r, &format!("/api/v1/contributions/{c}"), None).await;
    assert_eq!(public.body["published"]["body"], "new line");
}
