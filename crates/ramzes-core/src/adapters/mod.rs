//! The kinds of source ramzes understands: its own API, bundled third parties, and any JSON.

use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

use crate::mapping::{FieldMap, Mapping, Verify};

/// A kind of source, for the UI to offer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct SourceKind {
    pub kind: String,
    pub name: String,
    pub description: String,
    /// How to configure the sender. `{url}` and `{key}` stand for the source's address and key.
    pub setup: String,
    /// The sender generates the secret, so the user has to give it to ramzes.
    pub own_secret: bool,
    /// How requests prove where they come from. `None` for kinds whose mapping decides.
    pub verify: Option<Verify>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Bundled {
    kind: String,
    name: String,
    description: String,
    setup: String,
    #[serde(default)]
    own_secret: bool,
    mapping: Mapping,
}

/// The kind that posts ramzes's own event format to `/v1/events`.
pub const NATIVE: &str = "native";
/// The kind whose mapping the user writes.
pub const JSON: &str = "json";

static BUNDLED: LazyLock<Vec<Bundled>> = LazyLock::new(|| {
    [
        include_str!("alertmanager.yaml"),
        include_str!("grafana.yaml"),
        include_str!("github.yaml"),
        include_str!("gitlab.yaml"),
        include_str!("sentry.yaml"),
    ]
    .iter()
    .map(|text| {
        let bundled: Bundled =
            serde_norway::from_str(text).expect("bundled adapters are valid YAML");
        bundled
            .mapping
            .validate()
            .expect("bundled adapters have valid expressions");
        bundled
    })
    .collect()
});

/// Every kind a source can have, the native one first.
pub fn source_kinds() -> Vec<SourceKind> {
    let mut kinds = vec![SourceKind {
        kind: NATIVE.into(),
        name: "ramzes API".into(),
        description: "Your own services, posting events in ramzes's format.".into(),
        setup: "curl -X POST {origin}/v1/events \\\n  -H 'Authorization: Bearer {key}' \\\n  -H 'Content-Type: application/json' \\\n  -d '{\"topic\": \"orders.failed\", \"title\": \"Order 17 failed\", \"severity\": \"error\"}'".into(),
        own_secret: false,
        verify: Some(Verify::Bearer),
    }];
    kinds.extend(BUNDLED.iter().map(|b| SourceKind {
        kind: b.kind.clone(),
        name: b.name.clone(),
        description: b.description.clone(),
        setup: b.setup.clone(),
        own_secret: b.own_secret,
        verify: Some(b.mapping.verify.clone()),
    }));
    kinds.push(SourceKind {
        kind: JSON.into(),
        name: "Any JSON".into(),
        description: "Any service that can POST JSON. You write the mapping from its payload to an event.".into(),
        setup: "Point the service at {url}. By default it must send the header\nAuthorization: Bearer {key}\nThe mapping can instead accept the key as a URL parameter or in another header.".into(),
        own_secret: false,
        verify: None,
    });
    kinds
}

/// The mapping a new any-JSON source starts with.
pub fn starter_mapping() -> Mapping {
    Mapping {
        verify: Verify::Bearer,
        events: None,
        when: None,
        fields: FieldMap {
            topic: "'webhook.received'".into(),
            title: "payload.title or payload.message or 'Webhook received'".into(),
            severity: Some("payload.severity or payload.level".into()),
            status: None,
            body: Some("payload.description or payload.text".into()),
            labels: None,
            dedup_key: None,
            ts: None,
        },
    }
}

impl Mapping {
    /// Reads a mapping written as YAML and checks its expressions.
    pub fn parse(text: &str) -> Result<Mapping, String> {
        let mapping: Mapping = serde_norway::from_str(text).map_err(|e| format!("YAML: {e}"))?;
        mapping.validate()?;
        Ok(mapping)
    }

    pub fn to_yaml(&self) -> String {
        serde_norway::to_string(self).unwrap_or_default()
    }
}

/// The mapping for a source of `kind`. `stored` is the source's own mapping, used by any-JSON
/// sources. `Ok(None)` for the native kind, which has no mapping.
pub fn mapping_for(kind: &str, stored: Option<&str>) -> Result<Option<Mapping>, String> {
    if kind == NATIVE {
        return Ok(None);
    }
    if let Some(bundled) = BUNDLED.iter().find(|b| b.kind == kind) {
        return Ok(Some(bundled.mapping.clone()));
    }
    if kind != JSON {
        return Err(format!(
            "this version of ramzes does not know sources of kind {kind:?}"
        ));
    }
    match stored {
        Some(text) => Mapping::parse(text).map(Some),
        None => Ok(Some(starter_mapping())),
    }
}
