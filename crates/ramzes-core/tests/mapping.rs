use std::collections::BTreeMap;

use ramzes_core::{EventStatus, Mapping, Severity, Verify};
use serde_json::json;

fn mapping(yaml_like: serde_json::Value) -> Mapping {
    serde_json::from_value(yaml_like).unwrap()
}

fn headers(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn one_payload_becomes_one_event() {
    let m = mapping(json!({
        "fields": {
            "topic": "'ci.' ~ payload.kind",
            "title": "payload.repo ~ ': build ' ~ payload.result",
            "severity": "'error' if payload.result == 'failed' else 'info'",
            "body": "payload.log",
            "labels": "{'repo': payload.repo, 'branch': payload.branch, 'run': payload.run}",
            "dedup_key": "payload.repo ~ '/' ~ payload.branch",
            "ts": "payload.at"
        }
    }));
    assert_eq!(m.verify, Verify::Bearer);
    let payload = json!({"kind": "build", "repo": "shop", "branch": "main", "result": "failed", "run": 42,
        "log": "exit 1", "at": "2026-10-13T09:41:07Z"});
    let events = m.apply(&payload, &headers(&[])).unwrap();
    assert_eq!(events.len(), 1);
    let e = &events[0];
    assert_eq!(e.topic, "ci.build");
    assert_eq!(e.title, "shop: build failed");
    assert_eq!(e.severity, Severity::Error);
    assert_eq!(e.status, EventStatus::Firing);
    assert_eq!(e.body.as_deref(), Some("exit 1"));
    assert_eq!(e.labels["repo"], "shop");
    assert_eq!(e.labels["run"], "42");
    assert_eq!(e.dedup_key.as_deref(), Some("shop/main"));
    assert_eq!(e.ts.as_deref(), Some("2026-10-13T09:41:07Z"));
    assert_eq!(e.payload, payload);
    assert!(e.validate().is_ok());
}

#[test]
fn a_list_in_the_payload_becomes_one_event_per_item() {
    let m = mapping(json!({
        "events": "payload.alerts",
        "fields": {
            "topic": "'alerts.' ~ item.name",
            "title": "item.summary",
            "status": "item.state",
            "labels": "item.labels"
        }
    }));
    let payload = json!({"alerts": [
        {"name": "disk", "summary": "Disk full", "state": "firing", "labels": {"host": "a"}},
        {"name": "cpu", "summary": "CPU ok again", "state": "resolved", "labels": {"host": "b"}}
    ]});
    let events = m.apply(&payload, &headers(&[])).unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].topic, "alerts.disk");
    assert_eq!(events[1].status, EventStatus::Resolved);
    assert_eq!(events[1].labels["host"], "b");
    assert_eq!(events[1].payload, payload["alerts"][1]);
}

