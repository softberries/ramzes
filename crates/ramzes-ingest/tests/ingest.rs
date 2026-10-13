use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use ramzes_connectors::Registry;
use ramzes_core::{RetryPolicy, Severity, generate_key, hash_key};
use ramzes_engine::{Clock, Engine, EngineConfig, Opener};
use ramzes_ingest::{IngestLimits, IngestState, router};
use ramzes_store::{EventFilter, Store};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

struct FakeClock(AtomicI64);

impl Clock for FakeClock {
    fn now_ms(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

struct NoOpener;

impl Opener for NoOpener {
    fn open(&self, _: &str) -> Result<Value, String> {
        Ok(json!({}))
    }

    fn sign(&self, text: &str) -> String {
        format!("signed-{text}")
    }
}

struct World {
    app: Router,
    store: Store,
    engine: Arc<Engine>,
    clock: Arc<FakeClock>,
    ws: String,
    source: String,
    key: String,
    _dir: tempfile::TempDir,
}

async fn world_with(limits: IngestLimits) -> World {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::connect(&format!(
        "sqlite://{}",
        dir.path().join("ramzes.db").display()
    ))
    .await
    .unwrap();
    store.migrate().await.unwrap();
    let ws = store.ensure_workspace(0).await.unwrap();
    let (key, prefix) = generate_key("rz_src_");
    let source = store
        .create_source(&ws, "orders-api", "native", &hash_key(&key), &prefix, 0)
        .await
        .unwrap()
        .id;
    let clock = Arc::new(FakeClock(AtomicI64::new(1_791_884_467_000)));
    let engine = Engine::new(EngineConfig {
        store: store.clone(),
        registry: Registry::builtin(Duration::from_secs(1)),
        opener: Arc::new(NoOpener),
        clock: clock.clone(),
        policy: RetryPolicy::default(),
        public_url: None,
        lease_ms: 60_000,
        jitter: Arc::new(|| 0.5),
    });
    let app = router(IngestState::new(
        store.clone(),
        engine.clone(),
        limits,
        clock.clone(),
        Arc::new(NoOpener),
    ));
    World {
        app,
        store,
        engine,
        clock,
        ws,
        source,
        key,
        _dir: dir,
    }
}

async fn world() -> World {
    world_with(IngestLimits::default()).await
}

struct Reply {
    status: StatusCode,
    retry_after: Option<String>,
    body: Value,
}

impl World {
    async fn post_raw(&self, key: Option<&str>, idempotency: Option<&str>, body: Vec<u8>) -> Reply {
        let mut request =
            Request::post("/v1/events").header(header::CONTENT_TYPE, "application/json");
        if let Some(key) = key {
            request = request.header(header::AUTHORIZATION, format!("Bearer {key}"));
        }
        if let Some(idempotency) = idempotency {
            request = request.header("idempotency-key", idempotency);
        }
        let response = self
            .app
            .clone()
            .oneshot(request.body(Body::from(body)).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let retry_after = response
            .headers()
            .get(header::RETRY_AFTER)
            .map(|v| v.to_str().unwrap().to_owned());
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        let body = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| panic!("not JSON: {:?}", String::from_utf8_lossy(&bytes)));
        Reply {
            status,
            retry_after,
            body,
        }
    }

    async fn post(&self, body: Value) -> Reply {
        self.post_raw(Some(&self.key), None, body.to_string().into_bytes())
            .await
    }

    async fn stored(&self) -> usize {
        self.store
            .list_events(
                &self.ws,
                &EventFilter {
                    limit: 500,
                    ..EventFilter::default()
                },
            )
            .await
            .unwrap()
            .len()
    }
}

fn error_of(reply: &Reply) -> (&str, &str) {
    (
        reply.body["error"].as_str().unwrap(),
        reply.body["message"].as_str().unwrap(),
    )
}

#[tokio::test]
async fn a_single_event_is_stored_with_its_fields() {
    let w = world().await;
    let reply = w
        .post(json!({
            "topic": "orders.failed", "title": "Order failed", "severity": "error",
            "body": "card declined", "labels": {"env": "prod"}, "payload": {"order": 7},
            "dedup_key": "order-7", "ts": "2026-10-13T09:00:00Z"
        }))
        .await;
    assert_eq!(reply.status, StatusCode::ACCEPTED);
    assert_eq!(reply.body["duplicate"], json!(false));
    let id = reply.body["ids"][0].as_str().unwrap();

    let e = w.store.get_event(&w.ws, id).await.unwrap().unwrap();
    assert_eq!(e.source_id, w.source);
    assert_eq!(e.topic, "orders.failed");
    assert_eq!(e.severity, Severity::Error);
    assert_eq!(e.body.as_deref(), Some("card declined"));
    assert_eq!(e.labels.get("env").map(String::as_str), Some("prod"));
    assert_eq!(e.payload, json!({"order": 7}));
    assert_eq!(e.dedup_key.as_deref(), Some("order-7"));
    assert_eq!(e.occurred_at, 1_791_882_000_000);
    assert_eq!(e.received_at, w.clock.now_ms());
}

#[tokio::test]
async fn a_batch_is_stored_in_order() {
    let w = world().await;
    let reply = w
        .post(json!([{"topic": "a", "title": "first"}, {"topic": "b", "title": "second"}]))
        .await;
    assert_eq!(reply.status, StatusCode::ACCEPTED);
    let ids = reply.body["ids"].as_array().unwrap();
    assert_eq!(ids.len(), 2);
    let first = w
        .store
        .get_event(&w.ws, ids[0].as_str().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.title, "first");
    assert_eq!(w.stored().await, 2);
}

#[tokio::test]
async fn ingest_wakes_the_engine() {
    let w = world().await;
    let shutdown = CancellationToken::new();
    let running = tokio::spawn(
        w.engine
            .clone()
            .run(shutdown.clone(), Duration::from_secs(3600)),
    );
    tokio::time::sleep(Duration::from_millis(100)).await;

    let reply = w.post(json!({"topic": "a", "title": "t"})).await;
    let id = reply.body["ids"][0].as_str().unwrap().to_owned();
    let mut routed = false;
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(20)).await;
        if w.store.journey(&w.ws, &id).await.unwrap().unwrap().routed {
            routed = true;
            break;
        }
    }
    assert!(routed, "the event was not routed after ingest");
    shutdown.cancel();
    running.await.unwrap();
}

