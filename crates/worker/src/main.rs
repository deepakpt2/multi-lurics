use infrastructure::{db, Config};
use std::time::Duration;
use tracing_subscriber::EnvFilter;
use worker::mail::Smtp as Mailer;
use worker::{DevMailbox, Handler, LogOnly, Revalidate, SearchIndexer};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let once = std::env::args().any(|a| a == "--once");
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn")))
        .init();

    let config = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(78);
        }
    };
    let pool = db::connect(&config.database_url, 2).await?;
    // Real delivery when SMTP is configured, the file sink for development, otherwise log only.
    let handler: Box<dyn Handler> = match (&config.smtp_url, &config.mail_from, &config.dev_mailbox_dir) {
        (Some(url), Some(from), _) => Box::new(Mailer::new(url, from)?),
        (_, _, Some(dir)) => Box::new(DevMailbox { dir: dir.into() }),
        _ => {
            tracing::warn!("no SMTP configured: emails are acknowledged but not delivered");
            Box::new(LogOnly)
        }
    };

    let revalidate = match (&config.revalidate_url, &config.revalidate_secret) {
        (Some(url), Some(secret)) => Some(Revalidate {
            url: url.clone(),
            secret: secret.clone(),
        }),
        _ => None,
    };
    let handler = SearchIndexer {
        pool: pool.clone(),
        inner: handler,
        revalidate,
    };
    let every = Duration::from_secs(config.home_refresh_secs);
    let mut next_refresh = tokio::time::Instant::now();
    loop {
        if tokio::time::Instant::now() >= next_refresh {
            // A failed refresh keeps the previous lists; try again next round.
            match worker::refresh_home(&pool).await {
                Ok(()) => tracing::info!("home lists refreshed"),
                Err(e) => tracing::error!(error = %e, "home list refresh failed"),
            }
            next_refresh = tokio::time::Instant::now() + every;
        }
        let n = worker::run_once(&pool, &handler).await?;
        if once {
            tracing::info!(processed = n, "single pass complete");
            return Ok(());
        }
        if n == 0 {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(2)) => {}
                _ = tokio::signal::ctrl_c() => return Ok(()),
            }
        }
    }
}
