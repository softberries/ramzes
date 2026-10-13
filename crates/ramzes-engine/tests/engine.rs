use std::collections::VecDeque;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use ramzes_connectors::{
    Capabilities, ConnectionConfig, Connector, ConnectorDescriptor, Preview, Registry, SendOutcome,
};
use ramzes_core::{
    Event, EventStatus, Message, RetryPolicy, Route, RouteMatch, Severity, Target, new_id,
};
use ramzes_engine::{Change, Clock, Engine, EngineConfig, EngineError, Opener};
use ramzes_store::{Connection, DeliveryState, Store};
use serde_json::json;
use tokio_util::sync::CancellationToken;

struct FakeClock(AtomicI64);

impl Clock for FakeClock {
    fn now_ms(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

/// Test credentials are stored as plain JSON.
struct PlainOpener;

impl Opener for PlainOpener {
    fn open(&self, sealed: &str) -> Result<serde_json::Value, String> {
        serde_json::from_str(sealed).map_err(|e| e.to_string())
    }

    fn sign(&self, text: &str) -> String {
        format!("signed-{text}")
    }
}

/// A connector that answers from a script and records what it was asked to send.
/// Credentials token, destination and dedup key of an acknowledgement passed on.
type Told = (String, Option<String>, Option<String>);

#[derive(Default)]
struct Scripted {
    outcomes: Mutex<VecDeque<SendOutcome>>,
    sent: Mutex<Vec<(Message, Option<String>, serde_json::Value)>>,
    /// `(reference, note)` of every message it was asked to rewrite.
    updates: Mutex<Vec<(String, String)>>,
    /// `(credentials token, destination, dedup key)` of every acknowledgement it was told of.
    acknowledged: Mutex<Vec<Told>>,
}

#[async_trait]
impl Connector for Scripted {
    fn descriptor(&self) -> ConnectorDescriptor {
        ConnectorDescriptor {
            kind: "scripted".into(),
            name: "Scripted".into(),
            description: "".into(),
            fields: vec![],
            capabilities: Capabilities {
                rich: false,
                update: false,
                threads: false,
                buttons: false,
                max_len: 100,
            },
            destination_label: None,
            custom: false,
        }
    }

    fn validate(&self, _: &ConnectionConfig) -> Result<(), String> {
        Ok(())
    }

    fn preview(
        &self,
        _c: &ConnectionConfig,
        m: &Message,
        _destination: Option<&str>,
        _now_ms: i64,
    ) -> Result<Preview, String> {
        Ok(Preview {
            method: "POST".into(),
            url: "scripted://".into(),
            headers: Vec::new(),
            body: m.title.clone(),
        })
    }

    async fn send(
        &self,
        c: &ConnectionConfig,
        m: &Message,
        destination: Option<&str>,
        _: i64,
    ) -> SendOutcome {
        self.sent.lock().unwrap().push((
            m.clone(),
            destination.map(str::to_owned),
            c.credentials.clone(),
        ));
        self.outcomes
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(SendOutcome::Delivered {
                external_ref: Some("ref-1".into()),
                status: Some(200),
            })
    }

    async fn update(
        &self,
        _c: &ConnectionConfig,
        reference: &str,
        _m: &Message,
        note: &str,
    ) -> Result<(), String> {
        self.updates
            .lock()
            .unwrap()
            .push((reference.to_owned(), note.to_owned()));
        Ok(())
    }

    async fn acknowledge(
        &self,
        c: &ConnectionConfig,
        m: &Message,
        destination: Option<&str>,
        _now_ms: i64,
    ) -> Result<(), String> {
        self.acknowledged.lock().unwrap().push((
            c.credentials["token"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            destination.map(str::to_owned),
            m.dedup_key.clone(),
        ));
        Ok(())
    }
}

struct World {
    store: Store,
    engine: Arc<Engine>,
    clock: Arc<FakeClock>,
    connector: Arc<Scripted>,
    ws: String,
    source: String,
    _dir: tempfile::TempDir,
}

fn retry(detail: &str) -> SendOutcome {
    SendOutcome::Retry {
        after_ms: None,
        status: Some(503),
        detail: detail.into(),
    }
}

async fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::connect(&format!(
        "sqlite://{}",
        dir.path().join("ramzes.db").display()
    ))
    .await
    .unwrap();
    store.migrate().await.unwrap();
    let ws = store.ensure_workspace(0).await.unwrap();
    let source = store
        .create_source(&ws, "orders-api", "native", "hash", "rz_src_abcd", 0)
        .await
        .unwrap()
        .id;
    let clock = Arc::new(FakeClock(AtomicI64::new(1_000_000)));
    let connector = Arc::new(Scripted::default());
    let mut registry = Registry::builtin(Duration::from_secs(1));
    registry.add(connector.clone());
    let engine = Engine::new(EngineConfig {
        store: store.clone(),
        registry,
        opener: Arc::new(PlainOpener),
        clock: clock.clone(),
        policy: RetryPolicy::default(),
        public_url: Some("https://ramzes.example.com".into()),
        lease_ms: 60_000,
        jitter: Arc::new(|| 0.5),
    });
    World {
        store,
        engine,
        clock,
        connector,
        ws,
        source,
        _dir: dir,
    }
}

impl World {
    async fn connection(&self, name: &str) -> String {
        let c = Connection {
            id: new_id(),
            workspace_id: self.ws.clone(),
            kind: "scripted".into(),
            name: name.into(),
            settings: json!({}),
            credentials: json!({"token": name}).to_string(),
            created_at: 0,
            updated_at: 0,
        };
        self.store.create_connection(&c).await.unwrap();
        c.id
    }

    async fn route(&self, topic: &str, targets: &[(&str, Option<&str>)]) -> String {
        let r = Route {
            id: new_id(),
            name: format!("route {topic}"),
            position: 0,
            enabled: true,
            stop: false,
            r#match: RouteMatch {
                topic: Some(topic.into()),
                ..RouteMatch::default()
            },
            targets: targets
                .iter()
                .map(|(c, d)| Target {
                    connection_id: c.to_string(),
                    destination: d.map(str::to_owned),
                    template_id: None,
                })
                .collect(),
            ..Route::default()
        };
        self.store.create_route(&self.ws, &r, 0).await.unwrap();
        r.id
    }

    async fn event(&self, topic: &str) -> Event {
        let now = self.clock.now_ms();
        let e = Event {
            id: new_id(),
            workspace_id: self.ws.clone(),
            source_id: self.source.clone(),
            topic: topic.into(),
            severity: Severity::Error,
            status: EventStatus::Firing,
            title: format!("{topic} happened"),
            body: None,
            labels: [("env".to_string(), "prod".to_string())].into(),
            payload: json!(null),
            dedup_key: None,
            occurred_at: now,
            received_at: now,
        };
        self.store
            .insert_events(std::slice::from_ref(&e), None)
            .await
            .unwrap();
        e
    }

    fn advance(&self, ms: i64) {
        self.clock.0.fetch_add(ms, Ordering::SeqCst);
    }

    async fn states(&self, e: &Event) -> Vec<DeliveryState> {
        let j = self.store.journey(&self.ws, &e.id).await.unwrap().unwrap();
        j.notifications
            .iter()
            .flat_map(|n| n.deliveries.iter().map(|d| d.delivery.state))
            .collect()
    }
}

#[tokio::test]
async fn an_event_is_delivered_to_every_target_of_a_matching_route() {
    let w = world().await;
    let (c1, c2) = (w.connection("one").await, w.connection("two").await);
    w.route("orders.*", &[(&c1, Some("#ops")), (&c2, None)])
        .await;
    let e = w.event("orders.failed").await;

    assert_eq!(w.engine.route_once().await.unwrap(), 1);
    assert_eq!(w.engine.deliver_once().await.unwrap(), 2);

    assert_eq!(
        w.states(&e).await,
        vec![DeliveryState::Delivered, DeliveryState::Delivered]
    );
    let sent = w.connector.sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 2);
    let (message, _, _) = &sent[0];
    assert_eq!(message.title, "orders.failed happened");
    assert_eq!(
        message.fields,
        vec![("env".to_string(), "prod".to_string())]
    );
    assert_eq!(
        message.link.as_deref(),
        Some(format!("https://ramzes.example.com/events/{}", e.id).as_str())
    );
    let mut destinations: Vec<_> = sent.iter().map(|(_, d, _)| d.clone()).collect();
    destinations.sort();
    assert_eq!(destinations, vec![None, Some("#ops".to_string())]);
    // Each connection's own credentials were opened and handed to the connector.
    let mut tokens: Vec<_> = sent
        .iter()
        .map(|(_, _, c)| c["token"].as_str().unwrap().to_owned())
        .collect();
    tokens.sort();
    assert_eq!(tokens, vec!["one", "two"]);
}

