//! Per-client token bucket for the typeahead endpoints. In-process: with several API replicas
//! each enforces its own bucket, which only loosens the limit, never tightens it for a client.

use crate::problem::ApiError;
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

const PRUNE_ABOVE: usize = 10_000;

pub struct RateLimiter {
    per_min: f64,
    buckets: Mutex<HashMap<String, (f64, Instant)>>,
}

impl RateLimiter {
    pub fn new(per_min: u32) -> Self {
        Self {
            per_min: f64::from(per_min.max(1)),
            buckets: Mutex::default(),
        }
    }

    /// `Err(seconds)` says how long until the client may try again.
    pub fn check(&self, key: &str) -> Result<(), u64> {
        let now = Instant::now();
        let rate = self.per_min / 60.0;
        let mut map = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        if map.len() > PRUNE_ABOVE {
            // A bucket that has had time to refill completely carries no information.
            map.retain(|_, (tokens, at)| *tokens + at.elapsed().as_secs_f64() * rate < self.per_min);
        }
        let (tokens, at) = map.entry(key.to_owned()).or_insert((self.per_min, now));
        *tokens = (*tokens + now.duration_since(*at).as_secs_f64() * rate).min(self.per_min);
        *at = now;
        if *tokens >= 1.0 {
            *tokens -= 1.0;
            Ok(())
        } else {
            Err((((1.0 - *tokens) / rate).ceil() as u64).max(1))
        }
    }
}

/// The caller as the edge proxy reports it. These headers are only trustworthy behind a proxy
/// that overwrites them (Cloudflare/Traefik here); direct exposure would let a client rotate them.
fn client_key(h: &HeaderMap) -> String {
    let first = |name: &str| {
        h.get(name)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(',').next())
            .map(|v| v.trim().chars().take(64).collect::<String>())
            .filter(|v| !v.is_empty())
    };
    first("cf-connecting-ip")
        .or_else(|| first("x-forwarded-for"))
        .unwrap_or_else(|| "unknown".into())
}

pub async fn limit(State(limiter): State<Arc<RateLimiter>>, req: Request, next: Next) -> Response {
    match limiter.check(&client_key(req.headers())) {
        Ok(()) => next.run(req).await,
        Err(wait) => {
            let mut res =
                ApiError::new(StatusCode::TOO_MANY_REQUESTS, "rate_limited", "Too many requests").into_response();
            res.headers_mut().insert(header::RETRY_AFTER, HeaderValue::from(wait));
            res.headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            res
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_allows_a_burst_then_refuses_and_isolates_clients() {
        let l = RateLimiter::new(3);
        assert!((0..3).all(|_| l.check("a").is_ok()));
        assert!(l.check("a").is_err());
        assert!(l.check("b").is_ok());
    }
}
