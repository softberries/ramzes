use std::collections::BTreeMap;

use ramzes_core::{Event, EventStatus, Next, RetryPolicy, Severity, build_message, next_attempt};
use serde_json::json;

const S: i64 = 1000;
const H: i64 = 3600 * S;

fn at(attempts_done: i64, jitter: f64) -> Next {
    next_attempt(&RetryPolicy::default(), attempts_done, 0, 0, None, jitter)
}

#[test]
fn delays_double_from_five_seconds() {
    assert_eq!(at(1, 0.5), Next::At(5 * S));
    assert_eq!(at(2, 0.5), Next::At(10 * S));
    assert_eq!(at(3, 0.5), Next::At(20 * S));
}

#[test]
fn delay_is_capped_at_one_hour() {
    let p = RetryPolicy {
        max_attempts: 100,
        max_age_ms: 1000 * H,
        ..RetryPolicy::default()
    };
    assert_eq!(next_attempt(&p, 30, 0, 0, None, 0.5), Next::At(H));
    assert_eq!(next_attempt(&p, 90, 0, 0, None, 0.5), Next::At(H));
}

#[test]
fn jitter_scales_between_80_and_120_percent() {
    assert_eq!(at(1, 0.0), Next::At(4 * S));
    assert_eq!(at(1, 1.0), Next::At(6 * S));
}

#[test]
fn delay_is_measured_from_now() {
    assert_eq!(
        next_attempt(&RetryPolicy::default(), 1, 0, 7 * S, None, 0.5),
        Next::At(12 * S)
    );
}

#[test]
fn retry_after_wins_only_when_larger() {
    let p = RetryPolicy::default();
    assert_eq!(
        next_attempt(&p, 1, 0, 0, Some(30 * S), 0.5),
        Next::At(30 * S)
    );
    assert_eq!(next_attempt(&p, 1, 0, 0, Some(S), 0.5), Next::At(5 * S));
}

#[test]
fn gives_up_after_max_attempts() {
    assert!(matches!(at(7, 0.5), Next::At(_)));
    assert_eq!(at(8, 0.5), Next::GiveUp);
}

#[test]
fn gives_up_when_next_time_is_past_max_age() {
    let p = RetryPolicy::default();
    assert!(matches!(
        next_attempt(&p, 1, 0, 24 * H - 6 * S, None, 0.5),
        Next::At(_)
    ));
    assert_eq!(
        next_attempt(&p, 1, 0, 24 * H - 4 * S, None, 0.5),
        Next::GiveUp
    );
}

fn event(labels: &[(&str, &str)]) -> Event {
    Event {
        id: "0199-id".into(),
        workspace_id: "w".into(),
        source_id: "s".into(),
        topic: "orders.failed".into(),
        severity: Severity::Error,
        status: EventStatus::Firing,
        title: "Order failed".into(),
        body: Some("card declined".into()),
        labels: labels
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect::<BTreeMap<_, _>>(),
        payload: json!({"order": 7}),
        dedup_key: None,
        occurred_at: 1,
        received_at: 2,
    }
}

#[test]
fn message_carries_event_fields_and_sorted_labels() {
    let m = build_message(&event(&[("service", "payments"), ("env", "prod")]), None);
    assert_eq!(m.title, "Order failed");
    assert_eq!(m.body.as_deref(), Some("card declined"));
    assert_eq!(m.severity, Severity::Error);
    assert_eq!(m.status, EventStatus::Firing);
    assert_eq!(m.topic, "orders.failed");
    assert_eq!(
        m.fields,
        vec![
            ("env".to_string(), "prod".to_string()),
            ("service".to_string(), "payments".to_string())
        ]
    );
    assert_eq!(m.event_ids, vec!["0199-id".to_string()]);
    assert_eq!(m.link, None);
}

#[test]
fn message_link_points_at_the_event_when_a_public_url_is_set() {
    let m = build_message(&event(&[]), Some("https://ramzes.example.com/"));
    assert_eq!(
        m.link.as_deref(),
        Some("https://ramzes.example.com/events/0199-id")
    );
}
