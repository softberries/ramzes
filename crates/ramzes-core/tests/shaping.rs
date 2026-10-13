use std::collections::BTreeMap;

use ramzes_core::shaping::{
    Action, Dedup, Group, HoldKind, NotificationState, OpenNotification, QuietHours, RateLimit,
    ReleaseFrom, ReleaseInput, Released, Resolution, Role, ShapeInput, Shaping, Silence, digest,
    quiet_until, release, shape,
};
use ramzes_core::{Event, EventStatus, RouteMatch, Severity};
use serde_json::json;

const HOUR: i64 = 3_600_000;
/// 2026-10-13 12:00:00 UTC, a Tuesday.
const NOON: i64 = 1_791_892_800_000;

fn event(title: &str) -> Event {
    Event {
        id: format!("e-{title}"),
        workspace_id: "w".into(),
        source_id: "s".into(),
        topic: "orders.failed".into(),
        severity: Severity::Error,
        status: EventStatus::Firing,
        title: title.into(),
        body: None,
        labels: BTreeMap::from([
            ("env".to_string(), "prod".to_string()),
            ("service".to_string(), "pay".to_string()),
        ]),
        payload: json!({"order": 17}),
        dedup_key: None,
        occurred_at: NOON,
        received_at: NOON,
    }
}

fn open(id: &str, state: NotificationState) -> OpenNotification {
    OpenNotification {
        id: id.into(),
        state,
        dedup_key: None,
        group_key: None,
        created_at: NOON - 60_000,
        sent: state == NotificationState::Sent || state == NotificationState::Acknowledged,
        hold: None,
        has_key: false,
        others_firing: false,
    }
}

/// An open notification one of whose firing events carries the key being looked up.
fn keyed(id: &str, state: NotificationState) -> OpenNotification {
    OpenNotification {
        has_key: true,
        ..open(id, state)
    }
}

fn resolves(what: &[(&str, bool, bool)]) -> Action {
    Action::Resolve(
        what.iter()
            .map(|(id, whole, notify)| Resolution {
                notification_id: id.to_string(),
                whole: *whole,
                notify: *notify,
            })
            .collect(),
    )
}

struct Case {
    event: Event,
    shaping: Shaping,
    silences: Vec<Silence>,
    open: Vec<OpenNotification>,
    sends: i64,
    oldest: Option<i64>,
    now: i64,
}

impl Case {
    fn new(shaping: Shaping) -> Case {
        Case {
            event: event("Order 17 failed"),
            shaping,
            silences: vec![],
            open: vec![],
            sends: 0,
            oldest: None,
            now: NOON,
        }
    }

    fn action(&self) -> Action {
        shape(&ShapeInput {
            event: &self.event,
            shaping: &self.shaping,
            silences: &self.silences,
            open: &self.open,
            sends_in_window: self.sends,
            oldest_send_in_window: self.oldest,
            now_ms: self.now,
        })
        .action
    }
}

fn dedup(window_s: i64) -> Shaping {
    Shaping {
        dedup: Some(Dedup {
            key: None,
            window_s,
        }),
        ..Shaping::default()
    }
}

fn group(wait_s: i64) -> Shaping {
    Shaping {
        group: Some(Group {
            by: vec!["service".into()],
            wait_s,
            repeat_s: None,
        }),
        ..Shaping::default()
    }
}

fn quiet(start: &str, end: &str) -> QuietHours {
    QuietHours {
        tz: "Europe/Warsaw".into(),
        start: start.into(),
        end: end.into(),
        days: vec![],
        bypass_severity: None,
    }
}

#[test]
fn without_a_policy_an_event_is_sent() {
    let case = Case::new(Shaping::default());
    assert_eq!(
        case.action(),
        Action::Send {
            dedup_key: None,
            group_key: None
        }
    );
}

#[test]
fn the_events_own_key_is_kept_even_without_a_dedup_policy() {
    let mut case = Case::new(Shaping::default());
    case.event.dedup_key = Some("order-17".into());
    assert_eq!(
        case.action(),
        Action::Send {
            dedup_key: Some("order-17".into()),
            group_key: None
        }
    );
}

