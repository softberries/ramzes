//! `POST /v1/in/{source_id}`: requests from third parties, turned into events by a mapping.

use std::collections::BTreeMap;

use axum::extract::{Path, RawQuery, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use hmac::{Hmac, KeyInit, Mac};
use ramzes_core::adapters::{NATIVE, mapping_for};
use ramzes_core::{Mapping, Verify, hash_key};
use ramzes_store::Source;
use serde_json::{Value, json};
use sha2::Sha256;
use subtle::ConstantTimeEq;

use crate::{IngestState, Rejection, commit, to_event};

/// Characters of a request body kept for the mapping editor.
const KEPT_BODY_CHARS: usize = 64_000;
/// Headers that identify a delivery; the first one present makes a repeated request harmless.
/// How far a signed request's timestamp may be from now, in seconds.
const SIGNATURE_AGE_S: i64 = 300;
const DELIVERY_HEADERS: [&str; 3] = ["idempotency-key", "x-github-delivery", "webhook-id"];

fn unauthorized(message: &str) -> Rejection {
    Rejection::new(StatusCode::UNAUTHORIZED, "unauthorized", message)
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
}

fn query_param(query: Option<&str>, name: &str) -> Option<String> {
    query?.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == name).then(|| value.to_owned())
    })
}

/// Whether `presented` is the source's key, compared through its hash.
async fn is_source_key(
    state: &IngestState,
    source: &Source,
    presented: Option<&str>,
) -> Result<bool, Rejection> {
    let Some(presented) = presented.filter(|p| !p.is_empty()) else {
        return Ok(false);
    };
    match state.0.store.source_by_key_hash(&hash_key(presented)).await {
        Ok(found) => Ok(found.is_some_and(|found| found.id == source.id)),
        Err(e) => {
            tracing::error!(error = %e, "could not look up the source key");
            Err(Rejection::unavailable())
        }
    }
}

/// The source's key as it was given, which checking a signature needs.
async fn stored_key(state: &IngestState, source: &Source) -> Result<String, Rejection> {
    let sealed = state.0.store.source_secret(&source.id).await.map_err(|e| {
        tracing::error!(error = %e, "could not read the source's secret");
        Rejection::unavailable()
    })?;
    sealed
        .and_then(|sealed| state.0.opener.open(&sealed).ok())
        .and_then(|opened| opened.get("key").and_then(Value::as_str).map(str::to_owned))
        .ok_or_else(|| {
            unauthorized(
                "this source has no stored secret to check signatures with; replace its key in ramzes",
            )
        })
}

/// Whether one of the `v1,<base64>` signatures fits the request and the request is recent.
fn is_signed(key: &str, headers: &HeaderMap, signatures: &str, body: &[u8], now_s: i64) -> bool {
    let standard = base64::engine::general_purpose::STANDARD;
    let (Some(id), Some(timestamp)) = (
        header(headers, "webhook-id"),
        header(headers, "webhook-timestamp"),
    ) else {
        return false;
    };
    let recent = timestamp
        .parse::<i64>()
        .is_ok_and(|at| (now_s - at).abs() <= SIGNATURE_AGE_S);
    let Ok(secret) = standard.decode(key.strip_prefix("whsec_").unwrap_or(key)) else {
        return false;
    };
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(&secret) else {
        return false;
    };
    mac.update(id.as_bytes());
    mac.update(b".");
    mac.update(timestamp.as_bytes());
    mac.update(b".");
    mac.update(body);
    let expected = standard.encode(mac.finalize().into_bytes());
    let fits = signatures
        .split_ascii_whitespace()
        .filter_map(|signature| signature.strip_prefix("v1,"))
        .fold(false, |fits, given| {
            fits | bool::from(given.as_bytes().ct_eq(expected.as_bytes()))
        });
    recent && fits
}