#[tokio::test]
async fn the_delivery_keeps_the_external_reference() {
    let w = world().await;
    let c = w.connection("one").await;
    w.route("*", &[(&c, None)]).await;
    let e = w.event("a").await;
    w.engine.route_once().await.unwrap();
    w.engine.deliver_once().await.unwrap();
    let j = w.store.journey(&w.ws, &e.id).await.unwrap().unwrap();
    let d = &j.notifications[0].deliveries[0];
    assert_eq!(d.delivery.external_ref.as_deref(), Some("ref-1"));
    assert_eq!(d.attempts[0].outcome, "delivered");
    assert_eq!(d.attempts[0].status, Some(200));
}

#[tokio::test]
async fn an_unmatched_event_ends_routed_with_no_routes() {
    let w = world().await;
    let c = w.connection("one").await;
    w.route("billing.*", &[(&c, None)]).await;
    let e = w.event("orders.failed").await;
    assert_eq!(w.engine.route_once().await.unwrap(), 1);
    assert_eq!(w.engine.route_once().await.unwrap(), 0);
    let j = w.store.journey(&w.ws, &e.id).await.unwrap().unwrap();
    assert!(j.routed);
    assert_eq!(j.route_count, 0);
    assert_eq!(w.engine.deliver_once().await.unwrap(), 0);
}

#[tokio::test]
async fn a_retryable_failure_is_rescheduled_and_delivered_later() {
    let w = world().await;
    let c = w.connection("one").await;
    w.route("*", &[(&c, None)]).await;
    w.connector
        .outcomes
        .lock()
        .unwrap()
        .push_back(retry("HTTP 503"));
    let e = w.event("a").await;
    w.engine.route_once().await.unwrap();
    w.engine.deliver_once().await.unwrap();

    let j = w.store.journey(&w.ws, &e.id).await.unwrap().unwrap();
    let d = &j.notifications[0].deliveries[0];
    assert_eq!(d.delivery.state, DeliveryState::Pending);
    assert_eq!(d.delivery.next_attempt_at, 1_000_000 + 5_000);
    assert_eq!(d.delivery.last_error.as_deref(), Some("HTTP 503"));
    assert_eq!(d.attempts[0].outcome, "retry");

    w.advance(4_999);
    assert_eq!(w.engine.deliver_once().await.unwrap(), 0);
    w.advance(1);
    assert_eq!(w.engine.deliver_once().await.unwrap(), 1);
    assert_eq!(w.states(&e).await, vec![DeliveryState::Delivered]);
}

#[tokio::test]
async fn the_service_can_ask_for_a_longer_delay() {
    let w = world().await;
    let c = w.connection("one").await;
    w.route("*", &[(&c, None)]).await;
    let slow = SendOutcome::Retry {
        after_ms: Some(90_000),
        status: Some(429),
        detail: "HTTP 429".into(),
    };
    w.connector.outcomes.lock().unwrap().push_back(slow);
    let e = w.event("a").await;
    w.engine.route_once().await.unwrap();
    w.engine.deliver_once().await.unwrap();
    let j = w.store.journey(&w.ws, &e.id).await.unwrap().unwrap();
    assert_eq!(
        j.notifications[0].deliveries[0].delivery.next_attempt_at,
        1_000_000 + 90_000
    );
}

#[tokio::test]
async fn retries_are_exhausted_after_eight_attempts() {
    let w = world().await;
    let c = w.connection("one").await;
    w.route("*", &[(&c, None)]).await;
    for _ in 0..8 {
        w.connector
            .outcomes
            .lock()
            .unwrap()
            .push_back(retry("HTTP 503"));
    }
    let e = w.event("a").await;
    w.engine.route_once().await.unwrap();
    for _ in 0..8 {
        w.engine.deliver_once().await.unwrap();
        w.advance(3_600_000);
    }
    let j = w.store.journey(&w.ws, &e.id).await.unwrap().unwrap();
    let d = &j.notifications[0].deliveries[0];
    assert_eq!(d.delivery.state, DeliveryState::Dead);
    assert_eq!(d.delivery.attempts, 8);
    assert!(
        d.delivery
            .last_error
            .as_deref()
            .unwrap()
            .contains("HTTP 503")
    );
    assert_eq!(w.engine.deliver_once().await.unwrap(), 0);
}

#[tokio::test]
async fn a_rejection_fails_after_one_attempt() {
    let w = world().await;
    let c = w.connection("one").await;
    w.route("*", &[(&c, None)]).await;
    let rejected = SendOutcome::Permanent {
        status: Some(401),
        detail: "HTTP 401: invalid_token".into(),
    };
    w.connector.outcomes.lock().unwrap().push_back(rejected);
    let e = w.event("a").await;
    w.engine.route_once().await.unwrap();
    w.engine.deliver_once().await.unwrap();
    let j = w.store.journey(&w.ws, &e.id).await.unwrap().unwrap();
    let d = &j.notifications[0].deliveries[0];
    assert_eq!(d.delivery.state, DeliveryState::Failed);
    assert_eq!(d.delivery.attempts, 1);
    assert_eq!(
        d.delivery.last_error.as_deref(),
        Some("HTTP 401: invalid_token")
    );
    assert_eq!(d.attempts[0].outcome, "failed");
    assert_eq!(d.attempts[0].status, Some(401));
}

