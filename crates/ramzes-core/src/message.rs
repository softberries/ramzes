use serde::{Deserialize, Serialize};

use crate::event::{Event, EventStatus, Severity};

/// The channel-neutral message a connector renders into its own format.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Message {
    pub title: String,
    pub body: Option<String>,
    pub severity: Severity,
    pub status: EventStatus,
    pub topic: String,
    /// The event's labels in key order.
    pub fields: Vec<(String, String)>,
    /// Link to the event in the ramzes UI, when a public URL is configured.
    pub link: Option<String>,
    pub event_ids: Vec<String>,
    /// Identifies what the message is about across firing and resolved messages, for services
    /// that pair them up.
    #[serde(default)]
    pub dedup_key: Option<String>,
    /// A link that opens a page to acknowledge the notification, when a public URL is configured.
    #[serde(default)]
    pub ack_link: Option<String>,
    #[serde(default)]
    pub notification_id: Option<String>,
    /// The service's reference to the first message sent for this notification to the same
    /// place, so follow-ups can be attached to it.
    #[serde(default)]
    pub thread_ref: Option<String>,
}

pub fn build_message(e: &Event, public_url: Option<&str>) -> Message {
    Message {
        title: e.title.clone(),
        body: e.body.clone(),
        severity: e.severity,
        status: e.status,
        topic: e.topic.clone(),
        fields: e
            .labels
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        link: public_url.map(|base| format!("{}/events/{}", base.trim_end_matches('/'), e.id)),
        event_ids: vec![e.id.clone()],
        ..Message::default()
    }
}

/// User-written wording for a message. An empty part keeps the default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Template {
    pub id: String,
    pub name: String,
    /// Template for the title.
    pub title: String,
    /// Template for the body.
    pub body: String,
}

impl Template {
    pub fn validate(&self) -> Result<(), String> {
        crate::expr::check_template(&self.title).map_err(|e| format!("title: {e}"))?;
        crate::expr::check_template(&self.body).map_err(|e| format!("body: {e}"))
    }
}

/// Rewords `message` with the template. The template sees the first event's fields, plus
/// `events` (all of them), `count` and `message` (the default wording).
pub fn apply_template(
    t: &Template,
    message: &Message,
    events: &[Event],
) -> Result<Message, String> {
    let mut ctx = match events.first() {
        Some(first) => crate::expr::event_context(first),
        None => serde_json::json!({}),
    };
    if let Some(fields) = ctx.as_object_mut() {
        let all: Vec<serde_json::Value> = events.iter().map(crate::expr::event_context).collect();
        fields.insert("events".into(), all.into());
        fields.insert("count".into(), events.len().into());
        fields.insert(
            "message".into(),
            serde_json::json!({"title": message.title, "body": message.body, "link": message.link}),
        );
    }
    let mut out = message.clone();
    if !t.title.trim().is_empty() {
        let title = crate::expr::render(&t.title, &ctx).map_err(|e| format!("title: {e}"))?;
        if title.trim().is_empty() {
            return Err("title: the template produced an empty title".into());
        }
        out.title = title.trim().to_owned();
    }
    if !t.body.trim().is_empty() {
        let body = crate::expr::render(&t.body, &ctx).map_err(|e| format!("body: {e}"))?;
        out.body = Some(body.trim().to_owned()).filter(|b| !b.is_empty());
    }
    Ok(out)
}
