use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

pub(crate) const TOPIC_MAX: usize = 200;
pub(crate) const TITLE_MAX: usize = 500;
pub(crate) const BODY_MAX: usize = 20_000;
pub(crate) const LABELS_MAX: usize = 64;
pub(crate) const LABEL_KEY_MAX: usize = 100;
pub(crate) const LABEL_VALUE_MAX: usize = 1000;
pub(crate) const DEDUP_KEY_MAX: usize = 200;

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
    Serialize,
    Deserialize,
    utoipa::ToSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Debug,
    #[default]
    Info,
    Warning,
    Error,
    Critical,
}

impl Severity {
    pub fn as_i64(self) -> i64 {
        match self {
            Severity::Debug => 0,
            Severity::Info => 1,
            Severity::Warning => 2,
            Severity::Error => 3,
            Severity::Critical => 4,
        }
    }

    pub fn from_i64(v: i64) -> Option<Severity> {
        match v {
            0 => Some(Severity::Debug),
            1 => Some(Severity::Info),
            2 => Some(Severity::Warning),
            3 => Some(Severity::Error),
            4 => Some(Severity::Critical),
            _ => None,
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, utoipa::ToSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum EventStatus {
    #[default]
    Firing,
    Resolved,
}

impl EventStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            EventStatus::Firing => "firing",
            EventStatus::Resolved => "resolved",
        }
    }

    pub fn parse(s: &str) -> Option<EventStatus> {
        match s {
            "firing" => Some(EventStatus::Firing),
            "resolved" => Some(EventStatus::Resolved),
            _ => None,
        }
    }
}

/// An event as a sender submits it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewEvent {
    pub topic: String,
    pub title: String,
    #[serde(default)]
    pub severity: Severity,
    #[serde(default)]
    pub status: EventStatus,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    #[serde(default)]
    pub payload: serde_json::Value,
    #[serde(default)]
    pub dedup_key: Option<String>,
    /// RFC 3339 time the event occurred. Defaults to the time of receipt.
    #[serde(default)]
    pub ts: Option<String>,
}

/// A stored event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Event {
    pub id: String,
    pub workspace_id: String,
    pub source_id: String,
    pub topic: String,
    pub severity: Severity,
    pub status: EventStatus,
    pub title: String,
    pub body: Option<String>,
    pub labels: BTreeMap<String, String>,
    pub payload: serde_json::Value,
    pub dedup_key: Option<String>,
    /// Unix milliseconds.
    pub occurred_at: i64,
    /// Unix milliseconds.
    pub received_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ValidationError {
    #[error("topic must be 1 to {TOPIC_MAX} characters")]
    TopicLength,
    #[error("topic may only contain letters, digits and . _ : / -")]
    TopicChars,
    #[error("title must be 1 to {TITLE_MAX} characters")]
    TitleLength,
    #[error("body must be at most {BODY_MAX} characters")]
    BodyLength,
    #[error("at most {LABELS_MAX} labels are allowed")]
    TooManyLabels,
    #[error("label key {0:?} must be 1 to {LABEL_KEY_MAX} characters of letters, digits and . _ -")]
    LabelKey(String),
    #[error("value of label {0:?} must be at most {LABEL_VALUE_MAX} characters")]
    LabelValue(String),
    #[error("dedup_key must be 1 to {DEDUP_KEY_MAX} characters")]
    DedupKeyLength,
    #[error("{0} must not contain a NUL character")]
    NulCharacter(&'static str),
    #[error("ts must be an RFC 3339 timestamp, for example 2026-10-13T09:41:07Z")]
    Timestamp,
}

pub(crate) fn is_topic_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '/' | '-')
}

pub(crate) fn is_label_key_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')
}

impl NewEvent {
    pub fn validate(&self) -> Result<(), ValidationError> {
        // A NUL cannot be stored as text in Postgres, so it is refused on every database.
        let has_nul = |text: &str| text.contains('\0');
        if has_nul(&self.title) {
            return Err(ValidationError::NulCharacter("title"));
        }
        if self.body.as_deref().is_some_and(has_nul) {
            return Err(ValidationError::NulCharacter("body"));
        }
        if self.labels.values().any(|value| has_nul(value)) {
            return Err(ValidationError::NulCharacter("labels"));
        }
        if self.dedup_key.as_deref().is_some_and(has_nul) {
            return Err(ValidationError::NulCharacter("dedup_key"));
        }
        let topic_len = self.topic.chars().count();
        if topic_len == 0 || topic_len > TOPIC_MAX {
            return Err(ValidationError::TopicLength);
        }
        if !self.topic.chars().all(is_topic_char) {
            return Err(ValidationError::TopicChars);
        }
        if self.title.trim().is_empty() || self.title.chars().count() > TITLE_MAX {
            return Err(ValidationError::TitleLength);
        }
        if self
            .body
            .as_ref()
            .is_some_and(|b| b.chars().count() > BODY_MAX)
        {
            return Err(ValidationError::BodyLength);
        }
        if self.labels.len() > LABELS_MAX {
            return Err(ValidationError::TooManyLabels);
        }
        for (key, value) in &self.labels {
            let len = key.chars().count();
            if len == 0 || len > LABEL_KEY_MAX || !key.chars().all(is_label_key_char) {
                return Err(ValidationError::LabelKey(key.clone()));
            }
            if value.chars().count() > LABEL_VALUE_MAX {
                return Err(ValidationError::LabelValue(key.clone()));
            }
        }
        if let Some(key) = &self.dedup_key {
            let len = key.chars().count();
            if len == 0 || len > DEDUP_KEY_MAX {
                return Err(ValidationError::DedupKeyLength);
            }
        }
        Ok(())
    }

    /// The time the event occurred in unix milliseconds, or `now_ms` when the sender gave none.
    pub fn occurred_at(&self, now_ms: i64) -> Result<i64, ValidationError> {
        let Some(ts) = &self.ts else {
            return Ok(now_ms);
        };
        let parsed = OffsetDateTime::parse(ts, &Rfc3339).map_err(|_| ValidationError::Timestamp)?;
        i64::try_from(parsed.unix_timestamp_nanos() / 1_000_000)
            .map_err(|_| ValidationError::Timestamp)
    }
}
