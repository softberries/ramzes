use std::collections::BTreeMap;

use ramzes_core::{
    Event, EventStatus, LabelCond, Route, RouteMatch, Severity, Target, route_event,
};
use serde_json::json;

fn event(topic: &str, severity: Severity, labels: &[(&str, &str)]) -> Event {
    Event {
        id: "e1".into(),
        workspace_id: "w".into(),
        source_id: "src-a".into(),
        topic: topic.into(),
        severity,
        status: EventStatus::Firing,
        title: "t".into(),
        body: None,
        labels: labels
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect::<BTreeMap<_, _>>(),
        payload: json!(null),
        dedup_key: None,
        occurred_at: 0,
        received_at: 0,
    }
}

fn topic(p: &str) -> RouteMatch {
    RouteMatch {
        topic: Some(p.into()),
        ..RouteMatch::default()
    }
}

fn route(id: &str, position: i64, m: RouteMatch, targets: &[&str]) -> Route {
    Route {
        id: id.into(),
        name: id.into(),
        position,
        enabled: true,
        stop: false,
        r#match: m,
        targets: targets
            .iter()
            .map(|c| Target {
                connection_id: c.to_string(),
                ..Target::default()
            })
            .collect(),
    }
}

#[test]
fn empty_match_accepts_everything() {
    assert!(RouteMatch::default().matches(&event("anything", Severity::Debug, &[])));
}

#[test]
fn topic_glob_matches_whole_topic() {
    let e = event("orders.failed", Severity::Info, &[]);
    assert!(topic("orders.*").matches(&e));
    assert!(topic("*").matches(&e));
    assert!(topic("*.failed").matches(&e));
    assert!(topic("orders.failed").matches(&e));
    assert!(topic("o*d").matches(&e));
    assert!(!topic("orders").matches(&e));
    assert!(!topic("rders.*").matches(&e));
    assert!(!topic("orders.*").matches(&event("xorders.failed", Severity::Info, &[])));
}

#[test]
fn topic_glob_treats_dots_literally() {
    assert!(!topic("orders.failed").matches(&event("ordersXfailed", Severity::Info, &[])));
}

#[test]
fn min_severity_is_a_threshold() {
    let m = RouteMatch {
        min_severity: Some(Severity::Error),
        ..RouteMatch::default()
    };
    assert!(!m.matches(&event("a", Severity::Warning, &[])));
    assert!(m.matches(&event("a", Severity::Error, &[])));
    assert!(m.matches(&event("a", Severity::Critical, &[])));
}

#[test]
fn source_must_be_equal() {
    let m = RouteMatch {
        source_id: Some("src-a".into()),
        ..RouteMatch::default()
    };
    assert!(m.matches(&event("a", Severity::Info, &[])));
    let m = RouteMatch {
        source_id: Some("src-b".into()),
        ..RouteMatch::default()
    };
    assert!(!m.matches(&event("a", Severity::Info, &[])));
}

#[test]
fn label_equals() {
    let m = RouteMatch {
        labels: vec![LabelCond::Equals {
            key: "env".into(),
            value: "prod".into(),
        }],
        ..RouteMatch::default()
    };
    assert!(m.matches(&event("a", Severity::Info, &[("env", "prod")])));
    assert!(!m.matches(&event("a", Severity::Info, &[("env", "dev")])));
    assert!(!m.matches(&event("a", Severity::Info, &[])));
}

#[test]
fn label_matches_regex() {
    let m = RouteMatch {
        labels: vec![LabelCond::Matches {
            key: "service".into(),
            pattern: "^pay(ments)?$".into(),
        }],
        ..RouteMatch::default()
    };
    assert!(m.matches(&event("a", Severity::Info, &[("service", "payments")])));
    assert!(!m.matches(&event("a", Severity::Info, &[("service", "payroll")])));
    assert!(!m.matches(&event("a", Severity::Info, &[])));
}

#[test]
fn label_exists() {
    let m = RouteMatch {
        labels: vec![LabelCond::Exists { key: "team".into() }],
        ..RouteMatch::default()
    };
    assert!(m.matches(&event("a", Severity::Info, &[("team", "")])));
    assert!(!m.matches(&event("a", Severity::Info, &[("env", "prod")])));
}