/// Checks that the request comes from the sender, the way the mapping says.
async fn verify(
    state: &IngestState,
    source: &Source,
    mapping: &Mapping,
    headers: &HeaderMap,
    query: Option<&str>,
    body: &[u8],
) -> Result<(), Rejection> {
    let ok = match &mapping.verify {
        Verify::Bearer => {
            let token = header(headers, "authorization").and_then(|v| v.strip_prefix("Bearer "));
            is_source_key(state, source, token.map(str::trim)).await?
        }
        Verify::Header { header: name } => {
            is_source_key(state, source, header(headers, name)).await?
        }
        Verify::Query { param } => {
            is_source_key(state, source, query_param(query, param).as_deref()).await?
        }
        Verify::HmacSha256 {
            header: name,
            prefix,
        } => {
            let key = stored_key(state, source).await?;
            let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(key.as_bytes()) else {
                return Err(unauthorized("the signature could not be checked"));
            };
            mac.update(body);
            let expected = hex::encode(mac.finalize().into_bytes());
            header(headers, name)
                .and_then(|value| value.strip_prefix(prefix.as_str()))
                .is_some_and(|given| {
                    bool::from(
                        given
                            .to_ascii_lowercase()
                            .as_bytes()
                            .ct_eq(expected.as_bytes()),
                    )
                })
        }
        Verify::StandardWebhooks { fallback_header } => {
            match (header(headers, "webhook-signature"), fallback_header) {
                (Some(signatures), _) => {
                    let key = stored_key(state, source).await?;
                    let now_s = state.0.clock.now_ms() / 1000;
                    is_signed(&key, headers, signatures, body, now_s)
                }
                (None, Some(name)) => is_source_key(state, source, header(headers, name)).await?,
                (None, None) => false,
            }
        }
    };
    if ok {
        Ok(())
    } else {
        Err(unauthorized(match &mapping.verify {
            Verify::Bearer => "send the source key as: Authorization: Bearer <key>",
            Verify::Header { .. } => {
                "the request does not carry this source's key in the expected header"
            }
            Verify::Query { .. } => "the URL does not carry this source's key",
            Verify::HmacSha256 { .. } => "the request's signature does not match this source's key",
            Verify::StandardWebhooks { .. } => {
                "the request's signature does not match this source's key, or the request is more than 5 minutes old"
            }
        }))
    }
}

/// The request's headers for the mapping and for the record: lower-case names, without anything
/// that authenticates.
fn visible_headers(headers: &HeaderMap, mapping: &Mapping) -> BTreeMap<String, String> {
    let secret_header = match &mapping.verify {
        Verify::Header { header } | Verify::HmacSha256 { header, .. } => {
            header.to_ascii_lowercase()
        }
        Verify::StandardWebhooks { fallback_header } => fallback_header
            .as_deref()
            .unwrap_or_default()
            .to_ascii_lowercase(),
        Verify::Bearer | Verify::Query { .. } => String::new(),
    };
    headers
        .iter()
        .filter_map(|(name, value)| {
            let name = name.as_str().to_ascii_lowercase();
            let hidden = name == secret_header
                || matches!(
                    name.as_str(),
                    "authorization" | "proxy-authorization" | "cookie"
                )
                || name.contains("signature")
                || name.contains("token");
            let value = value.to_str().ok()?;
            (!hidden).then(|| (name, value.to_owned()))
        })
        .collect()
}