#[tokio::test]
async fn a_missing_or_wrong_key_is_unauthorized() {
    let w = world().await;
    let body = json!({"topic": "a", "title": "t"}).to_string().into_bytes();
    for key in [None, Some("rz_src_wrong"), Some("")] {
        let reply = w.post_raw(key, None, body.clone()).await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{key:?}");
        assert_eq!(error_of(&reply).0, "unauthorized");
    }
    assert_eq!(w.stored().await, 0);
}

#[tokio::test]
async fn invalid_json_is_a_400() {
    let w = world().await;
    for body in [&b"{not json"[..], b"", b"\"just a string\"", b"42"] {
        let reply = w.post_raw(Some(&w.key), None, body.to_vec()).await;
        assert_eq!(
            reply.status,
            StatusCode::BAD_REQUEST,
            "{:?}",
            String::from_utf8_lossy(body)
        );
        assert_eq!(error_of(&reply).0, "invalid_json");
    }
}

#[tokio::test]
async fn invalid_events_are_a_422_naming_the_problem() {
    let w = world().await;
    let cases = [
        (
            json!({"topic": "a", "title": "t", "severity": "loud"}),
            "severity",
        ),
        (
            json!({"topic": "a", "title": "t", "labels": {"n": 1}}),
            "labels",
        ),
        (json!({"topic": "", "title": "t"}), "topic"),
        (json!({"title": "t"}), "topic"),
        (json!({"topic": "a", "title": "t", "ts": "yesterday"}), "ts"),
        (
            json!({"topic": "a", "title": "t", "colour": "red"}),
            "colour",
        ),
    ];
    for (body, mentions) in cases {
        let reply = w.post(body.clone()).await;
        assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        let (code, message) = error_of(&reply);
        assert_eq!(code, "invalid_event");
        assert!(
            message.contains(mentions),
            "{message:?} should mention {mentions}"
        );
    }
    assert_eq!(w.stored().await, 0);
}