#[test]
fn when_false_produces_no_event() {
    let m = mapping(json!({
        "when": "headers['x-kind'] != 'ping'",
        "fields": {"topic": "'x'", "title": "'hello'"}
    }));
    assert!(
        m.apply(&json!({}), &headers(&[("x-kind", "ping")]))
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        m.apply(&json!({}), &headers(&[("x-kind", "push")]))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn awkward_values_are_cleaned_rather_than_rejected() {
    let m = mapping(json!({
        "fields": {
            "topic": "payload.topic",
            "title": "payload.title",
            "severity": "payload.level",
            "body": "payload.body",
            "labels": "payload.labels",
            "dedup_key": "payload.key",
            "ts": "payload.when"
        }
    }));
    let long = "x".repeat(30_000);
    let payload = json!({
        "topic": "Pull Request/opened #5",
        "title": format!("  {long}  "),
        "level": "FATAL",
        "body": long,
        "labels": {"alert name": "a b", "ok.key": true, "empty": null, "nested": {"a": 1}, "long": long, "": "x"},
        "key": long,
        "when": "yesterday"
    });
    let events = m.apply(&payload, &headers(&[])).unwrap();
    let e = &events[0];
    assert_eq!(e.topic, "Pull_Request/opened__5");
    assert_eq!(e.title.chars().count(), 500);
    assert_eq!(e.severity, Severity::Critical);
    assert_eq!(e.body.as_ref().unwrap().chars().count(), 20_000);
    assert_eq!(e.labels["alert_name"], "a b");
    assert_eq!(e.labels["ok.key"], "true");
    assert_eq!(e.labels["nested"], r#"{"a":1}"#);
    assert_eq!(e.labels["long"].chars().count(), 1000);
    assert!(!e.labels.contains_key("empty") && !e.labels.contains_key(""));
    assert_eq!(e.dedup_key.as_ref().unwrap().chars().count(), 200);
    assert_eq!(e.ts, None);
    assert!(e.validate().is_ok(), "{:?}", e.validate());
}

#[test]
fn an_unknown_severity_is_info_and_common_spellings_are_understood() {
    let m = mapping(
        json!({"fields": {"topic": "'t'", "title": "'x'", "severity": "payload.s", "status": "payload.st"}}),
    );
    let sev = |s: &str| m.apply(&json!({"s": s}), &headers(&[])).unwrap()[0].severity;
    assert_eq!(sev("warn"), Severity::Warning);
    assert_eq!(sev("Warning"), Severity::Warning);
    assert_eq!(sev("crit"), Severity::Critical);
    assert_eq!(sev("err"), Severity::Error);
    assert_eq!(sev("page-me"), Severity::Info);
    let status = |s: &str| m.apply(&json!({"st": s}), &headers(&[])).unwrap()[0].status;
    assert_eq!(status("ok"), EventStatus::Resolved);
    assert_eq!(status("alerting"), EventStatus::Firing);
}

#[test]
fn a_mapping_that_gives_no_title_or_topic_is_an_error_naming_the_field() {
    let m = mapping(json!({"fields": {"topic": "payload.t", "title": "payload.missing"}}));
    let error = m.apply(&json!({"t": "a"}), &headers(&[])).unwrap_err();
    assert!(error.starts_with("title:"), "{error}");
    let error = m.apply(&json!({}), &headers(&[])).unwrap_err();
    assert!(error.starts_with("topic:"), "{error}");
}

#[test]
fn events_must_be_a_list() {
    let m =
        mapping(json!({"events": "payload.alerts", "fields": {"topic": "'t'", "title": "'x'"}}));
    let error = m
        .apply(&json!({"alerts": "nope"}), &headers(&[]))
        .unwrap_err();
    assert!(error.starts_with("events:"), "{error}");
    // A payload without the list is a request with nothing in it, not an error.
    assert!(m.apply(&json!({}), &headers(&[])).unwrap().is_empty());
}

#[test]
fn validation_names_the_broken_expression() {
    let m = mapping(json!({"fields": {"topic": "'t'", "title": "payload.title ~"}}));
    assert!(m.validate().unwrap_err().starts_with("fields.title:"));
    let m = mapping(json!({"when": "(", "fields": {"topic": "'t'", "title": "'x'"}}));
    assert!(m.validate().unwrap_err().starts_with("when:"));
    let m = mapping(
        json!({"verify": {"method": "hmac_sha256", "header": "bad header"},
        "fields": {"topic": "'t'", "title": "'x'"}}),
    );
    assert!(m.validate().unwrap_err().starts_with("verify.header:"));
}

#[test]
fn verification_methods_are_read_from_the_definition() {
    let m = mapping(
        json!({"verify": {"method": "hmac_sha256", "header": "X-Hub-Signature-256", "prefix": "sha256="},
        "fields": {"topic": "'t'", "title": "'x'"}}),
    );
    assert_eq!(
        m.verify,
        Verify::HmacSha256 {
            header: "X-Hub-Signature-256".into(),
            prefix: "sha256=".into()
        }
    );
    let m = mapping(
        json!({"verify": {"method": "header", "header": "X-Gitlab-Token"},
        "fields": {"topic": "'t'", "title": "'x'"}}),
    );
    assert_eq!(
        m.verify,
        Verify::Header {
            header: "X-Gitlab-Token".into()
        }
    );
    let m = mapping(json!({"verify": {"method": "query", "param": "key"},
        "fields": {"topic": "'t'", "title": "'x'"}}));
    assert_eq!(
        m.verify,
        Verify::Query {
            param: "key".into()
        }
    );
}

#[test]
fn a_thousand_alerts_are_mapped_in_time_proportional_to_their_number() {
    let mapping = ramzes_core::adapters::mapping_for("alertmanager", None)
        .unwrap()
        .unwrap();
    let alerts = |n: usize| {
        let alerts: Vec<serde_json::Value> = (0..n)
            .map(|i| json!({"status": "firing", "fingerprint": format!("f{i}"),
                "labels": {"alertname": "HighErrorRate", "instance": format!("host-{i}"), "severity": "critical"},
                "annotations": {"summary": "Error rate above 5%", "description": "x".repeat(400)},
                "startsAt": "2026-10-13T09:41:07Z"}))
            .collect();
        json!({"version": "4", "status": "firing", "alerts": alerts})
    };
    let time = |n: usize| {
        let payload = alerts(n);
        let started = std::time::Instant::now();
        assert_eq!(mapping.apply(&payload, &headers(&[])).unwrap().len(), n);
        started.elapsed().as_secs_f64()
    };
    let (small, large) = (time(100).max(0.001), time(1000));
    // Ten times the alerts may take a little more than ten times as long, not a hundred.
    assert!(
        large < small * 30.0,
        "100 alerts took {small:.3} s, 1000 took {large:.3} s"
    );
}