#[tokio::test]
async fn a_deleted_connection_fails_the_delivery_and_the_rest_continue() {
    let w = world().await;
    let (gone, kept) = (w.connection("gone").await, w.connection("kept").await);
    w.route("*", &[(&gone, None), (&kept, None)]).await;
    let e = w.event("a").await;
    w.engine.route_once().await.unwrap();
    w.store.delete_connection(&w.ws, &gone).await.unwrap();

    assert_eq!(w.engine.deliver_once().await.unwrap(), 2);
    let j = w.store.journey(&w.ws, &e.id).await.unwrap().unwrap();
    for n in &j.notifications {
        let d = &n.deliveries[0].delivery;
        if n.connection_id == gone {
            assert_eq!(d.state, DeliveryState::Failed);
            assert_eq!(d.last_error.as_deref(), Some("the connection was deleted"));
        } else {
            assert_eq!(d.state, DeliveryState::Delivered);
        }
    }
    assert_eq!(w.connector.sent.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn an_unknown_connector_kind_fails_the_delivery() {
    let w = world().await;
    let c = Connection {
        id: new_id(),
        workspace_id: w.ws.clone(),
        kind: "carrier-pigeon".into(),
        name: "x".into(),
        settings: json!({}),
        credentials: "{}".into(),
        created_at: 0,
        updated_at: 0,
    };
    w.store.create_connection(&c).await.unwrap();
    w.route("*", &[(&c.id, None)]).await;
    let e = w.event("a").await;
    w.engine.route_once().await.unwrap();
    w.engine.deliver_once().await.unwrap();
    let j = w.store.journey(&w.ws, &e.id).await.unwrap().unwrap();
    let d = &j.notifications[0].deliveries[0].delivery;
    assert_eq!(d.state, DeliveryState::Failed);
    assert!(d.last_error.as_deref().unwrap().contains("carrier-pigeon"));
}

#[tokio::test]
async fn unreadable_credentials_fail_the_delivery() {
    let w = world().await;
    let c = Connection {
        id: new_id(),
        workspace_id: w.ws.clone(),
        kind: "scripted".into(),
        name: "x".into(),
        settings: json!({}),
        credentials: "not json".into(),
        created_at: 0,
        updated_at: 0,
    };
    w.store.create_connection(&c).await.unwrap();
    w.route("*", &[(&c.id, None)]).await;
    let e = w.event("a").await;
    w.engine.route_once().await.unwrap();
    w.engine.deliver_once().await.unwrap();
    assert_eq!(w.states(&e).await, vec![DeliveryState::Failed]);
    assert!(w.connector.sent.lock().unwrap().is_empty());
}

#[tokio::test]
async fn replaying_a_delivery_sends_again() {
    let w = world().await;
    let c = w.connection("one").await;
    w.route("*", &[(&c, None)]).await;
    let e = w.event("a").await;
    w.engine.route_once().await.unwrap();
    w.engine.deliver_once().await.unwrap();
    let j = w.store.journey(&w.ws, &e.id).await.unwrap().unwrap();
    let first = j.notifications[0].deliveries[0].delivery.id.clone();

    let fresh = w.engine.replay_delivery(&w.ws, &first).await.unwrap();
    assert_ne!(fresh.id, first);
    assert_eq!(w.engine.deliver_once().await.unwrap(), 1);
    assert_eq!(
        w.states(&e).await,
        vec![DeliveryState::Delivered, DeliveryState::Delivered]
    );
    assert_eq!(w.connector.sent.lock().unwrap().len(), 2);
    assert!(matches!(
        w.engine.replay_delivery(&w.ws, "nope").await,
        Err(EngineError::NotFound)
    ));
}

#[tokio::test]
async fn replaying_an_event_routes_it_again_through_current_routes() {
    let w = world().await;
    let c = w.connection("one").await;
    let e = w.event("a").await;
    w.engine.route_once().await.unwrap();
    assert!(w.states(&e).await.is_empty());

    // The route is added after the event was first routed.
    w.route("*", &[(&c, None)]).await;
    assert_eq!(w.engine.replay_event(&w.ws, &e.id).await.unwrap(), 1);
    w.engine.deliver_once().await.unwrap();
    let j = w.store.journey(&w.ws, &e.id).await.unwrap().unwrap();
    assert_eq!(j.notifications.len(), 1);
    assert!(j.notifications[0].replay);
    assert_eq!(
        j.notifications[0].deliveries[0].delivery.state,
        DeliveryState::Delivered
    );
    assert!(matches!(
        w.engine.replay_event(&w.ws, "nope").await,
        Err(EngineError::NotFound)
    ));
}

#[tokio::test]
async fn changes_are_broadcast() {
    let w = world().await;
    let c = w.connection("one").await;
    w.route("*", &[(&c, None)]).await;
    let mut changes = w.engine.subscribe();
    let e = w.event("a").await;
    w.engine.route_once().await.unwrap();
    w.engine.deliver_once().await.unwrap();

    assert_eq!(
        changes.recv().await.unwrap(),
        Change::EventRouted {
            event_id: e.id.clone()
        }
    );
    assert!(matches!(
        changes.recv().await.unwrap(),
        Change::NotificationChanged { .. }
    ));
    match changes.recv().await.unwrap() {
        Change::DeliveryUpdated {
            event_ids, state, ..
        } => {
            assert_eq!(event_ids, vec![e.id.clone()]);
            assert_eq!(state, DeliveryState::Delivered);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn wake_makes_the_running_engine_process_without_waiting_for_the_poll() {
    let w = world().await;
    let c = w.connection("one").await;
    w.route("*", &[(&c, None)]).await;
    let shutdown = CancellationToken::new();
    // A poll interval far longer than the test: only a wake can trigger processing.
    let running = tokio::spawn(
        w.engine
            .clone()
            .run(shutdown.clone(), Duration::from_secs(3600)),
    );
    tokio::time::sleep(Duration::from_millis(100)).await;

    let e = w.event("a").await;
    w.engine.wake();
    let mut delivered = false;
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(20)).await;
        if w.states(&e).await == vec![DeliveryState::Delivered] {
            delivered = true;
            break;
        }
    }
    assert!(delivered, "the event was not delivered after wake");

    shutdown.cancel();
    tokio::time::timeout(Duration::from_secs(5), running)
        .await
        .expect("run did not stop")
        .unwrap();
}

#[tokio::test]
async fn retention_honours_the_instance_setting_and_topic_overrides() {
    let w = world().await;
    const DAY: i64 = 86_400_000;
    w.clock.0.store(100 * DAY, Ordering::SeqCst);
    w.store
        .set_setting(&w.ws, "retention_days", "30")
        .await
        .unwrap();
    w.store
        .set_setting(&w.ws, "retention_overrides", r#"{"audit": 0, "noise": 1}"#)
        .await
        .unwrap();

    w.clock.0.store(60 * DAY, Ordering::SeqCst);
    let old = w.event("orders.failed").await;
    let old_audit = w.event("audit").await;
    w.clock.0.store(98 * DAY, Ordering::SeqCst);
    let recent = w.event("orders.failed").await;
    let stale_noise = w.event("noise").await;
    w.clock.0.store(100 * DAY, Ordering::SeqCst);
    let fresh_noise = w.event("noise").await;

    assert_eq!(w.engine.retention_once(&w.ws).await.unwrap(), 2);
    let exists = |e: &Event| {
        let (store, ws, id) = (w.store.clone(), w.ws.clone(), e.id.clone());
        async move { store.get_event(&ws, &id).await.unwrap().is_some() }
    };
    assert!(!exists(&old).await, "older than 30 days");
    assert!(exists(&old_audit).await, "audit is kept forever");
    assert!(exists(&recent).await);
    assert!(!exists(&stale_noise).await, "noise is kept one day");
    assert!(exists(&fresh_noise).await);
}

#[tokio::test]
async fn retention_of_zero_days_keeps_everything() {
    let w = world().await;
    w.store
        .set_setting(&w.ws, "retention_days", "0")
        .await
        .unwrap();
    w.clock.0.store(1, Ordering::SeqCst);
    w.event("a").await;
    w.clock.0.store(10_000 * 86_400_000, Ordering::SeqCst);
    assert_eq!(w.engine.retention_once(&w.ws).await.unwrap(), 0);
}

#[tokio::test]
async fn nul_characters_in_a_service_answer_are_not_stored() {
    let w = world().await;
    let c = w.connection("one").await;
    w.route("*", &[(&c, None)]).await;
    let binary = SendOutcome::Permanent {
        status: Some(400),
        detail: "HTTP 400: bad\u{0}body".into(),
    };
    w.connector.outcomes.lock().unwrap().push_back(binary);
    let e = w.event("a").await;
    w.engine.route_once().await.unwrap();
    w.engine.deliver_once().await.unwrap();
    let j = w.store.journey(&w.ws, &e.id).await.unwrap().unwrap();
    let d = &j.notifications[0].deliveries[0];
    assert_eq!(d.delivery.state, DeliveryState::Failed);
    assert_eq!(d.delivery.last_error.as_deref(), Some("HTTP 400: badbody"));
    assert_eq!(d.attempts[0].detail.as_deref(), Some("HTTP 400: badbody"));
}

#[tokio::test]
async fn a_delivery_past_its_maximum_age_is_given_up_without_sending() {
    let w = world().await;
    let c = w.connection("one").await;
    w.route("*", &[(&c, None)]).await;
    let e = w.event("a").await;
    w.engine.route_once().await.unwrap();
    // Nothing could be recorded for more than the 24 hours a delivery may be retried.
    w.advance(24 * 3_600_000 + 1);
    assert_eq!(w.engine.deliver_once().await.unwrap(), 1);

    assert!(w.connector.sent.lock().unwrap().is_empty());
    let j = w.store.journey(&w.ws, &e.id).await.unwrap().unwrap();
    let d = &j.notifications[0].deliveries[0];
    assert_eq!(d.delivery.state, DeliveryState::Dead);
    assert!(
        d.delivery.last_error.as_deref().unwrap().contains("24 h"),
        "{:?}",
        d.delivery.last_error
    );
    assert_eq!(w.engine.deliver_once().await.unwrap(), 0);
}

// ---- milestone 2: expressions, templates, connection limits ----

impl World {
    async fn route_with(
        &self,
        m: RouteMatch,
        connection: &str,
        template_id: Option<&str>,
    ) -> String {
        let r = Route {
            id: new_id(),
            name: "custom".into(),
            enabled: true,
            r#match: m,
            targets: vec![Target {
                connection_id: connection.into(),
                destination: None,
                template_id: template_id.map(str::to_owned),
            }],
            ..Route::default()
        };
        self.store.create_route(&self.ws, &r, 0).await.unwrap();
        r.id
    }

    async fn template(&self, title: &str, body: &str) -> String {
        let t = ramzes_core::Template {
            id: new_id(),
            name: "ops wording".into(),
            title: title.into(),
            body: body.into(),
        };
        self.store.create_template(&self.ws, &t, 0).await.unwrap();
        t.id
    }

    async fn event_with_payload(&self, payload: serde_json::Value) -> Event {
        let mut e = self.event("orders.failed").await;
        // Stored again under a new id with the payload set.
        e.id = new_id();
        e.payload = payload;
        self.store
            .insert_events(std::slice::from_ref(&e), None)
            .await
            .unwrap();
        e
    }

    async fn warning(&self, e: &Event) -> Option<String> {
        let j = self.store.journey(&self.ws, &e.id).await.unwrap().unwrap();
        j.notifications[0].deliveries[0].delivery.warning.clone()
    }
}

fn expr(source: &str) -> RouteMatch {
    RouteMatch {
        expr: Some(source.into()),
        ..RouteMatch::default()
    }
}

#[tokio::test]
async fn a_route_expression_selects_events_by_payload() {
    let w = world().await;
    let c = w.connection("one").await;
    w.route_with(expr("payload.amount > 100"), &c, None).await;
    let big = w.event_with_payload(json!({"amount": 500})).await;
    let small = w.event_with_payload(json!({"amount": 5})).await;
    w.engine.route_once().await.unwrap();
    w.engine.deliver_once().await.unwrap();
    assert_eq!(w.states(&big).await, vec![DeliveryState::Delivered]);
    assert!(w.states(&small).await.is_empty());
}

#[tokio::test]
async fn a_failing_route_expression_is_recorded_on_the_route_and_other_routes_still_fire() {
    let w = world().await;
    let c = w.connection("one").await;
    let broken = w
        .route_with(expr("payload.amount | nosuchfilter"), &c, None)
        .await;
    w.route("*", &[(&c, None)]).await;
    let e = w.event("orders.failed").await;
    w.engine.route_once().await.unwrap();
    assert_eq!(w.states(&e).await.len(), 1);
    let errors = w.store.route_errors(&w.ws).await.unwrap();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].0, broken);
    assert!(errors[0].1.contains("nosuchfilter"), "{}", errors[0].1);
}

#[tokio::test]
async fn a_template_words_the_message() {
    let w = world().await;
    let c = w.connection("one").await;
    let t = w
        .template(
            "[{{ labels.env }}] {{ title }}",
            "amount {{ payload.amount }}",
        )
        .await;
    w.route_with(RouteMatch::default(), &c, Some(&t)).await;
    let e = w.event_with_payload(json!({"amount": 7})).await;
    w.engine.route_once().await.unwrap();
    w.engine.deliver_once().await.unwrap();
    let sent = w.connector.sent.lock().unwrap().clone();
    let (message, _, _) = sent
        .iter()
        .find(|(m, _, _)| m.event_ids == vec![e.id.clone()])
        .unwrap();
    assert_eq!(message.title, "[prod] orders.failed happened");
    assert_eq!(message.body.as_deref(), Some("amount 7"));
    assert_eq!(w.warning(&e).await, None);
}

#[tokio::test]
async fn a_template_that_fails_sends_the_default_wording_with_a_warning() {
    let w = world().await;
    let c = w.connection("one").await;
    let t = w.template("{{ title | nosuchfilter }}", "").await;
    w.route_with(RouteMatch::default(), &c, Some(&t)).await;
    let e = w.event("orders.failed").await;
    w.engine.route_once().await.unwrap();
    w.engine.deliver_once().await.unwrap();
    assert_eq!(w.states(&e).await, vec![DeliveryState::Delivered]);
    let sent = w.connector.sent.lock().unwrap().clone();
    assert_eq!(sent[0].0.title, "orders.failed happened");
    let warning = w.warning(&e).await.unwrap();
    assert!(
        warning.contains("ops wording") && warning.contains("default wording"),
        "{warning}"
    );
}

#[tokio::test]
async fn a_deleted_template_sends_the_default_wording_with_a_warning() {
    let w = world().await;
    let c = w.connection("one").await;
    let t = w.template("x {{ title }}", "").await;
    w.route_with(RouteMatch::default(), &c, Some(&t)).await;
    let e = w.event("orders.failed").await;
    w.engine.route_once().await.unwrap();
    w.store.delete_template(&w.ws, &t).await.unwrap();
    w.engine.deliver_once().await.unwrap();
    assert_eq!(
        w.connector.sent.lock().unwrap()[0].0.title,
        "orders.failed happened"
    );
    assert!(w.warning(&e).await.unwrap().contains("deleted"));
}

/// A connector that takes a while and counts how many sends overlap.
#[derive(Default)]
struct Slow {
    in_flight: std::sync::atomic::AtomicUsize,
    most: std::sync::atomic::AtomicUsize,
}

#[async_trait]
impl Connector for Slow {
    fn descriptor(&self) -> ConnectorDescriptor {
        ConnectorDescriptor {
            kind: "slow".into(),
            name: "Slow".into(),
            description: String::new(),
            fields: Vec::new(),
            capabilities: Capabilities {
                rich: false,
                update: false,
                threads: false,
                buttons: false,
                max_len: 100,
            },
            destination_label: None,
            custom: false,
        }
    }

    fn validate(&self, _c: &ConnectionConfig) -> Result<(), String> {
        Ok(())
    }

    fn preview(
        &self,
        _c: &ConnectionConfig,
        _m: &Message,
        _d: Option<&str>,
        _now: i64,
    ) -> Result<Preview, String> {
        Err("no preview".into())
    }

    async fn send(
        &self,
        _c: &ConnectionConfig,
        _m: &Message,
        _d: Option<&str>,
        _now: i64,
    ) -> SendOutcome {
        let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.most.fetch_max(now, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(40)).await;
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
        SendOutcome::Delivered {
            external_ref: None,
            status: Some(200),
        }
    }
}

#[tokio::test]
async fn a_connection_has_at_most_four_sends_in_flight() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::connect(&format!(
        "sqlite://{}",
        dir.path().join("ramzes.db").display()
    ))
    .await
    .unwrap();
    store.migrate().await.unwrap();
    let ws = store.ensure_workspace(0).await.unwrap();
    let source = store
        .create_source(&ws, "s", "native", "hash", "rz_src_abcd", 0)
        .await
        .unwrap()
        .id;
    let slow = Arc::new(Slow::default());
    let mut registry = Registry::builtin(Duration::from_secs(1));
    registry.add(slow.clone());
    let clock = Arc::new(FakeClock(AtomicI64::new(1_000_000)));
    let engine = Engine::new(EngineConfig {
        store: store.clone(),
        registry,
        opener: Arc::new(PlainOpener),
        clock: clock.clone(),
        policy: RetryPolicy::default(),
        public_url: None,
        lease_ms: 60_000,
        jitter: Arc::new(|| 0.5),
    });
    let connection = Connection {
        id: new_id(),
        workspace_id: ws.clone(),
        kind: "slow".into(),
        name: "slow".into(),
        settings: json!({}),
        credentials: "{}".into(),
        created_at: 0,
        updated_at: 0,
    };
    store.create_connection(&connection).await.unwrap();
    let route = Route {
        id: new_id(),
        name: "all".into(),
        enabled: true,
        targets: vec![Target {
            connection_id: connection.id.clone(),
            ..Target::default()
        }],
        ..Route::default()
    };
    store.create_route(&ws, &route, 0).await.unwrap();
    let events: Vec<Event> = (0..12)
        .map(|i| Event {
            id: new_id(),
            workspace_id: ws.clone(),
            source_id: source.clone(),
            topic: "a".into(),
            severity: Severity::Info,
            status: EventStatus::Firing,
            title: format!("event {i}"),
            body: None,
            labels: Default::default(),
            payload: json!(null),
            dedup_key: None,
            occurred_at: 0,
            received_at: 0,
        })
        .collect();
    store.insert_events(&events, None).await.unwrap();
    engine.route_once().await.unwrap();
    assert_eq!(engine.deliver_once().await.unwrap(), 12);
    assert_eq!(slow.most.load(Ordering::SeqCst), 4);
}

// ---- milestone 3: shaping, timers, pausing, system events ----

use ramzes_core::shaping::{NotificationState, Shaping, Silence};
use ramzes_store::{EventFilter, NotificationFilter};

const MINUTE: i64 = 60_000;
/// 2026-10-13 12:00 UTC, 14:00 in Warsaw.
const NOON: i64 = 1_791_892_800_000;

fn shaping(policy: serde_json::Value) -> Shaping {
    serde_json::from_value(policy).unwrap()
}

impl World {
    async fn shaped(&self, connection: &str, policy: serde_json::Value) -> String {
        let r = Route {
            id: new_id(),
            name: "shaped".into(),
            enabled: true,
            r#match: RouteMatch {
                topic: Some("orders.*".into()),
                ..RouteMatch::default()
            },
            targets: vec![Target {
                connection_id: connection.into(),
                ..Target::default()
            }],
            shaping: shaping(policy),
            ..Route::default()
        };
        self.store.create_route(&self.ws, &r, 0).await.unwrap();
        r.id
    }

    /// Stores an event on `orders.failed` after applying `change`.
    async fn send(&self, title: &str, change: impl FnOnce(&mut Event)) -> Event {
        let now = self.clock.now_ms();
        let mut e = Event {
            id: new_id(),
            workspace_id: self.ws.clone(),
            source_id: self.source.clone(),
            topic: "orders.failed".into(),
            severity: Severity::Error,
            status: EventStatus::Firing,
            title: title.into(),
            body: None,
            labels: [("service".to_string(), "pay".to_string())].into(),
            payload: json!(null),
            dedup_key: None,
            occurred_at: now,
            received_at: now,
        };
        change(&mut e);
        self.store
            .insert_events(std::slice::from_ref(&e), None)
            .await
            .unwrap();
        e
    }

    /// Runs every worker until nothing is left to do at the current time.
    async fn settle(&self) {
        for _ in 0..10 {
            let done = self.engine.route_once().await.unwrap()
                + self.engine.timers_once().await.unwrap()
                + self.engine.deliver_once().await.unwrap();
            if done == 0 {
                return;
            }
        }
        panic!("the engine did not settle");
    }

    fn messages(&self) -> Vec<Message> {
        self.connector
            .sent
            .lock()
            .unwrap()
            .iter()
            .map(|(m, _, _)| m.clone())
            .collect()
    }

    async fn notifications(&self) -> Vec<ramzes_store::NotificationRow> {
        let mut rows = self
            .store
            .list_notifications(&self.ws, &NotificationFilter::default())
            .await
            .unwrap();
        rows.reverse();
        rows
    }
}

