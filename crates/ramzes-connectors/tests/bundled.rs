//! The requests the bundled definitions produce, pinned as snapshots.

use std::time::Duration;

use ramzes_connectors::{ConnectionConfig, Preview, Registry};
use ramzes_core::{EventStatus, Message, Severity};
use serde_json::{Value, json};

fn full() -> Message {
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
        ..Message::default()
    }
}

fn bare() -> Message {
    Message {
        title: "Deploy finished".into(),
        body: None,
        severity: Severity::Info,
        status: EventStatus::Resolved,
        topic: "deploy.finished".into(),
        fields: vec![],
        link: None,
        event_ids: vec!["0200".into()],
        ..Message::default()
    }
}

fn awkward() -> Message {
    Message {
        title: "say \"hi\" <b>&\n{{ 7 * 7 }} \\ é 日本 ".repeat(40),
        body: Some("line\r\n\t\"quoted\" </code> {% boom %} \u{1F525}".repeat(400)),
        severity: Severity::Warning,
        status: EventStatus::Firing,
        topic: "a.b".into(),
        fields: (0..40)
            .map(|i| {
                (
                    format!("k{i}"),
                    if i == 0 {
                        String::new()
                    } else {
                        "v\"<>&".repeat(200)
                    },
                )
            })
            .collect(),
        link: Some("https://ramzes.example.com/events/1?a=\"b\"&c=<d>".into()),
        event_ids: vec!["1".into()],
        ..Message::default()
    }
}

fn config(kind: &str) -> ConnectionConfig {
    let (settings, credentials) = match kind {
        "discord" => (
            json!({}),
            json!({"webhook_url": "https://discord.com/api/webhooks/1/secret-token"}),
        ),
        "teams" => (
            json!({}),
            json!({"webhook_url": "https://example.environment.api.powerplatform.com/workflows/1?sig=secret-sig"}),
        ),
        "ntfy" => (
            json!({"server": "https://ntfy.sh", "topic": "ops"}),
            json!({"token": "tk_secret"}),
        ),
        "telegram" => (
            json!({"chat_id": "-1001234567890"}),
            json!({"bot_token": "123456:secret-bot-token"}),
        ),
        "pushover" => (
            json!({}),
            json!({"token": "app-secret-token", "user": "user-secret-key"}),
        ),
        other => panic!("no config for {other}"),
    };
    ConnectionConfig {
        settings,
        credentials,
    }
}

const KINDS: [&str; 5] = ["discord", "teams", "ntfy", "telegram", "pushover"];

fn preview(kind: &str, m: &Message, destination: Option<&str>) -> Preview {
    let registry = Registry::builtin(Duration::from_secs(1));
    let connector = registry.get(kind).unwrap();
    connector.validate(&config(kind)).unwrap();
    connector
        .preview(&config(kind), m, destination, 1_760_000_000_000)
        .unwrap()
}

#[test]
fn full_messages() {
    for kind in KINDS {
        insta::assert_json_snapshot!(format!("{kind}_full"), preview(kind, &full(), None));
    }
}

#[test]
fn bare_messages() {
    for kind in KINDS {
        insta::assert_json_snapshot!(format!("{kind}_bare"), preview(kind, &bare(), None));
    }
}

#[test]
fn awkward_text_still_produces_valid_json_within_the_services_limits() {
    for kind in KINDS {
        let p = preview(kind, &awkward(), None);
        let body: Value = serde_json::from_str(&p.body).unwrap_or_else(|e| panic!("{kind}: {e}"));
        assert!(!p.body.contains("49"), "{kind}: event text was evaluated");
        match kind {
            "discord" => {
                let embed = &body["embeds"][0];
                let total = embed["title"].as_str().unwrap().chars().count()
                    + embed["description"].as_str().unwrap().chars().count()
                    + embed["footer"]["text"].as_str().unwrap().chars().count()
                    + embed["fields"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|f| {
                            assert!(!f["value"].as_str().unwrap().is_empty());
                            f["name"].as_str().unwrap().chars().count()
                                + f["value"].as_str().unwrap().chars().count()
                        })
                        .sum::<usize>();
                assert!(total <= 6000, "discord embed has {total} characters");
                assert!(embed["title"].as_str().unwrap().chars().count() <= 256);
            }
            "telegram" => {
                let text = body["text"].as_str().unwrap();
                // The limit counts the text after entities are parsed, so tags and escapes are free.
                let parsed = text
                    .replace("&lt;", "<")
                    .replace("&gt;", ">")
                    .replace("&quot;", "\"")
                    .replace("&#x27;", "'")
                    .replace("&#x2f;", "/")
                    .replace("&amp;", "&");
                assert!(
                    parsed.chars().count() <= 4096,
                    "telegram text has {}",
                    parsed.chars().count()
                );
                assert!(
                    !text.contains("<b>&"),
                    "telegram text is not escaped: {text}"
                );
                assert_eq!(body["chat_id"], json!(-1001234567890_i64));
            }
            "pushover" => {
                assert!(body["message"].as_str().unwrap().chars().count() <= 1024);
                assert!(body["title"].as_str().unwrap().chars().count() <= 250);
                assert!(body["url"].as_str().unwrap().chars().count() <= 512);
            }
            "ntfy" => {
                assert!(body["message"].as_str().unwrap().len() <= 4096);
                assert!(body["title"].as_str().unwrap().len() <= 1024);
            }
            _ => assert!(
                p.body.len() < 28_000,
                "teams message is {} bytes",
                p.body.len()
            ),
        }
    }
}

#[test]
fn a_route_destination_overrides_the_default() {
    let ntfy: Value =
        serde_json::from_str(&preview("ntfy", &full(), Some("payments")).body).unwrap();
    assert_eq!(ntfy["topic"], "payments");
    let telegram: Value =
        serde_json::from_str(&preview("telegram", &full(), Some("@ops_channel")).body).unwrap();
    assert_eq!(telegram["chat_id"], "@ops_channel");
}

#[test]
fn an_optional_credential_adds_no_header_when_absent() {
    let registry = Registry::builtin(Duration::from_secs(1));
    let ntfy = registry.get("ntfy").unwrap();
    let open = ConnectionConfig {
        settings: json!({"server": "https://ntfy.sh", "topic": "ops"}),
        credentials: json!({}),
    };
    let p = ntfy.preview(&open, &full(), None, 1).unwrap();
    assert!(
        p.headers.iter().all(|(name, _)| name != "Authorization"),
        "{:?}",
        p.headers
    );
}

#[test]
fn secrets_never_appear_in_a_preview() {
    for kind in KINDS {
        let shown = format!("{:?}", preview(kind, &full(), None));
        assert!(!shown.contains("secret"), "{kind}: {shown}");
    }
}
