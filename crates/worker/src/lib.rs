//! Outbox consumer skeleton: lease due events, dispatch by topic, record outcome.
//! Delivery is at-least-once; handlers must be idempotent.

pub mod mail;

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

/// Tells the web app which cached pages are stale. Plain HTTP on the internal network.
#[derive(Clone)]
pub struct Revalidate {
    pub url: String,
    pub secret: String,
}

impl Revalidate {
    pub async fn tags(&self, tags: &[String]) -> anyhow::Result<()> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let rest = self
            .url
            .strip_prefix("http://")
            .ok_or_else(|| anyhow::anyhow!("revalidate url must be http://"))?;
        let (host, path) = rest
            .split_once('/')
            .map_or((rest, "/".to_owned()), |(h, p)| (h, format!("/{p}")));
        let addr = if host.contains(':') {
            host.to_owned()
        } else {
            format!("{host}:80")
        };
        let body = serde_json::json!({ "tags": tags }).to_string();
        let request = format!(
            "POST {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nx-revalidate-secret: {}\r\nConnection: close\r\n\r\n{body}",
            body.len(),
            self.secret
        );
        let io = async {
            let mut stream = tokio::net::TcpStream::connect(&addr).await?;
            stream.write_all(request.as_bytes()).await?;
            let mut head = Vec::new();
            stream.take(4096).read_to_end(&mut head).await?;
            anyhow::Ok(head)
        };
        let head = tokio::time::timeout(std::time::Duration::from_secs(5), io).await??;
        let status = String::from_utf8_lossy(&head);
        let code = status.split_whitespace().nth(1).unwrap_or("");
        anyhow::ensure!(code.starts_with('2'), "revalidate answered {code}");
        Ok(())
    }
}

/// Cache tags that identify every page address of an entity: one per slug it ever had, per locale
/// (a visitor may ask for any slug under any locale), plus the entity and the sitemap.
pub fn page_tags(
    locales: &[String],
    entity_type: &str,
    id: uuid::Uuid,
    slugs: &[infrastructure::seo::SlugRow],
) -> Vec<String> {
    let scope = infrastructure::seo::scope_of(entity_type);
    let mut tags = vec![format!("entity:{id}"), "sitemap".to_owned()];
    for s in slugs {
        for l in locales {
            tags.push(format!("slug:{scope}:{l}:{}", s.slug));
        }
    }
    tags
}

/// Keeps the public read models current: `search.reindex` events rebuild one entity's search
/// documents and its URL slugs from the source tables (idempotent, so redelivery is harmless),
/// then drop the cached pages that depend on it. Every other topic goes to `inner`.
pub struct SearchIndexer<H> {
    pub pool: PgPool,
    pub inner: H,
    pub revalidate: Option<Revalidate>,
}

#[async_trait::async_trait]
impl<H: Handler> Handler for SearchIndexer<H> {
    async fn handle(&self, e: &OutboxEvent) -> anyhow::Result<()> {
        if e.topic != "search.reindex" {
            return self.inner.handle(e).await;
        }
        if !matches!(
            e.aggregate_type.as_str(),
            "composition" | "recording" | "artist" | "album" | "raga" | "deity" | "tag"
        ) {
            anyhow::bail!("cannot index aggregate type '{}'", e.aggregate_type);
        }
        let mut tx = self.pool.begin().await?;
        infrastructure::search::reindex(&mut tx, &e.aggregate_type, e.aggregate_id).await?;
        let pages = matches!(e.aggregate_type.as_str(), "composition" | "recording" | "artist");
        let slugs = if pages {
            infrastructure::seo::sync(&mut tx, &e.aggregate_type, e.aggregate_id).await?
        } else {
            Vec::new()
        };
        // Browse filters address artists and albums by a name-based slug; a rename retires the old one.
        if matches!(e.aggregate_type.as_str(), "artist" | "album" | "raga" | "deity" | "tag") {
            infrastructure::browse::sync_slug(&mut tx, &e.aggregate_type, e.aggregate_id).await?;
        }
        let cfg = infrastructure::seo::load_config(&mut tx).await?;
        tx.commit().await?;
        // After the commit: a page re-rendered now sees the new state. A failure retries the event.
        if let (Some(r), true) = (&self.revalidate, pages) {
            r.tags(&page_tags(&cfg.locales, &e.aggregate_type, e.aggregate_id, &slugs))
                .await?;
        }
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

/// Rebuilds the home page lists (popular and new songs, albums, artists) and clears expired
/// view-dedupe rows. The formula lives in `infrastructure::home`.
pub async fn refresh_home(pool: &PgPool) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    infrastructure::home::refresh(&mut tx).await?;
    infrastructure::home::purge_seen(&mut tx).await?;
    tx.commit().await?;
    Ok(())
}