#[test]
fn all_conditions_must_hold() {
    let m = RouteMatch {
        topic: Some("orders.*".into()),
        min_severity: Some(Severity::Error),
        labels: vec![LabelCond::Equals {
            key: "env".into(),
            value: "prod".into(),
        }],
        ..RouteMatch::default()
    };
    assert!(m.matches(&event("orders.failed", Severity::Error, &[("env", "prod")])));
    assert!(!m.matches(&event("orders.failed", Severity::Info, &[("env", "prod")])));
    assert!(!m.matches(&event(
        "billing.failed",
        Severity::Error,
        &[("env", "prod")]
    )));
}

#[test]
fn invalid_regex_fails_validation_and_matches_nothing() {
    let m = RouteMatch {
        labels: vec![LabelCond::Matches {
            key: "k".into(),
            pattern: "(".into(),
        }],
        ..RouteMatch::default()
    };
    assert!(m.validate().is_err());
    assert!(!m.matches(&event("a", Severity::Info, &[("k", "(")])));
    assert!(RouteMatch::default().validate().is_ok());
}

#[test]
fn empty_topic_pattern_and_label_key_fail_validation() {
    assert!(topic("").validate().is_err());
    let m = RouteMatch {
        labels: vec![LabelCond::Exists { key: String::new() }],
        ..RouteMatch::default()
    };
    assert!(m.validate().is_err());
}

#[test]
fn match_serialises_with_op_tags() {
    let m = RouteMatch {
        topic: Some("a.*".into()),
        min_severity: Some(Severity::Warning),
        labels: vec![
            LabelCond::Equals {
                key: "env".into(),
                value: "prod".into(),
            },
            LabelCond::Exists { key: "team".into() },
        ],
        ..RouteMatch::default()
    };
    let j = serde_json::to_value(&m).unwrap();
    assert_eq!(
        j,
        json!({"topic": "a.*", "min_severity": "warning", "source_id": null, "labels": [
            {"op": "equals", "key": "env", "value": "prod"}, {"op": "exists", "key": "team"}]})
    );
    assert_eq!(serde_json::from_value::<RouteMatch>(j).unwrap(), m);
    assert_eq!(
        serde_json::from_value::<RouteMatch>(json!({})).unwrap(),
        RouteMatch::default()
    );
}

#[test]
fn routes_fire_in_position_order_with_every_target() {
    let routes = vec![
        route("second", 2, topic("*"), &["c3"]),
        route("first", 1, topic("orders.*"), &["c1", "c2"]),
    ];
    let e = event("orders.failed", Severity::Info, &[]);
    let got: Vec<(String, String)> = route_event(&e, &routes)
        .matched
        .iter()
        .map(|m| (m.route.id.clone(), m.target.connection_id.clone()))
        .collect();
    assert_eq!(
        got,
        vec![
            ("first".to_string(), "c1".to_string()),
            ("first".to_string(), "c2".to_string()),
            ("second".to_string(), "c3".to_string())
        ]
    );
}

#[test]
fn disabled_routes_are_skipped() {
    let mut r = route("r", 1, topic("*"), &["c1"]);
    r.enabled = false;
    assert!(
        route_event(&event("a", Severity::Info, &[]), &[r])
            .matched
            .is_empty()
    );
}

#[test]
fn a_matching_stop_route_ends_evaluation() {
    let mut first = route("first", 1, topic("orders.*"), &["c1"]);
    first.stop = true;
    let routes = vec![first, route("second", 2, topic("*"), &["c2"])];
    let got = route_event(&event("orders.failed", Severity::Info, &[]), &routes).matched;
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].route.id, "first");
}

#[test]
fn a_non_matching_stop_route_does_not_stop() {
    let mut first = route("first", 1, topic("billing.*"), &["c1"]);
    first.stop = true;
    let routes = vec![first, route("second", 2, topic("*"), &["c2"])];
    let got = route_event(&event("orders.failed", Severity::Info, &[]), &routes).matched;
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].route.id, "second");
}

#[test]
fn no_match_returns_empty() {
    let routes = vec![route("r", 1, topic("billing.*"), &["c1"])];
    assert!(
        route_event(&event("orders.failed", Severity::Info, &[]), &routes)
            .matched
            .is_empty()
    );
}
