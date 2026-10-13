//! The HTTP endpoint services send events to.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use ramzes_core::{Event, NewEvent, hash_key, new_id};
use ramzes_engine::{Clock, Engine};
use ramzes_store::{Source, Store};
use serde_json::{Value, json};

const MINUTE_MS: i64 = 60_000;
const IDEMPOTENCY_KEY_MAX: usize = 200;
/// Characters of a rejected body kept for debugging.
const KEPT_BODY_CHARS: usize = 10_000;

#[derive(Debug, Clone)]
pub struct IngestLimits {
    pub max_body_bytes: usize,
    pub max_batch: usize,
    /// Events one source may send per minute.
    pub per_source_per_minute: u32,
}

impl Default for IngestLimits {
    fn default() -> Self {
        IngestLimits {
            max_body_bytes: 1024 * 1024,
            max_batch: 1000,
            per_source_per_minute: 6000,
        }
    }
}

struct Inner {
    store: Store,
    engine: Arc<Engine>,
    limits: IngestLimits,
    clock: Arc<dyn Clock>,
    /// Per source: the minute being counted and the events accepted in it.
    windows: Mutex<HashMap<String, (i64, u32)>>,
    ingested: AtomicU64,
}

#[derive(Clone)]
pub struct IngestState(Arc<Inner>);

impl IngestState {
    pub fn new(
        store: Store,
        engine: Arc<Engine>,
        limits: IngestLimits,
        clock: Arc<dyn Clock>,
    ) -> IngestState {
        IngestState(Arc::new(Inner {
            store,
            engine,
            limits,
            clock,
            windows: Mutex::new(HashMap::new()),
            ingested: AtomicU64::new(0),
        }))
    }

    /// Events accepted since start.
    pub fn ingested(&self) -> u64 {
        self.0.ingested.load(Ordering::Relaxed)
    }

    /// Counts `events` against the source's allowance for the current minute.
    /// Returns the seconds to wait when the allowance is used up.
    fn admit(&self, source_id: &str, events: usize, now_ms: i64) -> Result<(), i64> {
        let minute = now_ms.div_euclid(MINUTE_MS);
        let events = u32::try_from(events).unwrap_or(u32::MAX);
        let mut windows = self
            .0
            .windows
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        windows.retain(|_, (counted_minute, _)| *counted_minute == minute);
        let (_, used) = windows.entry(source_id.to_owned()).or_insert((minute, 0));
        if used.saturating_add(events) > self.0.limits.per_source_per_minute {
            let wait_ms = (minute + 1) * MINUTE_MS - now_ms;
            return Err((wait_ms + 999) / 1000);
        }
        *used += events;
        Ok(())
    }
}

pub fn router(state: IngestState) -> Router {
    Router::new()
        .route("/v1/events", post(ingest))
        .with_state(state)
}

struct Rejection {
    status: StatusCode,
    code: &'static str,
    message: String,
    retry_after: Option<i64>,
}

impl Rejection {
    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Rejection {
        Rejection {
            status,
            code,
            message: message.into(),
            retry_after: None,
        }
    }

    fn unavailable() -> Rejection {
        Rejection::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "unavailable",
            "ramzes cannot store events right now; retry shortly",
        )
    }
}

impl IntoResponse for Rejection {
    fn into_response(self) -> Response {
        let body = axum::Json(json!({"error": self.code, "message": self.message}));
        match self.retry_after {
            Some(seconds) => (
                self.status,
                [(header::RETRY_AFTER, seconds.to_string())],
                body,
            )
                .into_response(),
            None => (self.status, body).into_response(),
        }
    }
}

async fn ingest(State(state): State<IngestState>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let source = match authenticate(&state, &parts.headers).await {
        Ok(source) => source,
        Err(rejection) => return rejection.into_response(),
    };
    let Ok(bytes) = axum::body::to_bytes(body, state.0.limits.max_body_bytes).await else {
        let limit = state.0.limits.max_body_bytes;
        return Rejection::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "too_large",
            format!("the body is larger than {limit} bytes"),
        )
        .into_response();
    };
    match accept(&state, &source, &parts.headers, &bytes).await {
        Ok(response) => response,
        Err(rejection) => {
            if matches!(
                rejection.status,
                StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY
            ) {
                let kept: String = String::from_utf8_lossy(&bytes)
                    .chars()
                    .take(KEPT_BODY_CHARS)
                    .collect();
                let now = state.0.clock.now_ms();
                let recorded = state.0.store.record_source_request(
                    &source.workspace_id,
                    &source.id,
                    &kept,
                    None,
                    Some(&rejection.message),
                    now,
                );
                if let Err(e) = recorded.await {
                    tracing::warn!(error = %e, "could not record a rejected request");
                }
            }
            rejection.into_response()
        }
    }
}

