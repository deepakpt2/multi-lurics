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
    /// Requests per minute per client for login, registration and password reset.
    pub auth_rate_per_min: u32,
    /// Bearer token for `/metrics`; the endpoint answers 404 when unset.
    pub metrics_token: Option<String>,
    /// Outbound mail for the worker: `smtp://host:25` or `smtps://user:pass@host:465`, and the From address.
    pub smtp_url: Option<String>,
    pub mail_from: Option<String>,
    /// Where the worker tells the web app to drop cached pages (`http://web:3000/api/revalidate`), and the shared secret.
    pub revalidate_url: Option<String>,
    pub revalidate_secret: Option<String>,
    /// First administrator, created (or promoted) at API start-up. Both or neither.
    pub admin_email: Option<String>,
    pub admin_password: Option<String>,
    pub admin_display_name: String,
    /// Google sign-in (OpenID Connect). Both or neither; the feature is off when unset.
    pub google_client_id: Option<String>,
    pub google_client_secret: Option<String>,
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

        let auth_rate_per_min: u32 = parse_or(
            &mut errs,
            get("LYRICS_AUTH_RATE_PER_MIN"),
            "LYRICS_AUTH_RATE_PER_MIN",
            "20",
        )
        .unwrap_or(0);
        if auth_rate_per_min == 0 {
            errs.push("LYRICS_AUTH_RATE_PER_MIN must be at least 1".into());
        }
        let metrics_token = get("LYRICS_METRICS_TOKEN").filter(|t| !t.is_empty());
        if metrics_token.as_ref().is_some_and(|t| t.len() < 16) {
            errs.push("LYRICS_METRICS_TOKEN must be at least 16 characters".into());
        }

        let smtp_url = get("LYRICS_SMTP_URL").filter(|v| !v.is_empty());
        let mail_from = get("LYRICS_MAIL_FROM").filter(|v| !v.is_empty());
        match (&smtp_url, &mail_from) {
            (Some(u), Some(_)) if !(u.starts_with("smtp://") || u.starts_with("smtps://")) => {
                errs.push("LYRICS_SMTP_URL must start with smtp:// or smtps://".into());
            }
            (Some(_), None) | (None, Some(_)) => {
                errs.push("LYRICS_SMTP_URL and LYRICS_MAIL_FROM must be set together".into());
            }
            _ => {}
        }

        let revalidate_url = get("LYRICS_REVALIDATE_URL");
        let revalidate_secret = get("LYRICS_REVALIDATE_SECRET");
        match (&revalidate_url, &revalidate_secret) {
            (None, None) => {}
            (Some(u), Some(sec)) => {
                if !u.starts_with("http://") {
                    errs.push("LYRICS_REVALIDATE_URL must be an http:// URL on the internal network".into());
                }
                if sec.len() < 16 {
                    errs.push("LYRICS_REVALIDATE_SECRET must be at least 16 characters".into());
                }
            }
            _ => errs.push("LYRICS_REVALIDATE_URL and LYRICS_REVALIDATE_SECRET must be set together".into()),
        }

        let admin_email = get("LYRICS_ADMIN_EMAIL").filter(|v| !v.trim().is_empty());
        let admin_password = get("LYRICS_ADMIN_PASSWORD").filter(|v| !v.is_empty());
        if admin_email.is_some() != admin_password.is_some() {
            errs.push("LYRICS_ADMIN_EMAIL and LYRICS_ADMIN_PASSWORD must be set together".into());
        }
        let admin_display_name = get("LYRICS_ADMIN_NAME")
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| "Administrator".into());
        let google_client_id = get("LYRICS_GOOGLE_CLIENT_ID").filter(|v| !v.trim().is_empty());
        let google_client_secret = get("LYRICS_GOOGLE_CLIENT_SECRET").filter(|v| !v.trim().is_empty());
        if google_client_id.is_some() != google_client_secret.is_some() {
            errs.push("LYRICS_GOOGLE_CLIENT_ID and LYRICS_GOOGLE_CLIENT_SECRET must be set together".into());
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
            auth_rate_per_min,
            metrics_token,
            smtp_url,
            mail_from,
            revalidate_url,
            revalidate_secret,
            admin_email,
            admin_password,
            admin_display_name,
            google_client_id,
            google_client_secret,
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

    #[test]
    fn admin_and_google_settings_must_be_complete_pairs() {
        let mut base = vec![
            ("LYRICS_DATABASE_URL", "postgres://u:p@localhost/lyrics_dev"),
            ("LYRICS_APP_BASE_URL", "http://localhost:3000"),
        ];
        assert!(cfg(&base).unwrap().admin_email.is_none());
        base.push(("LYRICS_ADMIN_EMAIL", "a@b.co"));
        assert!(cfg(&base)
            .unwrap_err()
            .0
            .iter()
            .any(|e| e.contains("LYRICS_ADMIN_EMAIL and LYRICS_ADMIN_PASSWORD")));
        base.push(("LYRICS_ADMIN_PASSWORD", "x"));
        assert_eq!(cfg(&base).unwrap().admin_display_name, "Administrator");
        base.push(("LYRICS_GOOGLE_CLIENT_ID", "id"));
        assert!(cfg(&base)
            .unwrap_err()
            .0
            .iter()
            .any(|e| e.contains("LYRICS_GOOGLE_CLIENT_ID and")));
        base.push(("LYRICS_GOOGLE_CLIENT_SECRET", "s"));
        assert!(cfg(&base).unwrap().google_client_id.is_some());
    }
}
