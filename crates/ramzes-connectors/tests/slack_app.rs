use std::time::Duration;

use ramzes_connectors::{ConnectionConfig, Connector, Registry, SendOutcome};
use ramzes_core::{EventStatus, Message, Severity};
use serde_json::{Value, json};
use std::sync::Arc;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn message() -> Message {
    Message {
        title: "Database down".into(),
        body: Some("db-1 does not answer".into()),
        severity: Severity::Critical,
        status: EventStatus::Firing,
        topic: "infra.db".into(),
        fields: vec![("host".into(), "db-1".into())],
        link: Some("https://ramzes.example.com/events/1".into()),
        event_ids: vec!["1".into()],
        dedup_key: Some("db-1".into()),
        ack_link: Some("https://ramzes.example.com/ack/n1?t=abc".into()),
        notification_id: Some("n1".into()),
        thread_ref: None,
    }
}

fn app() -> Arc<dyn Connector> {
    Registry::builtin(Duration::from_millis(500))
        .get("slack_app")
        .unwrap()
}

fn config(api: &str) -> ConnectionConfig {
    ConnectionConfig {
        settings: json!({"channel": "C0DEFAULT", "api_url": api}),
        credentials: json!({"bot_token": "xoxb-secret-token", "signing_secret": "shh"}),
    }
}

async fn slack(api_method: &str, answer: ResponseTemplate) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/{api_method}")))
        .respond_with(answer)
        .mount(&server)
        .await;
    server
}

fn ok() -> ResponseTemplate {
    ResponseTemplate::new(200)
        .set_body_json(json!({"ok": true, "channel": "C0OPS", "ts": "1503435956.000247"}))
}

#[tokio::test]
async fn a_message_is_posted_with_buttons_and_its_reference_is_kept() {
    let server = slack("chat.postMessage", ok()).await;
    let outcome = app()
        .send(&config(&server.uri()), &message(), Some("C0OPS"), 1)
        .await;
    assert_eq!(
        outcome,
        SendOutcome::Delivered {
            external_ref: Some("C0OPS:1503435956.000247".into()),
            status: Some(200)
        }
    );
    let request = server.received_requests().await.unwrap().remove(0);
    assert_eq!(request.headers["authorization"], "Bearer xoxb-secret-token");
    let body: Value = serde_json::from_slice(&request.body).unwrap();
    insta::assert_json_snapshot!("slack_app_post", body);
    assert_eq!(body["channel"], "C0OPS");
    assert!(body.get("thread_ts").is_none());
}

#[tokio::test]
async fn a_follow_up_goes_into_the_thread_of_the_first_message_in_the_same_channel() {
    let server = slack("chat.postMessage", ok()).await;
    let mut follow_up = message();
    follow_up.thread_ref = Some("C0OPS:1503435956.000247".into());
    app()
        .send(&config(&server.uri()), &follow_up, Some("C0OPS"), 1)
        .await;
    app()
        .send(&config(&server.uri()), &follow_up, Some("C0OTHER"), 1)
        .await;
    let requests = server.received_requests().await.unwrap();
    let body = |i: usize| serde_json::from_slice::<Value>(&requests[i].body).unwrap();
    assert_eq!(body(0)["thread_ts"], "1503435956.000247");
    assert!(body(1).get("thread_ts").is_none());
}

#[tokio::test]
async fn a_resolved_message_has_no_buttons_and_the_default_channel_is_used() {
    let server = slack("chat.postMessage", ok()).await;
    let mut resolved = message();
    resolved.status = EventStatus::Resolved;
    app().send(&config(&server.uri()), &resolved, None, 1).await;
    let body: Value =
        serde_json::from_slice(&server.received_requests().await.unwrap()[0].body).unwrap();
    assert_eq!(body["channel"], "C0DEFAULT");
    assert!(
        body["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|b| b["type"] != "actions")
    );
}

#[tokio::test]
async fn slacks_errors_are_sorted_into_retry_and_give_up() {
    let answer = |error: &str| {
        ResponseTemplate::new(200).set_body_json(json!({"ok": false, "error": error}))
    };
    let send = |server: MockServer| async move {
        app()
            .send(&config(&server.uri()), &message(), None, 1)
            .await
    };

    let outcome = send(slack("chat.postMessage", answer("not_in_channel")).await).await;
    let SendOutcome::Permanent { detail, .. } = outcome else {
        panic!("{outcome:?}")
    };
    assert!(
        detail.contains("not_in_channel") && detail.contains("invite"),
        "{detail}"
    );

    let outcome = send(slack("chat.postMessage", answer("ratelimited")).await).await;
    assert!(matches!(outcome, SendOutcome::Retry { .. }), "{outcome:?}");

    let limited = ResponseTemplate::new(429).insert_header("Retry-After", "30");
    let outcome = send(slack("chat.postMessage", limited).await).await;
    assert!(
        matches!(
            outcome,
            SendOutcome::Retry {
                after_ms: Some(30_000),
                ..
            }
        ),
        "{outcome:?}"
    );

    let outcome = send(
        slack(
            "chat.postMessage",
            ResponseTemplate::new(200).set_body_string("<html>"),
        )
        .await,
    )
    .await;
    assert!(matches!(outcome, SendOutcome::Retry { .. }), "{outcome:?}");
}

#[tokio::test]
async fn an_acknowledged_message_is_rewritten_without_buttons() {
    let server = slack("chat.update", ok()).await;
    app()
        .update(
            &config(&server.uri()),
            "C0OPS:1503435956.000247",
            &message(),
            "Acknowledged by kris@example.com",
        )
        .await
        .unwrap();
    let body: Value =
        serde_json::from_slice(&server.received_requests().await.unwrap()[0].body).unwrap();
    assert_eq!(
        (body["channel"].as_str(), body["ts"].as_str()),
        (Some("C0OPS"), Some("1503435956.000247"))
    );
    let blocks = body["blocks"].as_array().unwrap();
    assert!(blocks.iter().all(|b| b["type"] != "actions"));
    assert_eq!(
        blocks.last().unwrap()["elements"][0]["text"],
        "Acknowledged by kris@example.com"
    );

    let failing = slack(
        "chat.update",
        ResponseTemplate::new(200)
            .set_body_json(json!({"ok": false, "error": "message_not_found"})),
    )
    .await;
    let error = app()
        .update(&config(&failing.uri()), "C0OPS:1.2", &message(), "x")
        .await
        .unwrap_err();
    assert!(error.contains("message_not_found"));
    assert!(
        app()
            .update(&config(&failing.uri()), "not-a-reference", &message(), "x")
            .await
            .is_err()
    );
}

#[test]
fn validation_and_preview() {
    let app = app();
    assert!(app.validate(&config("https://slack.com/api")).is_ok());
    let no_token = ConnectionConfig {
        settings: json!({}),
        credentials: json!({}),
    };
    assert_eq!(
        app.validate(&no_token).unwrap_err(),
        "Bot token is required"
    );
    let preview = app
        .preview(&config("https://slack.com/api"), &message(), None, 1)
        .unwrap();
    assert_eq!(preview.url, "https://slack.com/api/chat.postMessage");
    assert!(!format!("{preview:?}").contains("xoxb-secret-token"));
    let descriptor = app.descriptor();
    assert!(
        descriptor.capabilities.buttons
            && descriptor.capabilities.update
            && descriptor.capabilities.threads
    );
    let no_channel = ConnectionConfig {
        settings: json!({}),
        credentials: json!({"bot_token": "x"}),
    };
    assert!(
        app.preview(&no_channel, &message(), None, 1)
            .unwrap_err()
            .contains("channel")
    );
}
