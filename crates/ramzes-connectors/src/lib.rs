//! Outbound connectors: how ramzes hands a message to a channel.

mod http;
mod slack;
mod webhook;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use ramzes_core::Message;
use serde::Serialize;

pub use http::classify;

/// What a connector is and which settings it needs. Drives the setup form in the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct ConnectorDescriptor {
    pub kind: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub fields: Vec<Field>,
    pub capabilities: Capabilities,
    /// What a route's destination means for this connector, when it uses one.
    pub destination_label: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub help: &'static str,
    /// Secret fields are stored encrypted and never returned by the API.
    pub secret: bool,
    pub required: bool,
    pub kind: FieldKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum FieldKind {
    Text,
    Url,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct Capabilities {
    pub rich: bool,
    pub update: bool,
    pub threads: bool,
    pub buttons: bool,
    pub max_len: usize,
}

/// A connection's settings and its credentials, already decrypted.
#[derive(Debug, Clone, PartialEq)]
pub struct ConnectionConfig {
    pub settings: serde_json::Value,
    pub credentials: serde_json::Value,
}

impl ConnectionConfig {
    fn setting(&self, key: &str) -> Option<&str> {
        self.settings
            .get(key)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
    }

    fn credential(&self, key: &str) -> Option<&str> {
        self.credentials
            .get(key)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendOutcome {
    Delivered {
        external_ref: Option<String>,
        status: Option<u16>,
    },
    /// Worth trying again, after `after_ms` when the service asked for a delay.
    Retry {
        after_ms: Option<i64>,
        status: Option<u16>,
        detail: String,
    },
    /// The service rejected the message; retrying would not help.
    Permanent { status: Option<u16>, detail: String },
}

#[async_trait]
pub trait Connector: Send + Sync {
    fn descriptor(&self) -> ConnectorDescriptor;

    /// Checks that the configuration is complete, without contacting the service.
    fn validate(&self, c: &ConnectionConfig) -> Result<(), String>;

    async fn send(
        &self,
        c: &ConnectionConfig,
        m: &Message,
        destination: Option<&str>,
        now_ms: i64,
    ) -> SendOutcome;
}

/// The connectors compiled into this binary, by kind.
#[derive(Clone)]
pub struct Registry {
    connectors: BTreeMap<&'static str, Arc<dyn Connector>>,
}

impl Registry {
    /// The built-in connectors. `timeout` bounds each request to a service.
    pub fn builtin(timeout: Duration) -> Registry {
        let http = http::Http::new(timeout);
        let mut registry = Registry {
            connectors: BTreeMap::new(),
        };
        registry.add(Arc::new(slack::Slack::new(http.clone())));
        registry.add(Arc::new(webhook::Webhook::new(http)));
        registry
    }

    pub fn add(&mut self, connector: Arc<dyn Connector>) {
        self.connectors
            .insert(connector.descriptor().kind, connector);
    }

    pub fn get(&self, kind: &str) -> Option<Arc<dyn Connector>> {
        self.connectors.get(kind).cloned()
    }

    /// Descriptors in order of kind.
    pub fn descriptors(&self) -> Vec<ConnectorDescriptor> {
        self.connectors.values().map(|c| c.descriptor()).collect()
    }
}

/// Checks that `value` is an absolute http or https URL.
fn require_http_url(label: &str, value: Option<&str>) -> Result<(), String> {
    let value = value.ok_or_else(|| format!("{label} is required"))?;
    match reqwest::Url::parse(value) {
        Ok(url) if matches!(url.scheme(), "http" | "https") && url.has_host() => Ok(()),
        _ => Err(format!("{label} must be an http or https URL")),
    }
}

/// Shortens `text` to at most `max` characters, ending in an ellipsis when cut.
fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut cut: String = text.chars().take(max.saturating_sub(1)).collect();
    cut.push('…');
    cut
}