#[test]
fn an_active_matching_silence_suppresses_and_is_named() {
    let mut case = Case::new(Shaping::default());
    let silence = |id: &str, topic: &str, starts: i64, ends: i64| Silence {
        id: id.into(),
        r#match: RouteMatch {
            topic: Some(topic.into()),
            ..RouteMatch::default()
        },
        starts_at: starts,
        ends_at: ends,
        comment: Some("deploy window".into()),
    };
    case.silences = vec![
        silence("other-topic", "billing.*", NOON - HOUR, NOON + HOUR),
        silence("over", "orders.*", NOON - 2 * HOUR, NOON - HOUR),
        silence("later", "orders.*", NOON + HOUR, NOON + 2 * HOUR),
        silence("now", "orders.*", NOON - HOUR, NOON + HOUR),
    ];
    let decision = shape(&ShapeInput {
        event: &case.event,
        shaping: &case.shaping,
        silences: &case.silences,
        open: &[],
        sends_in_window: 0,
        oldest_send_in_window: None,
        now_ms: NOON,
    });
    let Action::Suppress { reason, silence_id } = decision.action else {
        panic!("{:?}", decision.action);
    };
    assert_eq!(silence_id.as_deref(), Some("now"));
    assert!(reason.contains("deploy window"), "{reason}");
    assert_eq!(decision.steps[0].step, "silence");
}

#[test]
fn a_duplicate_within_the_window_is_counted_not_sent() {
    let mut case = Case::new(dedup(600));
    case.event.dedup_key = Some("order-17".into());
    let mut sent = keyed("n1", NotificationState::Sent);
    sent.created_at = NOON - 300_000;
    case.open = vec![sent.clone()];
    assert_eq!(
        case.action(),
        Action::Attach {
            notification_id: "n1".into(),
            role: Role::Duplicate
        }
    );

    // Another key, or the same key after the window, is a new notification.
    case.event.dedup_key = Some("order-18".into());
    case.open[0].has_key = false;
    assert!(matches!(case.action(), Action::Send { .. }));
    case.event.dedup_key = Some("order-17".into());
    case.open[0].has_key = true;
    case.open[0].created_at = NOON - 601_000;
    assert_eq!(
        case.action(),
        Action::Send {
            dedup_key: Some("order-17".into()),
            group_key: None
        }
    );
}

#[test]
fn without_an_own_key_dedup_uses_topic_and_labels() {
    let case = Case::new(dedup(600));
    let Action::Send {
        dedup_key: Some(first),
        ..
    } = case.action()
    else {
        panic!()
    };
    let mut other = Case::new(dedup(600));
    other.event.title = "Another title, same topic and labels".into();
    let Action::Send {
        dedup_key: Some(second),
        ..
    } = other.action()
    else {
        panic!()
    };
    assert_eq!(first, second);
    other.event.labels.insert("env".into(), "staging".into());
    let Action::Send {
        dedup_key: Some(third),
        ..
    } = other.action()
    else {
        panic!()
    };
    assert_ne!(first, third);
}

#[test]
fn a_dedup_key_expression_is_evaluated() {
    let mut case = Case::new(Shaping {
        dedup: Some(Dedup {
            key: Some("'order-' ~ payload.order".into()),
            window_s: 60,
        }),
        ..Shaping::default()
    });
    assert_eq!(
        case.action(),
        Action::Send {
            dedup_key: Some("order-17".into()),
            group_key: None
        }
    );
    // An expression that fails falls back to the default key instead of losing the event.
    case.shaping.dedup = Some(Dedup {
        key: Some("payload | nosuch".into()),
        window_s: 60,
    });
    assert!(matches!(
        case.action(),
        Action::Send {
            dedup_key: Some(_),
            ..
        }
    ));
}

#[test]
fn a_resolved_event_closes_what_is_open_and_tells_only_those_who_were_told() {
    let mut case = Case::new(Shaping::default());
    case.event.status = EventStatus::Resolved;
    case.event.dedup_key = Some("order-17".into());
    let with_key = keyed;
    case.open = vec![with_key("sent", NotificationState::Sent)];
    assert_eq!(case.action(), resolves(&[("sent", true, true)]));
    case.open = vec![with_key("acked", NotificationState::Acknowledged)];
    assert_eq!(case.action(), resolves(&[("acked", true, true)]));
    case.open = vec![with_key("grouping", NotificationState::Grouping)];
    assert_eq!(case.action(), resolves(&[("grouping", true, false)]));
    case.open = vec![with_key("held", NotificationState::Held)];
    assert_eq!(case.action(), resolves(&[("held", true, false)]));

    case.shaping.notify_resolved = false;
    case.open = vec![with_key("sent", NotificationState::Sent)];
    assert_eq!(case.action(), resolves(&[("sent", true, false)]));
}

