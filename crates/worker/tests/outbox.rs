use infrastructure::outbox::{self, OutboxEvent};
use serde_json::json;
use sqlx::PgPool;
use std::collections::HashSet;
use uuid::Uuid;
use worker::{run_once, DevMailbox, Handler};

struct Failing;
#[async_trait::async_trait]
impl Handler for Failing {
    async fn handle(&self, _: &OutboxEvent) -> anyhow::Result<()> {
        anyhow::bail!("smtp down")
    }
}

async fn enqueue(pool: &PgPool, topic: &str) -> Uuid {
    let mut conn = pool.acquire().await.unwrap();
    outbox::enqueue(&mut conn, topic, "user", Uuid::new_v4(), json!({ "to": "a@b.co" }))
        .await
        .unwrap()
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn processes_event_once_and_writes_mailbox(pool: PgPool) {
    let id = enqueue(&pool, "email.verify_requested").await;
    let dir = std::env::temp_dir().join(format!("lyrics-mailbox-{}", Uuid::new_v4()));
    let h = DevMailbox { dir: dir.clone() };
    assert_eq!(run_once(&pool, &h).await.unwrap(), 1);
    assert!(dir.join(format!("{id}.json")).exists());
    let processed: bool = sqlx::query_scalar("SELECT processed_at IS NOT NULL FROM outbox WHERE id = $1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(processed);
    assert_eq!(
        run_once(&pool, &h).await.unwrap(),
        0,
        "processed events are not redelivered"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn failure_records_error_and_backs_off(pool: PgPool) {
    let id = enqueue(&pool, "email.verify_requested").await;
    assert_eq!(run_once(&pool, &Failing).await.unwrap(), 1);
    let (attempts, err, delayed): (i32, Option<String>, bool) = sqlx::query_as(
        "SELECT attempts, last_error, available_at > clock_timestamp() FROM outbox WHERE id = $1 AND processed_at IS NULL",
    ).bind(id).fetch_one(&pool).await.unwrap();
    assert_eq!((attempts, err.as_deref(), delayed), (1, Some("smtp down"), true));
    assert_eq!(
        run_once(&pool, &Failing).await.unwrap(),
        0,
        "not retried before backoff elapses"
    );
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn unknown_topic_is_not_silently_dropped(pool: PgPool) {
    let id = enqueue(&pool, "mystery.topic").await;
    let dir = std::env::temp_dir().join(format!("lyrics-mailbox-{}", Uuid::new_v4()));
    run_once(&pool, &DevMailbox { dir }).await.unwrap();
    let err: Option<String> =
        sqlx::query_scalar("SELECT last_error FROM outbox WHERE id = $1 AND processed_at IS NULL")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(err.unwrap().contains("no handler"));
}

#[sqlx::test(migrator = "infrastructure::MIGRATOR")]
async fn concurrent_claims_never_overlap(pool: PgPool) {
    for _ in 0..30 {
        enqueue(&pool, "email.verify_requested").await;
    }
    let claims = futures_join(&pool).await;
    let mut seen = HashSet::new();
    for batch in &claims {
        for e in batch {
            assert!(seen.insert(e.id), "event {} leased twice", e.id);
        }
    }
    assert_eq!(seen.len(), 30);
}

async fn futures_join(pool: &PgPool) -> Vec<Vec<OutboxEvent>> {
    let tasks: Vec<_> = (0..4)
        .map(|_| {
            let p = pool.clone();
            tokio::spawn(async move { outbox::claim(&p, 10, 60).await.unwrap() })
        })
        .collect();
    let mut out = Vec::new();
    for t in tasks {
        out.push(t.await.unwrap());
    }
    out
}
