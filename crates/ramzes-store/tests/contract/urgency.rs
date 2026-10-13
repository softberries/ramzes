use ramzes_core::shaping::Role;
use ramzes_core::{EscalationStep, Route, RouteMatch, Target};
use ramzes_store::{Attempt, Finish, Join, NewDelivery, NewDispatch, NewTimer, Store};

crate::contract!(
    a_route_keeps_its_escalation_chain,
    an_escalation_step_adds_deliveries_to_other_connections,
    resolving_tells_the_listed_targets,
    the_first_reference_of_a_place_is_remembered,
    retention_leaves_events_that_still_have_work_waiting,
    a_failure_that_was_sent_again_is_no_longer_listed,
    open_notifications_are_found_by_the_key_of_any_member,
    a_message_leaves_out_members_that_are_already_resolved,
);

async fn a_route_keeps_its_escalation_chain(store: Store) {
    let ws = store.ensure_workspace(1).await.unwrap();
    let chain = vec![EscalationStep {
        after_s: 300,
        targets: vec![Target {
            connection_id: "c2".into(),
            destination: Some("oncall".into()),
            template_id: None,
        }],
    }];
    let mut route = Route {
        id: ramzes_core::new_id(),
        name: "paged".into(),
        enabled: true,
        r#match: RouteMatch::default(),
        targets: vec![Target {
            connection_id: "c1".into(),
            ..Target::default()
        }],
        escalation: chain.clone(),
        ..Route::default()
    };
    store.create_route(&ws, &route, 1).await.unwrap();
    assert_eq!(store.list_routes(&ws).await.unwrap()[0].escalation, chain);
    let config = store.read_config(&ws).await.unwrap();
    store.apply_config(&ws, &config, 2).await.unwrap();
    assert_eq!(store.list_routes(&ws).await.unwrap()[0].escalation, chain);
    route.escalation.clear();
    store.update_route(&ws, &route, 3).await.unwrap();
    assert!(
        store.list_routes(&ws).await.unwrap()[0]
            .escalation
            .is_empty()
    );
}

async fn sent(store: &Store, ws: &str, source: &str) -> NewDispatch {
    let e = crate::event(ws, source, "orders.failed", 100);
    store
        .insert_events(std::slice::from_ref(&e), None)
        .await
        .unwrap();
    let d = NewDispatch {
        notification_id: ramzes_core::new_id(),
        delivery_id: Some(ramzes_core::new_id()),
        route_id: "r".into(),
        route_name: "r".into(),
        connection_id: "c1".into(),
        target_key: "r|c1|".into(),
        ..NewDispatch::default()
    };
    store
        .write_routing(&e, std::slice::from_ref(&d), &[], true, 100)
        .await
        .unwrap();
    d
}

fn to(connection: &str, destination: Option<&str>, step: Option<i64>) -> NewDelivery {
    NewDelivery {
        id: ramzes_core::new_id(),
        connection_id: connection.into(),
        destination: destination.map(str::to_owned),
        template_id: Some("t1".into()),
        step,
    }
}