#[test]
fn a_resolved_event_with_nothing_open_is_dropped_under_dedup_and_sent_otherwise() {
    let mut case = Case::new(dedup(600));
    case.event.status = EventStatus::Resolved;
    case.event.dedup_key = Some("order-17".into());
    assert!(matches!(
        case.action(),
        Action::Suppress {
            silence_id: None,
            ..
        }
    ));
    case.shaping = Shaping::default();
    assert!(matches!(case.action(), Action::Send { .. }));
}

#[test]
fn the_first_event_opens_a_group_and_later_ones_join() {
    let mut case = Case::new(group(30));
    assert_eq!(
        case.action(),
        Action::OpenGroup {
            group_key: "service=pay".into(),
            dedup_key: None,
            flush_at: NOON + 30_000
        }
    );
    let mut grouping = open("g1", NotificationState::Grouping);
    grouping.group_key = Some("service=pay".into());
    let mut flushed = open("g0", NotificationState::Sent);
    flushed.group_key = Some("service=pay".into());
    case.open = vec![flushed, grouping];
    assert_eq!(
        case.action(),
        Action::Attach {
            notification_id: "g1".into(),
            role: Role::Grouped
        }
    );
    // Another value of the grouping label is another group.
    case.event.labels.insert("service".into(), "ship".into());
    assert!(
        matches!(case.action(), Action::OpenGroup { group_key, .. } if group_key == "service=ship")
    );
}

#[test]
fn quiet_hours_hold_until_they_end_also_across_midnight() {
    // 12:00 UTC is 14:00 in Warsaw (summer time).
    let hours = quiet("13:00", "15:30");
    assert_eq!(quiet_until(&hours, NOON).unwrap(), Some(NOON + 90 * 60_000));
    assert_eq!(quiet_until(&quiet("15:00", "16:00"), NOON).unwrap(), None);
    // 22:00 to 07:00: at 23:30 Warsaw (21:30 UTC) the end is 07:00 the next morning (05:00 UTC).
    let night = quiet("22:00", "07:00");
    let late = NOON + 9 * HOUR + 30 * 60_000;
    assert_eq!(quiet_until(&night, late).unwrap(), Some(NOON + 17 * HOUR));
    // At 03:00 Warsaw (01:00 UTC) the window that began the evening before still applies.
    assert_eq!(
        quiet_until(&night, NOON + 13 * HOUR).unwrap(),
        Some(NOON + 17 * HOUR)
    );
    assert_eq!(quiet_until(&night, NOON).unwrap(), None);
    // The same start and end is no period at all (and is refused when a route is saved).
    assert_eq!(quiet_until(&quiet("09:00", "09:00"), NOON).unwrap(), None);
}

#[test]
fn quiet_hours_apply_only_on_their_days() {
    let mut weekend = quiet("00:00", "23:59");
    weekend.days = vec!["sat".into(), "sun".into()];
    assert_eq!(quiet_until(&weekend, NOON).unwrap(), None);
    // Saturday 2026-10-17 12:00 UTC.
    assert!(
        quiet_until(&weekend, NOON + 4 * 24 * HOUR)
            .unwrap()
            .is_some()
    );
    // A night window belongs to the day it starts on: Friday night is not weekend quiet,
    // Sunday night into Monday is.
    let mut nights = quiet("22:00", "07:00");
    nights.days = vec!["sun".into()];
    let monday_3am_warsaw = NOON + 6 * 24 * HOUR - 11 * HOUR;
    assert!(quiet_until(&nights, monday_3am_warsaw).unwrap().is_some());
    assert_eq!(
        quiet_until(&nights, monday_3am_warsaw - 24 * HOUR).unwrap(),
        None
    );
}

#[test]
fn quiet_hours_end_at_the_wall_clock_time_when_the_clocks_change() {
    // Warsaw leaves summer time on 2026-10-25 at 03:00 (01:00 UTC). At 23:00 on the 24th
    // (21:00 UTC) a window ending 07:00 ends at 06:00 UTC: nine hours later, not eight.
    let night = quiet("22:00", "07:00");
    let saturday_23 = 1_792_875_600_000;
    assert_eq!(
        quiet_until(&night, saturday_23).unwrap(),
        Some(saturday_23 + 9 * HOUR)
    );
    assert!(quiet_until(&quiet("22:00", "07:00"), 0).is_ok());
    let mut bad = quiet("22:00", "07:00");
    bad.tz = "Mars/Olympus".into();
    assert!(quiet_until(&bad, NOON).is_err());
}

