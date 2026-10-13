use std::time::Duration;

use ramzes_connectors::{ConnectionConfig, Definition, HttpOptions, Registry, SendOutcome};
use ramzes_core::{EventStatus, Message, Severity};
use serde_json::{Value, json};
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

const CHAT: &str = r#"
kind: chat
name: Chat
description: A test chat service.
destination_label: Room
fields:
  - { key: base_url, label: Base URL, kind: url, required: true }
  - { key: token, label: Token, secret: true, required: true }
request:
  method: POST
  url: "{{ settings.base_url }}/rooms/{{ destination | default('general') }}/messages"
  headers:
    Authorization: "Bearer {{ credentials.token }}"
    X-Severity: "{{ message.severity }}"
  body: |
    {
      "title": {{ message.title | cut(20) | tojson }},
      "text": {{ message.body | default("") | tojson }},
      "labels": {{ message.labels | tojson }},
      "link": {{ message.link | tojson }},
      "count": {{ message.count }}
    }
success: [200, 201]
retry_after_header: X-Wait
max_len: 4000
"#;

fn message(title: &str) -> Message {
    Message {
        title: title.into(),
        body: Some("line one\nline \"two\" \\ {{ 7 * 7 }}".into()),
        severity: Severity::Critical,
        status: EventStatus::Firing,
        topic: "payments.charge.failed".into(),
        fields: vec![("env".into(), "prod".into())],
        link: Some("https://ramzes.example.com/events/0199".into()),
        event_ids: vec!["0199".into()],
        ..Message::default()
    }
}

fn registry() -> Registry {
    let registry = Registry::builtin(Duration::from_millis(500));
    registry.set_custom(vec![Definition::parse(CHAT).unwrap()]);
    registry
}

fn config(base: &str) -> ConnectionConfig {
    ConnectionConfig {
        settings: json!({"base_url": base}),
        credentials: json!({"token": "s3cr3t-token"}),
    }
}

async fn server(response: ResponseTemplate) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(response)
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn a_definition_sends_the_request_it_describes() {
    let server = server(ResponseTemplate::new(201)).await;
    let chat = registry().get("chat").unwrap();
    let outcome = chat
        .send(
            &config(&server.uri()),
            &message("Charge failed: card network timeout"),
            Some("ops"),
            1,
        )
        .await;
    assert_eq!(
        outcome,
        SendOutcome::Delivered {
            external_ref: None,
            status: Some(201)
        }
    );
    let request = server.received_requests().await.unwrap().remove(0);
    assert_eq!(request.url.path(), "/rooms/ops/messages");
    assert_eq!(request.headers["authorization"], "Bearer s3cr3t-token");
    assert_eq!(request.headers["x-severity"], "critical");
    assert_eq!(request.headers["content-type"], "application/json");
    let body: Value = serde_json::from_slice(&request.body).unwrap();
    assert_eq!(
        body,
        json!({
            "title": "Charge failed: card…",
            "text": "line one\nline \"two\" \\ {{ 7 * 7 }}",
            "labels": {"env": "prod"},
            "link": "https://ramzes.example.com/events/0199",
            "count": 1
        })
    );
}

#[tokio::test]
async fn a_missing_destination_uses_the_default_in_the_definition() {
    let server = server(ResponseTemplate::new(200)).await;
    let chat = registry().get("chat").unwrap();
    chat.send(&config(&server.uri()), &message("x"), None, 1)
        .await;
    let request = server.received_requests().await.unwrap().remove(0);
    assert_eq!(request.url.path(), "/rooms/general/messages");
}

