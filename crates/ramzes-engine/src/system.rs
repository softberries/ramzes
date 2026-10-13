//! Events ramzes raises about itself, on topics under `ramzes.`. They are routed like any other.

use std::collections::BTreeMap;

use ramzes_core::{Event, EventStatus, Severity, new_id};

use crate::Engine;

/// Topics under this prefix are ramzes's own.
pub const SYSTEM_TOPIC_PREFIX: &str = "ramzes.";

pub(crate) struct SystemEvent<'a> {
    pub topic: &'a str,
    pub title: String,
    pub body: Option<String>,
    pub severity: Severity,
    pub labels: &'a [(&'a str, &'a str)],
    pub dedup_key: String,
}

impl Engine {
    /// Stores an event from the built-in source. A failure is logged: the thing being reported
    /// has already happened, and reporting it must not make matters worse.
    pub(crate) async fn raise(&self, workspace_id: &str, e: SystemEvent<'_>) {
        let store = &self.config.store;
        let now = self.now();
        let stored = async {
            let source = store.system_source(workspace_id, now).await?;
            let labels: BTreeMap<String, String> = e
                .labels
                .iter()
                .map(|(k, v)| ((*k).to_owned(), v.chars().take(1000).collect()))
                .collect();
            let event = Event {
                id: new_id(),
                workspace_id: workspace_id.to_owned(),
                source_id: source.id,
                topic: e.topic.to_owned(),
                severity: e.severity,
                status: EventStatus::Firing,
                title: e.title.chars().take(500).collect(),
                body: e
                    .body
                    .map(|b| b.replace('\0', "").chars().take(20_000).collect()),
                labels,
                payload: serde_json::Value::Null,
                dedup_key: Some(e.dedup_key.chars().take(200).collect()),
                occurred_at: now,
                received_at: now,
            };
            store.insert_events(&[event], None).await
        };
        match stored.await {
            Ok(_) => self.route_wake.notify_one(),
            Err(error) => tracing::warn!(%error, topic = e.topic, "could not raise a system event"),
        }
    }
}