async fn authenticate(state: &IngestState, headers: &HeaderMap) -> Result<Source, Rejection> {
    let unauthorized = || {
        Rejection::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "send the source key as: Authorization: Bearer <key>",
        )
    };
    let key = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .ok_or_else(unauthorized)?;
    match state.0.store.source_by_key_hash(&hash_key(key)).await {
        Ok(Some(source)) => Ok(source),
        Ok(None) => Err(unauthorized()),
        Err(e) => {
            tracing::error!(error = %e, "could not look up the source key");
            Err(Rejection::unavailable())
        }
    }
}

async fn accept(
    state: &IngestState,
    source: &Source,
    headers: &HeaderMap,
    bytes: &[u8],
) -> Result<Response, Rejection> {
    let invalid = |message: String| {
        Rejection::new(StatusCode::UNPROCESSABLE_ENTITY, "invalid_event", message)
    };
    let idempotency_key = match headers.get("idempotency-key") {
        None => None,
        Some(value) => match value.to_str() {
            Ok(key) if !key.is_empty() && key.chars().count() <= IDEMPOTENCY_KEY_MAX => Some(key),
            _ => {
                return Err(Rejection::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "invalid_idempotency_key",
                    format!(
                        "Idempotency-Key must be 1 to {IDEMPOTENCY_KEY_MAX} visible ASCII characters"
                    ),
                ));
            }
        },
    };

    let parsed: Value = serde_json::from_slice(bytes).map_err(|e| {
        Rejection::new(
            StatusCode::BAD_REQUEST,
            "invalid_json",
            format!("the body is not valid JSON: {e}"),
        )
    })?;
    let (items, is_batch) = match parsed {
        Value::Array(items) => (items, true),
        item @ Value::Object(_) => (vec![item], false),
        _ => {
            return Err(Rejection::new(
                StatusCode::BAD_REQUEST,
                "invalid_json",
                "the JSON body must be an event object or an array of event objects",
            ));
        }
    };
    if items.is_empty() {
        return Err(invalid("the batch is empty".into()));
    }
    let max_batch = state.0.limits.max_batch;
    if items.len() > max_batch {
        return Err(Rejection::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "batch_too_large",
            format!(
                "a request may carry at most {max_batch} events; this one has {}",
                items.len()
            ),
        ));
    }

    let now = state.0.clock.now_ms();
    let mut events = Vec::with_capacity(items.len());
    for (index, item) in items.into_iter().enumerate() {
        let position = if is_batch {
            format!("event {index}: ")
        } else {
            String::new()
        };
        let new: NewEvent = serde_path_to_error::deserialize(item).map_err(|e| {
            let path = e.path().to_string();
            let field = if path == "." {
                String::new()
            } else {
                format!("{path}: ")
            };
            invalid(format!("{position}{field}{}", e.inner()))
        })?;
        new.validate()
            .map_err(|e| invalid(format!("{position}{e}")))?;
        let occurred_at = new
            .occurred_at(now)
            .map_err(|e| invalid(format!("{position}{e}")))?;
        events.push(Event {
            id: new_id(),
            workspace_id: source.workspace_id.clone(),
            source_id: source.id.clone(),
            topic: new.topic,
            severity: new.severity,
            status: new.status,
            title: new.title,
            body: new.body,
            labels: new.labels,
            payload: new.payload,
            dedup_key: new.dedup_key,
            occurred_at,
            received_at: now,
        });
    }

    if let Err(wait_seconds) = state.admit(&source.id, events.len(), now) {
        let limit = state.0.limits.per_source_per_minute;
        let mut rejection = Rejection::new(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            format!("this source may send {limit} events per minute"),
        );
        rejection.retry_after = Some(wait_seconds);
        return Err(rejection);
    }

    let outcome = state
        .0
        .store
        .insert_events(&events, idempotency_key)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "could not store events");
            Rejection::unavailable()
        })?;
    if !outcome.duplicate {
        state
            .0
            .ingested
            .fetch_add(outcome.ids.len() as u64, Ordering::Relaxed);
        state.0.engine.wake();
    }
    Ok((
        StatusCode::ACCEPTED,
        axum::Json(json!({"ids": outcome.ids, "duplicate": outcome.duplicate})),
    )
        .into_response())
}