#[test]
fn an_event_in_quiet_hours_is_held_unless_it_is_severe_enough() {
    let mut case = Case::new(Shaping {
        quiet_hours: Some(QuietHours {
            bypass_severity: Some(Severity::Critical),
            ..quiet("13:00", "15:30")
        }),
        ..Shaping::default()
    });
    let Action::Hold {
        kind,
        until,
        existing,
        ..
    } = case.action()
    else {
        panic!("{:?}", case.action())
    };
    assert_eq!(
        (kind, until, existing),
        (HoldKind::Quiet, NOON + 90 * 60_000, None)
    );
    case.event.severity = Severity::Critical;
    assert!(matches!(case.action(), Action::Send { .. }));

    // A second quiet event joins what is already held for the target.
    case.event.severity = Severity::Error;
    let mut held = open("h1", NotificationState::Held);
    held.hold = Some(HoldKind::Quiet);
    case.open = vec![held];
    assert!(matches!(case.action(), Action::Hold { existing: Some(id), .. } if id == "h1"));
}

#[test]
fn over_the_rate_limit_events_are_held_until_the_window_ends() {
    let mut case = Case::new(Shaping {
        rate_limit: Some(RateLimit {
            max: 3,
            window_s: 600,
        }),
        ..Shaping::default()
    });
    case.sends = 2;
    assert!(matches!(case.action(), Action::Send { .. }));
    case.sends = 3;
    case.oldest = Some(NOON - 400_000);
    let Action::Hold {
        kind,
        until,
        reason,
        ..
    } = case.action()
    else {
        panic!()
    };
    assert_eq!((kind, until), (HoldKind::Rate, NOON + 200_000));
    assert!(reason.contains('3'), "{reason}");
}

#[test]
fn a_group_that_flushes_is_checked_against_quiet_hours_and_the_rate_limit() {
    let shaping = Shaping {
        quiet_hours: Some(quiet("13:00", "15:30")),
        rate_limit: Some(RateLimit {
            max: 1,
            window_s: 600,
        }),
        ..Shaping::default()
    };
    let input = |from, now, sends| ReleaseInput {
        shaping: &shaping,
        max_severity: Severity::Error,
        from,
        sends_in_window: sends,
        oldest_send_in_window: Some(now - 1000),
        now_ms: now,
    };
    assert!(matches!(
        release(&input(ReleaseFrom::Group, NOON, 0)),
        Released::Hold {
            kind: HoldKind::Quiet,
            ..
        }
    ));
    let after = NOON + 2 * HOUR;
    assert!(matches!(
        release(&input(ReleaseFrom::Group, after, 1)),
        Released::Hold {
            kind: HoldKind::Rate,
            ..
        }
    ));
    assert_eq!(
        release(&input(ReleaseFrom::Group, after, 0)),
        Released::Send
    );
    // What was held is the summary: it goes out when its hold ends, whatever the rate.
    assert_eq!(release(&input(ReleaseFrom::Hold, after, 5)), Released::Send);
    // Unless quiet hours began meanwhile.
    assert!(matches!(
        release(&input(ReleaseFrom::Hold, NOON, 5)),
        Released::Hold {
            kind: HoldKind::Quiet,
            ..
        }
    ));
}

#[test]
fn a_digest_summarises_several_events() {
    let one = digest(&[event("Order 17 failed")], Some("https://r.example"), "n1");
    assert_eq!(one.title, "Order 17 failed");
    assert_eq!(
        one.link.as_deref(),
        Some("https://r.example/events/e-Order 17 failed")
    );

    let same = digest(
        &[
            event("Order failed"),
            event("Order failed"),
            event("Order failed"),
        ],
        None,
        "n1",
    );
    assert_eq!(same.title, "Order failed (3 times)");

    let mut warning = event("Disk almost full");
    warning.severity = Severity::Warning;
    warning.topic = "infra.disk".into();
    warning.labels.insert("service".into(), "db".into());
    let mut critical = event("Database down");
    critical.severity = Severity::Critical;
    let mixed = digest(
        &[event("Order 17 failed"), warning, critical],
        Some("https://r.example"),
        "n1",
    );
    assert_eq!(mixed.title, "3 events");
    assert_eq!(mixed.severity, Severity::Critical);
    assert_eq!(mixed.fields, vec![("env".to_string(), "prod".to_string())]);
    let body = mixed.body.unwrap();
    assert!(
        body.contains("error: Order 17 failed") && body.contains("warning: Disk almost full"),
        "{body}"
    );
    assert_eq!(
        mixed.link.as_deref(),
        Some("https://r.example/notifications/n1")
    );
    assert_eq!(mixed.event_ids.len(), 3);

    let many: Vec<Event> = (0..25).map(|i| event(&format!("event {i}"))).collect();
    let long = digest(&many, None, "n1");
    assert_eq!(long.title, "25 events on orders.failed");
    assert!(long.body.unwrap().ends_with("and 15 more"));
}

