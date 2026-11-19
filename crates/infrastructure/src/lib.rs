//! I/O adapters: configuration, Postgres (sqlx), audit log, outbox, crypto.

pub mod audit;
pub mod config;
pub mod db;
pub mod outbox;
pub mod repo;
pub mod security;

pub use config::{Config, ConfigError};
pub use db::{AppTx, MIGRATOR};
