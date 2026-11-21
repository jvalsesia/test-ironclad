//! Request ID and request logging: the outermost HTTP layer.
//!
//! Every request gets an ID (a valid incoming `X-Request-Id` or a fresh
//! UUID v4), runs inside a `request` span carrying it, and yields one
//! completion log line. The ID is echoed in the `X-Request-Id` response
//! header. Query strings and bodies are never logged.

use std::time::Instant;

use axum::extract::Request;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use tracing::{Instrument, Level, debug, error, info, warn};
use uuid::Uuid;

/// Name of the request/response header carrying the request ID.
pub const REQUEST_ID_HEADER: &str = "x-request-id";

/// Longest accepted client-supplied request ID, in characters.
const MAX_REQUEST_ID_LEN: usize = 128;

/// The ID of the current request, available to handlers as an extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestId(pub String);

/// A client ID is reusable when it is 1-128 characters of `[A-Za-z0-9._-]`.
pub fn is_valid_request_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_REQUEST_ID_LEN
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// Reuses the first `X-Request-Id` header when valid, otherwise generates a
/// UUID v4. Invalid values are replaced, never rejected.
pub fn resolve_request_id(headers: &HeaderMap) -> String {
    headers
        .get(REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .filter(|value| is_valid_request_id(value))
        .map_or_else(|| Uuid::new_v4().to_string(), str::to_owned)
}

/// Level of the completion line: `5xx` error, `4xx` warn, otherwise info;
/// a successful `GET /health` is debug so probes do not flood the logs.
pub fn completion_level(method: &Method, path: &str, status: StatusCode) -> Level {
    if status.is_server_error() {
        Level::ERROR
    } else if status.is_client_error() {
        Level::WARN
    } else if method == Method::GET && path == "/health" {
        Level::DEBUG
    } else {
        Level::INFO
    }
}

/// Middleware: resolve the ID, open the span, echo the header, log once.
pub async fn request_context(mut request: Request, next: Next) -> Response {
    let started = Instant::now();
    let request_id = resolve_request_id(request.headers());
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    request
        .extensions_mut()
        .insert(RequestId(request_id.clone()));

    // Error level so the span (and with it `span.request_id` on every event
    // of the request) survives even a `LOG_LEVEL=error` filter.
    let span = tracing::error_span!("request", request_id = %request_id);
    async move {
        let mut response = next.run(request).await;
        if let Ok(value) = HeaderValue::from_str(&request_id) {
            response.headers_mut().insert(REQUEST_ID_HEADER, value);
        }
        let status = response.status();
        let latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let code = status.as_u16();
        let verb = method.as_str();
        match completion_level(&method, &path, status) {
            Level::ERROR => error!(
                request_id = %request_id, method = verb, path = %path, status = code,
                latency_ms, "request completed"
            ),
            Level::WARN => warn!(
                request_id = %request_id, method = verb, path = %path, status = code,
                latency_ms, "request completed"
            ),
            Level::DEBUG => debug!(
                request_id = %request_id, method = verb, path = %path, status = code,
                latency_ms, "request completed"
            ),
            _ => info!(
                request_id = %request_id, method = verb, path = %path, status = code,
                latency_ms, "request completed"
            ),
        }
        response
    }
    .instrument(span)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers_with(values: &[&str]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for value in values {
            map.append(
                REQUEST_ID_HEADER,
                HeaderValue::from_bytes(value.as_bytes()).unwrap(),
            );
        }
        map
    }

    fn is_uuid_v4(value: &str) -> bool {
        Uuid::parse_str(value).is_ok_and(|id| id.get_version_num() == 4)
    }

    #[test]
    fn valid_ids_are_reused() {
        let long = "a".repeat(128);
        for id in ["client-abc-123", "a", long.as_str(), "A.b_c-1"] {
            assert_eq!(resolve_request_id(&headers_with(&[id])), id);
        }
    }

    #[test]
    fn invalid_ids_are_replaced() {
        let l129 = "a".repeat(129);
        let l200 = "a".repeat(200);
        for id in [
            "",
            l129.as_str(),
            l200.as_str(),
            "has space",
            "bad/id",
            "bad!",
            "caf\u{e9}",
        ] {
            let resolved = resolve_request_id(&headers_with(&[id]));
            assert!(is_uuid_v4(&resolved), "{id:?} -> {resolved}");
            assert_ne!(resolved, id);
        }
    }

    #[test]
    fn missing_header_generates_an_id() {
        assert!(is_uuid_v4(&resolve_request_id(&HeaderMap::new())));
    }

    #[test]
    fn first_header_wins() {
        assert_eq!(
            resolve_request_id(&headers_with(&["first", "second"])),
            "first"
        );
    }

    #[test]
    fn generated_ids_are_unique_uuid_v4() {
        let ids: std::collections::HashSet<String> = (0..100)
            .map(|_| resolve_request_id(&HeaderMap::new()))
            .collect();
        assert_eq!(ids.len(), 100);
        assert!(ids.iter().all(|id| is_uuid_v4(id)));
    }

    #[test]
    fn level_mapping_by_status() {
        let expected = [
            (200, Level::INFO),
            (301, Level::INFO),
            (404, Level::WARN),
            (405, Level::WARN),
            (413, Level::WARN),
            (422, Level::WARN),
            (500, Level::ERROR),
            (503, Level::ERROR),
        ];
        for (status, level) in expected {
            let status = StatusCode::from_u16(status).unwrap();
            assert_eq!(completion_level(&Method::GET, "/tasks", status), level);
        }
    }

    #[test]
    fn health_success_is_debug() {
        assert_eq!(
            completion_level(&Method::GET, "/health", StatusCode::OK),
            Level::DEBUG
        );
    }

    #[test]
    fn health_failure_keeps_normal_level() {
        assert_eq!(
            completion_level(&Method::GET, "/health", StatusCode::SERVICE_UNAVAILABLE),
            Level::ERROR
        );
    }

    #[test]
    fn health_rule_is_exact_and_get_only() {
        for (method, path) in [
            (Method::GET, "/healthz"),
            (Method::POST, "/health"),
            (Method::GET, "/health/x"),
        ] {
            assert_eq!(
                completion_level(&method, path, StatusCode::OK),
                Level::INFO,
                "{method} {path}"
            );
        }
    }
}
