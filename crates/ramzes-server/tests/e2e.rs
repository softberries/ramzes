//! The real `ramzes` binary, driven over HTTP, delivering to a local mock receiver.

use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

const EMAIL: &str = "kris@example.com";
const PASSWORD: &str = "correct horse battery";

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// A running `ramzes` process, killed when dropped.
struct Ramzes {
    child: Child,
    base: String,
    http: Client,
    token: Option<String>,
}

impl Drop for Ramzes {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Ramzes {
    async fn start(data_dir: &Path, port: u16, env: &[(&str, String)], args: &[String]) -> Ramzes {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ramzes"));
        command
            .args(args)
            .env("RAMZES_LISTEN", format!("127.0.0.1:{port}"))
            .env("RAMZES_DATA_DIR", data_dir)
            .env_remove("RAMZES_DATABASE_URL")
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        for (name, value) in env {
            command.env(name, value);
        }
        let child = command.spawn().unwrap();
        let ramzes = Ramzes {
            child,
            base: format!("http://127.0.0.1:{port}"),
            http: Client::new(),
            token: None,
        };
        for _ in 0..200 {
            if ramzes
                .http
                .get(format!("{}/healthz", ramzes.base))
                .send()
                .await
                .is_ok_and(|r| r.status().is_success())
            {
                return ramzes;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("ramzes did not start on port {port}");
    }

    /// Creates the admin and an API token used for every later call.
    async fn set_up(&mut self) {
        let response = self
            .http
            .post(format!("{}/api/setup", self.base))
            .json(&json!({"email": EMAIL, "password": PASSWORD}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let cookie = response
            .headers()
            .get("set-cookie")
            .unwrap()
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let created: Value = self
            .http
            .post(format!("{}/api/tokens", self.base))
            .header("cookie", cookie)
            .header("x-ramzes-request", "1")
            .json(&json!({"name": "e2e"}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        self.token = Some(created["value"].as_str().unwrap().to_owned());
    }

    async fn post(&self, path: &str, body: Value) -> Value {
        let response = self
            .http
            .post(format!("{}{path}", self.base))
            .bearer_auth(self.token.as_ref().unwrap())
            .json(&body)
            .send()
            .await
            .unwrap();
        assert!(
            response.status().is_success(),
            "POST {path}: {}",
            response.status()
        );
        response.json().await.unwrap()
    }

    async fn get(&self, path: &str) -> Value {
        let response = self
            .http
            .get(format!("{}{path}", self.base))
            .bearer_auth(self.token.as_ref().unwrap())
            .send()
            .await
            .unwrap();
        assert!(
            response.status().is_success(),
            "GET {path}: {}",
            response.status()
        );
        response.json().await.unwrap()
    }

    /// Creates a source, a webhook connection to `receiver` and a catch-all route. Returns the source key.
    async fn wire_to(&self, receiver: &MockServer) -> String {
        let source = self
            .post("/api/sources", json!({"name": "orders-api"}))
            .await;
        let connection = self
            .post(
                "/api/connections",
                json!({"kind": "webhook", "name": "receiver", "settings": {"url": receiver.uri()},
                       "credentials": {"secret": "s3cret"}}),
            )
            .await;
        self.post(
            "/api/routes",
            json!({"name": "everything", "match": {}, "targets": [{"connection_id": connection["id"]}]}),
        )
        .await;
        source["key"].as_str().unwrap().to_owned()
    }

    async fn send_event(&self, key: &str, body: Value) -> String {
        let response = self
            .http
            .post(format!("{}/v1/events", self.base))
            .bearer_auth(key)
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let reply: Value = response.json().await.unwrap();
        reply["ids"][0].as_str().unwrap().to_owned()
    }

    /// Waits until the event's first delivery reaches `state`.
    async fn wait_for_delivery(&self, event_id: &str, state: &str) -> Value {
        let mut last = Value::Null;
        for _ in 0..300 {
            last = self.get(&format!("/api/events/{event_id}")).await;
            if last["notifications"][0]["deliveries"][0]["state"] == json!(state) {
                return last;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("delivery never became {state}: {last}");
    }
}

async fn delivers_end_to_end(env: &[(&str, String)]) {
    let receiver = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&receiver)
        .await;
    let data = tempfile::tempdir().unwrap();
    let mut ramzes = Ramzes::start(data.path(), free_port(), env, &[]).await;
    ramzes.set_up().await;
    let key = ramzes.wire_to(&receiver).await;

    let id = ramzes
        .send_event(&key, json!({"topic": "orders.failed", "title": "Charge failed", "severity": "error", "labels": {"env": "prod"}}))
        .await;
    let journey = ramzes.wait_for_delivery(&id, "delivered").await;
    assert_eq!(
        journey["notifications"][0]["route_name"],
        json!("everything")
    );
    assert_eq!(
        journey["notifications"][0]["deliveries"][0]["attempts"][0]["status"],
        json!(200)
    );

    let requests = receiver.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(body["title"], json!("Charge failed"));
    assert_eq!(body["labels"], json!({"env": "prod"}));
    assert!(
        requests[0]
            .headers
            .get("x-ramzes-signature")
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("sha256=")
    );

    let ready = ramzes
        .http
        .get(format!("{}/readyz", ramzes.base))
        .send()
        .await
        .unwrap();
    assert_eq!(ready.status(), StatusCode::OK);
    let metrics = ramzes
        .http
        .get(format!("{}/metrics", ramzes.base))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        metrics.contains("ramzes_events_ingested_total 1"),
        "{metrics}"
    );
    assert!(
        metrics.contains(r#"ramzes_deliveries_total{connector="webhook",outcome="delivered"} 1"#),
        "{metrics}"
    );
    assert!(metrics.contains("ramzes_queue_unrouted 0"), "{metrics}");
    assert!(
        metrics.contains("ramzes_queue_pending_deliveries 0"),
        "{metrics}"
    );
    assert!(
        metrics.contains("ramzes_delivery_duration_seconds_count 1"),
        "{metrics}"
    );
}

#[tokio::test]
async fn an_event_is_delivered_on_sqlite() {
    delivers_end_to_end(&[]).await;
}

#[tokio::test]
async fn an_event_is_delivered_on_postgres() {
    let Ok(url) = std::env::var("RAMZES_TEST_POSTGRES_URL") else {
        eprintln!("skipped: RAMZES_TEST_POSTGRES_URL is not set");
        return;
    };
    let schema = format!("e2e_{}", free_port());
    let admin = sqlx::PgPool::connect(&url).await.unwrap();
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP SCHEMA IF EXISTS {schema} CASCADE"
    )))
    .execute(&admin)
    .await
    .unwrap();
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    let sep = if url.contains('?') { '&' } else { '?' };
    delivers_end_to_end(&[(
        "RAMZES_DATABASE_URL",
        format!("{url}{sep}options=-c%20search_path%3D{schema}"),
    )])
    .await;
}

#[tokio::test]
async fn a_delivery_interrupted_by_a_crash_is_sent_after_restart() {
    let receiver = MockServer::start().await;
    // The first request hangs long enough for the process to be killed while it waits.
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(30)))
        .up_to_n_times(1)
        .mount(&receiver)
        .await;
    let data = tempfile::tempdir().unwrap();
    let env = [("RAMZES_LEASE_MS", "1500".to_string())];
    let port = free_port();

    let mut ramzes = Ramzes::start(data.path(), port, &env, &[]).await;
    ramzes.set_up().await;
    let key = ramzes.wire_to(&receiver).await;
    let token = ramzes.token.clone();
    let id = ramzes
        .send_event(&key, json!({"topic": "a", "title": "survives a crash"}))
        .await;
    for _ in 0..200 {
        if !receiver.received_requests().await.unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(
        receiver.received_requests().await.unwrap().len(),
        1,
        "the first attempt never started"
    );
    drop(ramzes); // SIGKILL: no shutdown code runs

    receiver.reset().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&receiver)
        .await;
    let mut ramzes = Ramzes::start(data.path(), port, &env, &[]).await;
    ramzes.token = token;
    let journey = ramzes.wait_for_delivery(&id, "delivered").await;
    assert_eq!(journey["event"]["title"], json!("survives a crash"));
    let requests = receiver.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(body["title"], json!("survives a crash"));
}

#[tokio::test]
async fn unknown_pages_get_the_ui_and_unknown_api_paths_get_json() {
    let data = tempfile::tempdir().unwrap();
    let ramzes = Ramzes::start(data.path(), free_port(), &[], &[]).await;

    let page = ramzes
        .http
        .get(format!("{}/events/some-id", ramzes.base))
        .send()
        .await
        .unwrap();
    assert_eq!(page.status(), StatusCode::OK);
    assert!(
        page.headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("text/html")
    );
    assert!(page.text().await.unwrap().to_lowercase().contains("<html"));

    for path in ["/api/nope", "/v1/nope"] {
        let missing = ramzes
            .http
            .get(format!("{}{path}", ramzes.base))
            .send()
            .await
            .unwrap();
        assert_eq!(missing.status(), StatusCode::NOT_FOUND, "{path}");
        let body: Value = missing.json().await.unwrap();
        assert_eq!(body["error"], json!("not_found"));
    }
    let health = ramzes
        .http
        .get(format!("{}/healthz", ramzes.base))
        .send()
        .await
        .unwrap();
    assert_eq!(health.text().await.unwrap(), "ok");
}

#[tokio::test]
async fn the_environment_overrides_the_config_file() {
    let data = tempfile::tempdir().unwrap();
    let config = data.path().join("ramzes.toml");
    // The file names a port nothing listens on; RAMZES_LISTEN (set by the harness) must win.
    std::fs::write(
        &config,
        format!(
            "listen = \"127.0.0.1:{}\"\nsecure_cookies = true\n",
            free_port()
        ),
    )
    .unwrap();
    let args = ["--config".to_string(), config.display().to_string()];
    let ramzes = Ramzes::start(data.path(), free_port(), &[], &args).await;

    // secure_cookies came from the file.
    let response = ramzes
        .http
        .post(format!("{}/api/setup", ramzes.base))
        .json(&json!({"email": EMAIL, "password": PASSWORD}))
        .send()
        .await
        .unwrap();
    assert!(
        response
            .headers()
            .get("set-cookie")
            .unwrap()
            .to_str()
            .unwrap()
            .contains("Secure")
    );
}

#[tokio::test]
async fn backup_writes_a_database_that_holds_the_events() {
    let receiver = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&receiver)
        .await;
    let data = tempfile::tempdir().unwrap();
    let mut ramzes = Ramzes::start(data.path(), free_port(), &[], &[]).await;
    ramzes.set_up().await;
    let key = ramzes.wire_to(&receiver).await;
    let id = ramzes
        .send_event(&key, json!({"topic": "a", "title": "kept in the backup"}))
        .await;

    let target = data.path().join("backup.db");
    let run = |file: &Path| {
        Command::new(env!("CARGO_BIN_EXE_ramzes"))
            .args(["backup", &file.display().to_string()])
            .env("RAMZES_DATA_DIR", data.path())
            .env_remove("RAMZES_DATABASE_URL")
            .output()
            .unwrap()
    };
    let output = run(&target);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let backup = sqlx::SqlitePool::connect(&format!("sqlite://{}", target.display()))
        .await
        .unwrap();
    let (title,): (String,) = sqlx::query_as("SELECT title FROM events WHERE id = ?1")
        .bind(&id)
        .fetch_one(&backup)
        .await
        .unwrap();
    assert_eq!(title, "kept in the backup");

    // An existing file is never overwritten.
    let again = run(&target);
    assert!(!again.status.success());
    assert!(String::from_utf8_lossy(&again.stderr).contains("exists"));
}

#[test]
fn openapi_prints_the_document() {
    let output = Command::new(env!("CARGO_BIN_EXE_ramzes"))
        .arg("openapi")
        .output()
        .unwrap();
    assert!(output.status.success());
    let document: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(document["paths"]["/api/events"]["get"].is_object());
    assert_eq!(document["info"]["title"], json!("ramzes admin API"));
}

fn cli(data_dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ramzes"))
        .args(args)
        .env("RAMZES_DATA_DIR", data_dir)
        .env_remove("RAMZES_DATABASE_URL")
        .output()
        .unwrap()
}

#[tokio::test]
async fn export_and_import_move_the_configuration_to_another_instance() {
    let receiver = MockServer::start().await;
    let first = tempfile::tempdir().unwrap();
    let file = first.path().join("ramzes.yaml");
    {
        let mut ramzes = Ramzes::start(first.path(), free_port(), &[], &[]).await;
        ramzes.set_up().await;
        ramzes.wire_to(&receiver).await;
        ramzes
            .post(
                "/api/templates",
                json!({"name": "Ops", "title": "{{ title }}", "body": ""}),
            )
            .await;
    }
    let exported = cli(first.path(), &["export", &file.display().to_string()]);
    assert!(
        exported.status.success(),
        "{}",
        String::from_utf8_lossy(&exported.stderr)
    );
    let yaml = std::fs::read_to_string(&file).unwrap();
    assert!(
        yaml.contains("everything") && yaml.contains("receiver"),
        "{yaml}"
    );
    assert!(
        !yaml.contains("s3cret") && !yaml.contains("credentials"),
        "{yaml}"
    );
    // Without a file the configuration goes to standard output.
    let printed = cli(first.path(), &["export"]);
    assert_eq!(String::from_utf8_lossy(&printed.stdout), yaml);

    let second = tempfile::tempdir().unwrap();
    let imported = cli(second.path(), &["import", &file.display().to_string()]);
    assert!(
        imported.status.success(),
        "{}",
        String::from_utf8_lossy(&imported.stderr)
    );
    let mut ramzes = Ramzes::start(second.path(), free_port(), &[], &[]).await;
    ramzes.set_up().await;
    assert_eq!(ramzes.get("/api/routes").await[0]["name"], "everything");
    assert_eq!(ramzes.get("/api/templates").await[0]["name"], "Ops");
    let connections = ramzes.get("/api/connections").await;
    assert_eq!(connections[0]["name"], "receiver");
    // Credentials do not travel: they have to be entered again on the new instance.
    assert_eq!(connections[0]["has_credentials"], false);

    std::fs::write(&file, "routes:\n  - {id: r, name: r, position: 1, enabled: true, stop: false, match: {}, targets: [{connection_id: nope}]}\n").unwrap();
    let refused = cli(second.path(), &["import", &file.display().to_string()]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("nope"));
}

#[tokio::test]
async fn private_addresses_can_be_blocked_for_the_whole_instance() {
    let receiver = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&receiver)
        .await;
    let data = tempfile::tempdir().unwrap();
    let env = [("RAMZES_BLOCK_PRIVATE_ADDRESSES", "true".to_string())];
    let mut ramzes = Ramzes::start(data.path(), free_port(), &env, &[]).await;
    ramzes.set_up().await;
    let key = ramzes.wire_to(&receiver).await;
    let id = ramzes
        .send_event(&key, json!({"topic": "a", "title": "x"}))
        .await;
    let journey = ramzes.wait_for_delivery(&id, "failed").await;
    let error = journey["notifications"][0]["deliveries"][0]["last_error"]
        .as_str()
        .unwrap();
    assert!(error.contains("private"), "{error}");
    assert!(receiver.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_custom_connector_survives_a_restart() {
    let receiver = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&receiver)
        .await;
    let data = tempfile::tempdir().unwrap();
    let definition = "kind: chat\nname: Chat\nfields:\n  - {key: url, label: URL, kind: url, required: true}\nrequest:\n  url: \"{{ settings.url }}\"\n  body: '{\"text\": {{ message.title | tojson }}}'\n";
    let key = {
        let mut ramzes = Ramzes::start(data.path(), free_port(), &[], &[]).await;
        ramzes.set_up().await;
        ramzes
            .post(
                "/api/connector-definitions",
                json!({"definition": definition}),
            )
            .await;
        let source = ramzes.post("/api/sources", json!({"name": "svc"})).await;
        let connection = ramzes
            .post(
                "/api/connections",
                json!({"kind": "chat", "name": "chat", "settings": {"url": receiver.uri()}}),
            )
            .await;
        ramzes
            .post("/api/routes", json!({"name": "all", "match": {}, "targets": [{"connection_id": connection["id"]}]}))
            .await;
        source["key"].as_str().unwrap().to_owned()
    };
    let mut ramzes = Ramzes::start(data.path(), free_port(), &[], &[]).await;
    // The admin exists already; sign in for a token.
    let response = ramzes
        .http
        .post(format!("{}/api/auth/login", ramzes.base))
        .header("x-ramzes-request", "1")
        .json(&json!({"email": EMAIL, "password": PASSWORD}))
        .send()
        .await
        .unwrap();
    let cookie = response
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let created: Value = ramzes
        .http
        .post(format!("{}/api/tokens", ramzes.base))
        .header("cookie", cookie)
        .header("x-ramzes-request", "1")
        .json(&json!({"name": "again"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    ramzes.token = Some(created["value"].as_str().unwrap().to_owned());
    let id = ramzes
        .send_event(&key, json!({"topic": "a", "title": "after restart"}))
        .await;
    ramzes.wait_for_delivery(&id, "delivered").await;
    let request = receiver.received_requests().await.unwrap().remove(0);
    assert_eq!(
        serde_json::from_slice::<Value>(&request.body).unwrap(),
        json!({"text": "after restart"})
    );
}

#[tokio::test]
async fn a_grouped_route_sends_one_message_for_several_events() {
    let receiver = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&receiver)
        .await;
    let data = tempfile::tempdir().unwrap();
    let mut ramzes = Ramzes::start(data.path(), free_port(), &[], &[]).await;
    ramzes.set_up().await;
    let source = ramzes.post("/api/sources", json!({"name": "svc"})).await;
    let connection = ramzes
        .post(
            "/api/connections",
            json!({"kind": "webhook", "name": "receiver", "settings": {"url": receiver.uri()}}),
        )
        .await;
    ramzes
        .post(
            "/api/routes",
            json!({"name": "grouped", "match": {}, "targets": [{"connection_id": connection["id"]}],
                   "shaping": {"group": {"by": [], "wait_s": 2}}}),
        )
        .await;
    let key = source["key"].as_str().unwrap();
    let first = ramzes
        .send_event(key, json!({"topic": "a", "title": "one"}))
        .await;
    ramzes
        .send_event(key, json!({"topic": "a", "title": "two"}))
        .await;
    ramzes
        .send_event(key, json!({"topic": "a", "title": "three"}))
        .await;

    // Nothing is sent while the group collects.
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert!(receiver.received_requests().await.unwrap().is_empty());
    let waiting = ramzes.get("/api/notifications?state=grouping").await;
    assert_eq!(waiting["notifications"][0]["event_count"], 3);

    ramzes.wait_for_delivery(&first, "delivered").await;
    let requests = receiver.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(body["title"], "3 events");
    assert_eq!(body["event_ids"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn the_link_in_a_message_acknowledges_and_stops_the_escalation() {
    let receiver = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&receiver)
        .await;
    let data = tempfile::tempdir().unwrap();
    let port = free_port();
    let env = [("RAMZES_PUBLIC_URL", format!("http://127.0.0.1:{port}"))];
    let mut ramzes = Ramzes::start(data.path(), port, &env, &[]).await;
    ramzes.set_up().await;
    let source = ramzes.post("/api/sources", json!({"name": "svc"})).await;
    let connection = ramzes
        .post(
            "/api/connections",
            json!({"kind": "webhook", "name": "receiver", "settings": {"url": receiver.uri()}}),
        )
        .await;
    ramzes
        .post(
            "/api/routes",
            json!({"name": "paged", "match": {}, "targets": [{"connection_id": connection["id"]}],
                   "escalation": [{"after_s": 2, "targets": [{"connection_id": connection["id"]}]}]}),
        )
        .await;
    let id = ramzes
        .send_event(
            source["key"].as_str().unwrap(),
            json!({"topic": "a", "title": "Database down"}),
        )
        .await;
    ramzes.wait_for_delivery(&id, "delivered").await;
    let first: Value =
        serde_json::from_slice(&receiver.received_requests().await.unwrap()[0].body).unwrap();
    let link = first["ack_link"].as_str().unwrap().to_owned();

    // Opening the link does nothing by itself; submitting its form acknowledges.
    let page = ramzes.http.get(&link).send().await.unwrap();
    assert_eq!(page.status(), StatusCode::OK);
    assert!(
        page.text()
            .await
            .unwrap()
            .contains("Acknowledge this notification?")
    );
    let (address, token) = link.split_once("?t=").unwrap();
    let done = ramzes
        .http
        .post(address)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(format!("t={token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(done.status(), StatusCode::OK);
    assert!(done.text().await.unwrap().contains("Acknowledged"));

    // The escalation step that was two seconds away never goes out.
    tokio::time::sleep(Duration::from_millis(3500)).await;
    assert_eq!(receiver.received_requests().await.unwrap().len(), 1);
    let acknowledged = ramzes.get("/api/notifications?state=acknowledged").await;
    assert_eq!(acknowledged["notifications"][0]["acked_by"], "link");
}

#[tokio::test]
async fn metrics_ask_for_the_token_when_one_is_set() {
    let data = tempfile::tempdir().unwrap();
    let env = [("RAMZES_METRICS_TOKEN", "scrape-me".to_string())];
    let ramzes = Ramzes::start(data.path(), free_port(), &env, &[]).await;
    let url = format!("{}/metrics", ramzes.base);
    let open = ramzes.http.get(&url).send().await.unwrap();
    assert_eq!(open.status(), StatusCode::UNAUTHORIZED);
    let wrong = ramzes.http.get(&url).bearer_auth("guess").send().await.unwrap();
    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);
    let right = ramzes.http.get(&url).bearer_auth("scrape-me").send().await.unwrap();
    assert_eq!(right.status(), StatusCode::OK);
    assert!(right.text().await.unwrap().contains("ramzes_queue_unrouted"));
    // Health checks stay open: an orchestrator has no token.
    let health = ramzes.http.get(format!("{}/healthz", ramzes.base)).send().await.unwrap();
    assert_eq!(health.status(), StatusCode::OK);
}
