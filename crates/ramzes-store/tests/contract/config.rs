use ramzes_core::{LabelCond, Route, RouteMatch, Severity, Target};
use ramzes_store::{Connection, Store, StoreError};
use serde_json::json;

crate::contract!(
    connection_crud_round_trips,
    updating_or_deleting_a_missing_connection_is_not_found,
    routes_are_appended_and_listed_in_position_order,
    a_route_survives_a_round_trip_unchanged,
    updating_a_route_keeps_its_position,
    updating_or_deleting_a_missing_route_is_not_found,
    reorder_applies_the_given_order,
    reorder_rejects_a_list_that_is_not_a_permutation,
);

fn connection(ws: &str, name: &str) -> Connection {
    Connection {
        id: ramzes_core::new_id(),
        workspace_id: ws.into(),
        kind: "webhook".into(),
        name: name.into(),
        settings: json!({"url": "https://example.com/hook"}),
        credentials: "sealed-bytes".into(),
        created_at: 10,
        updated_at: 10,
    }
}

fn route(name: &str) -> Route {
    Route {
        id: ramzes_core::new_id(),
        name: name.into(),
        position: 0,
        enabled: true,
        stop: false,
        r#match: RouteMatch::default(),
        targets: vec![Target {
            connection_id: "c1".into(),
            destination: None,
        }],
    }
}

async fn names(store: &Store, ws: &str) -> Vec<String> {
    store
        .list_routes(ws)
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.name)
        .collect()
}

async fn connection_crud_round_trips(store: Store) {
    let ws = store.ensure_workspace(1).await.unwrap();
    let mut c = connection(&ws, "ops hook");
    store.create_connection(&c).await.unwrap();
    assert_eq!(
        store.get_connection(&ws, &c.id).await.unwrap(),
        Some(c.clone())
    );
    assert_eq!(store.list_connections(&ws).await.unwrap(), vec![c.clone()]);

    c.name = "renamed".into();
    c.settings = json!({"url": "https://example.com/other"});
    c.credentials = "resealed".into();
    c.updated_at = 20;
    store.update_connection(&c).await.unwrap();
    assert_eq!(
        store.get_connection(&ws, &c.id).await.unwrap(),
        Some(c.clone())
    );

    store.delete_connection(&ws, &c.id).await.unwrap();
    assert_eq!(store.get_connection(&ws, &c.id).await.unwrap(), None);
    assert!(store.list_connections(&ws).await.unwrap().is_empty());
}

async fn updating_or_deleting_a_missing_connection_is_not_found(store: Store) {
    let ws = store.ensure_workspace(1).await.unwrap();
    assert!(matches!(
        store.update_connection(&connection(&ws, "x")).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        store.delete_connection(&ws, "nope").await,
        Err(StoreError::NotFound)
    ));
}

async fn routes_are_appended_and_listed_in_position_order(store: Store) {
    let ws = store.ensure_workspace(1).await.unwrap();
    for name in ["first", "second", "third"] {
        store.create_route(&ws, &route(name), 1).await.unwrap();
    }
    let routes = store.list_routes(&ws).await.unwrap();
    assert_eq!(
        routes.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
        vec!["first", "second", "third"]
    );
    assert!(routes[0].position < routes[1].position && routes[1].position < routes[2].position);
}

async fn a_route_survives_a_round_trip_unchanged(store: Store) {
    let ws = store.ensure_workspace(1).await.unwrap();
    let mut r = route("payments");
    r.enabled = false;
    r.stop = true;
    r.r#match = RouteMatch {
        topic: Some("orders.*".into()),
        min_severity: Some(Severity::Error),
        source_id: Some("src".into()),
        labels: vec![
            LabelCond::Equals {
                key: "env".into(),
                value: "prod".into(),
            },
            LabelCond::Matches {
                key: "service".into(),
                pattern: "^pay".into(),
            },
            LabelCond::Exists { key: "team".into() },
        ],
    };
    r.targets = vec![
        Target {
            connection_id: "c1".into(),
            destination: Some("#payments".into()),
        },
        Target {
            connection_id: "c2".into(),
            destination: None,
        },
    ];
    store.create_route(&ws, &r, 1).await.unwrap();
    let stored = store.list_routes(&ws).await.unwrap().pop().unwrap();
    r.position = stored.position;
    assert_eq!(stored, r);
}

async fn updating_a_route_keeps_its_position(store: Store) {
    let ws = store.ensure_workspace(1).await.unwrap();
    let mut a = route("a");
    store.create_route(&ws, &a, 1).await.unwrap();
    store.create_route(&ws, &route("b"), 1).await.unwrap();
    a.name = "a2".into();
    a.position = 99;
    a.enabled = false;
    store.update_route(&ws, &a, 2).await.unwrap();
    assert_eq!(names(&store, &ws).await, vec!["a2", "b"]);
    assert!(!store.list_routes(&ws).await.unwrap()[0].enabled);
}

async fn updating_or_deleting_a_missing_route_is_not_found(store: Store) {
    let ws = store.ensure_workspace(1).await.unwrap();
    assert!(matches!(
        store.update_route(&ws, &route("x"), 1).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        store.delete_route(&ws, "nope").await,
        Err(StoreError::NotFound)
    ));
    let r = route("x");
    store.create_route(&ws, &r, 1).await.unwrap();
    store.delete_route(&ws, &r.id).await.unwrap();
    assert!(store.list_routes(&ws).await.unwrap().is_empty());
}

async fn reorder_applies_the_given_order(store: Store) {
    let ws = store.ensure_workspace(1).await.unwrap();
    let (a, b, c) = (route("a"), route("b"), route("c"));
    for r in [&a, &b, &c] {
        store.create_route(&ws, r, 1).await.unwrap();
    }
    store
        .reorder_routes(&ws, &[c.id.clone(), a.id.clone(), b.id.clone()], 2)
        .await
        .unwrap();
    assert_eq!(names(&store, &ws).await, vec!["c", "a", "b"]);
    store.create_route(&ws, &route("d"), 3).await.unwrap();
    assert_eq!(names(&store, &ws).await, vec!["c", "a", "b", "d"]);
}

async fn reorder_rejects_a_list_that_is_not_a_permutation(store: Store) {
    let ws = store.ensure_workspace(1).await.unwrap();
    let (a, b) = (route("a"), route("b"));
    store.create_route(&ws, &a, 1).await.unwrap();
    store.create_route(&ws, &b, 1).await.unwrap();
    for ids in [
        vec![a.id.clone()],
        vec![a.id.clone(), "unknown".to_string()],
        vec![a.id.clone(), a.id.clone()],
        vec![a.id.clone(), b.id.clone(), "extra".to_string()],
    ] {
        assert!(
            matches!(
                store.reorder_routes(&ws, &ids, 2).await,
                Err(StoreError::Conflict(_))
            ),
            "{ids:?}"
        );
    }
    assert_eq!(names(&store, &ws).await, vec!["a", "b"]);
}
