//! Outbound connectors: how ramzes hands a message to a channel.

mod declarative;
mod email;
mod http;
mod slack;
mod webhook;

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use async_trait::async_trait;
use ramzes_core::Message;
use serde::{Deserialize, Serialize};

pub use declarative::{BodyFormat, Definition, RequestDef};
pub use http::{HttpOptions, classify};

/// What a connector is and which settings it needs. Drives the setup form in the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct ConnectorDescriptor {
    pub kind: String,
    pub name: String,
    pub description: String,
    pub fields: Vec<Field>,
    pub capabilities: Capabilities,
    /// What a route's destination means for this connector, when it uses one.
    pub destination_label: Option<String>,
    /// Added by a user as a definition, rather than shipped with ramzes.
    pub custom: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub help: String,
    /// Secret fields are stored encrypted and never returned by the API.
    #[serde(default)]
    pub secret: bool,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub kind: FieldKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum FieldKind {
    #[default]
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

/// The request a connector would send for a message, with secrets masked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct Preview {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

const MASK: &str = "••••";

/// Replaces every credential value that appears in the preview.
pub(crate) fn mask_secrets(mut preview: Preview, credentials: &serde_json::Value) -> Preview {
    let mut secrets: Vec<&str> = credentials
        .as_object()
        .into_iter()
        .flat_map(|fields| fields.values())
        .filter_map(|value| value.as_str())
        .filter(|secret| !secret.is_empty())
        .collect();
    // Longest first, so a secret that contains another is replaced whole.
    secrets.sort_by_key(|secret| std::cmp::Reverse(secret.len()));
    for secret in secrets {
        preview.url = preview.url.replace(secret, MASK);
        preview.body = preview.body.replace(secret, MASK);
        for (_, value) in &mut preview.headers {
            *value = value.replace(secret, MASK);
        }
    }
    preview
}

/// A connection's settings and its credentials, already decrypted.
#[derive(Debug, Clone, PartialEq)]
pub struct ConnectionConfig {
    pub settings: serde_json::Value,
    pub credentials: serde_json::Value,
}

impl ConnectionConfig {
    pub(crate) fn setting(&self, key: &str) -> Option<&str> {
        self.settings
            .get(key)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
    }

    pub(crate) fn credential(&self, key: &str) -> Option<&str> {
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

    /// The request `send` would make, without making it.
    fn preview(
        &self,
        c: &ConnectionConfig,
        m: &Message,
        destination: Option<&str>,
        now_ms: i64,
    ) -> Result<Preview, String>;

    async fn send(
        &self,
        c: &ConnectionConfig,
        m: &Message,
        destination: Option<&str>,
        now_ms: i64,
    ) -> SendOutcome;
}

/// The connectors of this instance, by kind: those shipped with ramzes and those users defined.
#[derive(Clone)]
pub struct Registry {
    builtin: BTreeMap<String, Arc<dyn Connector>>,
    /// Shared between clones, so a change made through the API reaches the workers.
    custom: Arc<RwLock<BTreeMap<String, Arc<dyn Connector>>>>,
    http: http::Http,
}

/// Definitions compiled into the binary.
const BUNDLED: &[&str] = &[];

impl Registry {
    /// The built-in connectors. `timeout` bounds each request to a service.
    pub fn builtin(timeout: Duration) -> Registry {
        Registry::with_options(HttpOptions {
            timeout,
            block_private: false,
        })
    }

    pub fn with_options(options: HttpOptions) -> Registry {
        let http = http::Http::new(options);
        let mut registry = Registry {
            builtin: BTreeMap::new(),
            custom: Arc::default(),
            http: http.clone(),
        };
        registry.add(Arc::new(slack::Slack::new(http.clone())));
        registry.add(Arc::new(webhook::Webhook::new(http.clone())));
        registry.add(Arc::new(email::Email::new(options.timeout)));
        for text in BUNDLED {
            let definition =
                Definition::parse_any(text).expect("bundled connector definitions are valid");
            registry.add(Arc::new(declarative::Declarative::new(
                definition,
                http.clone(),
                false,
            )));
        }
        registry
    }

    pub fn add(&mut self, connector: Arc<dyn Connector>) {
        self.builtin.insert(connector.descriptor().kind, connector);
    }

    /// Replaces the connectors defined by users.
    pub fn set_custom(&self, definitions: Vec<Definition>) {
        let connectors = definitions
            .into_iter()
            .filter(|d| !self.builtin.contains_key(&d.kind))
            .map(|d| {
                let kind = d.kind.clone();
                let connector: Arc<dyn Connector> =
                    Arc::new(declarative::Declarative::new(d, self.http.clone(), true));
                (kind, connector)
            })
            .collect();
        *self
            .custom
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = connectors;
    }

    pub fn get(&self, kind: &str) -> Option<Arc<dyn Connector>> {
        if let Some(connector) = self.builtin.get(kind) {
            return Some(connector.clone());
        }
        self.custom
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(kind)
            .cloned()
    }

    /// Descriptors in order of kind, shipped connectors first.
    pub fn descriptors(&self) -> Vec<ConnectorDescriptor> {
        let custom = self
            .custom
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.builtin
            .values()
            .chain(custom.values())
            .map(|c| c.descriptor())
            .collect()
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
