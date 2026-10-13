//! The one language for route expressions, templates, connector bodies and inbound mappings.
//!
//! Everything here runs text written by a user, so evaluation is bounded and never panics.

use std::sync::LazyLock;

use minijinja::{AutoEscape, Environment, ErrorKind, UndefinedBehavior};
use serde_json::Value;

use crate::event::Event;

/// Instructions one evaluation may execute.
const FUEL: u64 = 50_000;
/// Longest text a template may produce, in bytes.
const OUTPUT_MAX: usize = 256 * 1024;
/// Longest source text accepted, in bytes.
const SOURCE_MAX: usize = 32 * 1024;

static ENV: LazyLock<Environment<'static>> = LazyLock::new(|| {
    let mut env = Environment::new();
    env.set_fuel(Some(FUEL));
    // `payload.a.b` on a payload without `a` is empty rather than an error.
    env.set_undefined_behavior(UndefinedBehavior::Chainable);
    env.set_auto_escape_callback(|_| AutoEscape::None);
    env
});

fn describe(e: &minijinja::Error) -> String {
    match e.kind() {
        ErrorKind::OutOfFuel => "it did too much work and was stopped".to_owned(),
        _ => match e.detail() {
            Some(detail) => format!("{}: {detail}", e.kind()),
            None => e.kind().to_string(),
        },
    }
}

fn check_size(source: &str) -> Result<(), String> {
    if source.len() > SOURCE_MAX {
        return Err(format!("it is longer than {SOURCE_MAX} bytes"));
    }
    Ok(())
}

/// Checks that `source` is a well-formed expression, without evaluating it.
pub fn check_expression(source: &str) -> Result<(), String> {
    check_size(source)?;
    ENV.compile_expression(source)
        .map(|_| ())
        .map_err(|e| describe(&e))
}

/// Checks that `source` is a well-formed template, without rendering it.
pub fn check_template(source: &str) -> Result<(), String> {
    check_size(source)?;
    ENV.template_from_str(source)
        .map(|_| ())
        .map_err(|e| describe(&e))
}

/// Evaluates an expression against `ctx`. A missing value is `null`.
pub fn eval(source: &str, ctx: &Value) -> Result<Value, String> {
    check_size(source)?;
    let expression = ENV.compile_expression(source).map_err(|e| describe(&e))?;
    let value = expression
        .eval(minijinja::Value::from(minijinja::value::Serde(ctx)))
        .map_err(|e| describe(&e))?;
    serde_json::to_value(&value).map_err(|e| e.to_string())
}

/// Evaluates an expression and reports whether its result is truthy.
pub fn eval_bool(source: &str, ctx: &Value) -> Result<bool, String> {
    check_size(source)?;
    let expression = ENV.compile_expression(source).map_err(|e| describe(&e))?;
    let value = expression
        .eval(minijinja::Value::from(minijinja::value::Serde(ctx)))
        .map_err(|e| describe(&e))?;
    Ok(value.is_true())
}

/// Renders a template against `ctx`. Values are inserted as they are, never evaluated again.
pub fn render(source: &str, ctx: &Value) -> Result<String, String> {
    check_size(source)?;
    let out = ENV
        .render_str(source, minijinja::Value::from(minijinja::value::Serde(ctx)))
        .map_err(|e| describe(&e))?;
    if out.len() > OUTPUT_MAX {
        return Err(format!("its output is too long (over {OUTPUT_MAX} bytes)"));
    }
    Ok(out)
}

/// What expressions and templates see of an event.
pub fn event_context(e: &Event) -> Value {
    serde_json::json!({
        "id": e.id,
        "source_id": e.source_id,
        "topic": e.topic,
        "severity": e.severity,
        "status": e.status,
        "title": e.title,
        "body": e.body,
        "labels": e.labels,
        "payload": e.payload,
        "dedup_key": e.dedup_key,
        "occurred_at": e.occurred_at,
        "received_at": e.received_at,
    })
}
