use reqwest::{Client, Method, header};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, time::Duration};
use thiserror::Error;

const DEFAULT_TIMEOUT_MS: u64 = 30_000;
const MAX_TIMEOUT_MS: u64 = 120_000;
const MAX_BODY_BYTES: usize = 5 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    pub body: Option<String>,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpResponse {
    pub status: u16,
    pub status_text: String,
    pub headers: BTreeMap<String, String>,
    pub body: String,
    pub duration_ms: u64,
    pub truncated: bool,
}

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("only http and https URLs are supported")]
    UnsupportedScheme,
    #[error("invalid request URL")]
    InvalidUrl,
    #[error("unsupported HTTP method")]
    InvalidMethod,
    #[error("invalid HTTP header: {0}")]
    InvalidHeader(String),
    #[error("request body exceeds the 5 MB limit")]
    BodyTooLarge,
    #[error("request timed out")]
    Timeout,
    #[error("request failed: {0}")]
    Request(String),
}

/// Executes one explicitly requested HTTP request without persisting or logging its contents.
pub async fn send(request: HttpRequest) -> Result<HttpResponse, ApiError> {
    let url = validate_url(request.url.trim())?;
    let method = Method::from_bytes(request.method.trim().as_bytes())
        .map_err(|_| ApiError::InvalidMethod)?;
    let timeout_ms = request
        .timeout_ms
        .unwrap_or(DEFAULT_TIMEOUT_MS)
        .clamp(1_000, MAX_TIMEOUT_MS);
    if request
        .body
        .as_ref()
        .is_some_and(|body| body.len() > MAX_BODY_BYTES)
    {
        return Err(ApiError::BodyTooLarge);
    }

    let client = Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .build()
        .map_err(|_| ApiError::Request("unable to create HTTP client".into()))?;
    let mut builder = client.request(method, url);
    for (name, value) in request.headers {
        let header_name = header::HeaderName::from_bytes(name.trim().as_bytes())
            .map_err(|_| ApiError::InvalidHeader(name))?;
        let header_value = header::HeaderValue::from_str(value.trim())
            .map_err(|_| ApiError::InvalidHeader(header_name.to_string()))?;
        builder = builder.header(header_name, header_value);
    }
    if let Some(body) = request.body {
        builder = builder.body(body);
    }

    let started = std::time::Instant::now();
    let response = builder.send().await.map_err(|error| {
        if error.is_timeout() {
            ApiError::Timeout
        } else {
            ApiError::Request("network request failed".into())
        }
    })?;
    let status = response.status();
    let status_text = status.canonical_reason().unwrap_or_default().to_owned();
    let headers = response
        .headers()
        .iter()
        .map(|(name, value)| {
            (
                name.to_string(),
                value.to_str().unwrap_or("<binary>").to_owned(),
            )
        })
        .collect();
    let bytes = response.bytes().await.map_err(|error| {
        if error.is_timeout() {
            ApiError::Timeout
        } else {
            ApiError::Request("failed to read response".into())
        }
    })?;
    let truncated = bytes.len() > MAX_BODY_BYTES;
    let body = String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_BODY_BYTES)]).into_owned();
    Ok(HttpResponse {
        status: status.as_u16(),
        status_text,
        headers,
        body,
        duration_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
        truncated,
    })
}

fn validate_url(value: &str) -> Result<reqwest::Url, ApiError> {
    let url = reqwest::Url::parse(value).map_err(|_| ApiError::InvalidUrl)?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(ApiError::UnsupportedScheme);
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_http_urls() {
        let request = HttpRequest {
            method: "GET".into(),
            url: "file:///secret.txt".into(),
            headers: BTreeMap::new(),
            body: None,
            timeout_ms: None,
        };
        assert!(matches!(
            validate_url(&request.url),
            Err(ApiError::UnsupportedScheme)
        ));
    }
}
