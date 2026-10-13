//! Per-source URLs: `POST /v1/in/{source_id}` for third-party senders.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use hmac::{Hmac, KeyInit, Mac};
use ramzes_connectors::Registry;
use ramzes_core::{EventStatus, RetryPolicy, hash_key};
use ramzes_engine::{Clock, Engine, EngineConfig, Opener};
use ramzes_ingest::{IngestLimits, IngestState, router};
use ramzes_store::{EventFilter, Store};
use serde_json::{Value, json};
use sha2::Sha256;
use tower::ServiceExt;

struct FakeClock(AtomicI64);

impl Clock for FakeClock {
    fn now_ms(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

/// Test secrets are stored as plain JSON.
struct PlainOpener;

impl Opener for PlainOpener {
    fn open(&self, sealed: &str) -> Result<Value, String> {
        serde_json::from_str(sealed).map_err(|e| e.to_string())
    }

    fn sign(&self, text: &str) -> String {
        format!("signed-{text}")
    }
}

struct World {
    app: Router,
    store: Store,
    ws: String,
    _dir: tempfile::TempDir,
}

const KEY: &str = "rz_src_test-key-0123456789";

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
    let clock = Arc::new(FakeClock(AtomicI64::new(1_791_884_467_000)));
    let engine = Engine::new(EngineConfig {
        store: store.clone(),
        registry: Registry::builtin(Duration::from_secs(1)),
        opener: Arc::new(PlainOpener),
        clock: clock.clone(),
        policy: RetryPolicy::default(),
        public_url: None,
        lease_ms: 60_000,
        jitter: Arc::new(|| 0.5),
    });
    let app = router(IngestState::new(
        store.clone(),
        engine,
        IngestLimits::default(),
        clock,
        Arc::new(PlainOpener),
    ));
    World {
        app,
        store,
        ws,
        _dir: dir,
    }
}

struct Reply {
    status: StatusCode,
    body: Value,
}

impl World {
    /// A source of `kind` whose key is [`KEY`].
    async fn source(&self, kind: &str, mapping: Option<&str>) -> String {
        let source = self
            .store
            .create_source(
                &self.ws,
                kind,
                kind,
                &hash_key(&format!("{KEY}-{kind}")),
                "rz_src_test",
                0,
            )
            .await
            .unwrap();
        self.store
            .set_source_secret(
                &self.ws,
                &source.id,
                &json!({"key": format!("{KEY}-{kind}")}).to_string(),
            )
            .await
            .unwrap();
        if mapping.is_some() {
            self.store
                .set_source_mapping(&self.ws, &source.id, mapping)
                .await
                .unwrap();
        }
        source.id
    }

