use std::net::SocketAddr;
use url::Url;

/// Validated process configuration. Built once at startup; any problem aborts boot.
#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub bind_addr: SocketAddr,
    pub app_base_url: Url,
    pub session_ttl_hours: i64,
    pub cookie_secure: bool,
    pub db_max_connections: u32,
    pub dev_mailbox_dir: Option<String>,
    /// Requests per minute per client for typeahead endpoints.
    pub search_rate_per_min: u32,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("invalid configuration:\n  - {}", .0.join("\n  - "))]
pub struct ConfigError(pub Vec<String>);

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|k| std::env::var(k).ok())
    }

    /// All problems are collected and reported together so an operator can fix them in one pass.
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let mut errs = Vec::new();
        let get = |k: &str| get(k).filter(|v| !v.trim().is_empty());

        let database_url = match get("LYRICS_DATABASE_URL") {
            None => {
                errs.push("LYRICS_DATABASE_URL is required".into());
                String::new()
            }
            Some(raw) => {
                match Url::parse(&raw) {
                    Ok(u) if matches!(u.scheme(), "postgres" | "postgresql") => {
                        let db = u.path().trim_start_matches('/');
                        // Guard: this host also serves other projects' databases.
                        if !db.starts_with("lyrics_") {
                            errs.push(format!(
                                "LYRICS_DATABASE_URL must name a database starting with 'lyrics_' (got '{db}')"
                            ));
                        }
                    }
                    Ok(_) => errs.push("LYRICS_DATABASE_URL must use the postgres:// scheme".into()),
                    Err(e) => errs.push(format!(
                        "LYRICS_DATABASE_URL is not a valid URL ({e}); percent-encode special characters in the password"
                    )),
                }
                raw
            }
        };

        let bind_addr = parse_or(&mut errs, get("LYRICS_BIND_ADDR"), "LYRICS_BIND_ADDR", "127.0.0.1:8787")
            .unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], 8787)));

        let app_base_url = match get("LYRICS_APP_BASE_URL").map(|s| Url::parse(&s)) {
            None => {
                errs.push("LYRICS_APP_BASE_URL is required".into());
                None
            }
            Some(Ok(u)) if matches!(u.scheme(), "http" | "https") => Some(u),
            Some(_) => {
                errs.push("LYRICS_APP_BASE_URL must be an absolute http(s) URL".into());
                None
            }
        };

        let session_ttl_hours: i64 = parse_or(
            &mut errs,
            get("LYRICS_SESSION_TTL_HOURS"),
            "LYRICS_SESSION_TTL_HOURS",
            "336",
        )
        .unwrap_or(0);
        if !(1..=8760).contains(&session_ttl_hours) {
            errs.push("LYRICS_SESSION_TTL_HOURS must be between 1 and 8760".into());
        }
        let cookie_secure =
            parse_or(&mut errs, get("LYRICS_COOKIE_SECURE"), "LYRICS_COOKIE_SECURE", "true").unwrap_or(true);
        let db_max_connections: u32 = parse_or(
            &mut errs,
            get("LYRICS_DB_MAX_CONNECTIONS"),
            "LYRICS_DB_MAX_CONNECTIONS",
            "10",
        )
        .unwrap_or(0);
        if db_max_connections == 0 {
            errs.push("LYRICS_DB_MAX_CONNECTIONS must be at least 1".into());
        }

        let search_rate_per_min: u32 = parse_or(
            &mut errs,
            get("LYRICS_SEARCH_RATE_PER_MIN"),
            "LYRICS_SEARCH_RATE_PER_MIN",
            "120",
        )
        .unwrap_or(0);
        if search_rate_per_min == 0 {
            errs.push("LYRICS_SEARCH_RATE_PER_MIN must be at least 1".into());
        }

        if !errs.is_empty() {
            return Err(ConfigError(errs));
        }
        Ok(Self {
            database_url,
            bind_addr,
            app_base_url: app_base_url.expect("checked above"),
            session_ttl_hours,
            cookie_secure,
            db_max_connections,
            dev_mailbox_dir: get("LYRICS_DEV_MAILBOX_DIR"),
            search_rate_per_min,
        })
    }
}

fn parse_or<T: std::str::FromStr>(
    errs: &mut Vec<String>,
    value: Option<String>,
    key: &str,
    default: &str,
) -> Option<T> {
    let raw = value.unwrap_or_else(|| default.to_owned());
    let parsed = raw.trim().parse().ok();
    if parsed.is_none() {
        errs.push(format!("{key} has an invalid value '{raw}'"));
    }
    parsed
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn cfg(pairs: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let m: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        Config::from_lookup(|k| m.get(k).cloned())
    }

    const OK: &[(&str, &str)] = &[
        ("LYRICS_DATABASE_URL", "postgres://u:p%40ss@localhost:5432/lyrics_dev"),
        ("LYRICS_APP_BASE_URL", "http://localhost:3000"),
    ];

    #[test]
    fn minimal_valid_config_uses_secure_defaults() {
        let c = cfg(OK).unwrap();
        assert_eq!(c.bind_addr.to_string(), "127.0.0.1:8787");
        assert_eq!(c.session_ttl_hours, 336);
        assert!(c.cookie_secure);
    }

    #[test]
    fn missing_required_values_are_all_reported() {
        let ConfigError(errs) = cfg(&[]).unwrap_err();
        assert!(errs.iter().any(|e| e.contains("LYRICS_DATABASE_URL is required")));
        assert!(errs.iter().any(|e| e.contains("LYRICS_APP_BASE_URL is required")));
    }

    #[test]
    fn refuses_foreign_database() {
        let ConfigError(errs) = cfg(&[
            ("LYRICS_DATABASE_URL", "postgres://u:p@localhost/erp_dev"),
            ("LYRICS_APP_BASE_URL", "http://localhost:3000"),
        ])
        .unwrap_err();
        assert!(errs[0].contains("'lyrics_'"), "{errs:?}");
    }

    #[test]
    fn rejects_malformed_values() {
        let mut pairs = OK.to_vec();
        pairs.extend([
            ("LYRICS_BIND_ADDR", "not-an-addr"),
            ("LYRICS_SESSION_TTL_HOURS", "0"),
            ("LYRICS_COOKIE_SECURE", "maybe"),
            ("LYRICS_DB_MAX_CONNECTIONS", "0"),
        ]);
        let ConfigError(errs) = cfg(&pairs).unwrap_err();
        assert_eq!(errs.len(), 4, "{errs:?}");
    }

    #[test]
    fn rejects_non_http_base_url_and_non_postgres_scheme() {
        let ConfigError(errs) = cfg(&[
            ("LYRICS_DATABASE_URL", "mysql://localhost/lyrics_dev"),
            ("LYRICS_APP_BASE_URL", "ftp://x"),
        ])
        .unwrap_err();
        assert_eq!(errs.len(), 2, "{errs:?}");
    }
}
