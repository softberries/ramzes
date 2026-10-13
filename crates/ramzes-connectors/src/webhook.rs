use std::collections::BTreeMap;

use async_trait::async_trait;
use hmac::{Hmac, KeyInit, Mac};
use ramzes_core::Message;
use serde_json::json;
use sha2::Sha256;

use crate::http::Http;
use crate::{
    Capabilities, ConnectionConfig, Connector, ConnectorDescriptor, Field, FieldKind, SendOutcome,
    require_http_url,
};

/// Posts the message as JSON to any URL, signed when a secret is configured.
///
/// The signature is `sha256=<hex>` of HMAC-SHA256 over `<timestamp>.<body>`, sent in
/// `X-Ramzes-Signature` with the timestamp in `X-Ramzes-Timestamp`.
pub(crate) struct Webhook {
    http: Http,
}

impl Webhook {
    pub fn new(http: Http) -> Webhook {
        Webhook { http }
    }
}

fn body(m: &Message, now_ms: i64) -> Vec<u8> {
    let labels: BTreeMap<&str, &str> = m
        .fields
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    json!({
        "title": m.title,
        "body": m.body,
        "severity": m.severity,
        "status": m.status,
        "topic": m.topic,
        "labels": labels,
        "link": m.link,
        "event_ids": m.event_ids,
        "sent_at": now_ms,
    })
    .to_string()
    .into_bytes()
}

fn signature(secret: &str, timestamp: &str, body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .expect("HMAC accepts a key of any length");
    mac.update(timestamp.as_bytes());
    mac.update(b".");
    mac.update(body);
    format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
}

#[async_trait]
impl Connector for Webhook {
    fn descriptor(&self) -> ConnectorDescriptor {
        ConnectorDescriptor {
            kind: "webhook",
            name: "Webhook",
            description: "POST each message as JSON to a URL of your choice.",
            fields: vec![
                Field {
                    key: "url",
                    label: "URL",
                    help: "Where to send the request.",
                    secret: false,
                    required: true,
                    kind: FieldKind::Url,
                },
                Field {
                    key: "secret",
                    label: "Signing secret",
                    help: "When set, each request carries an HMAC-SHA256 signature in X-Ramzes-Signature.",
                    secret: true,
                    required: false,
                    kind: FieldKind::Text,
                },
            ],
            capabilities: Capabilities {
                rich: false,
                update: false,
                threads: false,
                buttons: false,
                max_len: 20_000,
            },
            destination_label: None,
        }
    }

    fn validate(&self, c: &ConnectionConfig) -> Result<(), String> {
        require_http_url("URL", c.setting("url"))
    }

    async fn send(
        &self,
        c: &ConnectionConfig,
        m: &Message,
        _destination: Option<&str>,
        now_ms: i64,
    ) -> SendOutcome {
        let Some(url) = c.setting("url") else {
            return SendOutcome::Permanent {
                status: None,
                detail: "the connection has no URL".into(),
            };
        };
        let body = body(m, now_ms);
        let mut headers = Vec::new();
        if let Some(secret) = c.credential("secret") {
            let timestamp = now_ms.to_string();
            headers.push(("x-ramzes-signature", signature(secret, &timestamp, &body)));
            headers.push(("x-ramzes-timestamp", timestamp));
        }
        self.http.post_json(url, &headers, body).await
    }
}