#[tokio::test]
async fn a_group_is_sent_once_after_its_wait_with_every_event() {
    let w = world().await;
    let c = w.connection("one").await;
    w.shaped(&c, json!({"group": {"by": ["service"], "wait_s": 30}}))
        .await;
    let a = w.send("first", |_| {}).await;
    w.settle().await;
    w.advance(10_000);
    let b = w.send("second", |_| {}).await;
    let other = w
        .send("other service", |e| {
            e.labels.insert("service".into(), "ship".into());
        })
        .await;
    w.settle().await;
    assert!(w.messages().is_empty());
    let rows = w.notifications().await;
    assert_eq!(rows.len(), 2);
    assert_eq!(
        (rows[0].state, rows[0].event_count),
        (NotificationState::Grouping, 2)
    );

    w.advance(20_000);
    w.settle().await;
    let sent = w.messages();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].title, "2 events");
    assert_eq!(sent[0].event_ids, vec![a.id.clone(), b.id.clone()]);
    assert_eq!(
        sent[0].link.as_deref(),
        Some(format!("https://ramzes.example.com/notifications/{}", rows[0].id).as_str())
    );

    // The second group flushes at its own time, and a later event starts a new group.
    w.advance(10_000);
    w.settle().await;
    assert_eq!(w.messages().len(), 2);
    assert_eq!(w.messages()[1].event_ids, vec![other.id.clone()]);
    w.send("third", |_| {}).await;
    w.settle().await;
    assert_eq!(w.messages().len(), 2);
    assert_eq!(w.notifications().await.len(), 3);
}

