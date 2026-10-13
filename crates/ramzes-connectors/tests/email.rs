use std::sync::{Arc, Mutex};
use std::time::Duration;

use ramzes_connectors::{ConnectionConfig, Registry, SendOutcome};
use ramzes_core::{EventStatus, Message, Severity};
use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

/// What the fake server saw: every command line, and the message data.
#[derive(Default, Clone)]
struct Seen {
    commands: Vec<String>,
    data: String,
}

/// A plain SMTP server good for one or more messages. `refuse` rejects a recipient.
async fn smtp(refuse: Option<&'static str>) -> (u16, Arc<Mutex<Seen>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Seen::default()));
    let shared = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((socket, _)) = listener.accept().await else {
                return;
            };
            let seen = shared.clone();
            tokio::spawn(async move {
                let (read, mut write) = socket.into_split();
                let mut lines = BufReader::new(read).lines();
                write.write_all(b"220 test ESMTP\r\n").await.unwrap();
                let mut in_data = false;
                while let Ok(Some(line)) = lines.next_line().await {
                    if in_data {
                        if line == "." {
                            in_data = false;
                            write.write_all(b"250 queued\r\n").await.unwrap();
                        } else {
                            let mut seen = seen.lock().unwrap();
                            seen.data.push_str(&line);
                            seen.data.push('\n');
                        }
                        continue;
                    }
                    seen.lock().unwrap().commands.push(line.clone());
                    let upper = line.to_ascii_uppercase();
                    let reply: &[u8] = if upper.starts_with("EHLO") {
                        b"250-test\r\n250 AUTH PLAIN LOGIN\r\n"
                    } else if upper.starts_with("AUTH") {
                        b"235 ok\r\n"
                    } else if upper.starts_with("RCPT") && refuse.is_some_and(|r| line.contains(r))
                    {
                        b"550 no such user\r\n"
                    } else if upper.starts_with("DATA") {
                        in_data = true;
                        b"354 go on\r\n"
                    } else if upper.starts_with("QUIT") {
                        write.write_all(b"221 bye\r\n").await.unwrap();
                        return;
                    } else {
                        b"250 ok\r\n"
                    };
                    write.write_all(reply).await.unwrap();
                }
            });
        }
    });
    (port, seen)
}

fn config(port: u16) -> ConnectionConfig {
    ConnectionConfig {
        settings: json!({
            "host": "127.0.0.1", "port": port.to_string(), "security": "none",
            "from": "ramzes <alerts@example.com>", "to": "fallback@example.com", "username": "alerts",
        }),
        credentials: json!({"password": "hunter2"}),
    }
}

fn message(title: &str) -> Message {
    Message {
        title: title.into(),
        body: Some("14 charges failed in the last 30 s".into()),
        severity: Severity::Critical,
        status: EventStatus::Firing,
        topic: "payments.charge.failed".into(),
        fields: vec![("env".into(), "<prod>".into())],
        link: Some("https://ramzes.example.com/events/0199".into()),
        event_ids: vec!["0199".into()],
        ..Message::default()
    }
}

fn email() -> Arc<dyn ramzes_connectors::Connector> {
    Registry::builtin(Duration::from_secs(2))
        .get("email")
        .unwrap()
}

#[tokio::test]
async fn a_message_is_sent_to_every_recipient_of_the_destination() {
    let (port, seen) = smtp(None).await;
    let outcome = email()
        .send(
            &config(port),
            &message("Charge failed"),
            Some("ops@example.com, oncall@example.com"),
            1,
        )
        .await;
    assert!(
        matches!(outcome, SendOutcome::Delivered { .. }),
        "{outcome:?}"
    );
    let seen = seen.lock().unwrap().clone();
    let commands = seen.commands.join("\n");
    assert!(
        commands.contains("MAIL FROM:<alerts@example.com>"),
        "{commands}"
    );
    assert!(commands.contains("RCPT TO:<ops@example.com>"), "{commands}");
    assert!(
        commands.contains("RCPT TO:<oncall@example.com>"),
        "{commands}"
    );
    assert!(!commands.contains("fallback@example.com"));
    assert!(commands.contains("AUTH"), "{commands}");
    assert!(
        seen.data.contains("Subject: [critical] Charge failed"),
        "{}",
        seen.data
    );
    assert!(seen.data.contains("14 charges failed in the last 30 s"));
    assert!(seen.data.contains("https://ramzes.example.com/events/0199"));
    // The HTML part escapes event text.
    assert!(seen.data.contains("&lt;prod&gt;"), "{}", seen.data);
}

