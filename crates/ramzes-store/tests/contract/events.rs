use ramzes_core::{EventStatus, Severity};
use ramzes_store::{EventFilter, Outcome, Store};
use serde_json::json;

use crate::{event, seed};

crate::contract!(
    inserted_events_read_back_equal,
    missing_event_is_none,
    a_repeated_idempotency_key_returns_the_original_ids,
    the_same_key_from_another_source_is_not_a_duplicate,
    idempotency_keys_with_like_wildcards_do_not_cross_match,
    an_idempotency_key_that_is_a_prefix_of_another_is_not_a_duplicate,
    idempotency_keys_are_case_sensitive,
    a_duplicate_batch_returns_its_ids_in_the_original_order,
    list_is_newest_first_and_pages_with_before_id,
    filters_narrow_the_list,
    outcome_filters_pending_and_unrouted,
    claim_unrouted_hands_out_each_event_once_per_lease,
    retention_deletes_old_events_and_their_labels,
    retention_respects_topic_arguments_and_limit,
);

fn all() -> EventFilter {
    EventFilter {
        limit: 100,
        ..EventFilter::default()
    }
}

async fn inserted_events_read_back_equal(store: Store) {
    let (ws, source) = seed(&store).await;
    let mut e = event(&ws, &source, "orders.failed", 1000);
    e.severity = Severity::Critical;
    e.status = EventStatus::Resolved;
    e.body = Some("card declined".into());
    e.labels.insert("env".into(), "prod".into());
    e.labels.insert("service".into(), "payments".into());
    e.payload = json!({"order": 7, "items": ["a", "b"]});
    e.dedup_key = Some("order-7".into());
    e.occurred_at = 900;

    let out = store
        .insert_events(std::slice::from_ref(&e), None)
        .await
        .unwrap();
    assert_eq!(out.ids, vec![e.id.clone()]);
    assert!(!out.duplicate);
    assert_eq!(store.get_event(&ws, &e.id).await.unwrap(), Some(e));
}

async fn missing_event_is_none(store: Store) {
    let (ws, _) = seed(&store).await;
    assert_eq!(store.get_event(&ws, "nope").await.unwrap(), None);
}

async fn a_repeated_idempotency_key_returns_the_original_ids(store: Store) {
    let (ws, source) = seed(&store).await;
    let first = vec![event(&ws, &source, "a", 1), event(&ws, &source, "b", 1)];
    let out1 = store.insert_events(&first, Some("req-1")).await.unwrap();
    assert!(!out1.duplicate);

    let retry = vec![event(&ws, &source, "a", 2), event(&ws, &source, "b", 2)];
    let out2 = store.insert_events(&retry, Some("req-1")).await.unwrap();
    assert!(out2.duplicate);
    assert_eq!(out2.ids, out1.ids);
    assert_eq!(store.list_events(&ws, &all()).await.unwrap().len(), 2);
}

async fn the_same_key_from_another_source_is_not_a_duplicate(store: Store) {
    let (ws, source) = seed(&store).await;
    let other = store
        .create_source(&ws, "other", "native", "hash-other", "rz_src_othr", 1)
        .await
        .unwrap();
    store
        .insert_events(&[event(&ws, &source, "a", 1)], Some("req-1"))
        .await
        .unwrap();
    let out = store
        .insert_events(&[event(&ws, &other.id, "a", 1)], Some("req-1"))
        .await
        .unwrap();
    assert!(!out.duplicate);
    assert_eq!(store.list_events(&ws, &all()).await.unwrap().len(), 2);
}

async fn idempotency_keys_with_like_wildcards_do_not_cross_match(store: Store) {
    let (ws, source) = seed(&store).await;
    store
        .insert_events(&[event(&ws, &source, "a", 1)], Some("req-1"))
        .await
        .unwrap();
    for key in ["req-_", "req%", "%", "req-1\\"] {
        let out = store
            .insert_events(&[event(&ws, &source, "a", 1)], Some(key))
            .await
            .unwrap();
        assert!(!out.duplicate, "{key} must not match req-1");
    }
}

async fn list_is_newest_first_and_pages_with_before_id(store: Store) {
    let (ws, source) = seed(&store).await;
    let events: Vec<_> = (0..5)
        .map(|i| event(&ws, &source, &format!("t{i}"), i))
        .collect();
    store.insert_events(&events, None).await.unwrap();

    let page1 = store
        .list_events(
            &ws,
            &EventFilter {
                limit: 2,
                ..EventFilter::default()
            },
        )
        .await
        .unwrap();
    let ids1: Vec<_> = page1.iter().map(|r| r.event.id.clone()).collect();
    assert_eq!(ids1, vec![events[4].id.clone(), events[3].id.clone()]);

    let filter = EventFilter {
        limit: 2,
        before_id: Some(ids1[1].clone()),
        ..EventFilter::default()
    };
    let ids2: Vec<_> = store
        .list_events(&ws, &filter)
        .await
        .unwrap()
        .iter()
        .map(|r| r.event.id.clone())
        .collect();
    assert_eq!(ids2, vec![events[2].id.clone(), events[1].id.clone()]);
}