#[tokio::test]
async fn a_duplicate_is_counted_and_not_sent_until_the_window_is_over() {
    let w = world().await;
    let c = w.connection("one").await;
    w.shaped(&c, json!({"dedup": {"window_s": 600}})).await;
    let key = |e: &mut Event| e.dedup_key = Some("order-17".into());
    w.send("failed", key).await;
    w.settle().await;
    let again = w.send("failed again", key).await;
    w.settle().await;
    assert_eq!(w.messages().len(), 1);
    let rows = w.notifications().await;
    assert_eq!((rows.len(), rows[0].event_count), (1, 2));
    let journey = w.store.journey(&w.ws, &again.id).await.unwrap().unwrap();
    assert_eq!(journey.notifications[0].role, "duplicate");
    assert!(journey.notifications[0].deliveries.len() == 1);

    w.advance(601_000);
    w.send("failed much later", key).await;
    w.settle().await;
    assert_eq!(w.messages().len(), 2);
}

#[tokio::test]
async fn a_resolved_event_tells_the_target_only_when_it_had_been_told() {
    let w = world().await;
    let c = w.connection("one").await;
    w.shaped(&c, json!({"dedup": {"window_s": 600}})).await;
    let down = |e: &mut Event| e.dedup_key = Some("db".into());
    let up = |e: &mut Event| {
        e.dedup_key = Some("db".into());
        e.status = EventStatus::Resolved;
    };
    w.send("Database down", down).await;
    w.settle().await;
    w.send("Database up", up).await;
    w.settle().await;
    let sent = w.messages();
    assert_eq!(sent.len(), 2);
    assert_eq!(
        (sent[1].title.as_str(), sent[1].status),
        ("Database down", EventStatus::Resolved)
    );
    assert_eq!(
        w.notifications().await[0].state,
        NotificationState::Resolved
    );

    // Resolved again with nothing open: recorded, not sent.
    w.send("Database up", up).await;
    w.settle().await;
    assert_eq!(w.messages().len(), 2);
    let rows = w.notifications().await;
    assert_eq!(rows[1].state, NotificationState::Suppressed);
    // After a resolution the same key opens a new notification.
    w.send("Database down", down).await;
    w.settle().await;
    assert_eq!(w.messages().len(), 3);
}

#[tokio::test]
async fn something_resolved_while_still_grouping_is_never_sent() {
    let w = world().await;
    let c = w.connection("one").await;
    w.shaped(&c, json!({"group": {"by": [], "wait_s": 30}}))
        .await;
    w.send("Database down", |e| e.dedup_key = Some("db".into()))
        .await;
    w.settle().await;
    w.send("Database up", |e| {
        e.dedup_key = Some("db".into());
        e.status = EventStatus::Resolved;
    })
    .await;
    w.settle().await;
    w.advance(60_000);
    w.settle().await;
    assert!(w.messages().is_empty());
    let rows = w.notifications().await;
    assert_eq!(
        (rows.len(), rows[0].state, rows[0].event_count),
        (1, NotificationState::Resolved, 2)
    );
}

#[tokio::test]
async fn a_silence_suppresses_matching_events_while_it_lasts() {
    let w = world().await;
    let c = w.connection("one").await;
    w.route("orders.*", &[(&c, None)]).await;
    let now = w.clock.now_ms();
    let silence = Silence {
        id: new_id(),
        r#match: RouteMatch {
            topic: Some("orders.*".into()),
            ..RouteMatch::default()
        },
        starts_at: now,
        ends_at: now + 10 * MINUTE,
        comment: Some("deploy".into()),
    };
    w.store
        .create_silence(&w.ws, &silence, "kris", now)
        .await
        .unwrap();
    let quiet = w.send("during the deploy", |_| {}).await;
    w.settle().await;
    assert!(w.messages().is_empty());
    let journey = w.store.journey(&w.ws, &quiet.id).await.unwrap().unwrap();
    assert_eq!(journey.notifications[0].state, "suppressed");
    assert_eq!(
        journey.notifications[0].reason.as_deref(),
        Some("silenced: deploy")
    );
    assert!(journey.notifications[0].deliveries.is_empty());

    w.advance(11 * MINUTE);
    w.send("after the deploy", |_| {}).await;
    w.settle().await;
    assert_eq!(w.messages().len(), 1);
}

#[tokio::test]
async fn quiet_hours_hold_everything_and_release_one_digest() {
    let w = world().await;
    w.clock.0.store(NOON, Ordering::SeqCst);
    let c = w.connection("one").await;
    w.shaped(
        &c,
        json!({"quiet_hours": {"tz": "Europe/Warsaw", "start": "13:00", "end": "15:00", "bypass_severity": "critical"}}),
    )
    .await;
    w.send("first", |_| {}).await;
    w.settle().await;
    w.advance(10 * MINUTE);
    w.send("second", |_| {}).await;
    let urgent = w.send("urgent", |e| e.severity = Severity::Critical).await;
    w.settle().await;
    // Only the critical one went out.
    assert_eq!(w.messages().len(), 1);
    assert_eq!(w.messages()[0].event_ids, vec![urgent.id.clone()]);
    let rows = w.notifications().await;
    let held = rows
        .iter()
        .find(|n| n.state == NotificationState::Held)
        .unwrap();
    assert_eq!(held.event_count, 2);
    assert_eq!(held.held_until, Some(NOON + 60 * MINUTE));
    assert!(
        held.reason.as_deref().unwrap().contains("15:00"),
        "{:?}",
        held.reason
    );

    w.advance(49 * MINUTE);
    w.settle().await;
    assert_eq!(w.messages().len(), 1);
    w.advance(MINUTE);
    w.settle().await;
    let sent = w.messages();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[1].event_ids.len(), 2);
    // Outside quiet hours an event goes straight out.
    w.send("afternoon", |_| {}).await;
    w.settle().await;
    assert_eq!(w.messages().len(), 3);
}