#[test]
fn a_policy_is_validated() {
    assert!(Shaping::default().validate().is_ok());
    assert!(dedup(600).validate().is_ok());
    assert!(dedup(0).validate().unwrap_err().contains("dedup"));
    assert!(group(0).validate().unwrap_err().contains("group"));
    let bad_quiet = Shaping {
        quiet_hours: Some(quiet("25:00", "07:00")),
        ..Shaping::default()
    };
    assert!(bad_quiet.validate().unwrap_err().contains("quiet"));
    let mut bad_day = quiet("22:00", "07:00");
    bad_day.days = vec!["someday".into()];
    assert!(
        Shaping {
            quiet_hours: Some(bad_day),
            ..Shaping::default()
        }
        .validate()
        .is_err()
    );
    let bad_rate = Shaping {
        rate_limit: Some(RateLimit {
            max: 0,
            window_s: 60,
        }),
        ..Shaping::default()
    };
    assert!(bad_rate.validate().unwrap_err().contains("rate"));
    let bad_key = Shaping {
        dedup: Some(Dedup {
            key: Some("a ==".into()),
            window_s: 60,
        }),
        ..Shaping::default()
    };
    assert!(bad_key.validate().is_err());
    // An empty policy serialises to nothing but the default flag, and reads back from {}.
    assert_eq!(
        serde_json::from_str::<Shaping>("{}").unwrap(),
        Shaping::default()
    );
}

#[test]
fn a_resolution_closes_every_open_notification_with_the_key() {
    let mut case = Case::new(Shaping::default());
    case.event.status = EventStatus::Resolved;
    case.event.dedup_key = Some("db".into());
    case.open = vec![
        keyed("older", NotificationState::Sent),
        open("unrelated", NotificationState::Sent),
        keyed("newer", NotificationState::Sent),
    ];
    assert_eq!(
        case.action(),
        resolves(&[("older", true, true), ("newer", true, true)])
    );
}

#[test]
fn resolving_one_member_of_a_group_leaves_the_rest_alone() {
    let mut case = Case::new(group(30));
    case.event.status = EventStatus::Resolved;
    case.event.dedup_key = Some("db".into());
    let mut collecting = keyed("g1", NotificationState::Grouping);
    collecting.others_firing = true;
    case.open = vec![collecting.clone()];
    // The event is recorded against the group; the group lives on for its other events.
    assert_eq!(case.action(), resolves(&[("g1", false, false)]));
    // The same for a group that was already sent: nobody is told it is over while part of it fires.
    collecting.state = NotificationState::Sent;
    collecting.sent = true;
    case.open = vec![collecting];
    assert_eq!(case.action(), resolves(&[("g1", false, false)]));
}

#[test]
fn a_silence_does_not_swallow_the_resolution_of_something_already_sent() {
    let mut case = Case::new(Shaping::default());
    case.event.status = EventStatus::Resolved;
    case.event.dedup_key = Some("db".into());
    case.silences = vec![Silence {
        id: "s".into(),
        r#match: RouteMatch {
            topic: Some("orders.*".into()),
            ..RouteMatch::default()
        },
        starts_at: NOON - HOUR,
        ends_at: NOON + HOUR,
        comment: None,
    }];
    case.open = vec![keyed("sent", NotificationState::Sent)];
    assert_eq!(case.action(), resolves(&[("sent", true, true)]));
    // With nothing open the silence applies as usual.
    case.open = vec![];
    assert!(matches!(
        case.action(),
        Action::Suppress {
            silence_id: Some(_),
            ..
        }
    ));
}

#[test]
fn quiet_hours_with_no_length_are_refused() {
    let none = Shaping {
        quiet_hours: Some(quiet("09:00", "09:00")),
        ..Shaping::default()
    };
    assert!(none.validate().unwrap_err().contains("quiet"));
}

#[test]
fn a_hold_never_ends_in_the_past_when_the_clocks_go_back() {
    // 2026-10-25 01:10 UTC is 02:10 in Warsaw for the second time; the period ends 02:30.
    let night = quiet("22:00", "02:30");
    let now = 1_792_890_600_000;
    let until = quiet_until(&night, now).unwrap().unwrap();
    assert!(until > now, "{until} is not after {now}");
}