async fn filters_narrow_the_list(store: Store) {
    let (ws, source) = seed(&store).await;
    let other = store
        .create_source(&ws, "other", "native", "hash-other", "rz_src_othr", 1)
        .await
        .unwrap();

    let mut a = event(&ws, &source, "orders.failed", 100);
    a.severity = Severity::Error;
    a.title = "Charge FAILED: 100% declined".into();
    a.labels.insert("env".into(), "prod".into());
    let mut b = event(&ws, &source, "orders.created", 200);
    b.labels.insert("env".into(), "dev".into());
    let c = event(&ws, &other.id, "deploy.finished", 300);
    store
        .insert_events(&[a.clone(), b.clone(), c.clone()], None)
        .await
        .unwrap();

    let ids = |f: EventFilter| {
        let store = store.clone();
        let ws = ws.clone();
        async move {
            store
                .list_events(&ws, &f)
                .await
                .unwrap()
                .into_iter()
                .map(|r| r.event.id)
                .collect::<Vec<_>>()
        }
    };

    assert_eq!(
        ids(EventFilter {
            source_id: Some(other.id.clone()),
            ..all()
        })
        .await,
        vec![c.id.clone()]
    );
    assert_eq!(
        ids(EventFilter {
            topic: Some("orders.failed".into()),
            ..all()
        })
        .await,
        vec![a.id.clone()]
    );
    assert_eq!(
        ids(EventFilter {
            topic: Some("orders.*".into()),
            ..all()
        })
        .await,
        vec![b.id.clone(), a.id.clone()]
    );
    assert_eq!(
        ids(EventFilter {
            min_severity: Some(Severity::Error),
            ..all()
        })
        .await,
        vec![a.id.clone()]
    );
    assert_eq!(
        ids(EventFilter {
            label: Some(("env".into(), "prod".into())),
            ..all()
        })
        .await,
        vec![a.id.clone()]
    );
    assert_eq!(
        ids(EventFilter {
            title_contains: Some("failed".into()),
            ..all()
        })
        .await,
        vec![a.id.clone()]
    );
    assert_eq!(
        ids(EventFilter {
            title_contains: Some("100%".into()),
            ..all()
        })
        .await,
        vec![a.id.clone()]
    );
    assert!(
        ids(EventFilter {
            title_contains: Some("1_0".into()),
            ..all()
        })
        .await
        .is_empty()
    );
    assert_eq!(
        ids(EventFilter {
            since_ms: Some(200),
            ..all()
        })
        .await,
        vec![c.id.clone(), b.id.clone()]
    );
    assert_eq!(
        ids(EventFilter {
            until_ms: Some(200),
            ..all()
        })
        .await,
        vec![b.id.clone(), a.id.clone()]
    );
    assert_eq!(
        ids(EventFilter {
            since_ms: Some(150),
            until_ms: Some(250),
            ..all()
        })
        .await,
        vec![b.id.clone()]
    );
}

async fn outcome_filters_pending_and_unrouted(store: Store) {
    let (ws, source) = seed(&store).await;
    let waiting = event(&ws, &source, "a", 1);
    let unrouted = event(&ws, &source, "b", 2);
    store
        .insert_events(&[waiting.clone(), unrouted.clone()], None)
        .await
        .unwrap();
    store.write_routing(&unrouted, &[], true, 5).await.unwrap();

    let pending = store
        .list_events(
            &ws,
            &EventFilter {
                outcome: Some(Outcome::Pending),
                ..all()
            },
        )
        .await
        .unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].event.id, waiting.id);
    assert!(!pending[0].routed);

    let none = store
        .list_events(
            &ws,
            &EventFilter {
                outcome: Some(Outcome::Unrouted),
                ..all()
            },
        )
        .await
        .unwrap();
    assert_eq!(none.len(), 1);
    assert_eq!(none[0].event.id, unrouted.id);
    assert!(none[0].routed);
    assert_eq!(none[0].route_count, 0);
    assert_eq!(none[0].deliveries_total, 0);
}

