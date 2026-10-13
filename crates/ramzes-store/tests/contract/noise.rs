use ramzes_core::shaping::{HoldKind, NotificationState, Role, Shaping, Silence};
use ramzes_core::{Event, EventStatus, Route, RouteMatch, Severity, Target};
use ramzes_store::{
    Connection, Join, NewDispatch, NewTimer, NotificationFilter, Store, StoreError,
};
use serde_json::json;

crate::contract!(
    a_route_keeps_its_shaping_policy,
    silences_are_listed_and_only_active_ones_apply,
    a_sent_notification_is_open_for_its_target_and_counts_as_a_send,
    a_grouping_notification_has_no_delivery_until_it_is_sent,
    joining_adds_the_event_and_raises_the_severity,
    resolving_closes_the_notification_and_cancels_its_timers,
    a_held_notification_is_listed_with_its_hold,
    merging_moves_events_and_removes_the_source,
    timers_are_claimed_once_and_can_be_cancelled,
    acknowledging_and_resolving_by_hand,
    notifications_are_listed_by_state_and_shown_in_detail,
    the_journey_says_what_role_an_event_played,
    a_connection_is_paused_after_repeated_failures_and_resumed,
    the_system_source_is_created_once,
    the_overview_counts_events_and_deliveries,
);

const TARGET: &str = "route-1|c1|";

fn sent(connection: &str) -> NewDispatch {
    NewDispatch {
        notification_id: ramzes_core::new_id(),
        delivery_id: Some(ramzes_core::new_id()),
        route_id: "route-1".into(),
        route_name: "Payments".into(),
        connection_id: connection.into(),
        target_key: format!("route-1|{connection}|"),
        ..NewDispatch::default()
    }
}

async fn stored_event(store: &Store, ws: &str, source: &str, title: &str, at: i64) -> Event {
    let mut e = crate::event(ws, source, "orders.failed", at);
    e.title = title.into();
    store
        .insert_events(std::slice::from_ref(&e), None)
        .await
        .unwrap();
    e
}

async fn open(store: &Store, ws: &str) -> Vec<ramzes_core::shaping::OpenNotification> {
    store.open_notifications(ws, TARGET, None).await.unwrap()
}

async fn a_route_keeps_its_shaping_policy(store: Store) {
    let ws = store.ensure_workspace(1).await.unwrap();
    let shaping: Shaping = serde_json::from_value(json!({
        "dedup": {"window_s": 600},
        "group": {"by": ["service"], "wait_s": 30, "repeat_s": 3600},
        "quiet_hours": {"tz": "Europe/Warsaw", "start": "22:00", "end": "07:00", "bypass_severity": "critical"},
        "rate_limit": {"max": 5, "window_s": 300},
        "notify_resolved": false
    }))
    .unwrap();
    let mut route = Route {
        id: ramzes_core::new_id(),
        name: "shaped".into(),
        enabled: true,
        r#match: RouteMatch::default(),
        targets: vec![Target {
            connection_id: "c1".into(),
            ..Target::default()
        }],
        shaping: shaping.clone(),
        ..Route::default()
    };
    store.create_route(&ws, &route, 1).await.unwrap();
    assert_eq!(store.list_routes(&ws).await.unwrap()[0].shaping, shaping);
    route.shaping = Shaping::default();
    store.update_route(&ws, &route, 2).await.unwrap();
    assert!(store.list_routes(&ws).await.unwrap()[0].shaping.is_empty());
    let config = store.read_config(&ws).await.unwrap();
    store.apply_config(&ws, &config, 3).await.unwrap();
    assert_eq!(store.list_routes(&ws).await.unwrap().len(), 1);
}

fn silence(topic: &str, starts_at: i64, ends_at: i64) -> Silence {
    Silence {
        id: ramzes_core::new_id(),
        r#match: RouteMatch {
            topic: Some(topic.into()),
            ..RouteMatch::default()
        },
        starts_at,
        ends_at,
        comment: Some("deploy".into()),
    }
}

