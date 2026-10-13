use std::collections::BTreeMap;

use ramzes_core::expr::{check_expression, check_template, eval, eval_bool, render};
use ramzes_core::{
    Event, EventStatus, Message, Route, RouteMatch, Severity, Target, Template, apply_template,
    build_message, event_context, route_event,
};
use serde_json::json;

fn event() -> Event {
    Event {
        id: "e1".into(),
        workspace_id: "w".into(),
        source_id: "src-a".into(),
        topic: "orders.failed".into(),
        severity: Severity::Error,
        status: EventStatus::Firing,
        title: "Order 17 failed".into(),
        body: Some("card declined".into()),
        labels: BTreeMap::from([("env".to_string(), "prod".to_string())]),
        payload: json!({"amount": 120, "customer": {"tier": "gold"}, "items": ["a", "b"]}),
        dedup_key: None,
        occurred_at: 0,
        received_at: 0,
    }
}

#[test]
fn an_expression_compares_payload_fields() {
    let ctx = event_context(&event());
    assert!(eval_bool("payload.amount > 100 and labels.env == 'prod'", &ctx).unwrap());
    assert!(!eval_bool("payload.amount > 500", &ctx).unwrap());
    assert!(eval_bool("payload.customer.tier in ['gold', 'platinum']", &ctx).unwrap());
    assert!(eval_bool("severity == 'error' and topic == 'orders.failed'", &ctx).unwrap());
}

#[test]
fn a_missing_field_is_false_not_an_error() {
    let ctx = event_context(&event());
    assert!(!eval_bool("payload.nothing.here == 1", &ctx).unwrap());
    assert!(!eval_bool("labels.team", &ctx).unwrap());
}

#[test]
fn an_expression_returns_json_values() {
    let ctx = event_context(&event());
    assert_eq!(eval("payload.items", &ctx).unwrap(), json!(["a", "b"]));
    assert_eq!(eval("payload.items | length", &ctx).unwrap(), json!(2));
    assert_eq!(
        eval("title ~ '!'", &ctx).unwrap(),
        json!("Order 17 failed!")
    );
    assert_eq!(eval("payload.nothing", &ctx).unwrap(), json!(null));
}

#[test]
fn a_syntax_error_is_reported_before_use() {
    assert!(check_expression("payload.amount >").is_err());
    assert!(check_expression("payload.amount > 1").is_ok());
    assert!(check_template("{{ title ").is_err());
    assert!(check_template("{% if x %}no end").is_err());
    assert!(check_template("{{ title }} on {{ labels.env }}").is_ok());
}

#[test]
fn evaluation_is_bounded() {
    let ctx = json!({});
    let endless = "{% for a in range(10000) %}{% for b in range(10000) %}x{% endfor %}{% endfor %}";
    let error = render(endless, &ctx).unwrap_err();
    assert!(
        error.contains("too much work") || error.contains("too long"),
        "{error}"
    );
}

#[test]
fn rendered_output_is_capped() {
    let ctx = json!({});
    let error = render(
        "{% for a in range(9000) %}{{ '0123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789' }}{% endfor %}",
        &ctx,
    );
    assert!(error.is_err());
}

#[test]
fn text_from_the_event_is_not_evaluated_again() {
    let ctx = json!({"title": "{{ 7 * 7 }} {% boom %}"});
    assert_eq!(
        render("got: {{ title }}", &ctx).unwrap(),
        "got: {{ 7 * 7 }} {% boom %}"
    );
}

#[test]
fn tojson_keeps_json_valid_for_awkward_text() {
    let ctx = json!({"title": "say \"hi\"\nnew line \\ <b>"});
    let out = render(r#"{"text": {{ title | tojson }}}"#, &ctx).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(parsed["text"], "say \"hi\"\nnew line \\ <b>");
}

fn route(id: &str, expr: &str) -> Route {
    Route {
        id: id.into(),
        name: id.into(),
        position: 1,
        enabled: true,
        r#match: RouteMatch {
            expr: Some(expr.into()),
            ..RouteMatch::default()
        },
        targets: vec![Target {
            connection_id: "c".into(),
            ..Target::default()
        }],
        ..Route::default()
    }
}

#[test]
fn a_route_expression_decides_the_match() {
    let routes = [
        route("big", "payload.amount > 100"),
        route("small", "payload.amount < 10"),
    ];
    let routed = route_event(&event(), &routes);
    assert_eq!(routed.matched.len(), 1);
    assert_eq!(routed.matched[0].route.id, "big");
    assert!(routed.errors.is_empty());
}

#[test]
fn a_route_whose_expression_fails_does_not_match_and_reports_why() {
    let routes = [
        route("broken", "payload.amount | nosuchfilter"),
        route("ok", "true"),
    ];
    let routed = route_event(&event(), &routes);
    assert_eq!(routed.matched.len(), 1);
    assert_eq!(routed.matched[0].route.id, "ok");
    assert_eq!(routed.errors.len(), 1);
    assert_eq!(routed.errors[0].0, "broken");
}

#[test]
fn route_validation_rejects_a_bad_expression() {
    let m = RouteMatch {
        expr: Some("a ==".into()),
        ..RouteMatch::default()
    };
    assert!(m.validate().is_err());
}

fn template(title: &str, body: &str) -> Template {
    Template {
        id: "t".into(),
        name: "t".into(),
        title: title.into(),
        body: body.into(),
    }
}

#[test]
fn a_template_rewrites_title_and_body() {
    let e = event();
    let message: Message = build_message(&e, None);
    let t = template(
        "[{{ labels.env }}] {{ title }}",
        "{{ body }} — {{ payload.amount }} from a {{ payload.customer.tier }} customer ({{ count }} event)",
    );
    let out = apply_template(&t, &message, std::slice::from_ref(&e)).unwrap();
    assert_eq!(out.title, "[prod] Order 17 failed");
    assert_eq!(
        out.body.as_deref(),
        Some("card declined — 120 from a gold customer (1 event)")
    );
    assert_eq!(out.fields, message.fields);
}

#[test]
fn an_empty_template_part_keeps_the_default() {
    let e = event();
    let message = build_message(&e, None);
    let out = apply_template(
        &template("", "only body"),
        &message,
        std::slice::from_ref(&e),
    )
    .unwrap();
    assert_eq!(out.title, "Order 17 failed");
    assert_eq!(out.body.as_deref(), Some("only body"));
}

#[test]
fn a_template_that_renders_an_empty_title_is_an_error() {
    let e = event();
    let message = build_message(&e, None);
    assert!(
        apply_template(
            &template("{{ labels.nope }}", ""),
            &message,
            std::slice::from_ref(&e)
        )
        .is_err()
    );
}

#[test]
fn a_template_that_fails_reports_the_part() {
    let e = event();
    let message = build_message(&e, None);
    let error = apply_template(
        &template("{{ title | nosuch }}", ""),
        &message,
        std::slice::from_ref(&e),
    )
    .unwrap_err();
    assert!(error.starts_with("title:"), "{error}");
}
