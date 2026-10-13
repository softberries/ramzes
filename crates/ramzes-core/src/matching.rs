use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

use crate::event::{Event, EventStatus, Severity};

/// Upper bound on a compiled label pattern, so a route cannot hold an arbitrarily expensive regex.
const REGEX_SIZE_LIMIT: usize = 100_000;

/// The conditions of a route. Every condition that is set must hold.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RouteMatch {
    /// Glob over the whole topic: `*` matches any run of characters.
    #[serde(default)]
    pub topic: Option<String>,
    #[serde(default)]
    pub min_severity: Option<Severity>,
    #[serde(default)]
    pub source_id: Option<String>,
    #[serde(default)]
    pub labels: Vec<LabelCond>,
    /// An expression over the event (`payload`, `labels`, `topic`, …) that must be true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expr: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(tag = "op", rename_all = "lowercase", deny_unknown_fields)]
pub enum LabelCond {
    Equals {
        key: String,
        value: String,
    },
    /// `pattern` is a regular expression searched for in the label value; anchor it to match the whole value.
    Matches {
        key: String,
        pattern: String,
    },
    Exists {
        key: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MatchError {
    #[error("topic pattern must not be empty")]
    EmptyTopic,
    #[error("label key must not be empty")]
    EmptyLabelKey,
    #[error("pattern for label {key:?} is not a valid regular expression: {reason}")]
    Regex { key: String, reason: String },
    #[error("the expression is not valid: {0}")]
    Expression(String),
}

fn compile(pattern: &str) -> Result<Regex, regex::Error> {
    RegexBuilder::new(pattern)
        .size_limit(REGEX_SIZE_LIMIT)
        .build()
}

/// Whole-string glob where `*` matches any run of characters and everything else is literal.
fn glob_matches(pattern: &str, text: &str) -> bool {
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or_default();
    let Some(mut rest) = text.strip_prefix(first) else {
        return false;
    };
    let mut parts: Vec<&str> = parts.collect();
    let Some(last) = parts.pop() else {
        // No `*` in the pattern: the whole text must equal it.
        return rest.is_empty();
    };
    for part in parts {
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    rest.ends_with(last)
}

impl LabelCond {
    fn key(&self) -> &str {
        match self {
            LabelCond::Equals { key, .. }
            | LabelCond::Matches { key, .. }
            | LabelCond::Exists { key } => key,
        }
    }

    fn holds(&self, e: &Event) -> bool {
        match self {
            LabelCond::Equals { key, value } => e.labels.get(key) == Some(value),
            LabelCond::Matches { key, pattern } => match (e.labels.get(key), compile(pattern)) {
                (Some(value), Ok(re)) => re.is_match(value),
                _ => false,
            },
            LabelCond::Exists { key } => e.labels.contains_key(key),
        }
    }
}

impl RouteMatch {
    pub fn validate(&self) -> Result<(), MatchError> {
        if self.topic.as_ref().is_some_and(String::is_empty) {
            return Err(MatchError::EmptyTopic);
        }
        for cond in &self.labels {
            if cond.key().is_empty() {
                return Err(MatchError::EmptyLabelKey);
            }
            if let LabelCond::Matches { key, pattern } = cond {
                compile(pattern).map_err(|e| MatchError::Regex {
                    key: key.clone(),
                    reason: e.to_string(),
                })?;
            }
        }
        if let Some(expr) = &self.expr {
            crate::expr::check_expression(expr).map_err(MatchError::Expression)?;
        }
        Ok(())
    }

    /// Whether the event satisfies every condition. An expression that fails is an error.
    pub fn evaluate(&self, e: &Event) -> Result<bool, String> {
        let severe = self.min_severity.is_none_or(|min| e.severity >= min);
        Ok(severe && self.holds_apart_from_severity(e)?)
    }

    /// Whether a resolved event fails only the minimum severity. What ends a failure is often
    /// reported as less severe than the failure, and still has to close it.
    pub fn closes(&self, e: &Event) -> Result<bool, String> {
        let too_mild = self.min_severity.is_some_and(|min| e.severity < min);
        Ok(e.status == EventStatus::Resolved && too_mild && self.holds_apart_from_severity(e)?)
    }

    fn holds_apart_from_severity(&self, e: &Event) -> Result<bool, String> {
        let structured = self
            .topic
            .as_ref()
            .is_none_or(|p| glob_matches(p, &e.topic))
            && self.source_id.as_ref().is_none_or(|s| *s == e.source_id)
            && self.labels.iter().all(|c| c.holds(e));
        match &self.expr {
            Some(expr) if structured && !expr.trim().is_empty() => {
                crate::expr::eval_bool(expr, &crate::expr::event_context(e))
            }
            _ => Ok(structured),
        }
    }

    /// Like [`RouteMatch::evaluate`], counting a failing expression as no match.
    pub fn matches(&self, e: &Event) -> bool {
        self.evaluate(e).unwrap_or(false)
    }
}
