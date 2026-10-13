use async_trait::async_trait;
use ramzes_core::{EventStatus, Message, Severity};
use serde_json::{Value, json};

use crate::http::Http;
use crate::{
    Capabilities, ConnectionConfig, Connector, ConnectorDescriptor, Field, FieldKind, SendOutcome,
    require_http_url, truncate,
};

// Block Kit limits.
const HEADER_MAX: usize = 150;
const SECTION_MAX: usize = 3000;
const FIELD_MAX: usize = 2000;
const FIELDS_MAX: usize = 10;

/// Sends to a Slack channel through an incoming webhook.
pub(crate) struct Slack {
    http: Http,
}

impl Slack {
    pub fn new(http: Http) -> Slack {
        Slack { http }
    }
}

fn marker(m: &Message) -> &'static str {
    match (m.status, m.severity) {
        (EventStatus::Resolved, _) => "✅",
        (_, Severity::Critical | Severity::Error) => "🔴",
        (_, Severity::Warning) => "🟠",
        (_, Severity::Info) => "🔵",
        (_, Severity::Debug) => "⚪",
    }
}

/// Escapes the three characters Slack's mrkdwn treats as control characters.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn payload(m: &Message) -> Value {
    let heading = format!("{} {}", marker(m), m.title);
    let mut blocks = vec![json!({
        "type": "header",
        "text": {"type": "plain_text", "text": truncate(&heading, HEADER_MAX), "emoji": true},
    })];
    if let Some(body) = &m.body {
        blocks.push(json!({
            "type": "section",
            "text": {"type": "plain_text", "text": truncate(body, SECTION_MAX), "emoji": true},
        }));
    }
    if !m.fields.is_empty() {
        let fields: Vec<Value> = m
            .fields
            .iter()
            .take(FIELDS_MAX)
            .map(|(key, value)| {
                let text = format!("*{}*\n{}", escape(key), escape(value));
                json!({"type": "mrkdwn", "text": truncate(&text, FIELD_MAX)})
            })
            .collect();
        blocks.push(json!({"type": "section", "fields": fields}));
    }
    let severity = match m.severity {
        Severity::Debug => "debug",
        Severity::Info => "info",
        Severity::Warning => "warning",
        Severity::Error => "error",
        Severity::Critical => "critical",
    };
    let mut context = format!(
        "`{}` · {} · {severity}",
        escape(&m.topic),
        m.status.as_str()
    );
    if let Some(link) = &m.link {
        context.push_str(&format!(" · <{}|Open in ramzes>", escape(link)));
    }
    blocks.push(json!({"type": "context", "elements": [{"type": "mrkdwn", "text": truncate(&context, SECTION_MAX)}]}));
    // The fallback text is parsed as mrkdwn, so it is escaped; the header block is plain text.
    json!({"text": truncate(&escape(&heading), SECTION_MAX), "blocks": blocks})
}

#[async_trait]
impl Connector for Slack {
    fn descriptor(&self) -> ConnectorDescriptor {
        ConnectorDescriptor {
            kind: "slack",
            name: "Slack",
            description: "Post to a Slack channel through an incoming webhook.",
            fields: vec![Field {
                key: "webhook_url",
                label: "Incoming webhook URL",
                help: "Create one in Slack under Apps → Incoming Webhooks. It decides which channel receives the messages.",
                secret: true,
                required: true,
                kind: FieldKind::Url,
            }],
            capabilities: Capabilities {
                rich: true,
                update: false,
                threads: false,
                buttons: false,
                max_len: SECTION_MAX,
            },
            destination_label: None,
        }
    }

    fn validate(&self, c: &ConnectionConfig) -> Result<(), String> {
        require_http_url("Incoming webhook URL", c.credential("webhook_url"))
    }

    async fn send(
        &self,
        c: &ConnectionConfig,
        m: &Message,
        _destination: Option<&str>,
        _now_ms: i64,
    ) -> SendOutcome {
        let Some(url) = c.credential("webhook_url") else {
            return SendOutcome::Permanent {
                status: None,
                detail: "the connection has no webhook URL".into(),
            };
        };
        self.http
            .post_json(url, &[], payload(m).to_string().into_bytes())
            .await
    }
}