async fn silences_are_listed_and_only_active_ones_apply(store: Store) {
    let ws = store.ensure_workspace(1).await.unwrap();
    let (past, now, future) = (
        silence("a.*", 0, 50),
        silence("b.*", 50, 200),
        silence("c.*", 150, 300),
    );
    for s in [&past, &now, &future] {
        store
            .create_silence(&ws, s, "kris@example.com", 10)
            .await
            .unwrap();
    }
    let all = store.list_silences(&ws).await.unwrap();
    assert_eq!(all.len(), 3);
    assert_eq!(all[0].silence.id, future.id);
    assert_eq!(
        (all[0].author.as_str(), all[0].created_at),
        ("kris@example.com", 10)
    );
    let active = store.active_silences(&ws, 100).await.unwrap();
    assert_eq!(active, vec![now.clone()]);

    store.end_silence(&ws, &now.id, 120).await.unwrap();
    assert!(store.active_silences(&ws, 130).await.unwrap().is_empty());
    assert_eq!(
        store
            .list_silences(&ws)
            .await
            .unwrap()
            .iter()
            .find(|s| s.silence.id == now.id)
            .unwrap()
            .silence
            .ends_at,
        120
    );
    // Ending one that is already over changes nothing.
    store.end_silence(&ws, &past.id, 120).await.unwrap();
    assert_eq!(
        store
            .list_silences(&ws)
            .await
            .unwrap()
            .iter()
            .find(|s| s.silence.id == past.id)
            .unwrap()
            .silence
            .ends_at,
        50
    );
    store.delete_silence(&ws, &future.id).await.unwrap();
    assert!(matches!(
        store.delete_silence(&ws, &future.id).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        store.end_silence(&ws, "nope", 1).await,
        Err(StoreError::NotFound)
    ));
}

async fn a_sent_notification_is_open_for_its_target_and_counts_as_a_send(store: Store) {
    let (ws, source) = crate::seed(&store).await;
    let e = stored_event(&store, &ws, &source, "first", 100).await;
    let mut d = sent("c1");
    d.dedup_key = Some("order-17".into());
    store
        .write_routing(&e, std::slice::from_ref(&d), &[], true, 100)
        .await
        .unwrap();
    let other = sent("c2");
    store
        .write_routing(&e, std::slice::from_ref(&other), &[], false, 100)
        .await
        .unwrap();

    // Without a key only notifications that are still collecting or held are returned.
    assert!(open(&store, &ws).await.is_empty());
    let found = store
        .open_notifications(&ws, TARGET, Some("order-17"))
        .await
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(
        (found[0].id.as_str(), found[0].state, found[0].sent),
        (d.notification_id.as_str(), NotificationState::Sent, true)
    );
    assert!(
        store
            .open_notifications(&ws, TARGET, Some("other"))
            .await
            .unwrap()
            .is_empty()
    );

    assert_eq!(
        store.sends_since(&ws, TARGET, 50).await.unwrap(),
        (1, Some(100))
    );
    assert_eq!(
        store.sends_since(&ws, TARGET, 101).await.unwrap(),
        (0, None)
    );
    assert_eq!(
        store
            .claim_due_deliveries(100, 1000, 10)
            .await
            .unwrap()
            .len(),
        2
    );
}

fn grouping(at: i64) -> NewDispatch {
    NewDispatch {
        delivery_id: None,
        state: NotificationState::Grouping,
        group_key: Some("service=pay".into()),
        timer: Some(NewTimer {
            kind: "flush".into(),
            due_at: at + 30_000,
            data: None,
        }),
        ..sent("c1")
    }
}

