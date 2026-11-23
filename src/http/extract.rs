//! The shared JSON request extractor.

use axum::extract::{FromRequest, Request};
use axum::http::HeaderMap;
use axum::http::header::CONTENT_TYPE;
use http_body_util::{BodyExt, LengthLimitError, Limited};
use serde::de::DeserializeOwned;
use serde_json::error::Category;

use super::error::{ApiError, ErrorDetail};
use super::middleware::MAX_BODY_BYTES;

/// JSON body extractor with uniform rejections:
/// 415 for a missing or non-JSON `Content-Type`, 413 when the body exceeds
/// 65,536 bytes (even when streamed), 400 `invalid_json` for malformed JSON
/// or wrongly typed fields (naming the field when known).
#[derive(Debug, Clone, Copy)]
pub struct ApiJson<T>(pub T);

impl<S, T> FromRequest<S> for ApiJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = ApiError;

    async fn from_request(request: Request, _state: &S) -> Result<Self, Self::Rejection> {
        if !has_json_content_type(request.headers()) {
            return Err(ApiError::unsupported_media_type());
        }

        let body = Limited::new(request.into_body(), MAX_BODY_BYTES);
        let bytes = body
            .collect()
            .await
            .map_err(|err| {
                if err.downcast_ref::<LengthLimitError>().is_some() {
                    ApiError::payload_too_large()
                } else {
                    ApiError::invalid_json(Vec::new())
                }
            })?
            .to_bytes();

        parse_json(&bytes).map(ApiJson)
    }
}

/// Accepts the media type `application/json`, with or without parameters
/// such as `charset=utf-8`.
fn has_json_content_type(headers: &HeaderMap) -> bool {
    headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|media_type| media_type.trim().eq_ignore_ascii_case("application/json"))
}

pub(crate) fn parse_json<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, ApiError> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    serde_path_to_error::deserialize(&mut deserializer).map_err(|err| {
        let is_type_error = err.inner().classify() == Category::Data;
        let path = err.path().to_string();
        if is_type_error && path != "." {
            ApiError::invalid_json(vec![ErrorDetail::new(path, err.inner().to_string())])
        } else {
            ApiError::invalid_json(Vec::new())
        }
    })
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;
    use serde::Deserialize;

    use super::*;

    #[derive(Debug, Deserialize)]
    struct Sample {
        value: String,
    }

    #[test]
    fn content_type_variants() {
        let check = |raw: Option<&str>| {
            let mut headers = HeaderMap::new();
            if let Some(raw) = raw {
                headers.insert(CONTENT_TYPE, HeaderValue::from_str(raw).unwrap());
            }
            has_json_content_type(&headers)
        };
        assert!(check(Some("application/json")));
        assert!(check(Some("application/json; charset=utf-8")));
        assert!(check(Some("Application/JSON")));
        assert!(!check(Some("text/plain")));
        assert!(!check(Some("application/jsonx")));
        assert!(!check(None));
    }

    #[test]
    fn valid_json_parses() {
        let sample: Sample = parse_json(br#"{"value":"x"}"#).unwrap();
        assert_eq!(sample.value, "x");
    }

    #[test]
    fn syntax_error_has_no_details() {
        let err = parse_json::<Sample>(br#"{"value":"#).unwrap_err();
        assert_eq!(err, ApiError::invalid_json(Vec::new()));
    }

    #[test]
    fn wrong_type_names_the_field() {
        let err = parse_json::<Sample>(br#"{"value":5}"#).unwrap_err();
        assert_eq!(err.details.len(), 1);
        assert_eq!(err.details[0].field, "value");
        assert!(!err.details[0].message.is_empty());
    }

    #[test]
    fn missing_field_and_root_type_errors_have_no_named_field() {
        let err = parse_json::<Sample>(b"{}").unwrap_err();
        assert!(err.details.len() <= 1);
        let err = parse_json::<Sample>(b"5").unwrap_err();
        assert!(err.details.is_empty());
    }
}