#[tokio::test]
async fn one_bad_event_rejects_the_whole_batch_and_names_its_position() {
    let w = world().await;
    let reply = w
        .post(json!([{"topic": "a", "title": "ok"}, {"topic": "has space", "title": "bad"}]))
        .await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        error_of(&reply).1.starts_with("event 1:"),
        "{}",
        error_of(&reply).1
    );
    assert_eq!(w.stored().await, 0);
}

#[tokio::test]
async fn an_oversized_body_is_a_413() {
    let w = world_with(IngestLimits {
        max_body_bytes: 1024,
        ..IngestLimits::default()
    })
    .await;
    let reply = w
        .post(json!({"topic": "a", "title": "t", "body": "x".repeat(2000)}))
        .await;
    assert_eq!(reply.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(error_of(&reply).0, "too_large");
}

#[tokio::test]
async fn batch_size_is_limited() {
    let w = world().await;
    let event = json!({"topic": "a", "title": "t"});
    let reply = w.post(Value::Array(vec![event.clone(); 1001])).await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error_of(&reply).0, "batch_too_large");
    let reply = w.post(json!([])).await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error_of(&reply).0, "invalid_event");
    assert_eq!(
        w.post(Value::Array(vec![event; 1000])).await.status,
        StatusCode::ACCEPTED
    );
}

#[tokio::test]
async fn a_repeated_idempotency_key_returns_the_original_ids() {
    let w = world().await;
    let body = json!({"topic": "a", "title": "t"}).to_string().into_bytes();
    let first = w.post_raw(Some(&w.key), Some("req-1"), body.clone()).await;
    let second = w.post_raw(Some(&w.key), Some("req-1"), body).await;
    assert_eq!(second.status, StatusCode::ACCEPTED);
    assert_eq!(second.body["duplicate"], json!(true));
    assert_eq!(second.body["ids"], first.body["ids"]);
    assert_eq!(w.stored().await, 1);
}

#[tokio::test]
async fn an_overlong_idempotency_key_is_rejected() {
    let w = world().await;
    let body = json!({"topic": "a", "title": "t"}).to_string().into_bytes();
    let reply = w.post_raw(Some(&w.key), Some(&"k".repeat(201)), body).await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error_of(&reply).0, "invalid_idempotency_key");
}

#[tokio::test]
async fn exceeding_the_per_source_rate_is_a_429_until_the_next_minute() {
    let w = world_with(IngestLimits {
        per_source_per_minute: 3,
        ..IngestLimits::default()
    })
    .await;
    let event = json!({"topic": "a", "title": "t"});
    assert_eq!(
        w.post(json!([event, event])).await.status,
        StatusCode::ACCEPTED
    );
    assert_eq!(w.post(event.clone()).await.status, StatusCode::ACCEPTED);
    let limited = w.post(event.clone()).await;
    assert_eq!(limited.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(error_of(&limited).0, "rate_limited");
    let wait: i64 = limited.retry_after.unwrap().parse().unwrap();
    assert!((1..=60).contains(&wait), "{wait}");
    assert_eq!(w.stored().await, 3);

    w.clock.0.fetch_add(60_000, Ordering::SeqCst);
    assert_eq!(w.post(event).await.status, StatusCode::ACCEPTED);
}

#[tokio::test]
async fn an_unavailable_database_is_a_503() {
    let w = world().await;
    w.store.close().await;
    let reply = w.post(json!({"topic": "a", "title": "t"})).await;
    assert_eq!(reply.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(error_of(&reply).0, "unavailable");
}

#[tokio::test]
async fn a_rejected_body_is_kept_with_its_error() {
    let w = world().await;
    w.post(json!({"topic": "has space", "title": "t"})).await;
    w.post_raw(Some(&w.key), None, b"{not json".to_vec()).await;
    let kept = w
        .store
        .list_source_requests(&w.ws, &w.source)
        .await
        .unwrap();
    assert_eq!(kept.len(), 2);
    assert_eq!(kept[0].body, "{not json");
    assert!(kept[0].error.as_deref().unwrap().contains("JSON"));
    assert!(kept[1].body.contains("has space"));
    assert!(kept[1].error.as_deref().unwrap().contains("topic"));
}

#[tokio::test]
async fn other_methods_and_paths_are_not_ingest() {
    let w = world().await;
    let request = Request::get("/v1/events").body(Body::empty()).unwrap();
    let response = w.app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
}
