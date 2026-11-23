//! Swagger UI at `/docs`, served from the vendored assets of
//! `utoipa-swagger-ui` by our own handlers.
//!
//! `GET /docs` answers 200 with the UI page directly (no redirect to
//! `/docs/`): a `<base href="/docs/">` element is injected into
//! `index.html` so its relative asset URLs resolve under `/docs/`.

use std::borrow::Cow;
use std::sync::Arc;

use axum::Router;
use axum::extract::Path;
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use tracing::error;
use utoipa_swagger_ui::{Config, SwaggerFile};

use crate::http::error::ApiError;
use crate::http::state::AppState;

/// Where the UI loads the document from (same origin as the page).
pub const OPENAPI_URL: &str = "/openapi.json";

const INDEX: &str = "index.html";
const HEAD_TAG: &str = "<head>";
const BASE_TAG: &str = r#"<base href="/docs/">"#;
const HTML_CONTENT_TYPE: &str = "text/html; charset=utf-8";

/// Swagger UI configuration: our document, with "Try it out" enabled.
fn config() -> Arc<Config<'static>> {
    Arc::new(Config::from(OPENAPI_URL).try_it_out_enabled(true))
}

/// `GET /docs`, `GET /docs/` and `GET /docs/{*rest}`. Other methods get the
/// router's uniform 405.
pub fn routes() -> Router<AppState> {
    let config = config();
    let index_config = Arc::clone(&config);
    let slash_config = Arc::clone(&config);
    Router::new()
        .route(
            "/docs",
            get(move || serve_file(INDEX.to_owned(), index_config)),
        )
        .route(
            "/docs/",
            get(move || serve_file(INDEX.to_owned(), slash_config)),
        )
        .route(
            "/docs/{*rest}",
            get(move |Path(rest): Path<String>| serve_file(rest, config)),
        )
}

async fn serve_file(path: String, config: Arc<Config<'static>>) -> Response {
    match utoipa_swagger_ui::serve(&path, config) {
        Ok(Some(file)) => file_response(&path, file),
        Ok(None) => ApiError::route_not_found().into_response(),
        Err(err) => {
            error!("failed to serve Swagger UI file {path}: {err}");
            ApiError::internal().into_response()
        }
    }
}

fn file_response(path: &str, file: SwaggerFile<'_>) -> Response {
    let (body, content_type) = if path == INDEX {
        (
            with_base_href(&file.bytes).into_owned(),
            Cow::Borrowed(HTML_CONTENT_TYPE),
        )
    } else {
        (file.bytes.into_owned(), Cow::Owned(file.content_type))
    };
    match HeaderValue::from_str(&content_type) {
        Ok(value) => ([(header::CONTENT_TYPE, value)], body).into_response(),
        Err(err) => {
            error!("invalid content type {content_type} for Swagger UI file {path}: {err}");
            ApiError::internal().into_response()
        }
    }
}

/// Inserts `<base href="/docs/">` right after the first `<head>`. A page
/// without `<head>` is returned unchanged.
fn with_base_href(html: &[u8]) -> Cow<'_, [u8]> {
    let Some(position) = find(html, HEAD_TAG.as_bytes()) else {
        return Cow::Borrowed(html);
    };
    let split = position + HEAD_TAG.len();
    let mut out = Vec::with_capacity(html.len() + BASE_TAG.len());
    out.extend_from_slice(&html[..split]);
    out.extend_from_slice(BASE_TAG.as_bytes());
    out.extend_from_slice(&html[split..]);
    Cow::Owned(out)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use axum::body::to_bytes;
    use axum::http::StatusCode;

    use super::*;

    async fn body_of(response: Response) -> String {
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[test]
    fn base_href_is_injected_after_head() {
        let html = b"<html>\n  <head>\n    <title>x</title>\n  </head></html>";
        let out = with_base_href(html);
        assert_eq!(
            std::str::from_utf8(&out).unwrap(),
            "<html>\n  <head><base href=\"/docs/\">\n    <title>x</title>\n  </head></html>"
        );
    }

    #[test]
    fn only_the_first_head_gets_a_base() {
        let out = with_base_href(b"<head></head><head>");
        assert_eq!(
            std::str::from_utf8(&out).unwrap(),
            "<head><base href=\"/docs/\"></head><head>"
        );
    }

    #[test]
    fn page_without_head_is_unchanged() {
        assert!(matches!(with_base_href(b"<p>hi</p>"), Cow::Borrowed(_)));
    }

    #[tokio::test]
    async fn index_is_html_with_base_and_swagger_root() {
        let response = serve_file(INDEX.to_owned(), config()).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-type"], HTML_CONTENT_TYPE);
        let body = body_of(response).await;
        assert!(body.contains(BASE_TAG), "{body}");
        assert!(body.contains(r#"id="swagger-ui""#), "{body}");
    }

    #[tokio::test]
    async fn initializer_embeds_the_config() {
        let response = serve_file("swagger-initializer.js".to_owned(), config()).await;
        assert_eq!(response.status(), StatusCode::OK);
        let content_type = response.headers()["content-type"].to_str().unwrap();
        assert!(content_type.contains("javascript"), "{content_type}");
        let body = body_of(response).await;
        assert!(body.contains(r#""url": "/openapi.json""#), "{body}");
        assert!(body.contains(r#""tryItOutEnabled": true"#), "{body}");
    }

    #[tokio::test]
    async fn stylesheet_keeps_its_guessed_content_type() {
        let response = serve_file("swagger-ui.css".to_owned(), config()).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-type"], "text/css");
    }

    #[tokio::test]
    async fn missing_file_is_the_uniform_404() {
        let response = serve_file("does-not-exist.js".to_owned(), config()).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            body_of(response).await,
            r#"{"error":{"code":"not_found","message":"Route not found","details":[]}}"#
        );
    }
}