#[tokio::test]
async fn without_a_destination_the_connections_recipient_is_used() {
    let (port, seen) = smtp(None).await;
    let outcome = email().send(&config(port), &message("x"), None, 1).await;
    assert!(
        matches!(outcome, SendOutcome::Delivered { .. }),
        "{outcome:?}"
    );
    assert!(
        seen.lock()
            .unwrap()
            .commands
            .join("\n")
            .contains("RCPT TO:<fallback@example.com>")
    );
}

#[tokio::test]
async fn a_refused_recipient_is_a_permanent_failure() {
    let (port, _) = smtp(Some("ghost@example.com")).await;
    let outcome = email()
        .send(&config(port), &message("x"), Some("ghost@example.com"), 1)
        .await;
    let SendOutcome::Permanent { detail, .. } = outcome else {
        panic!("{outcome:?}");
    };
    assert!(detail.contains("550"), "{detail}");
}

#[tokio::test]
async fn an_unreachable_server_is_retried() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let outcome = email().send(&config(port), &message("x"), None, 1).await;
    assert!(matches!(outcome, SendOutcome::Retry { .. }), "{outcome:?}");
}

#[tokio::test]
async fn a_title_cannot_add_headers() {
    let (port, seen) = smtp(None).await;
    let outcome = email()
        .send(
            &config(port),
            &message("Charge failed\r\nBcc: attacker@example.com"),
            None,
            1,
        )
        .await;
    assert!(
        matches!(outcome, SendOutcome::Delivered { .. }),
        "{outcome:?}"
    );
    let seen = seen.lock().unwrap().clone();
    assert!(!seen.commands.join("\n").contains("attacker@example.com"));
    let headers = seen.data.split("\n\n").next().unwrap();
    assert!(
        !headers.lines().any(|line| line.starts_with("Bcc:")),
        "{headers}"
    );
}

#[test]
fn validation_explains_what_is_missing() {
    let email = email();
    assert!(email.validate(&config(25)).is_ok());
    let mut c = config(25);
    c.settings["from"] = json!("not an address");
    assert!(email.validate(&c).unwrap_err().contains("From"));
    let mut c = config(25);
    c.settings["port"] = json!("smtp");
    assert!(email.validate(&c).unwrap_err().contains("Port"));
    let mut c = config(25);
    c.settings["security"] = json!("maybe");
    assert!(email.validate(&c).unwrap_err().contains("Security"));
    let mut c = config(25);
    c.settings["host"] = json!("");
    assert!(email.validate(&c).unwrap_err().contains("Server"));
}

#[test]
fn the_preview_shows_the_email() {
    let preview = email()
        .preview(
            &config(25),
            &message("Charge failed"),
            Some("ops@example.com"),
            1,
        )
        .unwrap();
    assert_eq!(preview.method, "SMTP");
    assert_eq!(preview.url, "127.0.0.1:25");
    assert!(
        preview
            .headers
            .contains(&("To".to_owned(), "ops@example.com".to_owned()))
    );
    assert!(
        preview
            .headers
            .contains(&("Subject".to_owned(), "[critical] Charge failed".to_owned()))
    );
    assert!(preview.body.contains("14 charges failed"));
    assert!(!format!("{preview:?}").contains("hunter2"));
}