#[tokio::test]
async fn over_the_rate_limit_the_rest_arrive_as_one_summary() {
    let w = world().await;
    let c = w.connection("one").await;
    w.shaped(&c, json!({"rate_limit": {"max": 2, "window_s": 600}}))
        .await;
    for i in 0..5 {
        w.send(&format!("event {i}"), |_| {}).await;
        w.settle().await;
        w.advance(1000);
    }
    assert_eq!(w.messages().len(), 2);
    let rows = w.notifications().await;
    assert_eq!(rows.len(), 3);
    assert_eq!(
        (rows[2].state, rows[2].event_count),
        (NotificationState::Held, 3)
    );

    w.advance(600_000);
    w.settle().await;
    let sent = w.messages();
    assert_eq!(sent.len(), 3);
    assert_eq!(sent[2].title, "3 events");
}

#[tokio::test]
async fn a_group_that_ends_in_quiet_hours_joins_what_is_already_held() {
    let w = world().await;
    w.clock.0.store(NOON - 20_000, Ordering::SeqCst);
    let c = w.connection("one").await;
    // Quiet from 14:00 Warsaw, which is NOON. The group opens just before and flushes inside.
    w.shaped(
        &c,
        json!({"group": {"by": ["service"], "wait_s": 30}, "quiet_hours": {"tz": "Europe/Warsaw", "start": "14:00", "end": "15:00"}}),
    )
    .await;
    w.send("pay 1", |_| {}).await;
    w.send("ship 1", |e| {
        e.labels.insert("service".into(), "ship".into());
    })
    .await;
    w.settle().await;
    w.advance(40_000);
    w.settle().await;
    assert!(w.messages().is_empty());
    let rows = w.notifications().await;
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(
        (rows[0].state, rows[0].event_count),
        (NotificationState::Held, 2)
    );

    w.clock.0.store(NOON + 60 * MINUTE, Ordering::SeqCst);
    w.settle().await;
    assert_eq!(w.messages().len(), 1);
    assert_eq!(w.messages()[0].event_ids.len(), 2);
}

#[tokio::test]
async fn an_open_group_is_repeated_until_it_is_acknowledged() {
    let w = world().await;
    let c = w.connection("one").await;
    w.shaped(
        &c,
        json!({"group": {"by": [], "wait_s": 10, "repeat_s": 600}}),
    )
    .await;
    w.send("Database down", |_| {}).await;
    w.settle().await;
    w.advance(10_000);
    w.settle().await;
    assert_eq!(w.messages().len(), 1);
    w.advance(600_000);
    w.settle().await;
    assert_eq!(w.messages().len(), 2);
    let id = w.notifications().await[0].id.clone();
    w.store
        .acknowledge(&w.ws, &id, "kris", w.clock.now_ms())
        .await
        .unwrap();
    w.advance(1_200_000);
    w.settle().await;
    assert_eq!(w.messages().len(), 2);
}

#[tokio::test]
async fn a_timer_for_a_notification_that_is_gone_does_nothing() {
    let w = world().await;
    w.store
        .add_timer(&w.ws, "deleted-by-retention", "flush", 0, None)
        .await
        .unwrap();
    assert_eq!(w.engine.timers_once().await.unwrap(), 1);
    assert_eq!(w.engine.timers_once().await.unwrap(), 0);
    assert!(w.messages().is_empty());
}

#[tokio::test]
async fn a_replay_is_sent_whatever_the_shaping_says() {
    let w = world().await;
    let c = w.connection("one").await;
    w.shaped(&c, json!({"group": {"by": [], "wait_s": 3600}}))
        .await;
    let e = w.send("held back", |_| {}).await;
    w.settle().await;
    assert!(w.messages().is_empty());
    assert_eq!(w.engine.replay_event(&w.ws, &e.id).await.unwrap(), 1);
    w.settle().await;
    assert_eq!(w.messages().len(), 1);
}

