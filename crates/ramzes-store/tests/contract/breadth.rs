use ramzes_core::{Route, RouteMatch, Target, Template};
use ramzes_store::{
    Attempt, ConfigSnapshot, Connection, ConnectionSnapshot, DefinitionRow, Finish, NewDispatch,
    SourceSnapshot, Store, StoreError,
};
use serde_json::json;

crate::contract!(
    template_crud_round_trips,
    definition_crud_round_trips_and_kinds_are_unique,
    a_route_error_is_stored_and_cleared,
    a_source_keeps_its_secret_and_mapping,
    source_requests_keep_their_headers,
    a_delivery_warning_is_stored,
    a_notification_remembers_its_template,
    revisions_are_listed_newest_first_and_trimmed,
    connection_health_counts_recent_deliveries,
    applying_a_config_replaces_routes_templates_and_definitions,
    applying_a_config_keeps_source_keys,
);

fn template(name: &str) -> Template {
    Template {
        id: ramzes_core::new_id(),
        name: name.into(),
        title: "[{{ labels.env }}] {{ title }}".into(),
        body: "{{ body }}".into(),
    }
}

async fn template_crud_round_trips(store: Store) {
    let ws = store.ensure_workspace(1).await.unwrap();
    let mut t = template("ops");
    store.create_template(&ws, &t, 5).await.unwrap();
    assert_eq!(store.list_templates(&ws).await.unwrap(), vec![t.clone()]);
    assert_eq!(
        store.get_template(&ws, &t.id).await.unwrap(),
        Some(t.clone())
    );
    t.title = "{{ title }}".into();
    t.name = "renamed".into();
    store.update_template(&ws, &t, 6).await.unwrap();
    assert_eq!(
        store.get_template(&ws, &t.id).await.unwrap(),
        Some(t.clone())
    );
    store.delete_template(&ws, &t.id).await.unwrap();
    assert_eq!(store.get_template(&ws, &t.id).await.unwrap(), None);
    assert!(matches!(
        store.delete_template(&ws, &t.id).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        store.update_template(&ws, &t, 7).await,
        Err(StoreError::NotFound)
    ));
}

fn definition(kind: &str) -> DefinitionRow {
    DefinitionRow {
        id: ramzes_core::new_id(),
        kind: kind.into(),
        name: "Chat".into(),
        definition: "kind: chat\n".into(),
        created_at: 3,
    }
}

async fn definition_crud_round_trips_and_kinds_are_unique(store: Store) {
    let ws = store.ensure_workspace(1).await.unwrap();
    let mut d = definition("chat");
    store.create_definition(&ws, &d).await.unwrap();
    assert_eq!(store.list_definitions(&ws).await.unwrap(), vec![d.clone()]);
    assert!(matches!(
        store.create_definition(&ws, &definition("chat")).await,
        Err(StoreError::Conflict(_))
    ));
    d.definition = "kind: chat\nname: Chat 2\n".into();
    d.name = "Chat 2".into();
    store.update_definition(&ws, &d).await.unwrap();
    assert_eq!(store.list_definitions(&ws).await.unwrap(), vec![d.clone()]);
    store.delete_definition(&ws, &d.id).await.unwrap();
    assert!(store.list_definitions(&ws).await.unwrap().is_empty());
    assert!(matches!(
        store.delete_definition(&ws, &d.id).await,
        Err(StoreError::NotFound)
    ));
}

fn route(name: &str) -> Route {
    Route {
        id: ramzes_core::new_id(),
        name: name.into(),
        enabled: true,
        r#match: RouteMatch {
            expr: Some("payload.amount > 1".into()),
            ..RouteMatch::default()
        },
        targets: vec![Target {
            connection_id: "c1".into(),
            destination: None,
            template_id: Some("t1".into()),
        }],
        ..Route::default()
    }
}