async fn an_escalation_step_adds_deliveries_to_other_connections(store: Store) {
    let (ws, source) = crate::seed(&store).await;
    let d = sent(&store, &ws, &source).await;
    store.claim_due_deliveries(100, 100_000, 10).await.unwrap();
    let next = NewTimer {
        kind: "escalate".into(),
        due_at: 900,
        data: Some("1".into()),
    };
    let step = [to("c2", Some("oncall"), Some(0)), to("c3", None, Some(0))];
    store
        .add_deliveries(&d.notification_id, &step, std::slice::from_ref(&next), 500)
        .await
        .unwrap();

    let due = store.claim_due_deliveries(500, 100_000, 10).await.unwrap();
    assert_eq!(due.len(), 2);
    let oncall = due.iter().find(|x| x.connection_id == "c2").unwrap();
    assert_eq!(
        (
            oncall.destination.as_deref(),
            oncall.step,
            oncall.template_id.as_deref()
        ),
        (Some("oncall"), Some(0), Some("t1"))
    );
    let timers = store.claim_due_timers(900, 1000, 10).await.unwrap();
    assert_eq!(
        (timers[0].kind.as_str(), timers[0].data.as_deref()),
        ("escalate", Some("1"))
    );
    let mut escalated = store.escalated_targets(&d.notification_id).await.unwrap();
    escalated.sort();
    assert_eq!(
        escalated,
        vec![
            (
                "c2".to_string(),
                Some("oncall".to_string()),
                Some("t1".to_string())
            ),
            ("c3".to_string(), None, Some("t1".to_string()))
        ]
    );
    // Escalating does not count as another send of the notification itself.
    assert_eq!(
        store
            .notification_core(&d.notification_id)
            .await
            .unwrap()
            .unwrap()
            .sent_count,
        1
    );
    let detail = store
        .get_notification(&ws, &d.notification_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(detail.deliveries.len(), 3);
}

async fn resolving_tells_the_listed_targets(store: Store) {
    let (ws, source) = crate::seed(&store).await;
    let d = sent(&store, &ws, &source).await;
    store.claim_due_deliveries(100, 100_000, 10).await.unwrap();
    let up = crate::event(&ws, &source, "orders.failed", 300);
    store
        .insert_events(std::slice::from_ref(&up), None)
        .await
        .unwrap();
    let join = Join {
        notification_id: d.notification_id.clone(),
        role: Role::Resolved,
        dedup_key: None,
        resolve: Some(vec![
            to("c1", None, None),
            to("c2", Some("oncall"), Some(0)),
        ]),
    };
    store
        .write_routing(&up, &[], std::slice::from_ref(&join), true, 300)
        .await
        .unwrap();
    let mut told: Vec<String> = store
        .claim_due_deliveries(300, 100_000, 10)
        .await
        .unwrap()
        .into_iter()
        .map(|x| x.connection_id)
        .collect();
    told.sort();
    assert_eq!(told, vec!["c1", "c2"]);
}

async fn the_first_reference_of_a_place_is_remembered(store: Store) {
    let (ws, source) = crate::seed(&store).await;
    let d = sent(&store, &ws, &source).await;
    assert_eq!(
        store
            .first_external_ref(&d.notification_id, "c1", None)
            .await
            .unwrap(),
        None
    );
    let finish = |reference: &str| Finish::Delivered {
        external_ref: Some(reference.to_owned()),
    };
    let attempt = |delivery_id: &str| Attempt {
        id: ramzes_core::new_id(),
        delivery_id: delivery_id.to_owned(),
        started_at: 100,
        duration_ms: 1,
        outcome: "delivered".into(),
        status: Some(200),
        detail: None,
    };
    let first = store
        .claim_due_deliveries(100, 100_000, 10)
        .await
        .unwrap()
        .remove(0);
    store
        .finish_attempt(&first, &attempt(&first.id), &finish("C1:111.1"), 100)
        .await
        .unwrap();
    store
        .send_notification(&d.notification_id, &ramzes_core::new_id(), &[], 200)
        .await
        .unwrap();
    let second = store
        .claim_due_deliveries(200, 100_000, 10)
        .await
        .unwrap()
        .remove(0);
    store
        .finish_attempt(&second, &attempt(&second.id), &finish("C1:222.2"), 200)
        .await
        .unwrap();

    assert_eq!(
        store
            .first_external_ref(&d.notification_id, "c1", None)
            .await
            .unwrap()
            .as_deref(),
        Some("C1:111.1")
    );
    assert_eq!(
        store
            .first_external_ref(&d.notification_id, "c1", Some("other"))
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        store
            .first_external_ref(&d.notification_id, "c9", None)
            .await
            .unwrap(),
        None
    );
    let refs = store.message_refs(&d.notification_id).await.unwrap();
    assert_eq!(refs.len(), 2);
    assert_eq!(refs[0], ("c1".to_string(), None, "C1:111.1".to_string()));
}

async fn retention_leaves_events_that_still_have_work_waiting(store: Store) {
    use ramzes_core::shaping::NotificationState;
    let (ws, source) = crate::seed(&store).await;
    let event = |title: &str| {
        let mut e = crate::event(&ws, &source, "orders.failed", 100);
        e.title = title.into();
        e
    };
    let (done, retrying, grouping, unrouted) = (
        event("done"),
        event("retrying"),
        event("grouping"),
        event("unrouted"),
    );
    store
        .insert_events(
            &[
                done.clone(),
                retrying.clone(),
                grouping.clone(),
                unrouted.clone(),
            ],
            None,
        )
        .await
        .unwrap();
    let dispatch = |state, delivery: bool| NewDispatch {
        notification_id: ramzes_core::new_id(),
        delivery_id: delivery.then(ramzes_core::new_id),
        route_id: "r".into(),
        route_name: "r".into(),
        connection_id: "c1".into(),
        target_key: "r|c1|".into(),
        state,
        ..NewDispatch::default()
    };
    store
        .write_routing(
            &done,
            &[dispatch(NotificationState::Sent, true)],
            &[],
            true,
            100,
        )
        .await
        .unwrap();
    let claimed = store
        .claim_due_deliveries(100, 100_000, 10)
        .await
        .unwrap()
        .remove(0);
    let attempt = Attempt {
        id: ramzes_core::new_id(),
        delivery_id: claimed.id.clone(),
        started_at: 100,
        duration_ms: 1,
        outcome: "delivered".into(),
        status: Some(200),
        detail: None,
    };
    store
        .finish_attempt(
            &claimed,
            &attempt,
            &Finish::Delivered { external_ref: None },
            100,
        )
        .await
        .unwrap();
    store
        .write_routing(
            &retrying,
            &[dispatch(NotificationState::Sent, true)],
            &[],
            true,
            100,
        )
        .await
        .unwrap();
    store
        .write_routing(
            &grouping,
            &[dispatch(NotificationState::Grouping, false)],
            &[],
            true,
            100,
        )
        .await
        .unwrap();

    let deleted = store
        .delete_events_before(&ws, 1_000_000, None, &[], 100)
        .await
        .unwrap();
    assert_eq!(deleted, 2);
    let filter = ramzes_store::EventFilter {
        limit: 10,
        ..Default::default()
    };
    let mut left: Vec<String> = store
        .list_events(&ws, &filter)
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.event.title)
        .collect();
    left.sort();
    assert_eq!(left, vec!["grouping", "retrying"]);
}

