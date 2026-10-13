//! Connectors described by a YAML definition instead of code.

use std::collections::BTreeMap;

use async_trait::async_trait;
use ramzes_core::{Message, expr};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::http::{Http, Outgoing};
use crate::{
    Capabilities, ConnectionConfig, Connector, ConnectorDescriptor, Field, FieldKind, Preview,
    SendOutcome, mask_secrets, require_http_url,
};

/// Kinds that are compiled in; a definition added by a user may not take one of these.
const RESERVED: [&str; 11] = [
    "slack",
    "slack_app",
    "webhook",
    "email",
    "discord",
    "teams",
    "ntfy",
    "telegram",
    "pushover",
    "pagerduty",
    "opsgenie",
];
const METHODS: [&str; 4] = ["POST", "PUT", "PATCH", "GET"];

fn default_method() -> String {
    "POST".into()
}

fn default_retry_header() -> String {
    "retry-after".into()
}

fn default_max_len() -> usize {
    4000
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BodyFormat {
    /// The body template must produce JSON; it is checked before sending.
    #[default]
    Json,
    /// The body is sent as rendered.
    Text,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestDef {
    #[serde(default = "default_method")]
    pub method: String,
    /// Template for the address.
    pub url: String,
    /// Header name to value template.
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// Template for the body.
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub format: BodyFormat,
}

impl RequestDef {
    /// Checks the request's templates; `section` names it in the problem.
    fn check(&self, section: &str) -> Result<(), String> {
        if !METHODS.contains(&self.method.as_str()) {
            return Err(format!(
                "{section}.method: must be one of {}",
                METHODS.join(", ")
            ));
        }
        if self.url.trim().is_empty() {
            return Err(format!("{section}.url: must not be empty"));
        }
        expr::check_template(&self.url).map_err(|e| format!("{section}.url: {e}"))?;
        for (name, value) in &self.headers {
            if reqwest::header::HeaderName::from_bytes(name.as_bytes()).is_err() {
                return Err(format!("{section}.headers.{name}: not a valid header name"));
            }
            expr::check_template(value).map_err(|e| format!("{section}.headers.{name}: {e}"))?;
        }
        expr::check_template(&self.body).map_err(|e| format!("{section}.body: {e}"))
    }
}

/// A connector as data: its settings and the HTTP request that delivers a message.
///
/// Templates see `message` (`title`, `body`, `severity`, `status`, `topic`, `labels`, `fields`,
/// `link`, `ack_link`, `dedup_key`, `event_ids`, `count`), `settings`, `credentials`, `destination` and `now_ms`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub kind: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// What a route's destination means for this connector, when it uses one.
    #[serde(default)]
    pub destination_label: Option<String>,
    #[serde(default)]
    pub fields: Vec<Field>,
    pub request: RequestDef,
    /// The request that tells the service a notification was acknowledged in ramzes, for
    /// services that track that themselves. It sees what `request` sees.
    #[serde(default)]
    pub acknowledge: Option<RequestDef>,
    /// Statuses that mean delivered. Empty means any 2xx.
    #[serde(default)]
    pub success: Vec<u16>,
    /// The response header holding the seconds to wait before a retry.
    #[serde(default = "default_retry_header")]
    pub retry_after_header: String,
    /// Longest message body the service accepts, for the UI.
    #[serde(default = "default_max_len")]
    pub max_len: usize,
}

fn valid_kind(kind: &str) -> bool {
    let mut chars = kind.chars();
    let starts_well = chars.next().is_some_and(|c| c.is_ascii_lowercase());
    starts_well
        && (2..=40).contains(&kind.len())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

impl Definition {
    /// Reads and checks a definition written by a user.
    pub fn parse(text: &str) -> Result<Definition, String> {
        let definition = Definition::parse_any(text)?;
        if RESERVED.contains(&definition.kind.as_str()) {
            return Err(format!(
                "kind: {:?} is the name of a connector that ships with ramzes; choose another",
                definition.kind
            ));
        }
        Ok(definition)
    }

    /// Reads and checks a definition, allowing the kinds that ship with ramzes.
    pub(crate) fn parse_any(text: &str) -> Result<Definition, String> {
        let definition: Definition =
            serde_norway::from_str(text).map_err(|e| format!("YAML: {e}"))?;
        definition.check()?;
        Ok(definition)
    }

    fn check(&self) -> Result<(), String> {
        if !valid_kind(&self.kind) {
            return Err(
                "kind: use 2 to 40 lowercase letters, digits, - or _, starting with a letter"
                    .into(),
            );
        }
        if self.name.trim().is_empty() || self.name.chars().count() > 100 {
            return Err("name: must be 1 to 100 characters".into());
        }
        let mut seen = Vec::new();
        for field in &self.fields {
            if field.key.is_empty() || field.label.is_empty() {
                return Err("fields: every field needs a key and a label".into());
            }
            if seen.contains(&&field.key) {
                return Err(format!("fields: the key {:?} is used twice", field.key));
            }
            seen.push(&field.key);
        }
        self.request.check("request")?;
        if let Some(acknowledge) = &self.acknowledge {
            acknowledge.check("acknowledge")?;
        }
        if reqwest::header::HeaderName::from_bytes(self.retry_after_header.as_bytes()).is_err() {
            return Err("retry_after_header: not a valid header name".into());
        }
        Ok(())
    }
}

/// What templates see of a message.
pub(crate) fn message_context(m: &Message) -> Value {
    let labels: BTreeMap<&str, &str> = m
        .fields
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let fields: Vec<Value> = m
        .fields
        .iter()
        .map(|(k, v)| json!({"key": k, "value": v}))
        .collect();
    json!({
        "title": m.title,
        "body": m.body,
        "severity": m.severity,
        "status": m.status,
        "topic": m.topic,
        "labels": labels,
        "fields": fields,
        "link": m.link,
        "event_ids": m.event_ids,
        "count": m.event_ids.len(),
        "dedup_key": m.dedup_key,
        "ack_link": m.ack_link,
        "notification_id": m.notification_id,
    })
}

struct Rendered {
    url: String,
    headers: Vec<(String, String)>,
    body: String,
}

pub(crate) struct Declarative {
    definition: Definition,
    http: Http,
    custom: bool,
}

impl Declarative {
    pub fn new(definition: Definition, http: Http, custom: bool) -> Declarative {
        Declarative {
            definition,
            http,
            custom,
        }
    }

    fn render(
        &self,
        request: &RequestDef,
        c: &ConnectionConfig,
        m: &Message,
        destination: Option<&str>,
        now_ms: i64,
    ) -> Result<Rendered, String> {
        let mut ctx = json!({
            "message": message_context(m),
            "settings": c.settings,
            "credentials": c.credentials,
            "now_ms": now_ms,
        });
        // An absent value is left out rather than null, so `| default(...)` applies to it.
        if let Some(message) = ctx["message"].as_object_mut() {
            message.retain(|_, value| !value.is_null());
        }
        if let Some(destination) = destination.filter(|d| !d.trim().is_empty()) {
            ctx["destination"] = destination.into();
        }
        let url = expr::render(&request.url, &ctx)
            .map_err(|e| format!("the connector's address could not be built: {e}"))?
            .trim()
            .to_owned();
        let mut headers = Vec::new();
        let mut has_content_type = false;
        for (name, template) in &request.headers {
            let value = expr::render(template, &ctx)
                .map_err(|e| format!("the connector's header {name} could not be built: {e}"))?;
            // A header value cannot span lines; this also stops text from adding headers.
            if value.contains(['\r', '\n']) {
                return Err(format!(
                    "the connector's header {name} would contain a line break"
                ));
            }
            // A header whose template renders to nothing is left out, so a header can be optional.
            if value.trim().is_empty() {
                continue;
            }
            has_content_type |= name.eq_ignore_ascii_case("content-type");
            headers.push((name.clone(), value.trim().to_owned()));
        }
        let mut body = expr::render(&request.body, &ctx)
            .map_err(|e| format!("the connector's body could not be built: {e}"))?;
        if request.format == BodyFormat::Json && !request.body.trim().is_empty() {
            let parsed: Value = serde_json::from_str(&body)
                .map_err(|e| format!("the connector's body is not valid JSON: {e}"))?;
            body = parsed.to_string();
        }
        if !has_content_type && !request.body.trim().is_empty() {
            let content_type = match request.format {
                BodyFormat::Json => "application/json",
                BodyFormat::Text => "text/plain; charset=utf-8",
            };
            headers.insert(0, ("Content-Type".to_owned(), content_type.to_owned()));
        }
        Ok(Rendered { url, headers, body })
    }
}

#[async_trait]
impl Connector for Declarative {
    fn descriptor(&self) -> ConnectorDescriptor {
        let d = &self.definition;
        ConnectorDescriptor {
            kind: d.kind.clone(),
            name: d.name.clone(),
            description: d.description.clone(),
            fields: d.fields.clone(),
            capabilities: Capabilities {
                rich: false,
                update: false,
                threads: false,
                buttons: false,
                max_len: d.max_len,
            },
            destination_label: d.destination_label.clone(),
            custom: self.custom,
        }
    }

    fn validate(&self, c: &ConnectionConfig) -> Result<(), String> {
        for field in &self.definition.fields {
            let value = if field.secret {
                c.credential(&field.key)
            } else {
                c.setting(&field.key)
            };
            match (value, field.required) {
                (None, true) => return Err(format!("{} is required", field.label)),
                (Some(_), _) if field.kind == FieldKind::Url => {
                    require_http_url(&field.label, value)?
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn preview(
        &self,
        c: &ConnectionConfig,
        m: &Message,
        destination: Option<&str>,
        now_ms: i64,
    ) -> Result<Preview, String> {
        let rendered = self.render(&self.definition.request, c, m, destination, now_ms)?;
        let body = match serde_json::from_str::<Value>(&rendered.body) {
            Ok(parsed) if self.definition.request.format == BodyFormat::Json => {
                serde_json::to_string_pretty(&parsed).unwrap_or(rendered.body)
            }
            _ => rendered.body,
        };
        Ok(mask_secrets(
            Preview {
                method: self.definition.request.method.clone(),
                url: rendered.url,
                headers: rendered.headers,
                body,
            },
            &c.credentials,
        ))
    }

    async fn send(
        &self,
        c: &ConnectionConfig,
        m: &Message,
        destination: Option<&str>,
        now_ms: i64,
    ) -> SendOutcome {
        let rendered = match self.render(&self.definition.request, c, m, destination, now_ms) {
            Ok(rendered) => rendered,
            Err(detail) => {
                return SendOutcome::Permanent {
                    status: None,
                    detail,
                };
            }
        };
        self.http
            .send(Outgoing {
                method: &self.definition.request.method,
                url: &rendered.url,
                headers: &rendered.headers,
                body: rendered.body.into_bytes(),
                success: &self.definition.success,
                retry_after_header: &self.definition.retry_after_header,
            })
            .await
    }

    async fn acknowledge(
        &self,
        c: &ConnectionConfig,
        m: &Message,
        destination: Option<&str>,
        now_ms: i64,
    ) -> Result<(), String> {
        let Some(request) = &self.definition.acknowledge else {
            return Ok(());
        };
        let rendered = self.render(request, c, m, destination, now_ms)?;
        let outcome = self
            .http
            .send(Outgoing {
                method: &request.method,
                url: &rendered.url,
                headers: &rendered.headers,
                body: rendered.body.into_bytes(),
                success: &self.definition.success,
                retry_after_header: &self.definition.retry_after_header,
            })
            .await;
        match outcome {
            SendOutcome::Delivered { .. } => Ok(()),
            SendOutcome::Retry { detail, .. } | SendOutcome::Permanent { detail, .. } => {
                Err(detail)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::http::HttpOptions;

    /// The request a bundled connector makes to pass an acknowledgement on.
    fn acknowledgement(file: &str, settings: Value, credentials: Value) -> Rendered {
        let definition = Definition::parse_any(file).unwrap();
        let request = definition.acknowledge.clone().unwrap();
        let http = Http::new(HttpOptions {
            timeout: Duration::from_secs(1),
            block_private: false,
        });
        let m = Message {
            title: "Database down".into(),
            dedup_key: Some("acme/shop CI".into()),
            ..Message::default()
        };
        let c = ConnectionConfig {
            settings,
            credentials,
        };
        Declarative::new(definition, http, false)
            .render(&request, &c, &m, None, 1)
            .unwrap()
    }

    #[test]
    fn pagerduty_is_told_of_an_acknowledgement() {
        let sent = acknowledgement(
            include_str!("definitions/pagerduty.yaml"),
            json!({"region": "eu"}),
            json!({"routing_key": "R0UT1NG"}),
        );
        assert_eq!(sent.url, "https://events.eu.pagerduty.com/v2/enqueue");
        assert_eq!(
            serde_json::from_str::<Value>(&sent.body).unwrap(),
            json!({"routing_key": "R0UT1NG", "event_action": "acknowledge", "dedup_key": "acme/shop CI"})
        );
    }

    #[test]
    fn opsgenie_is_told_of_an_acknowledgement() {
        let sent = acknowledgement(
            include_str!("definitions/opsgenie.yaml"),
            json!({}),
            json!({"api_key": "k3y"}),
        );
        assert_eq!(
            sent.url,
            "https://api.opsgenie.com/v2/alerts/acme%2Fshop%20CI/acknowledge?identifierType=alias"
        );
        assert!(
            sent.headers
                .contains(&("Authorization".to_owned(), "GenieKey k3y".to_owned()))
        );
        assert_eq!(
            serde_json::from_str::<Value>(&sent.body).unwrap(),
            json!({"source": "ramzes", "note": "Acknowledged in ramzes"})
        );
    }
}
