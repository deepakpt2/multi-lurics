//! I/O adapters: configuration, Postgres (sqlx), audit log, outbox, crypto.

pub mod audit;
pub mod catalogue;
pub mod community;
pub mod config;
pub mod db;
pub mod entities;
pub mod legal;
pub mod localization;
pub mod lyrics;
pub mod outbox;
pub mod repo;
pub mod search;
pub mod security;
pub mod seo;

pub use config::{Config, ConfigError};
pub use db::{AppTx, MIGRATOR};
