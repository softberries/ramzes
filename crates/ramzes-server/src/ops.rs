use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use prometheus_client::encoding::EncodeLabelSet;
use prometheus_client::encoding::text::encode;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::gauge::Gauge;
use prometheus_client::metrics::histogram::{Histogram, exponential_buckets};
use prometheus_client::registry::Registry;
use ramzes_engine::{Change, Engine};
use ramzes_ingest::IngestState;
use ramzes_store::Store;
use subtle::ConstantTimeEq;
use tokio::sync::broadcast::error::RecvError;

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct DeliveryLabels {
    connector: String,
    outcome: String,
}

/// What `/metrics` reports and where the numbers come from.
pub struct Ops {
    store: Store,
    ingest: IngestState,
    registry: Registry,
    ingested: Counter,
    /// Ingest total already added to `ingested`.
    ingested_seen: AtomicU64,
    deliveries: Family<DeliveryLabels, Counter>,
    delivery_seconds: Histogram,
    queue_unrouted: Gauge,
    queue_pending: Gauge,
    /// When set, `/metrics` answers only requests that carry it.
    metrics_token: Option<String>,
}

impl Ops {
    pub fn new(store: Store, ingest: IngestState, metrics_token: Option<String>) -> Arc<Ops> {
        let mut registry = Registry::default();
        let ingested = Counter::default();
        let deliveries = Family::<DeliveryLabels, Counter>::default();
        let delivery_seconds = Histogram::new(exponential_buckets(0.01, 2.0, 12));
        let queue_unrouted = Gauge::default();
        let queue_pending = Gauge::default();
        registry.register(
            "ramzes_events_ingested",
            "Events accepted by the ingest API",
            ingested.clone(),
        );
        registry.register(
            "ramzes_deliveries",
            "Delivery attempts by connector and resulting state",
            deliveries.clone(),
        );
        registry.register(
            "ramzes_delivery_duration_seconds",
            "Duration of delivery attempts",
            delivery_seconds.clone(),
        );
        registry.register(
            "ramzes_queue_unrouted",
            "Events waiting to be routed",
            queue_unrouted.clone(),
        );
        registry.register(
            "ramzes_queue_pending_deliveries",
            "Deliveries waiting to be sent",
            queue_pending.clone(),
        );
        Arc::new(Ops {
            store,
            ingest,
            registry,
            ingested,
            ingested_seen: AtomicU64::new(0),
            deliveries,
            delivery_seconds,
            queue_unrouted,
            queue_pending,
            metrics_token,
        })
    }

    /// Counts delivery attempts as the engine reports them. Runs until the engine is dropped.
    pub async fn watch(self: Arc<Self>, engine: Arc<Engine>) {
        let mut changes = engine.subscribe();
        drop(engine);
        loop {
            match changes.recv().await {
                Ok(Change::DeliveryUpdated {
                    state,
                    connector_kind,
                    duration_ms,
                    ..
                }) => {
                    let labels = DeliveryLabels {
                        connector: connector_kind,
                        outcome: state.as_str().to_owned(),
                    };
                    self.deliveries.get_or_create(&labels).inc();
                    self.delivery_seconds.observe(duration_ms as f64 / 1000.0);
                }
                Ok(Change::EventRouted { .. } | Change::NotificationChanged { .. }) => {}
                Err(RecvError::Lagged(missed)) => {
                    tracing::warn!(
                        missed,
                        "metrics fell behind; some delivery attempts were not counted"
                    );
                }
                Err(RecvError::Closed) => return,
            }
        }
    }

    pub fn router(self: Arc<Self>) -> Router {
        Router::new()
            .route("/healthz", get(|| async { "ok" }))
            .route("/readyz", get(ready))
            .route("/metrics", get(metrics))
            .with_state(self)
    }
}

async fn ready(State(ops): State<Arc<Ops>>) -> Response {
    match ops.store.ping().await {
        Ok(()) => "ready".into_response(),
        Err(e) => {
            tracing::warn!(error = %e, "readiness check failed");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "the database is not reachable",
            )
                .into_response()
        }
    }
}

async fn metrics(State(ops): State<Arc<Ops>>, headers: HeaderMap) -> Response {
    if let Some(token) = &ops.metrics_token {
        let given = headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .unwrap_or_default();
        if !bool::from(given.trim().as_bytes().ct_eq(token.as_bytes())) {
            return (
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Bearer")],
                "send the metrics token as: Authorization: Bearer <token>",
            )
                .into_response();
        }
    }
    let total = ops.ingest.ingested();
    let seen = ops.ingested_seen.swap(total, Ordering::Relaxed);
    ops.ingested.inc_by(total.saturating_sub(seen));
    if let Ok(depth) = ops.store.queue_depth().await {
        ops.queue_unrouted.set(depth.unrouted_events);
        ops.queue_pending.set(depth.pending_deliveries);
    }
    let mut body = String::new();
    if let Err(e) = encode(&mut body, &ops.registry) {
        tracing::error!(error = %e, "could not encode metrics");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    (
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        body,
    )
        .into_response()
}