#[tokio::test]
async fn a_status_outside_the_success_list_is_not_delivered() {
    let server = server(ResponseTemplate::new(204)).await;
    let chat = registry().get("chat").unwrap();
    let outcome = chat
        .send(&config(&server.uri()), &message("x"), None, 1)
        .await;
    assert!(
        matches!(
            outcome,
            SendOutcome::Permanent {
                status: Some(204),
                ..
            }
        ),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn the_named_header_sets_the_retry_delay() {
    let server = server(ResponseTemplate::new(429).insert_header("X-Wait", "7")).await;
    let chat = registry().get("chat").unwrap();
    let outcome = chat
        .send(&config(&server.uri()), &message("x"), None, 1)
        .await;
    assert!(
        matches!(
            outcome,
            SendOutcome::Retry {
                after_ms: Some(7000),
                status: Some(429),
                ..
            }
        ),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn a_body_that_is_not_json_is_a_permanent_failure_and_nothing_is_sent() {
    let broken = CHAT.replace(
        "\"count\": {{ message.count }}",
        "\"count\": {{ message.title }}",
    );
    let registry = Registry::builtin(Duration::from_millis(500));
    registry.set_custom(vec![Definition::parse(&broken).unwrap()]);
    let server = server(ResponseTemplate::new(200)).await;
    let outcome = registry
        .get("chat")
        .unwrap()
        .send(&config(&server.uri()), &message("not a number"), None, 1)
        .await;
    assert!(
        matches!(outcome, SendOutcome::Permanent { status: None, .. }),
        "{outcome:?}"
    );
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[test]
fn the_preview_shows_the_request_with_secrets_masked() {
    let chat = registry().get("chat").unwrap();
    let preview = chat
        .preview(
            &config("https://chat.example.com"),
            &message("Charge failed"),
            Some("ops"),
            1,
        )
        .unwrap();
    assert_eq!(preview.method, "POST");
    assert_eq!(preview.url, "https://chat.example.com/rooms/ops/messages");
    let auth = preview
        .headers
        .iter()
        .find(|(name, _)| name == "Authorization")
        .unwrap();
    assert_eq!(auth.1, "Bearer ••••");
    assert!(!format!("{preview:?}").contains("s3cr3t-token"));
    let body: Value = serde_json::from_str(&preview.body).unwrap();
    assert_eq!(body["title"], "Charge failed");
}

#[test]
fn the_descriptor_comes_from_the_definition() {
    let d = registry().get("chat").unwrap().descriptor();
    assert_eq!((d.kind.as_str(), d.name.as_str()), ("chat", "Chat"));
    assert_eq!(d.destination_label.as_deref(), Some("Room"));
    assert_eq!(d.fields.len(), 2);
    assert!(d.fields[1].secret && d.fields[1].required);
    assert_eq!(d.capabilities.max_len, 4000);
    assert!(d.custom);
    assert!(!registry().get("slack").unwrap().descriptor().custom);
}

#[test]
fn validation_checks_required_fields_and_urls() {
    let chat = registry().get("chat").unwrap();
    assert!(chat.validate(&config("https://chat.example.com")).is_ok());
    let no_token = ConnectionConfig {
        settings: json!({"base_url": "https://chat.example.com"}),
        credentials: json!({}),
    };
    assert_eq!(chat.validate(&no_token).unwrap_err(), "Token is required");
    assert_eq!(
        chat.validate(&config("ftp://x")).unwrap_err(),
        "Base URL must be an http or https URL"
    );
}

#[test]
fn a_definition_with_a_mistake_names_the_place() {
    let cases = [
        (CHAT.replace("kind: chat", "kind: Chat Room"), "kind"),
        (
            CHAT.replace("{{ credentials.token }}", "{{ credentials.token "),
            "request.headers.Authorization",
        ),
        (
            CHAT.replace("{{ message.count }}", "{% if %}"),
            "request.body",
        ),
        (
            CHAT.replace("method: POST", "method: FETCH"),
            "request.method",
        ),
        (CHAT.replace("kind: chat", "kind: slack"), "kind"),
        ("kind: [".to_string(), "YAML"),
    ];
    for (text, place) in cases {
        let error = Definition::parse(&text).unwrap_err();
        assert!(error.contains(place), "expected {place:?} in {error:?}");
    }
}

#[test]
fn a_custom_definition_cannot_replace_a_builtin_connector() {
    let registry = registry();
    assert!(registry.get("webhook").is_some());
    let webhook = registry.get("webhook").unwrap().descriptor();
    assert!(!webhook.custom);
}

#[test]
fn replacing_the_custom_set_removes_the_old_kinds() {
    let registry = registry();
    registry.set_custom(vec![]);
    assert!(registry.get("chat").is_none());
    assert!(registry.descriptors().iter().all(|d| d.kind != "chat"));
}

fn local(block_private: bool) -> Registry {
    let registry = Registry::with_options(HttpOptions {
        timeout: Duration::from_millis(500),
        block_private,
    });
    registry.set_custom(vec![Definition::parse(CHAT).unwrap()]);
    registry
}

#[tokio::test]
async fn private_addresses_are_refused_when_the_instance_blocks_them() {
    let server = server(ResponseTemplate::new(200)).await;
    let outcome = local(true)
        .get("chat")
        .unwrap()
        .send(&config(&server.uri()), &message("x"), None, 1)
        .await;
    let SendOutcome::Permanent { detail, .. } = outcome else {
        panic!("{outcome:?}");
    };
    assert!(detail.contains("private"), "{detail}");
    assert!(server.received_requests().await.unwrap().is_empty());

    let by_name = local(true)
        .get("chat")
        .unwrap()
        .send(&config("http://localhost:9"), &message("x"), None, 1)
        .await;
    assert!(
        matches!(by_name, SendOutcome::Permanent { .. }),
        "{by_name:?}"
    );
}

#[tokio::test]
async fn private_addresses_are_allowed_by_default() {
    let server = server(ResponseTemplate::new(200)).await;
    let outcome = local(false)
        .get("chat")
        .unwrap()
        .send(&config(&server.uri()), &message("x"), None, 1)
        .await;
    assert!(matches!(outcome, SendOutcome::Delivered { .. }));
}
