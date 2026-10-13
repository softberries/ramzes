use std::time::Duration;

use crate::{SendOutcome, truncate};

const EXCERPT_CHARS: usize = 500;

/// Maps an HTTP response to an outcome: 2xx delivered; 408, 425, 429 and 5xx retry; anything else is final.
pub fn classify(status: u16, retry_after: Option<&str>, body_excerpt: String) -> SendOutcome {
    let detail = if body_excerpt.is_empty() {
        format!("HTTP {status}")
    } else {
        format!("HTTP {status}: {body_excerpt}")
    };
    match status {
        200..=299 => SendOutcome::Delivered {
            external_ref: None,
            status: Some(status),
        },
        408 | 425 | 429 | 500..=599 => SendOutcome::Retry {
            after_ms: retry_after
                .and_then(|s| s.trim().parse::<i64>().ok())
                .map(|s| s.saturating_mul(1000)),
            status: Some(status),
            detail,
        },
        _ => SendOutcome::Permanent {
            status: Some(status),
            detail,
        },
    }
}

/// The HTTP client shared by connectors.
#[derive(Clone)]
pub(crate) struct Http {
    client: reqwest::Client,
}

impl Http {
    pub fn new(timeout: Duration) -> Http {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("ramzes/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("the HTTP client configuration is static and valid");
        Http { client }
    }

    /// Posts a JSON body and classifies the answer. Network failures and timeouts are retryable.
    pub async fn post_json(
        &self,
        url: &str,
        headers: &[(&str, String)],
        body: Vec<u8>,
    ) -> SendOutcome {
        let mut request = self
            .client
            .post(url)
            .header("content-type", "application/json")
            .body(body);
        for (name, value) in headers {
            request = request.header(*name, value);
        }
        let response = match request.send().await {
            Ok(response) => response,
            Err(e) => {
                let detail = if e.is_timeout() {
                    "the request timed out".to_owned()
                } else if e.is_connect() {
                    "could not connect to the service".to_owned()
                } else {
                    format!("the request failed: {}", e.without_url())
                };
                return SendOutcome::Retry {
                    after_ms: None,
                    status: None,
                    detail,
                };
            }
        };
        let status = response.status().as_u16();
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        // A NUL cannot be stored as text in Postgres; binary answers are kept without them.
        let body = response.text().await.unwrap_or_default().replace('\0', "");
        classify(
            status,
            retry_after.as_deref(),
            truncate(body.trim(), EXCERPT_CHARS),
        )
    }
}
