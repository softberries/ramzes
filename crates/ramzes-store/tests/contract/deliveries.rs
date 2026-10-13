use ramzes_core::Event;
use ramzes_store::{
    Attempt, Delivery, DeliveryState, EventFilter, Finish, NewDispatch, Outcome, Store, StoreError,
};

use crate::{event, seed};

crate::contract!(
    write_routing_creates_notifications_and_pending_deliveries,
    write_routing_with_no_dispatches_marks_the_event_unrouted,
    write_routing_for_a_replay_leaves_the_routed_state_alone,
    claiming_returns_each_due_delivery_once,
    an_unfinished_delivery_is_claimable_again_after_the_lease,
    a_future_delivery_is_not_claimed,
    finish_delivered_records_the_attempt_and_reference,
    finish_retry_reschedules_and_keeps_the_error,
    finish_failed_and_dead_are_final,
    notification_events_returns_the_events,
    journey_returns_the_full_tree,
    journey_for_a_missing_event_is_none,
    list_failures_contains_only_failed_and_dead,
    requeue_adds_a_pending_delivery_and_leaves_the_original,
    requeue_of_a_missing_delivery_is_not_found,
    queue_depth_counts_unrouted_and_pending,
    outcome_filters_delivered_and_failed,
    retention_removes_notifications_deliveries_and_attempts,
);

fn dispatch(connection_id: &str) -> NewDispatch {
    NewDispatch {
        notification_id: ramzes_core::new_id(),
        delivery_id: ramzes_core::new_id(),
        route_id: "route-1".into(),
        route_name: "Payment failures".into(),
        connection_id: connection_id.into(),
        destination: Some("#ops".into()),
        template_id: None,
        replay: false,
    }
}

fn attempt(d: &Delivery, started_at: i64) -> Attempt {
    Attempt {
        id: ramzes_core::new_id(),
        delivery_id: d.id.clone(),
        started_at,
        duration_ms: 12,
        outcome: "delivered".into(),
        status: Some(200),
        detail: Some("ok".into()),
    }
}

/// One stored event routed to `connections`, returning the event and the claimed deliveries.
async fn routed(store: &Store, connections: &[&str], now: i64) -> (String, Event, Vec<Delivery>) {
    let (ws, source) = seed(store).await;
    let e = event(&ws, &source, "orders.failed", now);
    store
        .insert_events(std::slice::from_ref(&e), None)
        .await
        .unwrap();
    let dispatches: Vec<_> = connections.iter().map(|c| dispatch(c)).collect();
    store
        .write_routing(&e, &dispatches, true, now)
        .await
        .unwrap();
    let claimed = store.claim_due_deliveries(now, 60_000, 10).await.unwrap();
    (ws, e, claimed)
}

async fn write_routing_creates_notifications_and_pending_deliveries(store: Store) {
    let (ws, source) = seed(&store).await;
    let e = event(&ws, &source, "orders.failed", 100);
    store
        .insert_events(std::slice::from_ref(&e), None)
        .await
        .unwrap();
    store
        .write_routing(&e, &[dispatch("c1"), dispatch("c2")], true, 100)
        .await
        .unwrap();

    let j = store.journey(&ws, &e.id).await.unwrap().unwrap();
    assert!(j.routed);
    assert_eq!(j.route_count, 2);
    assert_eq!(j.notifications.len(), 2);
    for n in &j.notifications {
        assert_eq!(n.state, "sent");
        assert_eq!(n.route_name, "Payment failures");
        assert_eq!(n.destination.as_deref(), Some("#ops"));
        assert!(!n.replay);
        assert_eq!(n.deliveries.len(), 1);
        let d = &n.deliveries[0].delivery;
        assert_eq!(d.state, DeliveryState::Pending);
        assert_eq!(d.attempts, 0);
        assert_eq!(d.next_attempt_at, 100);
    }
    assert!(
        store
            .claim_unrouted(1_000_000, 1000, 10)
            .await
            .unwrap()
            .is_empty()
    );
}

async fn write_routing_with_no_dispatches_marks_the_event_unrouted(store: Store) {
    let (ws, source) = seed(&store).await;
    let e = event(&ws, &source, "a", 100);
    store
        .insert_events(std::slice::from_ref(&e), None)
        .await
        .unwrap();
    store.write_routing(&e, &[], true, 100).await.unwrap();
    let j = store.journey(&ws, &e.id).await.unwrap().unwrap();
    assert!(j.routed);
    assert_eq!(j.route_count, 0);
    assert!(j.notifications.is_empty());
}

