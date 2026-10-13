use std::time::{Duration, Instant};

use hmac::{Hmac, KeyInit, Mac};
use ramzes_connectors::{ConnectionConfig, Registry, SendOutcome, classify};
use ramzes_core::{EventStatus, Message, Severity};
use serde_json::{Value, json};
use sha2::Sha256;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

fn message() -> Message {
    Message {
        title: "Charge failed: card network timeout".into(),
        body: Some("14 charges failed in the last 30 s".into()),
        severity: Severity::Critical,
        status: EventStatus::Firing,
        topic: "payments.charge.failed".into(),
        fields: vec![
            ("env".into(), "prod".into()),
            ("service".into(), "payments".into()),
        ],
        link: Some("https://ramzes.example.com/events/0199".into()),
        event_ids: vec!["0199".into()],
    }
}

fn bare_message() -> Message {
    Message {
        title: "Deploy finished".into(),
        body: None,
        severity: Severity::Info,
        status: EventStatus::Resolved,
        topic: "deploy.finished".into(),
        fields: vec![],
        link: None,
        event_ids: vec!["0200".into()],
    }
}

fn registry() -> Registry {
    Registry::builtin(Duration::from_millis(500))
}

fn webhook(url: &str, secret: Option<&str>) -> ConnectionConfig {
    ConnectionConfig {
        settings: json!({"url": url}),
        credentials: json!({"secret": secret}),
    }
}