#[tokio::test]
async fn a_failing_connection_is_paused_reported_once_and_resumes_when_it_answers() {
    let w = world().await;
    let c = w.connection("flaky").await;
    w.route("orders.*", &[(&c, None)]).await;
    for _ in 0..5 {
        w.connector.outcomes.lock().unwrap().push_back(retry("503"));
    }
    for i in 0..5 {
        w.send(&format!("event {i}"), |_| {}).await;
    }
    w.settle().await;
    let status = w.store.connection_status(&w.ws).await.unwrap();
    let paused_until = status[&c].paused_until.unwrap();
    assert_eq!(paused_until, w.clock.now_ms() + 60_000);
    assert_eq!(status[&c].failure_streak, 5);
    let system = w
        .store
        .list_events(
            &w.ws,
            &EventFilter {
                topic: Some("ramzes.*".into()),
                limit: 10,
                ..EventFilter::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(system.len(), 1);
    assert_eq!(system[0].event.topic, "ramzes.connection.paused");
    assert!(system[0].event.title.contains("flaky"));

    // Nothing is attempted while paused, even though the retries would be due.
    let attempts_before = w.connector.sent.lock().unwrap().len();
    w.advance(30_000);
    w.settle().await;
    assert_eq!(w.connector.sent.lock().unwrap().len(), attempts_before);

    // When the pause ends the service answers again: everything goes out and the pause is over.
    w.advance(31_000);
    w.settle().await;
    let status = w.store.connection_status(&w.ws).await.unwrap();
    assert_eq!(
        (status[&c].paused_until, status[&c].failure_streak),
        (None, 0)
    );
    assert_eq!(w.connector.sent.lock().unwrap().len(), attempts_before + 5);
}

#[tokio::test]
async fn a_message_that_cannot_be_delivered_raises_an_event_but_never_about_itself() {
    let w = world().await;
    let c = w.connection("broken").await;
    // Everything, including ramzes's own events, goes to the connection that rejects.
    w.route("*", &[(&c, None)]).await;
    for _ in 0..10 {
        w.connector
            .outcomes
            .lock()
            .unwrap()
            .push_back(SendOutcome::Permanent {
                status: Some(400),
                detail: "HTTP 400: bad payload".into(),
            });
    }
    w.send("will be rejected", |_| {}).await;
    w.settle().await;
    let system = w
        .store
        .list_events(
            &w.ws,
            &EventFilter {
                topic: Some("ramzes.*".into()),
                limit: 10,
                ..EventFilter::default()
            },
        )
        .await
        .unwrap();
    // One report for the rejected message; its own rejection is not reported again.
    assert_eq!(system.len(), 1);
    assert_eq!(system[0].event.topic, "ramzes.delivery.failed");
    assert_eq!(system[0].event.labels["outcome"], "rejected");
    assert_eq!(
        system[0].event.body.as_deref(),
        Some("HTTP 400: bad payload")
    );
    assert_eq!(system[0].deliveries_failed, 1);
}

// ---- milestone 4: escalation, acknowledgement links, message updates ----

use ramzes_core::EscalationStep;

impl World {
    /// A route to `first` that escalates to each of `then` after the given seconds.
    async fn escalating(
        &self,
        first: &str,
        then: &[(i64, &str)],
        policy: serde_json::Value,
    ) -> String {
        let r = Route {
            id: new_id(),
            name: "paged".into(),
            enabled: true,
            r#match: RouteMatch {
                topic: Some("orders.*".into()),
                ..RouteMatch::default()
            },
            targets: vec![Target {
                connection_id: first.into(),
                ..Target::default()
            }],
            shaping: shaping(policy),
            escalation: then
                .iter()
                .map(|(after_s, connection)| EscalationStep {
                    after_s: *after_s,
                    targets: vec![Target {
                        connection_id: connection.to_string(),
                        destination: Some("oncall".into()),
                        template_id: None,
                    }],
                })
                .collect(),
            ..Route::default()
        };
        self.store.create_route(&self.ws, &r, 0).await.unwrap();
        r.id
    }

    /// `(token of the connection, destination, title)` of every message sent so far.
    fn sent_to(&self) -> Vec<(String, Option<String>, String)> {
        self.connector
            .sent
            .lock()
            .unwrap()
            .iter()
            .map(|(m, d, c)| {
                (
                    c["token"].as_str().unwrap().to_owned(),
                    d.clone(),
                    m.title.clone(),
                )
            })
            .collect()
    }
}

#[tokio::test]
async fn an_unacknowledged_notification_escalates_step_by_step() {
    let w = world().await;
    let (team, lead, boss) = (
        w.connection("team").await,
        w.connection("lead").await,
        w.connection("boss").await,
    );
    w.escalating(&team, &[(300, &lead), (600, &boss)], json!({}))
        .await;
    w.send("Database down", |_| {}).await;
    w.settle().await;
    assert_eq!(
        w.sent_to(),
        vec![("team".to_string(), None, "Database down".to_string())]
    );

    w.advance(299_000);
    w.settle().await;
    assert_eq!(w.sent_to().len(), 1);
    w.advance(1_000);
    w.settle().await;
    assert_eq!(
        w.sent_to()[1],
        (
            "lead".to_string(),
            Some("oncall".to_string()),
            "Escalated: Database down".to_string()
        )
    );
    // The second step counts from the first.
    w.advance(599_000);
    w.settle().await;
    assert_eq!(w.sent_to().len(), 2);
    w.advance(1_000);
    w.settle().await;
    assert_eq!(w.sent_to()[2].0, "boss");
    w.advance(3_600_000);
    w.settle().await;
    assert_eq!(w.sent_to().len(), 3);
}

#[tokio::test]
async fn acknowledging_stops_the_chain() {
    let w = world().await;
    let (team, lead, boss) = (
        w.connection("team").await,
        w.connection("lead").await,
        w.connection("boss").await,
    );
    w.escalating(&team, &[(300, &lead), (600, &boss)], json!({}))
        .await;
    w.send("Database down", |_| {}).await;
    w.settle().await;
    w.advance(300_000);
    w.settle().await;
    assert_eq!(w.sent_to().len(), 2);
    let id = w.notifications().await[0].id.clone();
    w.store
        .acknowledge(&w.ws, &id, "kris", w.clock.now_ms())
        .await
        .unwrap();
    w.advance(3_600_000);
    w.settle().await;
    assert_eq!(w.sent_to().len(), 2);
}

#[tokio::test]
async fn a_resolution_reaches_everyone_who_was_told_and_nobody_else() {
    let w = world().await;
    let (team, lead, boss) = (
        w.connection("team").await,
        w.connection("lead").await,
        w.connection("boss").await,
    );
    w.escalating(&team, &[(300, &lead), (600, &boss)], json!({}))
        .await;
    w.send("Database down", |e| e.dedup_key = Some("db".into()))
        .await;
    w.settle().await;
    w.advance(300_000);
    w.settle().await;
    w.send("Database up", |e| {
        e.dedup_key = Some("db".into());
        e.status = EventStatus::Resolved;
    })
    .await;
    w.settle().await;
    let sent = w.connector.sent.lock().unwrap().clone();
    let mut resolved: Vec<String> = sent
        .iter()
        .filter(|(m, _, _)| m.status == EventStatus::Resolved)
        .map(|(m, _, c)| format!("{}:{}", c["token"].as_str().unwrap(), m.title))
        .collect();
    resolved.sort();
    assert_eq!(resolved, vec!["lead:Database down", "team:Database down"]);
    // The chain is over: the third step never fires.
    w.advance(3_600_000);
    w.settle().await;
    assert_eq!(w.sent_to().len(), 4);
}

#[tokio::test]
async fn a_grouped_notification_starts_its_chain_when_it_is_sent() {
    let w = world().await;
    let (team, lead) = (w.connection("team").await, w.connection("lead").await);
    w.escalating(
        &team,
        &[(300, &lead)],
        json!({"group": {"by": [], "wait_s": 60}}),
    )
    .await;
    w.send("one", |_| {}).await;
    w.settle().await;
    w.advance(60_000);
    w.settle().await;
    assert_eq!(w.sent_to().len(), 1);
    w.advance(299_000);
    w.settle().await;
    assert_eq!(w.sent_to().len(), 1);
    w.advance(1_000);
    w.settle().await;
    assert_eq!(w.sent_to()[1].0, "lead");
}

#[tokio::test]
async fn a_chain_follows_the_route_as_it_is_now_and_ends_when_the_route_is_deleted() {
    let w = world().await;
    let (team, lead, boss) = (
        w.connection("team").await,
        w.connection("lead").await,
        w.connection("boss").await,
    );
    let route = w
        .escalating(&team, &[(300, &lead), (600, &boss)], json!({}))
        .await;
    w.send("Database down", |_| {}).await;
    w.settle().await;
    w.advance(300_000);
    w.settle().await;
    assert_eq!(w.sent_to().len(), 2);
    w.store.delete_route(&w.ws, &route).await.unwrap();
    w.advance(600_000);
    w.settle().await;
    assert_eq!(w.sent_to().len(), 2);
}

#[tokio::test]
async fn a_message_carries_its_key_its_acknowledgement_link_and_its_thread() {
    let w = world().await;
    let c = w.connection("team").await;
    w.shaped(
        &c,
        json!({"group": {"by": [], "wait_s": 10, "repeat_s": 600}}),
    )
    .await;
    w.send("Database down", |e| e.dedup_key = Some("db".into()))
        .await;
    w.settle().await;
    w.advance(10_000);
    w.settle().await;
    let id = w.notifications().await[0].id.clone();
    let first = w.messages().remove(0);
    assert_eq!(first.dedup_key.as_deref(), Some("db"));
    assert_eq!(first.notification_id.as_deref(), Some(id.as_str()));
    assert_eq!(
        first.ack_link.as_deref(),
        Some(format!("https://ramzes.example.com/ack/{id}?t=signed-ack:{id}").as_str())
    );
    assert_eq!(first.thread_ref, None);

    // The reminder goes under the first message, whose reference the connector returned.
    w.advance(600_000);
    w.settle().await;
    assert_eq!(w.messages()[1].thread_ref.as_deref(), Some("ref-1"));

    // Without its own key a notification is identified by its id; a resolved message has no link.
    w.send("Database up", |e| {
        e.dedup_key = Some("db".into());
        e.status = EventStatus::Resolved;
    })
    .await;
    w.settle().await;
    let last = w.messages().pop().unwrap();
    assert_eq!(
        (last.status, last.ack_link.clone()),
        (EventStatus::Resolved, None)
    );
}

#[tokio::test]
async fn delivered_messages_are_updated_when_the_notification_is_acknowledged() {
    let w = world().await;
    let c = w.connection("team").await;
    w.route("orders.*", &[(&c, None)]).await;
    w.send("Database down", |_| {}).await;
    w.settle().await;
    let id = w.notifications().await[0].id.clone();
    w.store
        .acknowledge(&w.ws, &id, "kris@example.com", w.clock.now_ms())
        .await
        .unwrap();
    w.engine.request_refresh(&id);
    w.settle().await;
    let updates = w.connector.updates.lock().unwrap().clone();
    assert_eq!(
        updates,
        vec![(
            "ref-1".to_string(),
            "Acknowledged by kris@example.com".to_string()
        )]
    );
    // Asking again for something that did not change state does nothing new.
    w.engine.request_refresh("no-such-notification");
    w.settle().await;
    assert_eq!(w.connector.updates.lock().unwrap().len(), 1);
}

// ---- findings of the milestone 2-4 review ----

fn key(name: &'static str) -> impl FnOnce(&mut Event) {
    move |e| e.dedup_key = Some(name.into())
}

fn resolves(name: &'static str) -> impl FnOnce(&mut Event) {
    move |e| {
        e.dedup_key = Some(name.into());
        e.status = EventStatus::Resolved;
    }
}

#[tokio::test]
async fn resolving_one_event_of_a_group_does_not_drop_the_others() {
    let w = world().await;
    let c = w.connection("team").await;
    w.shaped(&c, json!({"group": {"by": [], "wait_s": 30}}))
        .await;
    w.send("Database down", key("db")).await;
    let disk = w.send("Disk full", key("disk")).await;
    w.settle().await;
    w.send("Database up", resolves("db")).await;
    w.settle().await;
    w.advance(60_000);
    w.settle().await;
    // The group goes out with what is still firing.
    let sent = w.messages();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        (sent[0].title.as_str(), sent[0].status),
        ("Disk full", EventStatus::Firing)
    );
    assert_eq!(sent[0].event_ids, vec![disk.id.clone()]);
    assert_eq!(w.notifications().await[0].state, NotificationState::Sent);

    // Resolving the last firing member closes it and tells the target.
    w.send("Disk fine", resolves("disk")).await;
    w.settle().await;
    let sent = w.messages();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[1].status, EventStatus::Resolved);
    assert_eq!(
        w.notifications().await[0].state,
        NotificationState::Resolved
    );
}

#[tokio::test]
async fn a_sent_group_is_not_called_resolved_while_part_of_it_still_fires() {
    let w = world().await;
    let c = w.connection("team").await;
    w.shaped(&c, json!({"group": {"by": [], "wait_s": 30}}))
        .await;
    w.send("Database down", key("db")).await;
    w.send("Disk full", key("disk")).await;
    w.settle().await;
    w.advance(30_000);
    w.settle().await;
    assert_eq!(w.messages().len(), 1);
    w.send("Database up", resolves("db")).await;
    w.settle().await;
    assert_eq!(w.messages().len(), 1);
    assert_eq!(w.notifications().await[0].state, NotificationState::Sent);
}