async fn a_route_error_is_stored_and_cleared(store: Store) {
    let ws = store.ensure_workspace(1).await.unwrap();
    let r = route("a");
    store.create_route(&ws, &r, 1).await.unwrap();
    assert!(store.route_errors(&ws).await.unwrap().is_empty());
    store
        .set_route_error(&r.id, Some("unknown filter"), 9)
        .await
        .unwrap();
    assert_eq!(
        store.route_errors(&ws).await.unwrap(),
        vec![(r.id.clone(), "unknown filter".to_string(), 9)]
    );
    store.set_route_error(&r.id, None, 10).await.unwrap();
    assert!(store.route_errors(&ws).await.unwrap().is_empty());
    // The expression and the target's template survive storage.
    assert_eq!(store.list_routes(&ws).await.unwrap()[0].targets, r.targets);
    assert_eq!(store.list_routes(&ws).await.unwrap()[0].r#match, r.r#match);
}

async fn a_source_keeps_its_secret_and_mapping(store: Store) {
    let ws = store.ensure_workspace(1).await.unwrap();
    let s = store
        .create_source(&ws, "gh", "github", "hash", "rz_src_abcd", 1)
        .await
        .unwrap();
    assert_eq!(s.mapping, None);
    assert_eq!(store.source_secret(&s.id).await.unwrap(), None);
    store.set_source_secret(&ws, &s.id, "sealed").await.unwrap();
    store
        .set_source_mapping(&ws, &s.id, Some("fields: {}"))
        .await
        .unwrap();
    assert_eq!(
        store.source_secret(&s.id).await.unwrap().as_deref(),
        Some("sealed")
    );
    let got = store.get_source(&s.id).await.unwrap().unwrap();
    assert_eq!(got.mapping.as_deref(), Some("fields: {}"));
    assert_eq!(got.kind, "github");
    assert_eq!(store.get_source("nope").await.unwrap(), None);
    assert!(matches!(
        store.set_source_mapping(&ws, "nope", None).await,
        Err(StoreError::NotFound)
    ));
}

async fn source_requests_keep_their_headers(store: Store) {
    let (ws, source) = crate::seed(&store).await;
    store
        .record_source_request(
            &ws,
            &source,
            "{}",
            Some(r#"{"x-github-event":"push"}"#),
            None,
            5,
        )
        .await
        .unwrap();
    let got = store.list_source_requests(&ws, &source).await.unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(
        got[0].headers.as_deref(),
        Some(r#"{"x-github-event":"push"}"#)
    );
    assert_eq!(got[0].error, None);
}

async fn dispatch(store: &Store, ws: &str, source: &str, connection: &str, at: i64) -> NewDispatch {
    let event = crate::event(ws, source, "orders.failed", at);
    store
        .insert_events(std::slice::from_ref(&event), None)
        .await
        .unwrap();
    let d = NewDispatch {
        notification_id: ramzes_core::new_id(),
        delivery_id: ramzes_core::new_id(),
        route_id: "r".into(),
        route_name: "r".into(),
        connection_id: connection.into(),
        destination: None,
        template_id: Some("t9".into()),
        replay: false,
    };
    store
        .write_routing(&event, std::slice::from_ref(&d), true, at)
        .await
        .unwrap();
    d
}

async fn finish(store: &Store, at: i64, f: Finish) {
    let claimed = store.claim_due_deliveries(at, 1000, 10).await.unwrap();
    for d in claimed {
        let a = Attempt {
            id: ramzes_core::new_id(),
            delivery_id: d.id.clone(),
            started_at: at,
            duration_ms: 1,
            outcome: "x".into(),
            status: None,
            detail: None,
        };
        store.finish_attempt(&d, &a, &f, at).await.unwrap();
    }
}

async fn a_delivery_warning_is_stored(store: Store) {
    let (ws, source) = crate::seed(&store).await;
    let d = dispatch(&store, &ws, &source, "c1", 10).await;
    store
        .set_delivery_warning(&d.delivery_id, "template failed: title")
        .await
        .unwrap();
    let claimed = store.claim_due_deliveries(10, 1000, 10).await.unwrap();
    assert_eq!(
        claimed[0].warning.as_deref(),
        Some("template failed: title")
    );
}

async fn a_notification_remembers_its_template(store: Store) {
    let (ws, source) = crate::seed(&store).await;
    let d = dispatch(&store, &ws, &source, "c1", 10).await;
    assert_eq!(
        store
            .notification_template(&d.notification_id)
            .await
            .unwrap()
            .as_deref(),
        Some("t9")
    );
}

async fn revisions_are_listed_newest_first_and_trimmed(store: Store) {
    let ws = store.ensure_workspace(1).await.unwrap();
    for i in 0..205 {
        store
            .add_revision(&ws, "kris", "created route", &format!("{{\"n\":{i}}}"), i)
            .await
            .unwrap();
    }
    let all = store.list_revisions(&ws).await.unwrap();
    assert_eq!(all.len(), 200);
    assert_eq!(all[0].created_at, 204);
    assert_eq!(all[0].author, "kris");
    assert_eq!(all[0].summary, "created route");
    let newest = store.get_revision(&ws, &all[0].id).await.unwrap().unwrap();
    assert_eq!(newest.snapshot, "{\"n\":204}");
    assert_eq!(store.get_revision(&ws, "nope").await.unwrap(), None);
}

async fn connection_health_counts_recent_deliveries(store: Store) {
    let (ws, source) = crate::seed(&store).await;
    dispatch(&store, &ws, &source, "old", 10).await;
    finish(
        &store,
        10,
        Finish::Failed {
            error: "ancient".into(),
        },
    )
    .await;
    dispatch(&store, &ws, &source, "a", 1000).await;
    dispatch(&store, &ws, &source, "a", 1001).await;
    finish(&store, 1001, Finish::Delivered { external_ref: None }).await;
    dispatch(&store, &ws, &source, "a", 1002).await;
    finish(
        &store,
        1002,
        Finish::Dead {
            error: "gave up: 503".into(),
        },
    )
    .await;
    dispatch(&store, &ws, &source, "b", 1003).await;

    let health = store.connection_health(&ws, 500).await.unwrap();
    assert_eq!(health.len(), 2);
    let a = health.iter().find(|h| h.connection_id == "a").unwrap();
    assert_eq!((a.delivered, a.failed, a.pending), (2, 1, 0));
    assert_eq!(a.last_error.as_deref(), Some("gave up: 503"));
    let b = health.iter().find(|h| h.connection_id == "b").unwrap();
    assert_eq!((b.delivered, b.failed, b.pending), (0, 0, 1));
    assert_eq!(b.last_error, None);
}

fn connection(ws: &str, id: &str, credentials: &str) -> Connection {
    Connection {
        id: id.into(),
        workspace_id: ws.into(),
        kind: "webhook".into(),
        name: format!("hook {id}"),
        settings: json!({"url": "https://example.com"}),
        credentials: credentials.into(),
        created_at: 1,
        updated_at: 1,
    }
}

async fn applying_a_config_replaces_routes_templates_and_definitions(store: Store) {
    let ws = store.ensure_workspace(1).await.unwrap();
    store.create_route(&ws, &route("old"), 1).await.unwrap();
    store
        .create_template(&ws, &template("old"), 1)
        .await
        .unwrap();
    store
        .create_definition(&ws, &definition("oldkind"))
        .await
        .unwrap();
    store
        .create_connection(&connection(&ws, "keep", "sealed-keep"))
        .await
        .unwrap();
    store
        .create_connection(&connection(&ws, "gone", "sealed-gone"))
        .await
        .unwrap();
    store
        .set_setting(&ws, "retention_days", "30")
        .await
        .unwrap();

    let (first, second) = (route("first"), route("second"));
    let snapshot = ConfigSnapshot {
        routes: vec![first.clone(), second.clone()],
        templates: vec![template("new")],
        definitions: vec![definition("newkind")],
        connections: vec![
            ConnectionSnapshot {
                id: "keep".into(),
                kind: "webhook".into(),
                name: "renamed".into(),
                settings: json!({"url": "https://other.example"}),
                credentials: None,
            },
            ConnectionSnapshot {
                id: "fresh".into(),
                kind: "webhook".into(),
                name: "fresh".into(),
                settings: json!({"url": "https://fresh.example"}),
                credentials: Some("sealed-fresh".into()),
            },
        ],
        sources: vec![],
        settings: [("retention_days".to_string(), "7".to_string())].into(),
    };
    store.apply_config(&ws, &snapshot, 50).await.unwrap();

    let routes = store.list_routes(&ws).await.unwrap();
    assert_eq!(
        routes
            .iter()
            .map(|r| (r.name.as_str(), r.position))
            .collect::<Vec<_>>(),
        vec![("first", 1), ("second", 2)]
    );
    assert_eq!(routes[0].id, first.id);
    assert_eq!(store.list_templates(&ws).await.unwrap()[0].name, "new");
    assert_eq!(
        store.list_definitions(&ws).await.unwrap()[0].kind,
        "newkind"
    );
    let connections = store.list_connections(&ws).await.unwrap();
    assert_eq!(connections.len(), 2);
    let keep = connections.iter().find(|c| c.id == "keep").unwrap();
    // Without credentials in the snapshot the stored ones stay.
    assert_eq!(
        (keep.name.as_str(), keep.credentials.as_str()),
        ("renamed", "sealed-keep")
    );
    let fresh = connections.iter().find(|c| c.id == "fresh").unwrap();
    assert_eq!(fresh.credentials, "sealed-fresh");
    assert_eq!(
        store
            .get_setting(&ws, "retention_days")
            .await
            .unwrap()
            .as_deref(),
        Some("7")
    );

    let read_back = store.read_config(&ws).await.unwrap();
    assert_eq!(read_back.routes.len(), 2);
    assert_eq!(read_back.connections.len(), 2);
    assert_eq!(
        read_back
            .connections
            .iter()
            .find(|c| c.id == "keep")
            .unwrap()
            .credentials
            .as_deref(),
        Some("sealed-keep")
    );
}

async fn applying_a_config_keeps_source_keys(store: Store) {
    let ws = store.ensure_workspace(1).await.unwrap();
    let s = store
        .create_source(&ws, "gh", "json", "hash", "rz_src_abcd", 1)
        .await
        .unwrap();
    let snapshot = ConfigSnapshot {
        sources: vec![
            SourceSnapshot {
                id: s.id.clone(),
                name: "github".into(),
                kind: "json".into(),
                mapping: Some("fields: {}".into()),
            },
            SourceSnapshot {
                id: "unknown".into(),
                name: "x".into(),
                kind: "json".into(),
                mapping: None,
            },
        ],
        ..ConfigSnapshot::default()
    };
    store.apply_config(&ws, &snapshot, 5).await.unwrap();
    let sources = store.list_sources(&ws).await.unwrap();
    // A source that is not in the database is not invented: it would have no key.
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].name, "github");
    assert_eq!(sources[0].mapping.as_deref(), Some("fields: {}"));
    assert_eq!(
        store.source_by_key_hash("hash").await.unwrap().unwrap().id,
        s.id
    );
}