async fn write_routing_for_a_replay_leaves_the_routed_state_alone(store: Store) {
    let (ws, e, _) = routed(&store, &["c1"], 100).await;
    let mut again = dispatch("c1");
    again.replay = true;
    store.write_routing(&e, &[again], false, 200).await.unwrap();
    let j = store.journey(&ws, &e.id).await.unwrap().unwrap();
    assert_eq!(j.route_count, 1);
    assert_eq!(j.notifications.len(), 2);
    assert_eq!(j.notifications.iter().filter(|n| n.replay).count(), 1);
}

async fn claiming_returns_each_due_delivery_once(store: Store) {
    let (_, _, claimed) = routed(&store, &["c1", "c2"], 100).await;
    assert_eq!(claimed.len(), 2);
    assert!(claimed.iter().all(|d| d.state == DeliveryState::InProgress));
    assert!(
        store
            .claim_due_deliveries(200, 60_000, 10)
            .await
            .unwrap()
            .is_empty()
    );
}

async fn an_unfinished_delivery_is_claimable_again_after_the_lease(store: Store) {
    let (_, _, claimed) = routed(&store, &["c1"], 100).await;
    assert!(
        store
            .claim_due_deliveries(60_000, 60_000, 10)
            .await
            .unwrap()
            .is_empty()
    );
    let again = store
        .claim_due_deliveries(60_101, 60_000, 10)
        .await
        .unwrap();
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].id, claimed[0].id);
}

async fn a_future_delivery_is_not_claimed(store: Store) {
    let (_, _, claimed) = routed(&store, &["c1"], 100).await;
    let d = &claimed[0];
    let retry = Finish::Retry {
        at: 5000,
        error: "503".into(),
    };
    store
        .finish_attempt(d, &attempt(d, 100), &retry, 110)
        .await
        .unwrap();
    assert!(
        store
            .claim_due_deliveries(4999, 60_000, 10)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store
            .claim_due_deliveries(5000, 60_000, 10)
            .await
            .unwrap()
            .len(),
        1
    );
}

async fn finish_delivered_records_the_attempt_and_reference(store: Store) {
    let (ws, e, claimed) = routed(&store, &["c1"], 100).await;
    let d = &claimed[0];
    let a = attempt(d, 100);
    store
        .finish_attempt(
            d,
            &a,
            &Finish::Delivered {
                external_ref: Some("msg-9".into()),
            },
            150,
        )
        .await
        .unwrap();

    let j = store.journey(&ws, &e.id).await.unwrap().unwrap();
    let jd = &j.notifications[0].deliveries[0];
    assert_eq!(jd.delivery.state, DeliveryState::Delivered);
    assert_eq!(jd.delivery.attempts, 1);
    assert_eq!(jd.delivery.external_ref.as_deref(), Some("msg-9"));
    assert_eq!(jd.delivery.last_error, None);
    assert_eq!(jd.delivery.updated_at, 150);
    assert_eq!(jd.attempts, vec![a]);
    assert!(
        store
            .claim_due_deliveries(1_000_000, 1000, 10)
            .await
            .unwrap()
            .is_empty()
    );
}

async fn finish_retry_reschedules_and_keeps_the_error(store: Store) {
    let (ws, e, claimed) = routed(&store, &["c1"], 100).await;
    let d = &claimed[0];
    let retry = Finish::Retry {
        at: 5000,
        error: "HTTP 503".into(),
    };
    store
        .finish_attempt(d, &attempt(d, 100), &retry, 110)
        .await
        .unwrap();
    let j = store.journey(&ws, &e.id).await.unwrap().unwrap();
    let got = &j.notifications[0].deliveries[0].delivery;
    assert_eq!(got.state, DeliveryState::Pending);
    assert_eq!(got.attempts, 1);
    assert_eq!(got.next_attempt_at, 5000);
    assert_eq!(got.last_error.as_deref(), Some("HTTP 503"));
}

async fn finish_failed_and_dead_are_final(store: Store) {
    let (ws, e, claimed) = routed(&store, &["c1", "c2"], 100).await;
    let failed = Finish::Failed {
        error: "HTTP 401".into(),
    };
    let dead = Finish::Dead {
        error: "gave up".into(),
    };
    store
        .finish_attempt(&claimed[0], &attempt(&claimed[0], 100), &failed, 110)
        .await
        .unwrap();
    store
        .finish_attempt(&claimed[1], &attempt(&claimed[1], 100), &dead, 110)
        .await
        .unwrap();

    let j = store.journey(&ws, &e.id).await.unwrap().unwrap();
    let mut states: Vec<_> = j
        .notifications
        .iter()
        .map(|n| n.deliveries[0].delivery.state)
        .collect();
    states.sort_by_key(|s| s.as_str());
    assert_eq!(states, vec![DeliveryState::Dead, DeliveryState::Failed]);
    assert!(
        store
            .claim_due_deliveries(1_000_000, 1000, 10)
            .await
            .unwrap()
            .is_empty()
    );
}

