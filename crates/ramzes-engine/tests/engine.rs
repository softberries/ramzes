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
}

/// A connector that answers from a script and records what it was asked to send.
#[derive(Default)]
struct Scripted {
    outcomes: Mutex<VecDeque<SendOutcome>>,
    sent: Mutex<Vec<(Message, Option<String>, serde_json::Value)>>,
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
