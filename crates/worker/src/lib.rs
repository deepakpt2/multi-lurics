//! Outbox consumer skeleton: lease due events, dispatch by topic, record outcome.
//! Delivery is at-least-once; handlers must be idempotent.

use infrastructure::outbox::{self, OutboxEvent};
use sqlx::PgPool;
use std::path::PathBuf;

pub const BATCH_SIZE: i64 = 20;
pub const LEASE_SECS: i64 = 60;

#[async_trait::async_trait]
pub trait Handler: Send + Sync {
    async fn handle(&self, event: &OutboxEvent) -> anyhow::Result<()>;
}

/// Development mail sink: one JSON file per email in a directory (no real delivery).
/// Production delivery is a later phase.
pub struct DevMailbox {
    pub dir: PathBuf,
}

#[async_trait::async_trait]
impl Handler for DevMailbox {
    async fn handle(&self, e: &OutboxEvent) -> anyhow::Result<()> {
        // Published-lyrics events have no consumer yet (search indexing and cache purging come
        // later); acknowledge them rather than retrying forever.
        if e.topic.starts_with("lyrics.") {
            tracing::info!(id = %e.id, topic = %e.topic, aggregate_id = %e.aggregate_id, "event acknowledged (no consumer yet)");
            return Ok(());
        }
        if !e.topic.starts_with("email.") {
            anyhow::bail!("no handler for topic '{}'", e.topic);
        }
        tokio::fs::create_dir_all(&self.dir).await?;
        // Named by event id, so a redelivery overwrites instead of duplicating.
        let path = self.dir.join(format!("{}.json", e.id));
        let body = serde_json::json!({ "topic": e.topic, "created_at": e.created_at, "payload": e.payload });
        tokio::fs::write(path, serde_json::to_vec_pretty(&body)?).await?;
        Ok(())
    }
}

/// Without a mailbox dir, email events are acknowledged and only their metadata is logged
/// (payloads can hold single-use links and are never logged).
pub struct LogOnly;

#[async_trait::async_trait]
impl Handler for LogOnly {
    async fn handle(&self, e: &OutboxEvent) -> anyhow::Result<()> {
        tracing::info!(id = %e.id, topic = %e.topic, aggregate_id = %e.aggregate_id, "event handled (log only)");
        Ok(())
    }
}

/// Keeps `search_documents` current: `search.reindex` events rebuild one entity from the source
/// tables (idempotent, so redelivery is harmless); every other topic goes to `inner`.
pub struct SearchIndexer<H> {
    pub pool: PgPool,
    pub inner: H,
}

#[async_trait::async_trait]
impl<H: Handler> Handler for SearchIndexer<H> {
    async fn handle(&self, e: &OutboxEvent) -> anyhow::Result<()> {
        if e.topic != "search.reindex" {
            return self.inner.handle(e).await;
        }
        if !matches!(
            e.aggregate_type.as_str(),
            "composition" | "recording" | "artist" | "album"
        ) {
            anyhow::bail!("cannot index aggregate type '{}'", e.aggregate_type);
        }
        let mut conn = self.pool.acquire().await?;
        infrastructure::search::reindex(&mut conn, &e.aggregate_type, e.aggregate_id).await?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl Handler for Box<dyn Handler> {
    async fn handle(&self, e: &OutboxEvent) -> anyhow::Result<()> {
        (**self).handle(e).await
    }
}

/// Processes one batch. Returns how many events were leased.
pub async fn run_once(pool: &PgPool, handler: &dyn Handler) -> anyhow::Result<usize> {
    let events = outbox::claim(pool, BATCH_SIZE, LEASE_SECS).await?;
    for e in &events {
        match handler.handle(e).await {
            Ok(()) => outbox::mark_processed(pool, e.id).await?,
            Err(err) => {
                tracing::warn!(id = %e.id, topic = %e.topic, attempts = e.attempts, error = %err, "event failed");
                outbox::mark_failed(pool, e.id, e.attempts, &err.to_string()).await?;
            }
        }
    }
    Ok(events.len())
}
