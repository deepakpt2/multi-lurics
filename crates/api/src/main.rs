use application::App;
use infrastructure::db::{self, MigrationState};
use infrastructure::Config;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

const USAGE: &str = "usage: lyrics-api [serve | migrate | openapi]";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cmd = std::env::args().nth(1).unwrap_or_else(|| "serve".into());
    if cmd == "openapi" {
        println!("{}", api::openapi_json());
        return Ok(());
    }
    if !matches!(cmd.as_str(), "serve" | "migrate") {
        eprintln!("{USAGE}");
        std::process::exit(64);
    }

    let _ = dotenvy::dotenv();
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn"));
    if std::env::var("LYRICS_LOG_JSON").is_ok_and(|v| v == "1") {
        tracing_subscriber::fmt().json().with_env_filter(filter).init();
    } else {
        tracing_subscriber::fmt().with_env_filter(filter).init();
    }

    // Fail fast: refuse to start on any configuration problem.
    let config = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(78);
        }
    };
    let pool = db::connect(&config.database_url, config.db_max_connections).await?;

    if cmd == "migrate" {
        db::MIGRATOR.run(&pool).await?;
        tracing::info!(state = ?db::migration_state(&pool).await?, "migrations applied");
        return Ok(());
    }

    match db::migration_state(&pool).await? {
        MigrationState::UpToDate { version } => tracing::info!(version, "schema up to date"),
        other => tracing::warn!(state = ?other, "schema not current; /readyz will report 503"),
    }

    let addr = config.bind_addr;
    let app = App::new(pool, Arc::new(config));
    if matches!(db::migration_state(&app.pool).await?, MigrationState::UpToDate { .. }) {
        application::oauth::bootstrap_admin(&app)
            .await
            .map_err(|e| anyhow::anyhow!("first-admin bootstrap failed: {e}"))?;
    }
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "listening");
    axum::serve(listener, api::router(app))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