pub(crate) async fn receive(
    State(state): State<IngestState>,
    Path(source_id): Path<String>,
    RawQuery(query): RawQuery,
    request: Request,
) -> Response {
    let (parts, body) = request.into_parts();
    let source = match state.0.store.get_source(&source_id).await {
        Ok(Some(source)) => source,
        Ok(None) => {
            return Rejection::new(
                StatusCode::NOT_FOUND,
                "not_found",
                "there is no source at this address",
            )
            .into_response();
        }
        Err(e) => {
            tracing::error!(error = %e, "could not look up the source");
            return Rejection::unavailable().into_response();
        }
    };
    if source.kind == NATIVE {
        return Rejection::new(
            StatusCode::BAD_REQUEST,
            "wrong_endpoint",
            "this source sends ramzes's own format; post to /v1/events instead",
        )
        .into_response();
    }
    let mapping = match mapping_for(&source.kind, source.mapping.as_deref()) {
        Ok(Some(mapping)) => mapping,
        Ok(None) => return Rejection::unavailable().into_response(),
        Err(reason) => {
            return Rejection::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid_mapping",
                format!("this source's mapping cannot be used: {reason}"),
            )
            .into_response();
        }
    };
    let limit = state.0.limits.max_body_bytes;
    let Ok(bytes) = axum::body::to_bytes(body, limit).await else {
        return Rejection::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "too_large",
            format!("the body is larger than {limit} bytes"),
        )
        .into_response();
    };
    if let Err(rejection) = verify(
        &state,
        &source,
        &mapping,
        &parts.headers,
        query.as_deref(),
        &bytes,
    )
    .await
    {
        return rejection.into_response();
    }

    let headers = visible_headers(&parts.headers, &mapping);
    let result = accept(&state, &source, &mapping, &parts.headers, &headers, &bytes).await;
    let error = match &result {
        Err(rejection)
            if rejection.status.is_client_error()
                && rejection.status != StatusCode::TOO_MANY_REQUESTS =>
        {
            Some(rejection.message.clone())
        }
        Err(_) => return result.into_response_or_rejection(),
        Ok(_) => None,
    };
    let kept: String = String::from_utf8_lossy(&bytes)
        .chars()
        .filter(|c| *c != '\0')
        .take(KEPT_BODY_CHARS)
        .collect();
    let shown_headers = json!(headers).to_string();
    let recorded = state
        .0
        .store
        .record_source_request(
            &source.workspace_id,
            &source.id,
            &kept,
            Some(&shown_headers),
            error.as_deref(),
            state.0.clock.now_ms(),
        )
        .await;
    if let Err(e) = recorded {
        tracing::warn!(error = %e, "could not record a source request");
    }
    result.into_response_or_rejection()
}

trait IntoResponseOrRejection {
    fn into_response_or_rejection(self) -> Response;
}

impl IntoResponseOrRejection for Result<Response, Rejection> {
    fn into_response_or_rejection(self) -> Response {
        match self {
            Ok(response) => response,
            Err(rejection) => rejection.into_response(),
        }
    }
}

async fn accept(
    state: &IngestState,
    source: &Source,
    mapping: &Mapping,
    raw_headers: &HeaderMap,
    headers: &BTreeMap<String, String>,
    bytes: &[u8],
) -> Result<Response, Rejection> {
    let payload: Value = serde_json::from_slice(bytes).map_err(|e| {
        Rejection::new(
            StatusCode::BAD_REQUEST,
            "invalid_json",
            format!("the body is not valid JSON: {e}"),
        )
    })?;
    let invalid = |message: String| {
        Rejection::new(StatusCode::UNPROCESSABLE_ENTITY, "invalid_event", message)
    };
    // Mapping a large request is CPU work; it runs off the threads that serve other requests.
    let mapped = {
        let (mapping, headers) = (mapping.clone(), headers.clone());
        tokio::task::spawn_blocking(move || mapping.apply(&payload, &headers))
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "mapping a request failed unexpectedly");
                Rejection::unavailable()
            })?
            .map_err(invalid)?
    };
    if mapped.is_empty() {
        return Ok((
            StatusCode::ACCEPTED,
            axum::Json(json!({"ids": [], "duplicate": false})),
        )
            .into_response());
    }
    let now = state.0.clock.now_ms();
    let mut events = Vec::with_capacity(mapped.len());
    for new in mapped {
        events.push(to_event(source, new, now).map_err(invalid)?);
    }
    let delivery = DELIVERY_HEADERS
        .iter()
        .find_map(|name| raw_headers.get(*name).and_then(|v| v.to_str().ok()))
        .filter(|id| !id.is_empty() && id.len() <= 200);
    commit(state, source, events, delivery, now).await
}
