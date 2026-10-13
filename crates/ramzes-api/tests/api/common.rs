//! A running API over a temporary SQLite database, with a scripted connector.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use ramzes_api::{AppState, SecretKey};
use ramzes_connectors::{
    Capabilities, ConnectionConfig, Connector, ConnectorDescriptor, Field, FieldKind, Preview,
    Registry, SendOutcome,
};
use ramzes_core::{Message, RetryPolicy};
use ramzes_engine::{Clock, Engine, EngineConfig};
use ramzes_ingest::{IngestLimits, IngestState};
use ramzes_store::Store;
use serde_json::{Value, json};
use tower::ServiceExt;

pub struct FakeClock(pub AtomicI64);

impl Clock for FakeClock {
    fn now_ms(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Default)]
pub struct Scripted {
    pub outcomes: Mutex<VecDeque<SendOutcome>>,
    pub sent: Mutex<Vec<(Message, ConnectionConfig)>>,
    /// `(reference, note)` of every message it was asked to rewrite.
    pub updates: Mutex<Vec<(String, String)>>,
}

#[async_trait]
impl Connector for Scripted {
    fn descriptor(&self) -> ConnectorDescriptor {
        ConnectorDescriptor {
            kind: "scripted".into(),
            name: "Scripted".into(),
            description: "Test connector".into(),
            fields: vec![
                Field {
                    key: "room".into(),
                    label: "Room".into(),
                    help: "".into(),
                    secret: false,
                    required: true,
                    kind: FieldKind::Text,
                },
                Field {
                    key: "token".into(),
                    label: "Token".into(),
                    help: "".into(),
                    secret: true,
                    required: true,
                    kind: FieldKind::Text,
                },
            ],
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

    fn validate(&self, c: &ConnectionConfig) -> Result<(), String> {
        if c.settings
            .get("room")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        {
            return Err("Room is required".into());
        }
        if c.credentials
            .get("token")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        {
            return Err("Token is required".into());
        }
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
        _: Option<&str>,
        _: i64,
    ) -> SendOutcome {
        self.sent.lock().unwrap().push((m.clone(), c.clone()));
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
}

pub struct Api {
    pub app: Router,
    pub openapi: Value,
    pub store: Store,
    pub engine: Arc<Engine>,
    pub clock: Arc<FakeClock>,
    pub connector: Arc<Scripted>,
    pub ws: String,
    cookie: Mutex<Option<String>>,
    _dir: tempfile::TempDir,
}

pub const EMAIL: &str = "kris@example.com";
pub const PASSWORD: &str = "correct horse battery";

impl Api {
    pub async fn new() -> Api {
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
        let key = Arc::new(SecretKey::load_or_create(dir.path()).unwrap());
        let connector = Arc::new(Scripted::default());
        let mut registry = Registry::builtin(Duration::from_secs(1));
        registry.add(connector.clone());
        let engine = Engine::new(EngineConfig {
            store: store.clone(),
            registry: registry.clone(),
            opener: key.clone(),
            clock: clock.clone(),
            policy: RetryPolicy::default(),
            public_url: None,
            lease_ms: 60_000,
            jitter: Arc::new(|| 0.5),
        });
        let state = AppState {
            store: store.clone(),
            engine: engine.clone(),
            registry,
            key: key.clone(),
            clock: clock.clone(),
            workspace_id: ws.clone(),
            secure_cookies: false,
            login_guard: Default::default(),
        };
        let (api, openapi) = ramzes_api::router(state);
        let ingest = ramzes_ingest::router(IngestState::new(
            store.clone(),
            engine.clone(),
            IngestLimits::default(),
            clock.clone(),
            key,
        ));
        Api {
            app: api.merge(ingest),
            openapi: serde_json::to_value(&openapi).unwrap(),
            store,
            engine,
            clock,
            connector,
            ws,
            cookie: Mutex::new(None),
            _dir: dir,
        }
    }

    /// An API with the admin created and signed in.
    pub async fn signed_in() -> Api {
        let api = Api::new().await;
        let (status, _) = api
            .post("/api/setup", json!({"email": EMAIL, "password": PASSWORD}))
            .await;
        assert_eq!(status, StatusCode::OK);
        api
    }

    /// Sends a request the way the UI does: with the session cookie and the request header.
    pub async fn call(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header("x-ramzes-request", "1");
        if let Some(cookie) = self.cookie.lock().unwrap().clone() {
            request = request.header(header::COOKIE, cookie);
        }
        let body = match body {
            Some(json) => {
                request = request.header(header::CONTENT_TYPE, "application/json");
                Body::from(json.to_string())
            }
            None => Body::empty(),
        };
        self.send(request.body(body).unwrap()).await
    }

    /// Sends a request exactly as given and remembers a session cookie the server sets.
    pub async fn send(&self, request: Request<Body>) -> (StatusCode, Value) {
        let response = self.app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        if let Some(set) = response.headers().get(header::SET_COOKIE) {
            let pair = set.to_str().unwrap().split(';').next().unwrap().to_owned();
            let cleared = pair.ends_with('=');
            *self.cookie.lock().unwrap() = (!cleared).then_some(pair);
        }
        let bytes = axum::body::to_bytes(response.into_body(), 8 << 20)
            .await
            .unwrap();
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(Value::Null)
        };
        (status, body)
    }

    pub async fn get(&self, path: &str) -> (StatusCode, Value) {
        self.call(Method::GET, path, None).await
    }

    pub async fn post(&self, path: &str, body: Value) -> (StatusCode, Value) {
        self.call(Method::POST, path, Some(body)).await
    }

    pub async fn put(&self, path: &str, body: Value) -> (StatusCode, Value) {
        self.call(Method::PUT, path, Some(body)).await
    }

    pub async fn delete(&self, path: &str) -> (StatusCode, Value) {
        self.call(Method::DELETE, path, None).await
    }

    pub fn cookie(&self) -> Option<String> {
        self.cookie.lock().unwrap().clone()
    }

    pub fn forget_cookie(&self) {
        *self.cookie.lock().unwrap() = None;
    }
}
