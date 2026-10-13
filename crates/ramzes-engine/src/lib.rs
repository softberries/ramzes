//! The workers that move events through ramzes: routing, delivery, replay and retention.
//!
//! All state lives in the store. A worker claims rows with a lease, does its work and writes
//! the result; if it dies in between, the lease expires and another pass picks the rows up.

mod delivery_worker;
mod render;
mod replay;
mod retention;
mod route_worker;
mod system;
mod timer_worker;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ramzes_connectors::Registry;
use ramzes_core::RetryPolicy;
use ramzes_store::{DeliveryState, Store, StoreError};
use tokio::sync::{Notify, Semaphore, broadcast};

pub use render::{Composed, compose};
pub use system::SYSTEM_TOPIC_PREFIX;
use tokio_util::sync::CancellationToken;

pub trait Clock: Send + Sync {
    /// Unix milliseconds.
    fn now_ms(&self) -> i64;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        let since_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        i64::try_from(since_epoch.as_millis()).unwrap_or(i64::MAX)
    }
}

/// Decrypts a connection's stored credentials.
pub trait Opener: Send + Sync {
    fn open(&self, sealed: &str) -> std::result::Result<serde_json::Value, String>;
}

/// Something the UI may want to show without polling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    EventRouted {
        event_id: String,
    },
    /// A notification was opened, joined, sent, held, acknowledged or resolved.
    NotificationChanged {
        notification_id: String,
    },
    DeliveryUpdated {
        event_ids: Vec<String>,
        delivery_id: String,
        state: DeliveryState,
        connector_kind: String,
        /// How long the attempt took.
        duration_ms: i64,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("not found")]
    NotFound,
    #[error(transparent)]
    Store(StoreError),
}

impl From<StoreError> for EngineError {
    fn from(e: StoreError) -> Self {
        match e {
            StoreError::NotFound => EngineError::NotFound,
            other => EngineError::Store(other),
        }
    }
}

pub type Result<T> = std::result::Result<T, EngineError>;

pub struct EngineConfig {
    pub store: Store,
    pub registry: Registry,
    pub opener: Arc<dyn Opener>,
    pub clock: Arc<dyn Clock>,
    pub policy: RetryPolicy,
    /// Base URL of the UI, used for links in messages. Falls back to the `public_url` setting.
    pub public_url: Option<String>,
    /// How long a worker may hold a claimed event or delivery before another pass may take it.
    pub lease_ms: i64,
    /// Source of retry jitter, 0.0 to 1.0.
    pub jitter: Arc<dyn Fn() -> f64 + Send + Sync>,
}

pub struct Engine {
    config: EngineConfig,
    route_wake: Notify,
    delivery_wake: Notify,
    /// Held while a shaping decision is made and applied, and while a timer acts on a
    /// notification. ramzes runs as one node; this is what keeps the two from interleaving.
    shaping_lock: tokio::sync::Mutex<()>,
    changes: broadcast::Sender<Change>,
    /// One semaphore per connection, bounding how many sends it has in flight.
    connection_slots: Mutex<HashMap<String, Arc<Semaphore>>>,
    /// The last error written for each route and when, so the same error is not written per event.
    route_errors: Mutex<HashMap<String, (String, i64)>>,
}

/// Pause after a failed pass, so a broken database is not hammered.
const ERROR_PAUSE: Duration = Duration::from_secs(1);

impl Engine {
    pub fn new(config: EngineConfig) -> Arc<Engine> {
        let (changes, _) = broadcast::channel(1024);
        Arc::new(Engine {
            config,
            route_wake: Notify::new(),
            delivery_wake: Notify::new(),
            shaping_lock: tokio::sync::Mutex::new(()),
            changes,
            connection_slots: Mutex::default(),
            route_errors: Mutex::default(),
        })
    }

    /// Jitter from the system's random source.
    pub fn random_jitter() -> Arc<dyn Fn() -> f64 + Send + Sync> {
        Arc::new(rand::random::<f64>)
    }

    /// Tells the workers there is new work. Call after committing events or deliveries.
    pub fn wake(&self) {
        self.route_wake.notify_one();
        self.delivery_wake.notify_one();
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Change> {
        self.changes.subscribe()
    }

    /// Tells listeners that a notification changed outside the workers, for example when a
    /// person acknowledged it.
    pub fn notification_changed(&self, notification_id: &str) {
        self.announce(Change::NotificationChanged {
            notification_id: notification_id.to_owned(),
        });
    }

    fn announce(&self, change: Change) {
        // No subscribers is not an error.
        let _ = self.changes.send(change);
    }

    fn now(&self) -> i64 {
        self.config.clock.now_ms()
    }

    /// Runs the route, delivery and timer workers until `shutdown` is cancelled. Each worker starts a
    /// pass when woken or every `poll`, and finishes the pass it is in before stopping.
    pub async fn run(self: Arc<Self>, shutdown: CancellationToken, poll: Duration) {
        let routing = async {
            while !shutdown.is_cancelled() {
                match self.route_once().await {
                    Ok(0) => idle(&shutdown, &self.route_wake, poll).await,
                    Ok(_) => {}
                    Err(e) => {
                        tracing::error!(error = %e, "routing pass failed");
                        idle(&shutdown, &self.route_wake, ERROR_PAUSE).await;
                    }
                }
            }
        };
        let delivering = async {
            while !shutdown.is_cancelled() {
                match self.deliver_once().await {
                    Ok(0) => idle(&shutdown, &self.delivery_wake, poll).await,
                    Ok(_) => {}
                    Err(e) => {
                        tracing::error!(error = %e, "delivery pass failed");
                        idle(&shutdown, &self.delivery_wake, ERROR_PAUSE).await;
                    }
                }
            }
        };
        let timing = async {
            while !shutdown.is_cancelled() {
                match self.timers_once().await {
                    Ok(0) => {
                        tokio::select! {
                            () = shutdown.cancelled() => {}
                            () = tokio::time::sleep(poll) => {}
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::error!(error = %e, "timer pass failed");
                        tokio::select! {
                            () = shutdown.cancelled() => {}
                            () = tokio::time::sleep(ERROR_PAUSE) => {}
                        }
                    }
                }
            }
        };
        tokio::join!(routing, delivering, timing);
    }
}

async fn idle(shutdown: &CancellationToken, wake: &Notify, pause: Duration) {
    tokio::select! {
        () = shutdown.cancelled() => {}
        () = wake.notified() => {}
        () = tokio::time::sleep(pause) => {}
    }
}