async fn claim_unrouted_hands_out_each_event_once_per_lease(store: Store) {
    let (ws, source) = seed(&store).await;
    let mut a = event(&ws, &source, "a", 1);
    a.labels.insert("k".into(), "v".into());
    let b = event(&ws, &source, "b", 1);
    store
        .insert_events(&[a.clone(), b.clone()], None)
        .await
        .unwrap();

    let first = store.claim_unrouted(1000, 60_000, 10).await.unwrap();
    assert_eq!(first, vec![a.clone(), b.clone()]);
    assert!(
        store
            .claim_unrouted(2000, 60_000, 10)
            .await
            .unwrap()
            .is_empty()
    );

    // The lease ran out without the events being marked routed: they are handed out again.
    let again = store.claim_unrouted(61_001, 60_000, 10).await.unwrap();
    assert_eq!(again.len(), 2);

    store.write_routing(&a, &[], true, 70_000).await.unwrap();
    store.write_routing(&b, &[], true, 70_000).await.unwrap();
    assert!(
        store
            .claim_unrouted(500_000, 60_000, 10)
            .await
            .unwrap()
            .is_empty()
    );
}

async fn retention_deletes_old_events_and_their_labels(store: Store) {
    let (ws, source) = seed(&store).await;
    let mut old = event(&ws, &source, "a", 100);
    old.labels.insert("env".into(), "prod".into());
    let fresh = event(&ws, &source, "a", 900);
    store
        .insert_events(&[old.clone(), fresh.clone()], None)
        .await
        .unwrap();

    assert_eq!(
        store
            .delete_events_before(&ws, 500, None, &[], 100)
            .await
            .unwrap(),
        1
    );
    assert_eq!(store.get_event(&ws, &old.id).await.unwrap(), None);
    assert!(store.get_event(&ws, &fresh.id).await.unwrap().is_some());
    let by_label = EventFilter {
        label: Some(("env".into(), "prod".into())),
        ..all()
    };
    assert!(store.list_events(&ws, &by_label).await.unwrap().is_empty());
    assert_eq!(
        store
            .delete_events_before(&ws, 500, None, &[], 100)
            .await
            .unwrap(),
        0
    );
}

async fn retention_respects_topic_arguments_and_limit(store: Store) {
    let (ws, source) = seed(&store).await;
    let events = vec![
        event(&ws, &source, "audit", 1),
        event(&ws, &source, "noise", 2),
        event(&ws, &source, "noise", 3),
        event(&ws, &source, "other", 4),
    ];
    store.insert_events(&events, None).await.unwrap();

    // Only the named topic.
    assert_eq!(
        store
            .delete_events_before(&ws, 500, Some("noise"), &[], 1)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        store
            .delete_events_before(&ws, 500, Some("noise"), &[], 100)
            .await
            .unwrap(),
        1
    );
    // Everything except the excluded topic.
    assert_eq!(
        store
            .delete_events_before(&ws, 500, None, &["audit".to_string()], 100)
            .await
            .unwrap(),
        1
    );
    let left = store.list_events(&ws, &all()).await.unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].event.topic, "audit");
}

async fn an_idempotency_key_that_is_a_prefix_of_another_is_not_a_duplicate(store: Store) {
    let (ws, source) = seed(&store).await;
    store
        .insert_events(&[event(&ws, &source, "a", 1)], Some("job#42"))
        .await
        .unwrap();
    store
        .insert_events(&[event(&ws, &source, "a", 1)], Some("a#0"))
        .await
        .unwrap();
    for key in ["job", "job#4", "a"] {
        let out = store
            .insert_events(&[event(&ws, &source, "a", 1)], Some(key))
            .await
            .unwrap();
        assert!(
            !out.duplicate,
            "{key} must not be taken for an earlier request"
        );
    }
    assert_eq!(store.list_events(&ws, &all()).await.unwrap().len(), 5);
}

async fn idempotency_keys_are_case_sensitive(store: Store) {
    let (ws, source) = seed(&store).await;
    store
        .insert_events(&[event(&ws, &source, "a", 1)], Some("Deploy-A"))
        .await
        .unwrap();
    let out = store
        .insert_events(&[event(&ws, &source, "a", 1)], Some("deploy-a"))
        .await
        .unwrap();
    assert!(!out.duplicate);
}

async fn a_duplicate_batch_returns_its_ids_in_the_original_order(store: Store) {
    let (ws, source) = seed(&store).await;
    let batch: Vec<_> = (0..12)
        .map(|i| event(&ws, &source, &format!("t{i}"), 1))
        .collect();
    let first = store.insert_events(&batch, Some("big")).await.unwrap();
    let again = store
        .insert_events(&[event(&ws, &source, "x", 2)], Some("big"))
        .await
        .unwrap();
    assert!(again.duplicate);
    assert_eq!(again.ids, first.ids);
    assert_eq!(again.ids.len(), 12);
}
