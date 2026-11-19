use crate::problem::ApiError;
use axum::extract::Request;
use axum::http::{HeaderName, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

pub static REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");
/// Required on state-changing requests. Browsers cannot attach a custom header
/// cross-origin without a CORS preflight (which this API never grants), so this
/// blocks CSRF on top of `SameSite=Lax` cookies.
pub static CSRF_HEADER: HeaderName = HeaderName::from_static("x-requested-with");

tokio::task_local! {
    static CURRENT_REQUEST_ID: String;
}

pub fn current_request_id() -> Option<String> {
    CURRENT_REQUEST_ID.try_with(Clone::clone).ok()
}

#[derive(Debug, Clone)]
pub struct RequestId(pub String);

fn acceptable(id: &str) -> bool {
    (1..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

/// Accepts a well-formed inbound `x-request-id` (e.g. from a proxy), otherwise mints one.
/// Echoed on the response and available to handlers, logs and problem bodies.
pub async fn request_id(mut req: Request, next: Next) -> Response {
    let id = req
        .headers()
        .get(&REQUEST_ID)
        .and_then(|v| v.to_str().ok())
        .filter(|v| acceptable(v))
        .map(str::to_owned)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let header = HeaderValue::from_str(&id).expect("validated or uuid");
    req.headers_mut().insert(REQUEST_ID.clone(), header.clone());
    req.extensions_mut().insert(RequestId(id.clone()));
    let mut res = CURRENT_REQUEST_ID.scope(id, next.run(req)).await;
    res.headers_mut().insert(REQUEST_ID.clone(), header);
    res
}

pub async fn csrf(req: Request, next: Next) -> Response {
    let safe = matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS);
    if safe || req.headers().contains_key(&CSRF_HEADER) {
        next.run(req).await
    } else {
        ApiError::new(
            StatusCode::FORBIDDEN,
            "csrf_header_missing",
            "Missing X-Requested-With header",
        )
        .into_response()
    }
}

/// Marks a response as private to the caller so no shared cache or CDN stores it.
pub async fn no_store(req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    res.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("private, no-store"),
    );
    res
}

#[cfg(test)]
mod tests {
    use super::acceptable;

    #[test]
    fn request_id_charset_and_length() {
        assert!(acceptable("abc-123_x.y"));
        assert!(!acceptable(""));
        assert!(!acceptable(&"a".repeat(65)));
        assert!(!acceptable("evil\nlog-injection"));
        assert!(!acceptable("spaces not ok"));
    }
}