fn slack(url: &str) -> ConnectionConfig {
    ConnectionConfig {
        settings: json!({}),
        credentials: json!({"webhook_url": url}),
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

async fn only_request(server: &MockServer) -> wiremock::Request {
    let mut requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    requests.remove(0)
}

#[test]
fn classify_maps_status_codes() {
    let outcome = |status, retry_after| classify(status, retry_after, "body".into());
    assert!(matches!(
        outcome(200, None),
        SendOutcome::Delivered {
            status: Some(200),
            ..
        }
    ));
    assert!(matches!(outcome(204, None), SendOutcome::Delivered { .. }));
    for status in [400, 401, 403, 404, 410, 422] {
        assert!(
            matches!(outcome(status, None), SendOutcome::Permanent { .. }),
            "{status}"
        );
    }
    for status in [408, 425, 429, 500, 502, 503, 504] {
        assert!(
            matches!(
                outcome(status, None),
                SendOutcome::Retry { after_ms: None, .. }
            ),
            "{status}"
        );
    }
    assert!(matches!(
        outcome(429, Some("30")),
        SendOutcome::Retry {
            after_ms: Some(30_000),
            ..
        }
    ));
    assert!(matches!(
        outcome(503, Some("not a number")),
        SendOutcome::Retry { after_ms: None, .. }
    ));
    // 3xx is not followed and not a success.
    assert!(matches!(outcome(302, None), SendOutcome::Permanent { .. }));
}

#[test]
fn classify_keeps_the_status_and_response_text() {
    match classify(401, None, "invalid_token".into()) {
        SendOutcome::Permanent { status, detail } => {
            assert_eq!(status, Some(401));
            assert!(
                detail.contains("401") && detail.contains("invalid_token"),
                "{detail}"
            );
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn registry_describes_the_builtin_connectors() {
    let kinds: Vec<_> = registry()
        .descriptors()
        .into_iter()
        .map(|d| d.kind)
        .collect();
    assert_eq!(kinds, vec!["slack", "webhook"]);
    assert!(registry().get("slack").is_some());
    assert!(registry().get("carrier-pigeon").is_none());
    let slack = registry().get("slack").unwrap().descriptor();
    assert!(
        slack
            .fields
            .iter()
            .any(|f| f.key == "webhook_url" && f.secret && f.required)
    );
}

#[tokio::test]
async fn webhook_posts_the_message_as_json() {
    let server = server(ResponseTemplate::new(200)).await;
    let connector = registry().get("webhook").unwrap();
    let outcome = connector
        .send(
            &webhook(&server.uri(), None),
            &message(),
            None,
            1_791_884_467_000,
        )
        .await;
    assert!(
        matches!(
            outcome,
            SendOutcome::Delivered {
                status: Some(200),
                ..
            }
        ),
        "{outcome:?}"
    );

    let request = only_request(&server).await;
    assert_eq!(
        request.headers.get("content-type").unwrap(),
        "application/json"
    );
    assert!(request.headers.get("x-ramzes-signature").is_none());
    assert!(
        request
            .headers
            .get("user-agent")
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("ramzes/")
    );
    let body: Value = serde_json::from_slice(&request.body).unwrap();
    insta::assert_json_snapshot!("webhook_body", body);
}

#[tokio::test]
async fn webhook_signature_verifies_independently() {
    let server = server(ResponseTemplate::new(200)).await;
    let connector = registry().get("webhook").unwrap();
    connector
        .send(
            &webhook(&server.uri(), Some("s3cret")),
            &message(),
            None,
            1_791_884_467_000,
        )
        .await;

    let request = only_request(&server).await;
    let timestamp = request
        .headers
        .get("x-ramzes-timestamp")
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(timestamp, "1791884467000");
    let mut mac = Hmac::<Sha256>::new_from_slice(b"s3cret").unwrap();
    mac.update(timestamp.as_bytes());
    mac.update(b".");
    mac.update(&request.body);
    let expected = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));
    assert_eq!(
        request
            .headers
            .get("x-ramzes-signature")
            .unwrap()
            .to_str()
            .unwrap(),
        expected
    );
}

#[tokio::test]
async fn slack_renders_a_full_message() {
    let server = server(ResponseTemplate::new(200).set_body_string("ok")).await;
    let connector = registry().get("slack").unwrap();
    let outcome = connector
        .send(&slack(&server.uri()), &message(), None, 0)
        .await;
    assert!(
        matches!(outcome, SendOutcome::Delivered { .. }),
        "{outcome:?}"
    );
    let body: Value = serde_json::from_slice(&only_request(&server).await.body).unwrap();
    insta::assert_json_snapshot!("slack_full", body);
}

#[tokio::test]
async fn slack_renders_a_bare_message() {
    let server = server(ResponseTemplate::new(200).set_body_string("ok")).await;
    let connector = registry().get("slack").unwrap();
    connector
        .send(&slack(&server.uri()), &bare_message(), None, 0)
        .await;
    let body: Value = serde_json::from_slice(&only_request(&server).await.body).unwrap();
    insta::assert_json_snapshot!("slack_bare", body);
}

#[tokio::test]
async fn slack_truncates_text_to_its_limits() {
    let server = server(ResponseTemplate::new(200).set_body_string("ok")).await;
    let connector = registry().get("slack").unwrap();
    let mut m = message();
    m.title = "t".repeat(3000);
    m.body = Some("b".repeat(5000));
    m.fields = (0..15)
        .map(|i| (format!("k{i:02}"), "v".repeat(3000)))
        .collect();
    connector.send(&slack(&server.uri()), &m, None, 0).await;

    let body: Value = serde_json::from_slice(&only_request(&server).await.body).unwrap();
    let blocks = body["blocks"].as_array().unwrap();
    let header = blocks[0]["text"]["text"].as_str().unwrap();
    assert!(
        header.chars().count() <= 150 && header.ends_with('…'),
        "{}",
        header.chars().count()
    );
    let section = blocks[1]["text"]["text"].as_str().unwrap();
    assert!(section.chars().count() <= 3000 && section.ends_with('…'));
    let fields = blocks[2]["fields"].as_array().unwrap();
    assert_eq!(fields.len(), 10);
    assert!(
        fields
            .iter()
            .all(|f| f["text"].as_str().unwrap().chars().count() <= 2000)
    );
}

#[tokio::test]
async fn slack_rejection_is_permanent_with_the_reason() {
    let server = server(ResponseTemplate::new(404).set_body_string("no_service")).await;
    let connector = registry().get("slack").unwrap();
    match connector
        .send(&slack(&server.uri()), &message(), None, 0)
        .await
    {
        SendOutcome::Permanent { status, detail } => {
            assert_eq!(status, Some(404));
            assert!(detail.contains("no_service"));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn rate_limiting_is_retried_after_the_given_delay() {
    let server = server(ResponseTemplate::new(429).insert_header("retry-after", "7")).await;
    let connector = registry().get("webhook").unwrap();
    let outcome = connector
        .send(&webhook(&server.uri(), None), &message(), None, 0)
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
async fn a_hanging_service_is_a_retry_within_the_timeout() {
    let server = server(ResponseTemplate::new(200).set_delay(Duration::from_secs(5))).await;
    let connector = registry().get("webhook").unwrap();
    let started = Instant::now();
    let outcome = connector
        .send(&webhook(&server.uri(), None), &message(), None, 0)
        .await;
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "took {:?}",
        started.elapsed()
    );
    match outcome {
        SendOutcome::Retry {
            status: None,
            detail,
            ..
        } => assert!(detail.contains("timed out"), "{detail}"),
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn a_refused_connection_is_a_retry() {
    let connector = registry().get("webhook").unwrap();
    let outcome = connector
        .send(
            &webhook("http://127.0.0.1:9/hook", None),
            &message(),
            None,
            0,
        )
        .await;
    assert!(
        matches!(outcome, SendOutcome::Retry { status: None, .. }),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn a_long_response_is_cut_to_an_excerpt() {
    let server = server(ResponseTemplate::new(400).set_body_string("x".repeat(5000))).await;
    let connector = registry().get("webhook").unwrap();
    match connector
        .send(&webhook(&server.uri(), None), &message(), None, 0)
        .await
    {
        SendOutcome::Permanent { detail, .. } => {
            assert!(detail.chars().count() < 600, "{}", detail.len())
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn validate_rejects_missing_or_non_http_urls() {
    let webhook_connector = registry().get("webhook").unwrap();
    assert!(
        webhook_connector
            .validate(&webhook("https://example.com/hook", None))
            .is_ok()
    );
    assert!(
        webhook_connector
            .validate(&webhook("http://10.0.0.5:8080/hook", None))
            .is_ok()
    );
    assert!(
        webhook_connector
            .validate(&webhook("ftp://example.com", None))
            .is_err()
    );
    assert!(
        webhook_connector
            .validate(&webhook("not a url", None))
            .is_err()
    );
    let missing = ConnectionConfig {
        settings: json!({}),
        credentials: json!({}),
    };
    assert!(webhook_connector.validate(&missing).is_err());

    let slack_connector = registry().get("slack").unwrap();
    assert!(
        slack_connector
            .validate(&slack("https://hooks.slack.com/services/T/B/x"))
            .is_ok()
    );
    assert!(slack_connector.validate(&missing).is_err());
    assert!(slack_connector.validate(&slack("nope")).is_err());
}

#[tokio::test]
async fn slack_keeps_the_case_of_the_topic() {
    let server = server(ResponseTemplate::new(200).set_body_string("ok")).await;
    let connector = registry().get("slack").unwrap();
    let mut m = bare_message();
    m.topic = "Orders.Failed".into();
    connector.send(&slack(&server.uri()), &m, None, 0).await;
    let body: Value = serde_json::from_slice(&only_request(&server).await.body).unwrap();
    let context = body["blocks"][1]["elements"][0]["text"].as_str().unwrap();
    assert_eq!(context, "`Orders.Failed` · resolved · info");
}

#[tokio::test]
async fn nul_characters_are_removed_from_the_response_excerpt() {
    let server = server(ResponseTemplate::new(400).set_body_bytes(b"bad\0body".to_vec())).await;
    let connector = registry().get("webhook").unwrap();
    match connector
        .send(&webhook(&server.uri(), None), &message(), None, 0)
        .await
    {
        SendOutcome::Permanent { detail, .. } => assert_eq!(detail, "HTTP 400: badbody"),
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn slack_fallback_text_cannot_carry_a_mention() {
    let server = server(ResponseTemplate::new(200).set_body_string("ok")).await;
    let connector = registry().get("slack").unwrap();
    let mut m = bare_message();
    m.title = "<!channel> disk & cpu".into();
    connector.send(&slack(&server.uri()), &m, None, 0).await;
    let body: Value = serde_json::from_slice(&only_request(&server).await.body).unwrap();
    assert_eq!(body["text"], "✅ &lt;!channel&gt; disk &amp; cpu");
    // The header block is plain text, where the characters are shown as typed.
    assert_eq!(
        body["blocks"][0]["text"]["text"],
        "✅ <!channel> disk & cpu"
    );
}
