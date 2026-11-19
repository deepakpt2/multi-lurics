//! Use cases. Each opens one `AppTx`, enforces authorization, and writes audit/outbox
//! rows in the same transaction as the change.

pub mod admin;
pub mod auth;
pub mod authz;
pub mod catalogue;
pub mod entities;
pub mod error;
pub mod localization;
pub mod lyrics;
pub mod pagination;
pub mod search;

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
}

impl App {
    pub fn new(pool: PgPool, config: Arc<Config>) -> Self {
        let search = Arc::new(search::PgSearch::new(pool.clone()));
        Self { pool, config, search }
    }

    pub fn with_search(mut self, search: Arc<dyn search::SearchService>) -> Self {
        self.search = search;
        self
    }
}
