//! Real SMTP delivery against an in-process fake server: the mail arrives with the link, a dead
//! server leaves the event queued for retry, and content events never reach the mailer.

use infrastructure::outbox;
use serde_json::json;
use sqlx::PgPool;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use uuid::Uuid;
use worker::mail::Smtp;
use worker::run_once;

/// Speaks just enough SMTP to accept one message and return everything the client sent as DATA.
async fn fake_smtp() -> (u16, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = tokio::spawn(async move {
        let (sock, _) = listener.accept().await.unwrap();
        let (r, mut w) = sock.into_split();
        let mut r = BufReader::new(r);
        w.write_all(b"220 fake ESMTP\r\n").await.unwrap();
        let mut data = String::new();
        let mut in_data = false;
        let mut line = String::new();
        loop {
            line.clear();
            if r.read_line(&mut line).await.unwrap() == 0 {
                break;
            }
            if in_data {
                if line == ".\r\n" {
                    in_data = false;
                    w.write_all(b"250 queued\r\n").await.unwrap();
                } else {
                    data.push_str(&line);
                }
                continue;
            }
            let cmd = line.trim().to_uppercase();
            let reply: &[u8] = if cmd.starts_with("EHLO") || cmd.starts_with("HELO") {
                b"250 fake\r\n"
            } else if cmd.starts_with("DATA") {
                in_data = true;
                b"354 go\r\n"
            } else if cmd.starts_with("QUIT") {
                w.write_all(b"221 bye\r\n").await.unwrap();
                break;
            } else {
                b"250 ok\r\n"
            };
            w.write_all(reply).await.unwrap();
        }
        data
    });
    (port, task)
}

async fn enqueue(pool: &PgPool, topic: &str, payload: serde_json::Value) -> Uuid {
    let mut c = pool.acquire().await.unwrap();
    outbox::enqueue(&mut c, topic, "user", Uuid::new_v4(), payload)
        .await
        .unwrap()
}

async fn processed(pool: &PgPool, id: Uuid) -> bool {
    sqlx::query_scalar("SELECT processed_at IS NOT NULL FROM outbox WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn verification_mail_is_delivered_with_its_link(pool: PgPool) {
    let (port, server) = fake_smtp().await;
    let smtp = Smtp::new(&format!("smtp://127.0.0.1:{port}"), "Lyrics <no-reply@example.org>").unwrap();
    let id = enqueue(
        &pool,
        "email.verify_requested",
        json!({ "to": "reader@example.org", "display_name": "Reader", "locale": "en", "link": "https://lyrics.example/en/verify#token=abc123" }),
    )
    .await;
    assert_eq!(run_once(&pool, &smtp).await.unwrap(), 1);
    let sent = server.await.unwrap();
    assert!(processed(&pool, id).await);
    assert!(sent.contains("Subject: Confirm your email"), "{sent}");
    assert!(sent.contains("https://lyrics.example/en/verify#token=abc123"), "{sent}");
    assert!(sent.contains("reader@example.org"));
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn an_unreachable_server_keeps_the_event_queued_and_content_events_are_not_mailed(pool: PgPool) {
    // Nothing listens on this port: the send fails, the event stays for retry with a recorded error.
    let closed = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = closed.local_addr().unwrap().port();
    drop(closed);
    let smtp = Smtp::new(&format!("smtp://127.0.0.1:{port}"), "no-reply@example.org").unwrap();
    let mail = enqueue(
        &pool,
        "email.password_reset_requested",
        json!({ "to": "a@b.co", "locale": "ar", "link": "l" }),
    )
    .await;
    let content = enqueue(&pool, "lyrics.published", json!({})).await;
    run_once(&pool, &smtp).await.unwrap();
    assert!(!processed(&pool, mail).await, "a failed send must not be marked done");
    let err: Option<String> = sqlx::query_scalar("SELECT last_error FROM outbox WHERE id = $1")
        .bind(mail)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(err.is_some());
    assert!(
        processed(&pool, content).await,
        "content events are acknowledged, not mailed"
    );
}