async fn a_grouping_notification_has_no_delivery_until_it_is_sent(store: Store) {
    let (ws, source) = crate::seed(&store).await;
    let e = stored_event(&store, &ws, &source, "first", 100).await;
    let d = grouping(100);
    store
        .write_routing(&e, std::slice::from_ref(&d), &[], true, 100)
        .await
        .unwrap();
    assert!(
        store
            .claim_due_deliveries(100, 1000, 10)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(store.sends_since(&ws, TARGET, 0).await.unwrap(), (0, None));
    let found = open(&store, &ws).await;
    assert_eq!(
        (found[0].state, found[0].group_key.as_deref(), found[0].sent),
        (NotificationState::Grouping, Some("service=pay"), false)
    );

    let delivery_id = ramzes_core::new_id();
    store
        .send_notification(&d.notification_id, &delivery_id, &[], 200)
        .await
        .unwrap();
    let claimed = store.claim_due_deliveries(200, 1000, 10).await.unwrap();
    assert_eq!(claimed.len(), 1);
    assert_eq!(
        (claimed[0].id.as_str(), claimed[0].connection_id.as_str()),
        (delivery_id.as_str(), "c1")
    );
    assert_eq!(
        store.sends_since(&ws, TARGET, 0).await.unwrap(),
        (1, Some(200))
    );
    let core = store
        .notification_core(&d.notification_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((core.state, core.sent_count), (NotificationState::Sent, 1));
    assert!(store.notification_core("nope").await.unwrap().is_none());
}

async fn joining_adds_the_event_and_raises_the_severity(store: Store) {
    let (ws, source) = crate::seed(&store).await;
    let first = stored_event(&store, &ws, &source, "first", 100).await;
    let d = grouping(100);
    store
        .write_routing(&first, std::slice::from_ref(&d), &[], true, 100)
        .await
        .unwrap();
    let mut second = crate::event(&ws, &source, "orders.failed", 110);
    second.severity = Severity::Critical;
    store
        .insert_events(std::slice::from_ref(&second), None)
        .await
        .unwrap();
    let join = Join {
        notification_id: d.notification_id.clone(),
        role: Role::Grouped,
        resolve: None,
    };
    store
        .write_routing(&second, &[], std::slice::from_ref(&join), true, 110)
        .await
        .unwrap();

    let events = store.notification_events(&d.notification_id).await.unwrap();
    assert_eq!(events.len(), 2);
    let core = store
        .notification_core(&d.notification_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((core.event_count, core.severity), (2, Severity::Critical));
    // Joining twice (a retried routing pass) does not count the event twice.
    store
        .write_routing(&second, &[], std::slice::from_ref(&join), true, 111)
        .await
        .unwrap();
    assert_eq!(
        store
            .notification_core(&d.notification_id)
            .await
            .unwrap()
            .unwrap()
            .event_count,
        2
    );
    let journey = store.journey(&ws, &second.id).await.unwrap().unwrap();
    assert_eq!(journey.route_count, 1);
}

async fn resolving_closes_the_notification_and_cancels_its_timers(store: Store) {
    let (ws, source) = crate::seed(&store).await;
    let first = stored_event(&store, &ws, &source, "down", 100).await;
    let mut d = sent("c1");
    d.dedup_key = Some("db".into());
    d.timer = Some(NewTimer {
        kind: "repeat".into(),
        due_at: 5000,
        data: None,
    });
    store
        .write_routing(&first, std::slice::from_ref(&d), &[], true, 100)
        .await
        .unwrap();
    store.claim_due_deliveries(100, 1, 10).await.unwrap();

    let mut up = crate::event(&ws, &source, "orders.failed", 300);
    up.status = EventStatus::Resolved;
    store
        .insert_events(std::slice::from_ref(&up), None)
        .await
        .unwrap();
    let notice = ramzes_core::new_id();
    let join = Join {
        notification_id: d.notification_id.clone(),
        role: Role::Resolved,
        resolve: Some(vec![ramzes_store::NewDelivery {
            id: notice.clone(),
            connection_id: "c1".into(),
            destination: None,
            template_id: None,
            step: None,
        }]),
    };
    store
        .write_routing(&up, &[], std::slice::from_ref(&join), true, 300)
        .await
        .unwrap();

    let core = store
        .notification_core(&d.notification_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(core.state, NotificationState::Resolved);
    assert!(
        store
            .open_notifications(&ws, TARGET, Some("db"))
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .claim_due_timers(10_000, 1000, 10)
            .await
            .unwrap()
            .is_empty()
    );
    let due: Vec<String> = store
        .claim_due_deliveries(10_000, 1000, 10)
        .await
        .unwrap()
        .into_iter()
        .map(|d| d.id)
        .collect();
    assert!(due.contains(&notice));
}

async fn a_held_notification_is_listed_with_its_hold(store: Store) {
    let (ws, source) = crate::seed(&store).await;
    let e = stored_event(&store, &ws, &source, "night", 100).await;
    let d = NewDispatch {
        delivery_id: None,
        state: NotificationState::Held,
        reason: Some("quiet hours until 07:00".into()),
        hold: Some((HoldKind::Quiet, 9000)),
        timer: Some(NewTimer {
            kind: "release".into(),
            due_at: 9000,
            data: None,
        }),
        ..sent("c1")
    };
    store
        .write_routing(&e, std::slice::from_ref(&d), &[], true, 100)
        .await
        .unwrap();
    let found = open(&store, &ws).await;
    assert_eq!(
        (found[0].state, found[0].hold),
        (NotificationState::Held, Some(HoldKind::Quiet))
    );

    // A group that flushes into quiet hours becomes held.
    let g = grouping(100);
    store
        .write_routing(&e, std::slice::from_ref(&g), &[], false, 100)
        .await
        .unwrap();
    store
        .hold_notification(&g.notification_id, HoldKind::Rate, 7000, "rate limit", 150)
        .await
        .unwrap();
    let core = store
        .notification_core(&g.notification_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (core.state, core.hold_kind, core.held_until),
        (NotificationState::Held, Some(HoldKind::Rate), Some(7000))
    );
    let timers = store.claim_due_timers(7000, 1000, 10).await.unwrap();
    assert_eq!(timers.len(), 1);
    assert_eq!(
        (timers[0].kind.as_str(), timers[0].notification_id.as_str()),
        ("release", g.notification_id.as_str())
    );
}

async fn merging_moves_events_and_removes_the_source(store: Store) {
    let (ws, source) = crate::seed(&store).await;
    let (a, b, c) = (
        stored_event(&store, &ws, &source, "a", 100).await,
        stored_event(&store, &ws, &source, "b", 110).await,
        stored_event(&store, &ws, &source, "c", 120).await,
    );
    let held = NewDispatch {
        delivery_id: None,
        state: NotificationState::Held,
        hold: Some((HoldKind::Quiet, 9000)),
        ..sent("c1")
    };
    store
        .write_routing(&a, std::slice::from_ref(&held), &[], true, 100)
        .await
        .unwrap();
    let group = grouping(110);
    store
        .write_routing(&b, std::slice::from_ref(&group), &[], true, 110)
        .await
        .unwrap();
    let join = Join {
        notification_id: group.notification_id.clone(),
        role: Role::Grouped,
        resolve: None,
    };
    store
        .write_routing(&c, &[], std::slice::from_ref(&join), true, 120)
        .await
        .unwrap();
    // Event a is also in the group already, so it must not be counted twice after the merge.
    store
        .write_routing(&a, &[], std::slice::from_ref(&join), false, 121)
        .await
        .unwrap();

    store
        .merge_notifications(&group.notification_id, &held.notification_id, 130)
        .await
        .unwrap();
    assert!(
        store
            .notification_core(&group.notification_id)
            .await
            .unwrap()
            .is_none()
    );
    let merged = store
        .notification_core(&held.notification_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(merged.event_count, 3);
    assert_eq!(
        store
            .notification_events(&held.notification_id)
            .await
            .unwrap()
            .len(),
        3
    );
    assert!(
        store
            .claim_due_timers(1_000_000, 1000, 10)
            .await
            .unwrap()
            .is_empty()
    );
}

async fn timers_are_claimed_once_and_can_be_cancelled(store: Store) {
    let ws = store.ensure_workspace(1).await.unwrap();
    store
        .add_timer(&ws, "n1", "flush", 100, None)
        .await
        .unwrap();
    store
        .add_timer(&ws, "n1", "repeat", 100, Some("2"))
        .await
        .unwrap();
    store
        .add_timer(&ws, "n2", "flush", 500, None)
        .await
        .unwrap();
    assert!(
        store
            .claim_due_timers(99, 1000, 10)
            .await
            .unwrap()
            .is_empty()
    );
    let due = store.claim_due_timers(100, 1000, 10).await.unwrap();
    assert_eq!(due.len(), 2);
    assert_eq!(
        due.iter()
            .find(|t| t.kind == "repeat")
            .unwrap()
            .data
            .as_deref(),
        Some("2")
    );
    // Leased: not handed out again until the lease runs out.
    assert!(
        store
            .claim_due_timers(200, 1000, 10)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store.claim_due_timers(1200, 1000, 10).await.unwrap().len(),
        3
    );
    store.delete_timer(&due[0].id).await.unwrap();
    store.cancel_timers("n1", Some("repeat")).await.unwrap();
    store.cancel_timers("n2", None).await.unwrap();
    let left = store.claim_due_timers(10_000, 1000, 10).await.unwrap();
    assert_eq!(left.len(), if due[0].kind == "flush" { 0 } else { 1 });
    assert_eq!(
        store.next_timer("n1").await.unwrap().is_some(),
        !left.is_empty()
    );
}

async fn acknowledging_and_resolving_by_hand(store: Store) {
    let (ws, source) = crate::seed(&store).await;
    let e = stored_event(&store, &ws, &source, "down", 100).await;
    let mut d = sent("c1");
    d.timer = Some(NewTimer {
        kind: "repeat".into(),
        due_at: 5000,
        data: None,
    });
    store
        .write_routing(&e, std::slice::from_ref(&d), &[], true, 100)
        .await
        .unwrap();
    store
        .acknowledge(&ws, &d.notification_id, "kris@example.com", 200)
        .await
        .unwrap();
    let row = &store
        .list_notifications(&ws, &NotificationFilter::default())
        .await
        .unwrap()[0];
    assert_eq!(
        (row.state, row.acked_by.as_deref(), row.acked_at),
        (
            NotificationState::Acknowledged,
            Some("kris@example.com"),
            Some(200)
        )
    );
    assert!(
        store
            .claim_due_timers(10_000, 1000, 10)
            .await
            .unwrap()
            .is_empty()
    );
    // Acknowledging again is a conflict; so is acknowledging something that is over.
    assert!(matches!(
        store.acknowledge(&ws, &d.notification_id, "x", 201).await,
        Err(StoreError::Conflict(_))
    ));
    store
        .resolve_notification(&ws, &d.notification_id, 300)
        .await
        .unwrap();
    let row = &store
        .list_notifications(&ws, &NotificationFilter::default())
        .await
        .unwrap()[0];
    assert_eq!(
        (row.state, row.resolved_at),
        (NotificationState::Resolved, Some(300))
    );
    assert!(matches!(
        store
            .resolve_notification(&ws, &d.notification_id, 301)
            .await,
        Err(StoreError::Conflict(_))
    ));
    assert!(matches!(
        store.acknowledge(&ws, &d.notification_id, "x", 302).await,
        Err(StoreError::Conflict(_))
    ));
    assert!(matches!(
        store.acknowledge(&ws, "nope", "x", 302).await,
        Err(StoreError::NotFound)
    ));
}

fn connection(ws: &str, id: &str) -> Connection {
    Connection {
        id: id.into(),
        workspace_id: ws.into(),
        kind: "webhook".into(),
        name: format!("hook {id}"),
        settings: json!({}),
        credentials: "x".into(),
        created_at: 1,
        updated_at: 1,
    }
}

async fn notifications_are_listed_by_state_and_shown_in_detail(store: Store) {
    let (ws, source) = crate::seed(&store).await;
    store
        .create_connection(&connection(&ws, "c1"))
        .await
        .unwrap();
    let e = stored_event(&store, &ws, &source, "Order 17 failed", 100).await;
    let first = sent("c1");
    let group = grouping(100);
    let silenced = NewDispatch {
        delivery_id: None,
        state: NotificationState::Suppressed,
        reason: Some("silenced: deploy".into()),
        silence_id: Some("s1".into()),
        ..sent("gone")
    };
    store
        .write_routing(
            &e,
            &[first.clone(), group.clone(), silenced.clone()],
            &[],
            true,
            100,
        )
        .await
        .unwrap();

    let all = store
        .list_notifications(&ws, &NotificationFilter::default())
        .await
        .unwrap();
    assert_eq!(all.len(), 3);
    assert_eq!(all[0].id, silenced.notification_id);
    assert_eq!(
        (all[0].connection_name.as_deref(), all[0].reason.as_deref()),
        (None, Some("silenced: deploy"))
    );
    let row = all.iter().find(|n| n.id == group.notification_id).unwrap();
    assert_eq!(
        (
            row.title.as_str(),
            row.connection_name.as_deref(),
            row.event_count
        ),
        ("Order 17 failed", Some("hook c1"), 1)
    );
    assert_eq!(row.next_timer_at, Some(30_100));

    let filter = NotificationFilter {
        states: vec![NotificationState::Grouping, NotificationState::Held],
        ..NotificationFilter::default()
    };
    let waiting = store.list_notifications(&ws, &filter).await.unwrap();
    assert_eq!(waiting.len(), 1);
    let page = NotificationFilter {
        limit: 1,
        before_id: Some(all[0].id.clone()),
        ..NotificationFilter::default()
    };
    assert_eq!(
        store.list_notifications(&ws, &page).await.unwrap()[0].id,
        all[1].id
    );

    let detail = store
        .get_notification(&ws, &first.notification_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(detail.events.len(), 1);
    assert_eq!(
        (
            detail.events[0].event.id.as_str(),
            detail.events[0].role.as_str()
        ),
        (e.id.as_str(), "first")
    );
    assert_eq!(detail.deliveries.len(), 1);
    assert!(store.get_notification(&ws, "nope").await.unwrap().is_none());
}

async fn the_journey_says_what_role_an_event_played(store: Store) {
    let (ws, source) = crate::seed(&store).await;
    let first = stored_event(&store, &ws, &source, "first", 100).await;
    let d = sent("c1");
    store
        .write_routing(&first, std::slice::from_ref(&d), &[], true, 100)
        .await
        .unwrap();
    let dup = stored_event(&store, &ws, &source, "dup", 110).await;
    let join = Join {
        notification_id: d.notification_id.clone(),
        role: Role::Duplicate,
        resolve: None,
    };
    store
        .write_routing(&dup, &[], std::slice::from_ref(&join), true, 110)
        .await
        .unwrap();
    let journey = store.journey(&ws, &dup.id).await.unwrap().unwrap();
    assert_eq!(journey.notifications[0].role, "duplicate");
    assert_eq!(journey.notifications[0].event_count, 2);
    let journey = store.journey(&ws, &first.id).await.unwrap().unwrap();
    assert_eq!(journey.notifications[0].role, "first");
}

async fn a_connection_is_paused_after_repeated_failures_and_resumed(store: Store) {
    let (ws, source) = crate::seed(&store).await;
    store
        .create_connection(&connection(&ws, "c1"))
        .await
        .unwrap();
    assert_eq!(
        store.record_connection_result("c1", false).await.unwrap(),
        1
    );
    assert_eq!(
        store.record_connection_result("c1", false).await.unwrap(),
        2
    );
    assert_eq!(store.record_connection_result("c1", true).await.unwrap(), 0);
    assert_eq!(
        store.record_connection_result("c1", false).await.unwrap(),
        1
    );

    let e = stored_event(&store, &ws, &source, "x", 100).await;
    store
        .write_routing(&e, &[sent("c1"), sent("c2")], &[], true, 100)
        .await
        .unwrap();
    store.pause_connection("c1", 5000).await.unwrap();
    let status = store.connection_status(&ws).await.unwrap();
    assert_eq!(status["c1"].paused_until, Some(5000));
    assert_eq!(status["c1"].failure_streak, 1);
    // Its waiting deliveries are put off until the pause ends; others are untouched.
    let due: Vec<String> = store
        .claim_due_deliveries(200, 100_000, 10)
        .await
        .unwrap()
        .into_iter()
        .map(|d| d.connection_id)
        .collect();
    assert_eq!(due, vec!["c2"]);
    assert!(
        store
            .claim_due_deliveries(4999, 100_000, 10)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store
            .claim_due_deliveries(5000, 100_000, 10)
            .await
            .unwrap()
            .len(),
        1
    );

    store.resume_connection(&ws, "c1").await.unwrap();
    let status = store.connection_status(&ws).await.unwrap();
    assert_eq!(
        (status["c1"].paused_until, status["c1"].failure_streak),
        (None, 0)
    );
    assert!(matches!(
        store.resume_connection(&ws, "nope").await,
        Err(StoreError::NotFound)
    ));
}

async fn the_system_source_is_created_once(store: Store) {
    let ws = store.ensure_workspace(1).await.unwrap();
    let first = store.system_source(&ws, 10).await.unwrap();
    let again = store.system_source(&ws, 20).await.unwrap();
    assert_eq!(first.id, again.id);
    assert_eq!(
        (first.kind.as_str(), first.name.as_str()),
        ("system", "ramzes")
    );
    assert_eq!(store.list_sources(&ws).await.unwrap().len(), 1);
}

async fn the_overview_counts_events_and_deliveries(store: Store) {
    let (ws, source) = crate::seed(&store).await;
    store
        .create_connection(&connection(&ws, "c1"))
        .await
        .unwrap();
    let hour = 3_600_000;
    let old = stored_event(&store, &ws, &source, "old", hour).await;
    let a = stored_event(&store, &ws, &source, "a", 10 * hour + 5).await;
    let _b = stored_event(&store, &ws, &source, "b", 10 * hour + 6).await;
    let c = stored_event(&store, &ws, &source, "c", 11 * hour + 1).await;
    store
        .write_routing(&old, &[sent("c1")], &[], true, hour)
        .await
        .unwrap();
    store
        .write_routing(&a, &[sent("c1")], &[], true, 10 * hour + 5)
        .await
        .unwrap();
    store
        .write_routing(&c, &[grouping(11 * hour + 1)], &[], true, 11 * hour + 1)
        .await
        .unwrap();

    let overview = store.overview(&ws, 9 * hour, hour).await.unwrap();
    assert_eq!(overview.events, 3);
    assert_eq!(overview.buckets, vec![(10 * hour, 2), (11 * hour, 1)]);
    assert_eq!(
        (
            overview.deliveries_pending,
            overview.deliveries_delivered,
            overview.deliveries_failed
        ),
        (1, 0, 0)
    );
    assert_eq!(overview.sources, vec![("orders-api".to_string(), 3)]);
    assert_eq!(overview.topics, vec![("orders.failed".to_string(), 3)]);
    assert_eq!(overview.waiting, 1);
    assert_eq!(overview.unacknowledged, 2);
}