async fn a_failure_that_was_sent_again_is_no_longer_listed(store: Store) {
    let (ws, source) = crate::seed(&store).await;
    let first = sent(&store, &ws, &source).await;
    let second = sent(&store, &ws, &source).await;
    for delivery in store.claim_due_deliveries(100, 100_000, 10).await.unwrap() {
        let attempt = Attempt {
            id: ramzes_core::new_id(),
            delivery_id: delivery.id.clone(),
            started_at: 100,
            duration_ms: 1,
            outcome: "failed".into(),
            status: Some(400),
            detail: None,
        };
        store
            .finish_attempt(
                &delivery,
                &attempt,
                &Finish::Failed {
                    error: "HTTP 400".into(),
                },
                100,
            )
            .await
            .unwrap();
    }
    assert_eq!(store.list_failures(&ws, 10).await.unwrap().len(), 2);
    store
        .requeue_delivery(
            &ws,
            first.delivery_id.as_deref().unwrap(),
            &ramzes_core::new_id(),
            200,
        )
        .await
        .unwrap();
    let listed = store.list_failures(&ws, 10).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].delivery.id, second.delivery_id.unwrap());
}

async fn keyed_event(
    store: &Store,
    ws: &str,
    source: &str,
    title: &str,
    resolved: bool,
) -> ramzes_core::Event {
    let mut e = crate::event(ws, source, "orders.failed", 100);
    e.title = title.into();
    if resolved {
        e.status = ramzes_core::EventStatus::Resolved;
    }
    store
        .insert_events(std::slice::from_ref(&e), None)
        .await
        .unwrap();
    e
}

/// A grouping notification with members keyed `db` and `disk`.
async fn group_of_two(store: &Store, ws: &str, source: &str) -> String {
    use ramzes_core::shaping::NotificationState;
    let db = keyed_event(store, ws, source, "Database down", false).await;
    let d = NewDispatch {
        notification_id: ramzes_core::new_id(),
        route_id: "r".into(),
        route_name: "r".into(),
        connection_id: "c1".into(),
        target_key: "r|c1|".into(),
        state: NotificationState::Grouping,
        dedup_key: Some("db".into()),
        ..NewDispatch::default()
    };
    store
        .write_routing(&db, std::slice::from_ref(&d), &[], true, 100)
        .await
        .unwrap();
    let disk = keyed_event(store, ws, source, "Disk full", false).await;
    let join = Join {
        notification_id: d.notification_id.clone(),
        role: Role::Grouped,
        dedup_key: Some("disk".into()),
        resolve: None,
    };
    store
        .write_routing(&disk, &[], std::slice::from_ref(&join), true, 110)
        .await
        .unwrap();
    d.notification_id
}

