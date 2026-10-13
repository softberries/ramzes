//! Turning a third party's payload into events, as described by a mapping.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::event::{
    BODY_MAX, DEDUP_KEY_MAX, EventStatus, LABEL_KEY_MAX, LABEL_VALUE_MAX, LABELS_MAX, NewEvent,
    Severity, TITLE_MAX, TOPIC_MAX, is_label_key_char, is_topic_char,
};
use crate::expr;

/// Most events one request may produce.
const EVENTS_MAX: usize = 1000;

/// How a request to a source's URL proves it comes from the sender.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
pub enum Verify {
    /// `Authorization: Bearer <source key>`.
    #[default]
    Bearer,
    /// The named header carries the source key.
    Header { header: String },
    /// The named header carries the hex HMAC-SHA256 of the body, keyed with the source key.
    HmacSha256 {
        header: String,
        /// Text before the hex digest, for example `sha256=`.
        #[serde(default)]
        prefix: String,
    },
    /// The source key is a query parameter of the URL.
    Query { param: String },
    /// Signed the Standard Webhooks way: `webhook-signature` carries `v1,` and the base64
    /// HMAC-SHA256 of `{webhook-id}.{webhook-timestamp}.{body}`, keyed with the source key
    /// (`whsec_` and the key bytes in base64).
    StandardWebhooks {
        /// A header that may carry the source key as it is, for senders not signing yet.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fallback_header: Option<String>,
    },
}

/// One expression per event field. `topic` and `title` are required.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct FieldMap {
    pub topic: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// An expression that gives an object.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedup_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ts: Option<String>,
}

/// How a source's requests become events.
///
/// Expressions see `payload` (the request body), `headers` (lower-case names) and `item`
/// (the current element of `events`, or the payload when there is no list).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Mapping {
    #[serde(default)]
    pub verify: Verify,
    /// An expression that gives the list to make events from. Without it the request is one event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub events: Option<String>,
    /// An expression; an item for which it is false produces no event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
    pub fields: FieldMap,
}

