use serde::{Deserialize, Serialize};

use crate::event::{Event, EventStatus, Severity};

/// The channel-neutral message a connector renders into its own format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    }
}
