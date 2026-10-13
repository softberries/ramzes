use ramzes_core::{EventStatus, NewEvent, Severity, ValidationError, new_id};
use serde_json::json;

fn valid() -> NewEvent {
    serde_json::from_value(json!({"topic": "orders.failed", "title": "Order failed"})).unwrap()
}

#[test]
fn ids_are_unique_and_sort_by_creation_order() {
    let ids: Vec<String> = (0..2000).map(|_| new_id()).collect();
    let mut sorted = ids.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted, ids);
}

#[test]
fn severity_orders_from_debug_to_critical() {
    assert!(Severity::Debug < Severity::Info);
    assert!(Severity::Info < Severity::Warning);
    assert!(Severity::Warning < Severity::Error);
    assert!(Severity::Error < Severity::Critical);
}

#[test]
fn severity_round_trips_through_i64_and_json() {
    for s in [
        Severity::Debug,
        Severity::Info,
        Severity::Warning,
        Severity::Error,
        Severity::Critical,
    ] {
        assert_eq!(Severity::from_i64(s.as_i64()), Some(s));
        let j = serde_json::to_value(s).unwrap();
        assert_eq!(serde_json::from_value::<Severity>(j).unwrap(), s);
    }
    assert_eq!(
        serde_json::to_value(Severity::Warning).unwrap(),
        json!("warning")
    );
    assert_eq!(Severity::from_i64(9), None);
}

#[test]
fn status_round_trips_through_str() {
    assert_eq!(EventStatus::parse("firing"), Some(EventStatus::Firing));
    assert_eq!(EventStatus::parse("resolved"), Some(EventStatus::Resolved));
    assert_eq!(EventStatus::parse("other"), None);
    assert_eq!(EventStatus::Resolved.as_str(), "resolved");
}

#[test]
fn defaults_are_info_firing_empty() {
    let e = valid();
    assert_eq!(e.severity, Severity::Info);
    assert_eq!(e.status, EventStatus::Firing);
    assert!(e.labels.is_empty());
    assert_eq!(e.payload, json!(null));
    assert_eq!(e.validate(), Ok(()));
}

#[test]
fn unknown_fields_are_rejected() {
    let r =
        serde_json::from_value::<NewEvent>(json!({"topic": "a", "title": "b", "sevrity": "info"}));
    assert!(r.unwrap_err().to_string().contains("sevrity"));
}

#[test]
fn unknown_severity_is_rejected() {
    let r =
        serde_json::from_value::<NewEvent>(json!({"topic": "a", "title": "b", "severity": "loud"}));
    assert!(r.is_err());
}

#[test]
fn non_string_label_value_is_rejected() {
    let r =
        serde_json::from_value::<NewEvent>(json!({"topic": "a", "title": "b", "labels": {"n": 1}}));
    assert!(r.is_err());
}

#[test]
fn topic_rules() {
    let mut e = valid();
    e.topic = "svc/orders:failed-1_a.b".into();
    assert_eq!(e.validate(), Ok(()));
    e.topic = String::new();
    assert_eq!(e.validate(), Err(ValidationError::TopicLength));
    e.topic = "a".repeat(201);
    assert_eq!(e.validate(), Err(ValidationError::TopicLength));
    e.topic = "has space".into();
    assert_eq!(e.validate(), Err(ValidationError::TopicChars));
}

#[test]
fn title_rules() {
    let mut e = valid();
    e.title = "x".repeat(500);
    assert_eq!(e.validate(), Ok(()));
    e.title = "x".repeat(501);
    assert_eq!(e.validate(), Err(ValidationError::TitleLength));
    e.title = "  ".into();
    assert_eq!(e.validate(), Err(ValidationError::TitleLength));
}

#[test]
fn body_rule() {
    let mut e = valid();
    e.body = Some("x".repeat(20_000));
    assert_eq!(e.validate(), Ok(()));
    e.body = Some("x".repeat(20_001));
    assert_eq!(e.validate(), Err(ValidationError::BodyLength));
}

#[test]
fn label_rules() {
    let mut e = valid();
    for i in 0..64 {
        e.labels.insert(format!("k{i}"), "v".into());
    }
    assert_eq!(e.validate(), Ok(()));
    e.labels.insert("one-more".into(), "v".into());
    assert_eq!(e.validate(), Err(ValidationError::TooManyLabels));

    let mut e = valid();
    e.labels.insert("bad key".into(), "v".into());
    assert_eq!(
        e.validate(),
        Err(ValidationError::LabelKey("bad key".into()))
    );

    let mut e = valid();
    e.labels.insert("k".into(), "v".repeat(1001));
    assert_eq!(e.validate(), Err(ValidationError::LabelValue("k".into())));
}

#[test]
fn dedup_key_rule() {
    let mut e = valid();
    e.dedup_key = Some("k".repeat(200));
    assert_eq!(e.validate(), Ok(()));
    e.dedup_key = Some("k".repeat(201));
    assert_eq!(e.validate(), Err(ValidationError::DedupKeyLength));
}

#[test]
fn occurred_at_parses_rfc3339() {
    let mut e = valid();
    e.ts = Some("2026-10-13T09:41:07Z".into());
    assert_eq!(e.occurred_at(5), Ok(1_791_884_467_000));
    e.ts = Some("2026-10-13T11:41:07.250+02:00".into());
    assert_eq!(e.occurred_at(5), Ok(1_791_884_467_250));
}

#[test]
fn occurred_at_defaults_to_now() {
    assert_eq!(valid().occurred_at(42), Ok(42));
}

#[test]
fn occurred_at_rejects_garbage() {
    let mut e = valid();
    e.ts = Some("yesterday".into());
    assert_eq!(e.occurred_at(5), Err(ValidationError::Timestamp));
}

#[test]
fn nul_characters_are_rejected_everywhere_text_is_stored() {
    let mut e = valid();
    e.title = "x\u{0}y".into();
    assert_eq!(e.validate(), Err(ValidationError::NulCharacter("title")));

    let mut e = valid();
    e.body = Some("a\u{0}".into());
    assert_eq!(e.validate(), Err(ValidationError::NulCharacter("body")));

    let mut e = valid();
    e.labels.insert("k".into(), "\u{0}".into());
    assert_eq!(e.validate(), Err(ValidationError::NulCharacter("labels")));

    let mut e = valid();
    e.dedup_key = Some("k\u{0}".into());
    assert_eq!(
        e.validate(),
        Err(ValidationError::NulCharacter("dedup_key"))
    );

    // Other control characters, such as newlines and tabs, are fine.
    let mut e = valid();
    e.body = Some("line one\n\tline two".into());
    assert_eq!(e.validate(), Ok(()));
}