fn is_header_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn clamp(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// A value as text: strings as they are, everything else as JSON. `null` is nothing.
fn text_of(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

fn severity_of(text: &str) -> Severity {
    match text.trim().to_ascii_lowercase().as_str() {
        "debug" | "trace" => Severity::Debug,
        "warning" | "warn" => Severity::Warning,
        "error" | "err" | "high" => Severity::Error,
        "critical" | "crit" | "fatal" | "emergency" | "page" => Severity::Critical,
        _ => Severity::Info,
    }
}

fn status_of(text: &str) -> EventStatus {
    match text.trim().to_ascii_lowercase().as_str() {
        "resolved" | "ok" | "closed" | "recovered" | "normal" | "success" => EventStatus::Resolved,
        _ => EventStatus::Firing,
    }
}

impl Mapping {
    /// Checks every expression without evaluating it.
    pub fn validate(&self) -> Result<(), String> {
        match &self.verify {
            Verify::Bearer => {}
            Verify::Header { header } | Verify::HmacSha256 { header, .. } => {
                if !is_header_name(header) {
                    return Err("verify.header: not a valid header name".into());
                }
            }
            Verify::Query { param } => {
                if !is_header_name(param) {
                    return Err("verify.param: use letters, digits, - and _".into());
                }
            }
            Verify::StandardWebhooks { fallback_header } => {
                if fallback_header.as_ref().is_some_and(|h| !is_header_name(h)) {
                    return Err("verify.fallback_header: not a valid header name".into());
                }
            }
        }
        let f = &self.fields;
        let optional = [
            ("events", &self.events),
            ("when", &self.when),
            ("fields.severity", &f.severity),
            ("fields.status", &f.status),
            ("fields.body", &f.body),
            ("fields.labels", &f.labels),
            ("fields.dedup_key", &f.dedup_key),
            ("fields.ts", &f.ts),
        ];
        expr::check_expression(&f.topic).map_err(|e| format!("fields.topic: {e}"))?;
        expr::check_expression(&f.title).map_err(|e| format!("fields.title: {e}"))?;
        for (name, source) in optional {
            if let Some(source) = source {
                expr::check_expression(source).map_err(|e| format!("{name}: {e}"))?;
            }
        }
        Ok(())
    }

    /// The events a request produces. Values that are too long or oddly spelled are cleaned,
    /// so a sender's quirks do not lose the event.
    pub fn apply(
        &self,
        payload: &Value,
        headers: &BTreeMap<String, String>,
    ) -> Result<Vec<NewEvent>, String> {
        // The payload is converted once and shared by every expression of every item.
        let base = expr::Scope::new(&json!({"payload": payload, "headers": headers}));
        let items: Vec<Value> = match &self.events {
            None => vec![payload.clone()],
            Some(source) => match base.eval(source).map_err(|e| format!("events: {e}"))? {
                Value::Array(items) => items,
                Value::Null => Vec::new(),
                _ => return Err("events: the expression must give a list".into()),
            },
        };
        if items.len() > EVENTS_MAX {
            return Err(format!(
                "events: a request may carry at most {EVENTS_MAX} events"
            ));
        }
        let mut events = Vec::with_capacity(items.len());
        for item in items {
            let ctx = base.with("item", &item);
            if let Some(when) = &self.when
                && !ctx.eval_bool(when).map_err(|e| format!("when: {e}"))?
            {
                continue;
            }
            events.push(self.event(&ctx, item)?);
        }
        Ok(events)
    }

    fn event(&self, ctx: &expr::Scope, item: Value) -> Result<NewEvent, String> {
        let f = &self.fields;
        let text = |name: &str, source: &str| -> Result<Option<String>, String> {
            let value = ctx.eval(source).map_err(|e| format!("{name}: {e}"))?;
            Ok(text_of(&value)
                .map(|t| t.trim().to_owned())
                .filter(|t| !t.is_empty()))
        };
        let optional = |name: &str, source: &Option<String>| -> Result<Option<String>, String> {
            match source {
                Some(source) => text(name, source),
                None => Ok(None),
            }
        };
        let topic: String = text("topic", &f.topic)?
            .ok_or("topic: the expression gave nothing")?
            .chars()
            .map(|c| if is_topic_char(c) { c } else { '_' })
            .take(TOPIC_MAX)
            .collect();
        let title = clamp(
            &text("title", &f.title)?.ok_or("title: the expression gave nothing")?,
            TITLE_MAX,
        );

        let mut labels = BTreeMap::new();
        if let Some(source) = &f.labels {
            match ctx.eval(source).map_err(|e| format!("labels: {e}"))? {
                Value::Object(found) => {
                    for (key, value) in found {
                        let key: String = key
                            .trim()
                            .chars()
                            .map(|c| if is_label_key_char(c) { c } else { '_' })
                            .take(LABEL_KEY_MAX)
                            .collect();
                        let Some(value) = text_of(&value) else {
                            continue;
                        };
                        if key.is_empty() || labels.len() >= LABELS_MAX {
                            continue;
                        }
                        labels.insert(key, clamp(&value.replace('\0', ""), LABEL_VALUE_MAX));
                    }
                }
                Value::Null => {}
                _ => return Err("labels: the expression must give an object".into()),
            }
        }
        let clean =
            |value: Option<String>, max: usize| value.map(|v| clamp(&v.replace('\0', ""), max));
        Ok(NewEvent {
            topic,
            title: title.replace('\0', ""),
            severity: optional("severity", &f.severity)?
                .map_or(Severity::Info, |s| severity_of(&s)),
            status: optional("status", &f.status)?.map_or(EventStatus::Firing, |s| status_of(&s)),
            body: clean(optional("body", &f.body)?, BODY_MAX),
            labels,
            payload: item,
            dedup_key: clean(optional("dedup_key", &f.dedup_key)?, DEDUP_KEY_MAX),
            // A time the sender wrote in another format is dropped; the event is stamped on receipt.
            ts: optional("ts", &f.ts)?.filter(|ts| OffsetDateTime::parse(ts, &Rfc3339).is_ok()),
        })
    }
}
