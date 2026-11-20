//! Prometheus text metrics, kept in process. Labels are the route *template* (never the raw path),
//! so ids and query strings cannot blow up cardinality or leak into the metrics store.

use axum::extract::{MatchedPath, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::collections::BTreeMap;
use std::fmt::Write;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

const BUCKETS: [f64; 10] = [0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0];

#[derive(Default)]
struct Series {
    count: u64,
    sum: f64,
    buckets: [u64; BUCKETS.len()],
}

type Key = (String, String, u16);
static REGISTRY: OnceLock<Mutex<BTreeMap<Key, Series>>> = OnceLock::new();

fn registry() -> &'static Mutex<BTreeMap<Key, Series>> {
    REGISTRY.get_or_init(Mutex::default)
}

pub fn observe(method: &str, route: &str, status: u16, secs: f64) {
    let mut map = registry().lock().unwrap_or_else(|e| e.into_inner());
    let s = map.entry((method.to_owned(), route.to_owned(), status)).or_default();
    s.count += 1;
    s.sum += secs;
    for (i, le) in BUCKETS.iter().enumerate() {
        if secs <= *le {
            s.buckets[i] += 1;
        }
    }
}

pub async fn track(req: Request, next: Next) -> Response {
    let started = Instant::now();
    let method = req.method().as_str().to_owned();
    let route = req
        .extensions()
        .get::<MatchedPath>()
        .map_or_else(|| "unmatched".to_owned(), |m| m.as_str().to_owned());
    let res = next.run(req).await;
    // Fallback responses (404/405) never matched a route; the template is then "unmatched".
    observe(&method, &route, res.status().as_u16(), started.elapsed().as_secs_f64());
    res
}

pub fn render_http() -> String {
    let map = registry().lock().unwrap_or_else(|e| e.into_inner());
    let mut out = String::from(
        "# HELP lyrics_http_request_duration_seconds Request latency by route template.\n# TYPE lyrics_http_request_duration_seconds histogram\n",
    );
    for ((m, r, s), v) in map.iter() {
        let l = format!("method=\"{m}\",route=\"{r}\",status=\"{s}\"");
        for (i, le) in BUCKETS.iter().enumerate() {
            let _ = writeln!(
                out,
                "lyrics_http_request_duration_seconds_bucket{{{l},le=\"{le}\"}} {}",
                v.buckets[i]
            );
        }
        let _ = writeln!(
            out,
            "lyrics_http_request_duration_seconds_bucket{{{l},le=\"+Inf\"}} {}",
            v.count
        );
        let _ = writeln!(out, "lyrics_http_request_duration_seconds_sum{{{l}}} {}", v.sum);
        let _ = writeln!(out, "lyrics_http_request_duration_seconds_count{{{l}}} {}", v.count);
    }
    out
}

fn authorised(h: &HeaderMap, token: &str) -> bool {
    h.get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|t| {
            // Length-independent compare of equal-length digests is overkill here; compare bytes in constant time.
            t.len() == token.len() && t.bytes().zip(token.bytes()).fold(0u8, |a, (x, y)| a | (x ^ y)) == 0
        })
}

/// `GET /metrics`: 404 unless `LYRICS_METRICS_TOKEN` is set, 401 without it. Not part of the public
/// contract and never routed through the web proxy.
pub async fn scrape(State(app): State<application::App>, headers: HeaderMap) -> Response {
    let Some(token) = app.config.metrics_token.as_deref() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !authorised(&headers, token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let mut body = render_http();
    let _ = writeln!(body, "# TYPE lyrics_db_pool_connections gauge");
    let _ = writeln!(
        body,
        "lyrics_db_pool_connections{{state=\"total\"}} {}",
        app.pool.size()
    );
    let _ = writeln!(
        body,
        "lyrics_db_pool_connections{{state=\"idle\"}} {}",
        app.pool.num_idle()
    );
    match infrastructure::outbox::stats(&app.pool).await {
        Ok(s) => {
            let _ = writeln!(
                body,
                "# TYPE lyrics_outbox_pending gauge\nlyrics_outbox_pending {}",
                s.pending
            );
            let _ = writeln!(
                body,
                "# TYPE lyrics_outbox_retrying gauge\nlyrics_outbox_retrying {}",
                s.retrying
            );
            let _ = writeln!(
                body,
                "# TYPE lyrics_outbox_oldest_pending_seconds gauge\nlyrics_outbox_oldest_pending_seconds {}",
                s.oldest_secs
            );
            let _ = writeln!(body, "lyrics_up_database 1");
        }
        Err(_) => {
            let _ = writeln!(body, "lyrics_up_database 0");
        }
    }
    let mut res = body.into_response();
    res.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("text/plain; version=0.0.4"),
    );
    res.headers_mut()
        .insert(header::CACHE_CONTROL, header::HeaderValue::from_static("no-store"));
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn histogram_is_cumulative_and_labels_are_templates() {
        observe("GET", "/t/{id}", 200, 0.02);
        observe("GET", "/t/{id}", 200, 3.0);
        let text = render_http();
        assert!(text.contains("route=\"/t/{id}\",status=\"200\",le=\"0.025\"} 1"));
        assert!(text.contains("le=\"+Inf\"} 2"));
        assert!(!text.contains("secret-id"));
    }

    #[test]
    fn token_compare() {
        let mut h = HeaderMap::new();
        h.insert(header::AUTHORIZATION, "Bearer abc".parse().unwrap());
        assert!(authorised(&h, "abc"));
        assert!(!authorised(&h, "abd"));
        assert!(!authorised(&HeaderMap::new(), "abc"));
    }
}
