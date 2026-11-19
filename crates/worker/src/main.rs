use infrastructure::{db, Config};
use std::time::Duration;
use tracing_subscriber::EnvFilter;
use worker::{DevMailbox, Handler, LogOnly};

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
    let handler: Box<dyn Handler> = match &config.dev_mailbox_dir {
        Some(dir) => Box::new(DevMailbox { dir: dir.into() }),
        None => Box::new(LogOnly),
    };

    loop {
        let n = worker::run_once(&pool, handler.as_ref()).await?;
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