async fn notification_events_returns_the_events(store: Store) {
    let (_, e, claimed) = routed(&store, &["c1"], 100).await;
    assert_eq!(
        store
            .notification_events(&claimed[0].notification_id)
            .await
            .unwrap(),
        vec![e]
    );
}

async fn journey_returns_the_full_tree(store: Store) {
    let (ws, e, claimed) = routed(&store, &["c1"], 100).await;
    let d = &claimed[0];
    store
        .finish_attempt(
            d,
            &attempt(d, 100),
            &Finish::Retry {
                at: 200,
                error: "x".into(),
            },
            110,
        )
        .await
        .unwrap();
    let d = &store.claim_due_deliveries(200, 60_000, 10).await.unwrap()[0];
    store
        .finish_attempt(
            d,
            &attempt(d, 200),
            &Finish::Delivered { external_ref: None },
            210,
        )
        .await
        .unwrap();

    let j = store.journey(&ws, &e.id).await.unwrap().unwrap();
    assert_eq!(j.event, e);
    assert_eq!(j.notifications.len(), 1);
    assert_eq!(j.notifications[0].route_id, "route-1");
    assert_eq!(j.notifications[0].connection_id, "c1");
    let jd = &j.notifications[0].deliveries[0];
    assert_eq!(jd.delivery.attempts, 2);
    assert_eq!(
        jd.attempts.iter().map(|a| a.started_at).collect::<Vec<_>>(),
        vec![100, 200]
    );
}

async fn journey_for_a_missing_event_is_none(store: Store) {
    let (ws, _) = seed(&store).await;
    assert!(store.journey(&ws, "nope").await.unwrap().is_none());
}

async fn list_failures_contains_only_failed_and_dead(store: Store) {
    let (ws, e, claimed) = routed(&store, &["c1", "c2", "c3"], 100).await;
    let finishes = [
        Finish::Failed {
            error: "HTTP 401".into(),
        },
        Finish::Dead {
            error: "gave up".into(),
        },
        Finish::Delivered { external_ref: None },
    ];
    for (d, f) in claimed.iter().zip(&finishes) {
        store
            .finish_attempt(d, &attempt(d, 100), f, 110)
            .await
            .unwrap();
    }
    let failures = store.list_failures(&ws, 50).await.unwrap();
    assert_eq!(failures.len(), 2);
    for f in &failures {
        assert!(matches!(
            f.delivery.state,
            DeliveryState::Failed | DeliveryState::Dead
        ));
        assert_eq!(f.event_id, e.id);
        assert_eq!(f.event_title, e.title);
        assert_eq!(f.route_name, "Payment failures");
        assert_eq!(f.connection_name, None);
    }
}

async fn requeue_adds_a_pending_delivery_and_leaves_the_original(store: Store) {
    let (ws, e, claimed) = routed(&store, &["c1"], 100).await;
    let d = &claimed[0];
    store
        .finish_attempt(
            d,
            &attempt(d, 100),
            &Finish::Failed {
                error: "HTTP 401".into(),
            },
            110,
        )
        .await
        .unwrap();

    let new_id = ramzes_core::new_id();
    let fresh = store
        .requeue_delivery(&ws, &d.id, &new_id, 500)
        .await
        .unwrap();
    assert_eq!(fresh.id, new_id);
    assert_eq!(fresh.state, DeliveryState::Pending);
    assert_eq!(fresh.attempts, 0);
    assert_eq!(fresh.next_attempt_at, 500);
    assert_eq!(fresh.notification_id, d.notification_id);
    assert_eq!(fresh.connection_id, "c1");
    assert_eq!(fresh.destination.as_deref(), Some("#ops"));

    let j = store.journey(&ws, &e.id).await.unwrap().unwrap();
    let deliveries = &j.notifications[0].deliveries;
    assert_eq!(deliveries.len(), 2);
    assert_eq!(deliveries[0].delivery.state, DeliveryState::Failed);
    assert_eq!(
        store
            .claim_due_deliveries(500, 60_000, 10)
            .await
            .unwrap()
            .len(),
        1
    );
}