#[tokio::test]
async fn a_held_digest_survives_the_resolution_of_one_of_its_events() {
    let w = world().await;
    w.clock.0.store(NOON, Ordering::SeqCst);
    let c = w.connection("team").await;
    w.shaped(
        &c,
        json!({"quiet_hours": {"tz": "Europe/Warsaw", "start": "13:00", "end": "15:00"}}),
    )
    .await;
    w.send("Database down", key("db")).await;
    w.settle().await;
    w.send("Disk full", key("disk")).await;
    w.settle().await;
    w.send("Database up", resolves("db")).await;
    w.settle().await;
    w.advance(61 * MINUTE);
    w.settle().await;
    let sent = w.messages();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].title, "Disk full");
}

#[tokio::test]
async fn a_resolution_closes_every_open_notification_with_the_key_and_stops_their_chains() {
    let w = world().await;
    let (team, lead) = (w.connection("team").await, w.connection("lead").await);
    w.escalating(&team, &[(1800, &lead)], json!({})).await;
    w.send("Database down", key("db")).await;
    w.settle().await;
    w.advance(10 * MINUTE);
    w.send("Database down", key("db")).await;
    w.settle().await;
    w.advance(MINUTE);
    w.send("Database up", resolves("db")).await;
    w.settle().await;
    w.advance(60 * MINUTE);
    w.settle().await;
    let states: Vec<NotificationState> = w.notifications().await.iter().map(|n| n.state).collect();
    assert_eq!(
        states,
        vec![NotificationState::Resolved, NotificationState::Resolved]
    );
    assert!(
        w.sent_to().iter().all(|(who, _, _)| who == "team"),
        "{:?}",
        w.sent_to()
    );
}

#[tokio::test]
async fn a_silence_does_not_keep_a_sent_notification_open_forever() {
    let w = world().await;
    let (team, lead) = (w.connection("team").await, w.connection("lead").await);
    w.escalating(&team, &[(1800, &lead)], json!({})).await;
    w.send("Database down", key("db")).await;
    w.settle().await;
    let now = w.clock.now_ms();
    let silence = Silence {
        id: new_id(),
        r#match: RouteMatch {
            topic: Some("orders.*".into()),
            ..RouteMatch::default()
        },
        starts_at: now,
        ends_at: now + 60 * MINUTE,
        comment: None,
    };
    w.store
        .create_silence(&w.ws, &silence, "kris", now)
        .await
        .unwrap();
    w.send("Database up", resolves("db")).await;
    w.settle().await;
    w.advance(40 * MINUTE);
    w.settle().await;
    assert_eq!(
        w.notifications().await[0].state,
        NotificationState::Resolved
    );
    assert!(
        w.sent_to().iter().all(|(who, _, _)| who == "team"),
        "{:?}",
        w.sent_to()
    );
    assert_eq!(w.messages().last().unwrap().status, EventStatus::Resolved);
}

#[tokio::test]
async fn being_rate_limited_by_a_service_does_not_pause_the_connection() {
    let w = world().await;
    let c = w.connection("busy").await;
    w.route("orders.*", &[(&c, None)]).await;
    for _ in 0..6 {
        w.connector
            .outcomes
            .lock()
            .unwrap()
            .push_back(SendOutcome::Retry {
                after_ms: Some(1000),
                status: Some(429),
                detail: "HTTP 429".into(),
            });
    }
    for i in 0..6 {
        w.send(&format!("event {i}"), |_| {}).await;
    }
    w.settle().await;
    let status = w.store.connection_status(&w.ws).await.unwrap();
    assert_eq!(
        (status[&c].paused_until, status[&c].failure_streak),
        (None, 0)
    );
}

#[tokio::test]
async fn an_acknowledgement_through_the_engine_stops_reminders_and_updates_messages() {
    let w = world().await;
    let c = w.connection("team").await;
    w.shaped(
        &c,
        json!({"group": {"by": [], "wait_s": 10, "repeat_s": 600}}),
    )
    .await;
    w.send("Database down", |_| {}).await;
    w.settle().await;
    w.advance(10_000);
    w.settle().await;
    let id = w.notifications().await[0].id.clone();
    w.engine.acknowledge(&w.ws, &id, "kris").await.unwrap();
    assert!(matches!(
        w.engine.acknowledge(&w.ws, &id, "kris").await,
        Err(ramzes_engine::EngineError::Conflict(_))
    ));
    w.advance(3_600_000);
    w.settle().await;
    assert_eq!(w.messages().len(), 1);
    assert_eq!(w.connector.updates.lock().unwrap().len(), 1);
    w.engine.resolve(&w.ws, &id).await.unwrap();
    assert_eq!(
        w.notifications().await[0].state,
        NotificationState::Resolved
    );
}

#[tokio::test]
async fn a_resolution_less_severe_than_the_route_asks_for_still_closes_what_the_route_opened() {
    let w = world().await;
    let c = w.connection("team").await;
    let r = Route {
        id: new_id(),
        name: "failures".into(),
        enabled: true,
        r#match: RouteMatch {
            topic: Some("orders.*".into()),
            min_severity: Some(Severity::Error),
            ..RouteMatch::default()
        },
        targets: vec![Target {
            connection_id: c.clone(),
            ..Target::default()
        }],
        shaping: shaping(json!({"dedup": {"window_s": 600}})),
        ..Route::default()
    };
    w.store.create_route(&w.ws, &r, 0).await.unwrap();
    let passed = |e: &mut Event| {
        e.dedup_key = Some("build".into());
        e.status = EventStatus::Resolved;
        e.severity = Severity::Info;
    };

    // Nothing is open: the route has nothing to do with the event.
    let quiet = w.send("Build passed", passed).await;
    w.settle().await;
    assert!(w.notifications().await.is_empty());
    let journey = w.store.journey(&w.ws, &quiet.id).await.unwrap().unwrap();
    assert!(journey.notifications.is_empty());

    w.send("Build failed", key("build")).await;
    w.settle().await;
    w.send("Build passed", passed).await;
    w.settle().await;
    let sent = w.messages();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[1].status, EventStatus::Resolved);
    let rows = w.notifications().await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, NotificationState::Resolved);
}

#[tokio::test]
async fn when_a_pause_ends_one_delivery_tests_the_connection() {
    let w = world().await;
    let c = w.connection("flaky").await;
    w.route("orders.*", &[(&c, None)]).await;
    for _ in 0..6 {
        w.connector.outcomes.lock().unwrap().push_back(retry("503"));
    }
    for i in 0..5 {
        w.send(&format!("event {i}"), |_| {}).await;
    }
    w.settle().await;
    let attempts_before = w.connector.sent.lock().unwrap().len();

    // Still failing when the pause ends: one attempt finds out, the rest keep waiting.
    w.advance(61_000);
    w.settle().await;
    assert_eq!(w.connector.sent.lock().unwrap().len(), attempts_before + 1);
    let status = w.store.connection_status(&w.ws).await.unwrap();
    assert_eq!(status[&c].failure_streak, 6);
    assert_eq!(status[&c].paused_until, Some(w.clock.now_ms() + 120_000));

    // It answers after the next pause: the test delivery goes first, then the others.
    w.advance(121_000);
    w.settle().await;
    assert_eq!(w.connector.sent.lock().unwrap().len(), attempts_before + 6);
    let status = w.store.connection_status(&w.ws).await.unwrap();
    assert_eq!(
        (status[&c].paused_until, status[&c].failure_streak),
        (None, 0)
    );
}

#[tokio::test]
async fn an_acknowledgement_is_passed_on_to_every_service_that_got_the_message() {
    let w = world().await;
    let team = w.connection("team").await;
    let pager = w.connection("pager").await;
    let unreachable = w.connection("unreachable").await;
    w.route("orders.*", &[(&team, Some("#ops")), (&pager, None)])
        .await;
    w.route("billing.*", &[(&unreachable, None)]).await;
    w.send("Database down", key("db")).await;
    w.settle().await;
    // This one never arrives, so there is nothing to acknowledge at its service.
    w.connector
        .outcomes
        .lock()
        .unwrap()
        .push_back(SendOutcome::Permanent {
            status: Some(400),
            detail: "no".into(),
        });
    w.send("Invoice failed", |e| e.topic = "billing.failed".into())
        .await;
    w.settle().await;
    assert!(w.connector.acknowledged.lock().unwrap().is_empty());
    for row in &w.notifications().await {
        w.engine.acknowledge(&w.ws, &row.id, "kris").await.unwrap();
    }
    w.settle().await;
    let mut told = w.connector.acknowledged.lock().unwrap().clone();
    told.sort();
    assert_eq!(
        told,
        vec![
            ("pager".to_string(), None, Some("db".to_string())),
            (
                "team".to_string(),
                Some("#ops".to_string()),
                Some("db".to_string())
            ),
        ]
    );
}