async fn open_notifications_are_found_by_the_key_of_any_member(store: Store) {
    let (ws, source) = crate::seed(&store).await;
    let id = group_of_two(&store, &ws, &source).await;
    for key in ["db", "disk"] {
        let found = store
            .open_notifications(&ws, "r|c1|", Some(key))
            .await
            .unwrap();
        assert_eq!(
            (
                found[0].id.as_str(),
                found[0].has_key,
                found[0].others_firing
            ),
            (id.as_str(), true, true),
            "{key}"
        );
    }
    let other = store
        .open_notifications(&ws, "r|c1|", Some("other"))
        .await
        .unwrap();
    assert_eq!((other.len(), other[0].has_key), (1, false));

    // Once `db` is resolved, `disk` is the only thing still firing in the group.
    let up = keyed_event(&store, &ws, &source, "Database up", true).await;
    let partial = Join {
        notification_id: id.clone(),
        role: Role::Resolved,
        dedup_key: Some("db".into()),
        resolve: None,
    };
    store
        .write_routing(&up, &[], std::slice::from_ref(&partial), true, 120)
        .await
        .unwrap();
    let found = store
        .open_notifications(&ws, "r|c1|", Some("disk"))
        .await
        .unwrap();
    assert_eq!((found[0].has_key, found[0].others_firing), (true, false));
    // And `db` no longer has a firing member to resolve or to deduplicate against.
    assert!(
        !store
            .open_notifications(&ws, "r|c1|", Some("db"))
            .await
            .unwrap()[0]
            .has_key
    );
}

async fn a_message_leaves_out_members_that_are_already_resolved(store: Store) {
    use ramzes_core::shaping::{HoldKind, NotificationState};
    let (ws, source) = crate::seed(&store).await;
    let id = group_of_two(&store, &ws, &source).await;
    let titles =
        |events: Vec<ramzes_core::Event>| events.into_iter().map(|e| e.title).collect::<Vec<_>>();
    assert_eq!(
        titles(store.sendable_events(&id).await.unwrap()),
        vec!["Database down", "Disk full"]
    );
    let up = keyed_event(&store, &ws, &source, "Database up", true).await;
    let partial = Join {
        notification_id: id.clone(),
        role: Role::Resolved,
        dedup_key: Some("db".into()),
        resolve: None,
    };
    store
        .write_routing(&up, &[], std::slice::from_ref(&partial), true, 120)
        .await
        .unwrap();
    assert_eq!(
        titles(store.sendable_events(&id).await.unwrap()),
        vec!["Disk full"]
    );

    // Merging into a held digest keeps who resolved what.
    let held = NewDispatch {
        notification_id: ramzes_core::new_id(),
        route_id: "r".into(),
        route_name: "r".into(),
        connection_id: "c1".into(),
        target_key: "r|c1|".into(),
        state: NotificationState::Held,
        hold: Some((HoldKind::Quiet, 9000)),
        dedup_key: Some("net".into()),
        ..NewDispatch::default()
    };
    let net = keyed_event(&store, &ws, &source, "Network slow", false).await;
    store
        .write_routing(&net, std::slice::from_ref(&held), &[], true, 130)
        .await
        .unwrap();
    store
        .merge_notifications(&id, &held.notification_id, 140)
        .await
        .unwrap();
    let mut merged = titles(store.sendable_events(&held.notification_id).await.unwrap());
    merged.sort();
    assert_eq!(merged, vec!["Disk full", "Network slow"]);

    // When the whole notification is resolved the message is about everything that had fired.
    store
        .resolve_notification(&ws, &held.notification_id, 150)
        .await
        .unwrap();
    assert_eq!(
        store
            .sendable_events(&held.notification_id)
            .await
            .unwrap()
            .len(),
        3
    );
}