async fn requeue_of_a_missing_delivery_is_not_found(store: Store) {
    let (ws, _) = seed(&store).await;
    assert!(matches!(
        store.requeue_delivery(&ws, "nope", "new", 1).await,
        Err(StoreError::NotFound)
    ));
}

async fn queue_depth_counts_unrouted_and_pending(store: Store) {
    let (ws, source) = seed(&store).await;
    let (a, b) = (event(&ws, &source, "a", 1), event(&ws, &source, "b", 1));
    store.insert_events(&[a.clone(), b], None).await.unwrap();
    store
        .write_routing(&a, &[dispatch("c1"), dispatch("c2")], true, 1)
        .await
        .unwrap();
    let depth = store.queue_depth().await.unwrap();
    assert_eq!(depth.unrouted_events, 1);
    assert_eq!(depth.pending_deliveries, 2);
}

async fn outcome_filters_delivered_and_failed(store: Store) {
    let (ws, source) = seed(&store).await;
    let (ok, bad) = (event(&ws, &source, "ok", 1), event(&ws, &source, "bad", 2));
    store
        .insert_events(&[ok.clone(), bad.clone()], None)
        .await
        .unwrap();
    store
        .write_routing(&ok, &[dispatch("c1")], true, 10)
        .await
        .unwrap();
    store
        .write_routing(&bad, &[dispatch("c1"), dispatch("c2")], true, 10)
        .await
        .unwrap();
    for d in store.claim_due_deliveries(10, 60_000, 10).await.unwrap() {
        let on_bad = store.notification_events(&d.notification_id).await.unwrap()[0].id == bad.id;
        let finish = if on_bad && d.connection_id == "c2" {
            Finish::Failed {
                error: "HTTP 401".into(),
            }
        } else {
            Finish::Delivered { external_ref: None }
        };
        store
            .finish_attempt(&d, &attempt(&d, 10), &finish, 20)
            .await
            .unwrap();
    }
    let with = |outcome| EventFilter {
        outcome: Some(outcome),
        limit: 10,
        ..EventFilter::default()
    };
    let delivered = store
        .list_events(&ws, &with(Outcome::Delivered))
        .await
        .unwrap();
    assert_eq!(
        delivered
            .iter()
            .map(|r| r.event.id.clone())
            .collect::<Vec<_>>(),
        vec![ok.id.clone()]
    );
    assert_eq!(
        (
            delivered[0].deliveries_total,
            delivered[0].deliveries_delivered
        ),
        (1, 1)
    );

    let failed = store
        .list_events(&ws, &with(Outcome::Failed))
        .await
        .unwrap();
    assert_eq!(
        failed
            .iter()
            .map(|r| r.event.id.clone())
            .collect::<Vec<_>>(),
        vec![bad.id.clone()]
    );
    assert_eq!(
        (
            failed[0].deliveries_total,
            failed[0].deliveries_delivered,
            failed[0].deliveries_failed
        ),
        (2, 1, 1)
    );
    assert!(
        store
            .list_events(&ws, &with(Outcome::Pending))
            .await
            .unwrap()
            .is_empty()
    );
}

async fn retention_removes_notifications_deliveries_and_attempts(store: Store) {
    let (ws, old, claimed) = routed(&store, &["c1"], 100).await;
    let d = &claimed[0];
    store
        .finish_attempt(
            d,
            &attempt(d, 100),
            &Finish::Failed {
                error: "HTTP 401".into(),
            },
            110,
        )
        .await
        .unwrap();

    let source = store.list_sources(&ws).await.unwrap()[0].id.clone();
    let fresh = event(&ws, &source, "orders.failed", 900);
    store
        .insert_events(std::slice::from_ref(&fresh), None)
        .await
        .unwrap();
    store
        .write_routing(&fresh, &[dispatch("c1")], true, 900)
        .await
        .unwrap();

    assert_eq!(
        store
            .delete_events_before(&ws, 500, None, &[], 100)
            .await
            .unwrap(),
        1
    );
    assert!(store.journey(&ws, &old.id).await.unwrap().is_none());
    assert!(store.list_failures(&ws, 50).await.unwrap().is_empty());
    assert!(
        store
            .notification_events(&d.notification_id)
            .await
            .unwrap()
            .is_empty()
    );
    // The newer event keeps its notification and delivery.
    let j = store.journey(&ws, &fresh.id).await.unwrap().unwrap();
    assert_eq!(j.notifications[0].deliveries.len(), 1);
    assert_eq!(store.queue_depth().await.unwrap().pending_deliveries, 1);
}
