//! Use cases. Each opens one `AppTx`, enforces authorization, and writes audit/outbox
//! rows in the same transaction as the change.

pub mod admin;
pub mod auth;
pub mod authz;
pub mod browse;
pub mod catalogue;
pub mod community;
pub mod comparison;
pub mod entities;
pub mod error;
pub mod legal;
pub mod localization;
pub mod lyrics;
pub mod oauth;
pub mod pagination;
pub mod search;
pub mod seo;

pub use error::AppError;

use infrastructure::Config;
use sqlx::PgPool;
use std::sync::Arc;

/// Per-request metadata that use cases attach to audit rows.
#[derive(Debug, Clone, Default)]
pub struct RequestMeta {
    pub request_id: Option<String>,
    pub user_agent: Option<String>,
}

#[derive(Clone)]
pub struct App {
    pub pool: PgPool,
    pub config: Arc<Config>,
    /// The search engine behind the public search API; swappable (see `search::SearchService`).
    pub search: Arc<dyn search::SearchService>,
    /// Google sign-in backend; `None` when not configured. Swappable so tests need no network.
    pub google: Option<Arc<dyn oauth::IdentityProvider>>,
}

impl App {
    pub fn new(pool: PgPool, config: Arc<Config>) -> Self {
        let search = Arc::new(search::PgSearch::new(pool.clone()));
        let google = oauth::GoogleHttp::from_config(&config).map(|g| Arc::new(g) as Arc<dyn oauth::IdentityProvider>);
        Self {
            pool,
            config,
            search,
            google,
        }
    }

    pub fn with_google(mut self, google: Arc<dyn oauth::IdentityProvider>) -> Self {
        self.google = Some(google);
        self
    }

    pub fn with_search(mut self, search: Arc<dyn search::SearchService>) -> Self {
        self.search = search;
        self
    }
}