    async fn send(&self, path: &str, headers: &[(&str, String)], body: &str) -> Reply {
        let mut request = Request::post(path).header(header::CONTENT_TYPE, "application/json");
        for (name, value) in headers {
            request = request.header(*name, value);
        }
        let response = self
            .app
            .clone()
            .oneshot(request.body(Body::from(body.to_owned())).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        Reply {
            status,
            body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        }
    }

    async fn events(&self) -> Vec<ramzes_store::EventRow> {
        let filter = EventFilter {
            limit: 100,
            ..EventFilter::default()
        };
        let mut rows = self.store.list_events(&self.ws, &filter).await.unwrap();
        rows.reverse();
        rows
    }
}

fn key(kind: &str) -> String {
    format!("{KEY}-{kind}")
}

fn bearer(kind: &str) -> (&'static str, String) {
    ("authorization", format!("Bearer {}", key(kind)))
}

fn sign(kind: &str, body: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(key(kind).as_bytes()).unwrap();
    mac.update(body.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

const ALERTS: &str = r#"{"version":"4","status":"firing","alerts":[
  {"status":"firing","labels":{"alertname":"HighErrorRate","severity":"critical"},"annotations":{"summary":"Error rate above 5%"},
   "startsAt":"2026-10-13T09:41:07Z","endsAt":"0001-01-01T00:00:00Z","fingerprint":"a1"},
  {"status":"resolved","labels":{"alertname":"DiskFull"},"annotations":{},"startsAt":"2026-10-13T08:00:00Z","fingerprint":"b2"}]}"#;

#[tokio::test]
async fn an_alertmanager_request_becomes_one_event_per_alert() {
    let w = world().await;
    let id = w.source("alertmanager", None).await;
    let reply = w
        .send(&format!("/v1/in/{id}"), &[bearer("alertmanager")], ALERTS)
        .await;
    assert_eq!(reply.status, StatusCode::ACCEPTED, "{}", reply.body);
    assert_eq!(reply.body["ids"].as_array().unwrap().len(), 2);
    let events = w.events().await;
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event.topic, "alertmanager.HighErrorRate");
    assert_eq!(events[0].event.source_id, id);
    assert_eq!(events[0].event.dedup_key.as_deref(), Some("a1"));
    assert_eq!(events[0].event.payload["labels"]["severity"], "critical");
    assert_eq!(events[1].event.status, EventStatus::Resolved);
}

#[tokio::test]
async fn a_wrong_or_missing_key_is_refused_and_nothing_is_kept() {
    let w = world().await;
    let id = w.source("alertmanager", None).await;
    let path = format!("/v1/in/{id}");
    for headers in [
        vec![],
        vec![("authorization", "Bearer nope".to_string())],
        vec![bearer("github")],
    ] {
        let reply = w.send(&path, &headers, ALERTS).await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
        assert_eq!(reply.body["error"], "unauthorized");
    }
    assert!(w.events().await.is_empty());
    assert!(
        w.store
            .list_source_requests(&w.ws, &id)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn an_unknown_source_is_not_found_and_a_native_source_is_pointed_to_its_endpoint() {
    let w = world().await;
    assert_eq!(
        w.send("/v1/in/nope", &[], "{}").await.status,
        StatusCode::NOT_FOUND
    );
    let native = w.source("native", None).await;
    let reply = w
        .send(&format!("/v1/in/{native}"), &[bearer("native")], "{}")
        .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
    assert!(
        reply.body["message"]
            .as_str()
            .unwrap()
            .contains("/v1/events")
    );
}

const RUN: &str = r#"{"action":"completed","workflow_run":{"name":"CI","conclusion":"failure","head_branch":"main","html_url":"https://github.com/acme/shop/actions/runs/1"},"repository":{"full_name":"acme/shop"},"sender":{"login":"kris"}}"#;

#[tokio::test]
async fn a_github_request_needs_a_valid_signature() {
    let w = world().await;
    let id = w.source("github", None).await;
    let path = format!("/v1/in/{id}");
    let event = ("x-github-event", "workflow_run".to_string());

    let unsigned = w.send(&path, std::slice::from_ref(&event), RUN).await;
    assert_eq!(unsigned.status, StatusCode::UNAUTHORIZED);
    let forged = w
        .send(
            &path,
            &[
                event.clone(),
                (
                    "x-hub-signature-256",
                    format!("sha256={}", sign("gitlab", RUN)),
                ),
            ],
            RUN,
        )
        .await;
    assert_eq!(forged.status, StatusCode::UNAUTHORIZED);
    let other_body = w
        .send(
            &path,
            &[
                event.clone(),
                (
                    "x-hub-signature-256",
                    format!("sha256={}", sign("github", "{}")),
                ),
            ],
            RUN,
        )
        .await;
    assert_eq!(other_body.status, StatusCode::UNAUTHORIZED);
    assert!(w.events().await.is_empty());

    let signed = w
        .send(
            &path,
            &[
                event,
                (
                    "x-hub-signature-256",
                    format!("sha256={}", sign("github", RUN).to_uppercase()),
                ),
            ],
            RUN,
        )
        .await;
    assert_eq!(signed.status, StatusCode::ACCEPTED, "{}", signed.body);
    let events = w.events().await;
    assert_eq!(
        events[0].event.title,
        "acme/shop: workflow CI failure on main"
    );
}

#[tokio::test]
async fn githubs_documented_signature_example_verifies() {
    // From GitHub's "Validating webhook deliveries": this secret and body give this signature.
    let w = world().await;
    let source = w
        .store
        .create_source(&w.ws, "gh", "github", "h", "p", 0)
        .await
        .unwrap();
    w.store
        .set_source_secret(
            &w.ws,
            &source.id,
            &json!({"key": "It's a Secret to Everybody"}).to_string(),
        )
        .await
        .unwrap();
    let reply = w
        .send(
            &format!("/v1/in/{}", source.id),
            &[
                ("x-github-event", "ping".to_string()),
                (
                    "x-hub-signature-256",
                    "sha256=757107ea0eb2509fc211221cce984b8a37570b6d7586c22c46f4379c8b043e17"
                        .to_string(),
                ),
            ],
            "Hello, World!",
        )
        .await;
    // The signature is accepted; the body then fails as JSON, which is a different answer.
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{}", reply.body);
}

#[tokio::test]
async fn a_request_the_mapping_skips_is_accepted_without_events() {
    let w = world().await;
    let id = w.source("github", None).await;
    let body = r#"{"zen":"Keep it logically awesome.","hook_id":1}"#;
    let reply = w
        .send(
            &format!("/v1/in/{id}"),
            &[
                ("x-github-event", "ping".to_string()),
                (
                    "x-hub-signature-256",
                    format!("sha256={}", sign("github", body)),
                ),
            ],
            body,
        )
        .await;
    assert_eq!(reply.status, StatusCode::ACCEPTED);
    assert_eq!(reply.body["ids"], json!([]));
    assert!(w.events().await.is_empty());
}

#[tokio::test]
async fn a_gitlab_request_carries_the_key_in_its_token_header() {
    let w = world().await;
    let id = w.source("gitlab", None).await;
    let body = r#"{"object_kind":"pipeline","project":{"path_with_namespace":"acme/shop"},"object_attributes":{"id":31,"ref":"main","status":"failed"}}"#;
    let path = format!("/v1/in/{id}");
    assert_eq!(
        w.send(&path, &[("x-gitlab-token", "wrong".into())], body)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    let reply = w
        .send(&path, &[("x-gitlab-token", key("gitlab"))], body)
        .await;
    assert_eq!(reply.status, StatusCode::ACCEPTED, "{}", reply.body);
    assert_eq!(
        w.events().await[0].event.title,
        "acme/shop: pipeline #31 failed on main"
    );
}

const QUERY_MAPPING: &str = "verify: {method: query, param: key}\nfields:\n  topic: \"'shop.' ~ payload.kind\"\n  title: payload.title\n";

#[tokio::test]
async fn an_any_json_source_can_take_the_key_from_the_url() {
    let w = world().await;
    let id = w.source("json", Some(QUERY_MAPPING)).await;
    let body = r#"{"kind":"order","title":"Order 17 failed"}"#;
    assert_eq!(
        w.send(&format!("/v1/in/{id}"), &[], body).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        w.send(&format!("/v1/in/{id}?key=wrong"), &[], body)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    let reply = w
        .send(&format!("/v1/in/{id}?key={}", key("json")), &[], body)
        .await;
    assert_eq!(reply.status, StatusCode::ACCEPTED, "{}", reply.body);
    assert_eq!(w.events().await[0].event.topic, "shop.order");
}

#[tokio::test]
async fn a_request_the_mapping_cannot_use_is_rejected_and_kept_with_the_reason() {
    let w = world().await;
    let id = w.source("json", Some(QUERY_MAPPING)).await;
    let path = format!("/v1/in/{id}?key={}", key("json"));
    let reply = w
        .send(&path, &[("x-custom", "yes".into())], r#"{"kind":"order"}"#)
        .await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        reply.body["message"]
            .as_str()
            .unwrap()
            .starts_with("title:"),
        "{}",
        reply.body
    );
    let kept = w.store.list_source_requests(&w.ws, &id).await.unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].body, r#"{"kind":"order"}"#);
    assert!(kept[0].error.as_deref().unwrap().starts_with("title:"));
    let headers: Value = serde_json::from_str(kept[0].headers.as_deref().unwrap()).unwrap();
    assert_eq!(headers["x-custom"], "yes");
}

#[tokio::test]
async fn accepted_requests_are_kept_too_without_their_credentials() {
    let w = world().await;
    let id = w.source("alertmanager", None).await;
    w.send(
        &format!("/v1/in/{id}"),
        &[bearer("alertmanager"), ("cookie", "session=1".into())],
        ALERTS,
    )
    .await;
    let kept = w.store.list_source_requests(&w.ws, &id).await.unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].error, None);
    let headers = kept[0].headers.clone().unwrap();
    assert!(
        !headers.contains(KEY) && !headers.contains("authorization") && !headers.contains("cookie"),
        "{headers}"
    );
    assert!(headers.contains("content-type"));
}

#[tokio::test]
async fn a_delivery_id_makes_a_repeated_request_harmless() {
    let w = world().await;
    let id = w.source("github", None).await;
    let headers = [
        ("x-github-event", "workflow_run".to_string()),
        (
            "x-github-delivery",
            "72d3162e-cc78-11e3-81ab-4c9367dc0958".to_string(),
        ),
        (
            "x-hub-signature-256",
            format!("sha256={}", sign("github", RUN)),
        ),
    ];
    let first = w.send(&format!("/v1/in/{id}"), &headers, RUN).await;
    let second = w.send(&format!("/v1/in/{id}"), &headers, RUN).await;
    assert_eq!(second.status, StatusCode::ACCEPTED);
    assert_eq!(second.body["duplicate"], true);
    assert_eq!(second.body["ids"], first.body["ids"]);
    assert_eq!(w.events().await.len(), 1);
}

#[tokio::test]
async fn a_source_without_a_stored_secret_cannot_check_signatures() {
    let w = world().await;
    let source = w
        .store
        .create_source(&w.ws, "gh", "github", "h", "p", 0)
        .await
        .unwrap();
    let reply = w
        .send(
            &format!("/v1/in/{}", source.id),
            &[("x-hub-signature-256", "sha256=00".to_string())],
            "{}",
        )
        .await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    assert!(
        reply.body["message"].as_str().unwrap().contains("replace"),
        "{}",
        reply.body
    );
}
