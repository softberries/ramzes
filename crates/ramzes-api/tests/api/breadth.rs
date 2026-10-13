//! Milestone 2 endpoints: templates, connector definitions, source kinds and mappings, the route
//! tester, previews, connection health and configuration history.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use ramzes_connectors::SendOutcome;
use serde_json::{Value, json};

use crate::common::Api;
use crate::resources::{catch_all_route, connection, ingest, process, source};

const CHAT: &str = r#"
kind: chat
name: Chat
fields:
  - { key: base_url, label: Base URL, kind: url, required: true }
  - { key: token, label: Token, secret: true, required: true }
request:
  url: "{{ settings.base_url }}/messages"
  headers:
    Authorization: "Bearer {{ credentials.token }}"
  body: '{"text": {{ message.title | tojson }}}'
"#;

#[tokio::test]
async fn templates_crud_and_validation() {
    let api = Api::signed_in().await;
    let (status, created) = api
        .post(
            "/api/templates",
            json!({"name": "Ops", "title": "[{{ labels.env }}] {{ title }}", "body": ""}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let id = created["id"].as_str().unwrap();
    assert_eq!(api.get("/api/templates").await.1[0]["name"], "Ops");

    let (status, error) = api
        .post(
            "/api/templates",
            json!({"name": "Bad", "title": "{{ title ", "body": ""}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        error["message"].as_str().unwrap().starts_with("title:"),
        "{error}"
    );
    let (status, _) = api
        .post(
            "/api/templates",
            json!({"name": "Empty", "title": " ", "body": ""}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (status, updated) = api
        .put(
            &format!("/api/templates/{id}"),
            json!({"name": "Ops 2", "title": "{{ title }}", "body": "b"}),
        )
        .await;
    assert_eq!(
        (status, updated["name"].as_str()),
        (StatusCode::OK, Some("Ops 2"))
    );
    assert_eq!(
        api.delete(&format!("/api/templates/{id}")).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        api.delete(&format!("/api/templates/{id}")).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn a_template_in_use_cannot_be_deleted_and_a_route_cannot_name_a_missing_one() {
    let api = Api::signed_in().await;
    let c = connection(&api, "ops").await;
    let (_, t) = api
        .post(
            "/api/templates",
            json!({"name": "Ops", "title": "{{ title }}", "body": ""}),
        )
        .await;
    let t = t["id"].as_str().unwrap();
    let route = |template: &str| json!({"name": "r", "match": {}, "targets": [{"connection_id": c, "template_id": template}]});
    let (status, error) = api.post("/api/routes", route("nope")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{error}");
    let (status, created) = api.post("/api/routes", route(t)).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["targets"][0]["template_id"], t);
    let (status, error) = api.delete(&format!("/api/templates/{t}")).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(error["message"].as_str().unwrap().contains("r"), "{error}");
}

#[tokio::test]
async fn a_template_preview_shows_the_wording_and_the_request() {
    let api = Api::signed_in().await;
    let c = connection(&api, "ops").await;
    let (_, key) = source(&api).await;
    let ids = ingest(&api, &key, json!({"topic": "orders.failed", "title": "Order 17 failed", "labels": {"env": "prod"}, "payload": {"amount": 12}})).await;

    let (status, preview) = api
        .post(
            "/api/templates/preview",
            json!({"title": "[{{ labels.env }}] {{ title }}", "body": "amount {{ payload.amount }}", "event_id": ids[0], "connection_id": c}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["title"], "[prod] Order 17 failed");
    assert_eq!(preview["body"], "amount 12");
    assert_eq!(preview["error"], Value::Null);
    assert_eq!(preview["request"]["body"], "[prod] Order 17 failed");

    // A template that fails says why and still shows the default wording.
    let (_, failing) = api
        .post(
            "/api/templates/preview",
            json!({"title": "{{ title | nosuch }}", "body": "", "event_id": ids[0]}),
        )
        .await;
    assert!(
        failing["error"].as_str().unwrap().contains("title"),
        "{failing}"
    );
    assert_eq!(failing["title"], "Order 17 failed");

    // Without an event a sample is used, so the editor works on an empty instance.
    let (status, sample) = api
        .post(
            "/api/templates/preview",
            json!({"title": "{{ topic }}", "body": ""}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        sample["title"].as_str().is_some_and(|t| !t.is_empty()),
        "{sample}"
    );
}

#[tokio::test]
async fn a_custom_connector_can_be_defined_used_and_removed() {
    let api = Api::signed_in().await;
    let (status, error) = api
        .post(
            "/api/connector-definitions",
            json!({"definition": "kind: ["}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(error["message"].as_str().unwrap().contains("YAML"));
    let (status, _) = api
        .post(
            "/api/connector-definitions",
            json!({"definition": CHAT.replace("kind: chat", "kind: slack")}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (status, created) = api
        .post("/api/connector-definitions", json!({"definition": CHAT}))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let id = created["id"].as_str().unwrap();
    assert_eq!(
        (created["kind"].as_str(), created["name"].as_str()),
        (Some("chat"), Some("Chat"))
    );
    assert_eq!(
        api.post("/api/connector-definitions", json!({"definition": CHAT}))
            .await
            .0,
        StatusCode::CONFLICT
    );

    let (_, connectors) = api.get("/api/connectors").await;
    let chat = connectors
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["kind"] == "chat")
        .unwrap();
    assert_eq!(chat["custom"], true);

    let body = json!({"kind": "chat", "name": "team chat", "settings": {"base_url": "https://chat.example.com"}, "credentials": {"token": "t0p-secret"}});
    let (status, connection) = api.post("/api/connections", body).await;
    assert_eq!(status, StatusCode::CREATED, "{connection}");

    let (status, error) = api
        .delete(&format!("/api/connector-definitions/{id}"))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(
        error["message"].as_str().unwrap().contains("team chat"),
        "{error}"
    );

    let renamed = CHAT.replace("name: Chat", "name: Team chat");
    let (status, updated) = api
        .put(
            &format!("/api/connector-definitions/{id}"),
            json!({"definition": renamed}),
        )
        .await;
    assert_eq!(
        (status, updated["name"].as_str()),
        (StatusCode::OK, Some("Team chat"))
    );
    let (_, connectors) = api.get("/api/connectors").await;
    assert!(
        connectors
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["name"] == "Team chat")
    );

    api.delete(&format!(
        "/api/connections/{}",
        connection["id"].as_str().unwrap()
    ))
    .await;
    assert_eq!(
        api.delete(&format!("/api/connector-definitions/{id}"))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    let (_, connectors) = api.get("/api/connectors").await;
    assert!(
        connectors
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["kind"] != "chat")
    );
}

#[tokio::test]
async fn source_kinds_are_listed_and_an_adapter_source_receives_at_its_own_address() {
    let api = Api::signed_in().await;
    let (_, kinds) = api.get("/api/source-kinds").await;
    let names: Vec<&str> = kinds
        .as_array()
        .unwrap()
        .iter()
        .map(|k| k["kind"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "native",
            "alertmanager",
            "grafana",
            "github",
            "gitlab",
            "sentry",
            "json"
        ]
    );

    let (status, created) = api
        .post(
            "/api/sources",
            json!({"name": "prometheus", "kind": "alertmanager"}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["kind"], "alertmanager");
    let path = created["path"].as_str().unwrap();
    assert_eq!(path, format!("/v1/in/{}", created["id"].as_str().unwrap()));
    let request = Request::post(path)
        .header(header::AUTHORIZATION, format!("Bearer {}", created["key"].as_str().unwrap()))
        .body(Body::from(r#"{"alerts":[{"status":"firing","labels":{"alertname":"Down"},"annotations":{},"fingerprint":"f"}]}"#))
        .unwrap();
    let (status, reply) = api.send(request).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{reply}");
    let (_, requests) = api
        .get(&format!(
            "/api/sources/{}/requests",
            created["id"].as_str().unwrap()
        ))
        .await;
    assert_eq!(requests[0]["error"], Value::Null);

    assert_eq!(
        api.post("/api/sources", json!({"name": "x", "kind": "pigeon"}))
            .await
            .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(api.get("/api/sources").await.1[0]["path"], path);
}

#[tokio::test]
async fn a_source_can_use_a_secret_the_sender_generated() {
    let api = Api::signed_in().await;
    let (status, created) = api
        .post(
            "/api/sources",
            json!({"name": "sentry", "kind": "sentry", "key": "client-secret-from-sentry"}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["key"], "client-secret-from-sentry");
    let (status, _) = api
        .post(
            "/api/sources",
            json!({"name": "short", "kind": "sentry", "key": "abc"}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let id = created["id"].as_str().unwrap();
    let (status, rotated) = api
        .post(
            &format!("/api/sources/{id}/rotate"),
            json!({"key": "another-client-secret"}),
        )
        .await;
    assert_eq!(
        (status, rotated["key"].as_str()),
        (StatusCode::OK, Some("another-client-secret"))
    );
}

#[tokio::test]
async fn an_any_json_mapping_is_previewed_saved_and_validated() {
    let api = Api::signed_in().await;
    let (_, created) = api
        .post("/api/sources", json!({"name": "shop", "kind": "json"}))
        .await;
    let id = created["id"].as_str().unwrap();
    assert!(
        created["mapping"].as_str().unwrap().contains("fields:"),
        "{created}"
    );

    let mapping = "verify: {method: query, param: key}\nfields:\n  topic: \"'shop.' ~ payload.kind\"\n  title: payload.title\n  labels: \"{'event': headers['x-kind']}\"\n";
    let (status, preview) = api
        .post(
            "/api/sources/preview-mapping",
            json!({"mapping": mapping, "body": r#"{"kind":"order","title":"Order 17 failed"}"#, "headers": {"x-kind": "created"}}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["error"], Value::Null);
    assert_eq!(preview["events"][0]["topic"], "shop.order");
    assert_eq!(preview["events"][0]["labels"]["event"], "created");

    let (_, failing) = api
        .post(
            "/api/sources/preview-mapping",
            json!({"mapping": mapping, "body": r#"{"kind":"order"}"#}),
        )
        .await;
    assert!(
        failing["error"].as_str().unwrap().starts_with("title:"),
        "{failing}"
    );
    let (_, bad_json) = api
        .post(
            "/api/sources/preview-mapping",
            json!({"mapping": mapping, "body": "{"}),
        )
        .await;
    assert!(
        bad_json["error"].as_str().unwrap().contains("JSON"),
        "{bad_json}"
    );

    let (status, saved) = api
        .put(
            &format!("/api/sources/{id}/mapping"),
            json!({"mapping": mapping}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["mapping"], mapping);
    let (status, error) = api
        .put(
            &format!("/api/sources/{id}/mapping"),
            json!({"mapping": "fields: {topic: \"(\", title: \"'x'\"}"}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        error["message"].as_str().unwrap().contains("fields.topic"),
        "{error}"
    );

    let (_, native) = api.post("/api/sources", json!({"name": "native"})).await;
    let (status, _) = api
        .put(
            &format!("/api/sources/{}/mapping", native["id"].as_str().unwrap()),
            json!({"mapping": mapping}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn the_route_tester_explains_every_route_without_writing_anything() {
    let api = Api::signed_in().await;
    let c = connection(&api, "ops").await;
    catch_all_route(&api, &c).await;
    let (_, key) = source(&api).await;
    let ids = ingest(&api, &key, json!({"topic": "orders.failed", "title": "Order 17 failed", "severity": "error", "payload": {"amount": 500}})).await;
    process(&api).await;

    // Against the stored routes.
    let (status, stored) = api
        .post("/api/routes/test", json!({"event_id": ids[0]}))
        .await;
    assert_eq!(status, StatusCode::OK, "{stored}");
    assert_eq!(stored["results"][0]["outcome"], "matched");
    assert_eq!(stored["results"][0]["targets"][0]["connection_name"], "ops");
    assert_eq!(
        stored["results"][0]["targets"][0]["title"],
        "Order 17 failed"
    );

    // Against routes that are not saved, with a sample event.
    let routes = json!([
        {"name": "big orders", "stop": true, "match": {"expr": "payload.amount > 100"}, "targets": [{"connection_id": c}]},
        {"name": "broken", "match": {"expr": "payload | nosuch"}, "targets": [{"connection_id": c}]},
        {"name": "rest", "match": {}, "targets": [{"connection_id": c}]},
        {"name": "off", "enabled": false, "match": {}, "targets": [{"connection_id": c}]}
    ]);
    let event = json!({"topic": "orders.failed", "title": "Sample", "payload": {"amount": 500}});
    let (status, unsaved) = api
        .post(
            "/api/routes/test",
            json!({"event": event, "routes": routes}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{unsaved}");
    let outcomes: Vec<&str> = unsaved["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["outcome"].as_str().unwrap())
        .collect();
    assert_eq!(outcomes, ["matched", "skipped", "skipped", "disabled"]);

    let small = json!({"topic": "orders.failed", "title": "Sample", "payload": {"amount": 5}});
    let (_, unsaved) = api
        .post(
            "/api/routes/test",
            json!({"event": small, "routes": routes}),
        )
        .await;
    let results = unsaved["results"].as_array().unwrap();
    assert_eq!(results[0]["outcome"], "not_matched");
    assert_eq!(results[1]["outcome"], "error");
    assert!(results[1]["error"].as_str().unwrap().contains("nosuch"));
    assert_eq!(results[2]["outcome"], "matched");

    // Nothing was written: still one stored route and one delivery.
    assert_eq!(api.get("/api/routes").await.1.as_array().unwrap().len(), 1);
    let (_, journey) = api.get(&format!("/api/events/{}", ids[0])).await;
    assert_eq!(journey["notifications"].as_array().unwrap().len(), 1);

    let (status, _) = api.post("/api/routes/test", json!({})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = api
        .post(
            "/api/routes/test",
            json!({"event": {"topic": "bad topic!", "title": "x"}}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn routes_show_their_last_expression_error() {
    let api = Api::signed_in().await;
    let c = connection(&api, "ops").await;
    let body = json!({"name": "broken", "match": {"expr": "payload | nosuch"}, "targets": [{"connection_id": c}]});
    let (status, created) = api.post("/api/routes", body).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["last_error"], Value::Null);
    let (_, key) = source(&api).await;
    ingest(&api, &key, json!({"topic": "a", "title": "x"})).await;
    process(&api).await;
    let (_, routes) = api.get("/api/routes").await;
    assert!(
        routes[0]["last_error"].as_str().unwrap().contains("nosuch"),
        "{routes}"
    );
    assert!(routes[0]["last_error_at"].is_i64());

    let (status, error) = api
        .post(
            "/api/routes",
            json!({"name": "bad", "match": {"expr": "a =="}, "targets": [{"connection_id": c}]}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        error["message"].as_str().unwrap().contains("expression"),
        "{error}"
    );
}

#[tokio::test]
async fn an_event_shows_the_message_each_target_received() {
    let api = Api::signed_in().await;
    let c = connection(&api, "ops").await;
    catch_all_route(&api, &c).await;
    let (_, key) = source(&api).await;
    let ids = ingest(
        &api,
        &key,
        json!({"topic": "orders.failed", "title": "Order 17 failed"}),
    )
    .await;
    process(&api).await;
    let (status, previews) = api.get(&format!("/api/events/{}/preview", ids[0])).await;
    assert_eq!(status, StatusCode::OK, "{previews}");
    assert_eq!(previews.as_array().unwrap().len(), 1);
    assert_eq!(previews[0]["connection_name"], "ops");
    assert_eq!(previews[0]["title"], "Order 17 failed");
    assert_eq!(previews[0]["request"]["method"], "POST");
    assert!(!previews.to_string().contains("t0p-secret"));
    assert_eq!(
        api.get("/api/events/nope/preview").await.0,
        StatusCode::NOT_FOUND
    );
    let (_, page) = api.get("/api/events").await;
    assert_eq!(page["events"][0]["targets"], json!(["ops"]));
}

#[tokio::test]
async fn connections_report_how_their_deliveries_went() {
    let api = Api::signed_in().await;
    let c = connection(&api, "ops").await;
    catch_all_route(&api, &c).await;
    let (_, key) = source(&api).await;
    api.connector
        .outcomes
        .lock()
        .unwrap()
        .push_back(SendOutcome::Permanent {
            status: Some(403),
            detail: "HTTP 403: nope".into(),
        });
    ingest(
        &api,
        &key,
        json!([{"topic": "a", "title": "one"}, {"topic": "a", "title": "two"}]),
    )
    .await;
    process(&api).await;
    let (_, connections) = api.get("/api/connections").await;
    let health = &connections[0]["health"];
    assert_eq!(
        (health["delivered"].as_i64(), health["failed"].as_i64()),
        (Some(1), Some(1)),
        "{connections}"
    );
    assert_eq!(health["last_error"], "HTTP 403: nope");
}

#[tokio::test]
async fn every_configuration_change_is_recorded_and_can_be_rolled_back() {
    let api = Api::signed_in().await;
    let c = connection(&api, "ops").await;
    let route = catch_all_route(&api, &c).await;
    let (_, revisions) = api.get("/api/config/revisions").await;
    let revisions = revisions.as_array().unwrap().clone();
    assert_eq!(revisions.len(), 2);
    assert_eq!(revisions[0]["summary"], "added route everything");
    assert_eq!(revisions[0]["author"], crate::common::EMAIL);
    let with_route = revisions[0]["id"].as_str().unwrap().to_owned();

    api.delete(&format!("/api/routes/{route}")).await;
    assert!(
        api.get("/api/routes")
            .await
            .1
            .as_array()
            .unwrap()
            .is_empty()
    );

    let (status, shown) = api
        .get(&format!("/api/config/revisions/{with_route}"))
        .await;
    assert_eq!(status, StatusCode::OK);
    let yaml = shown["config"].as_str().unwrap();
    assert!(yaml.contains("everything"), "{yaml}");
    assert!(!yaml.contains("credentials"), "{yaml}");

    let (status, _) = api
        .post(
            &format!("/api/config/revisions/{with_route}/rollback"),
            json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, routes) = api.get("/api/routes").await;
    assert_eq!(routes[0]["id"], route.as_str());
    let (_, revisions) = api.get("/api/config/revisions").await;
    assert_eq!(revisions.as_array().unwrap().len(), 4);
    assert!(
        revisions[0]["summary"]
            .as_str()
            .unwrap()
            .starts_with("rolled back")
    );
    assert_eq!(
        api.post("/api/config/revisions/nope/rollback", json!({}))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn export_has_no_credentials_and_import_keeps_the_stored_ones() {
    let api = Api::signed_in().await;
    let c = connection(&api, "ops").await;
    catch_all_route(&api, &c).await;
    api.post(
        "/api/templates",
        json!({"name": "Ops", "title": "{{ title }}", "body": ""}),
    )
    .await;

    let (status, exported) = api.get("/api/config/export").await;
    assert_eq!(status, StatusCode::OK);
    let yaml = exported["yaml"].as_str().unwrap().to_owned();
    assert!(
        yaml.contains("everything") && yaml.contains("Ops"),
        "{yaml}"
    );
    assert!(
        !yaml.contains("t0p-secret") && !yaml.contains("credentials"),
        "{yaml}"
    );

    // Change things, then import the export: the old state comes back and the connection still works.
    let (_, routes) = api.get("/api/routes").await;
    api.delete(&format!(
        "/api/routes/{}",
        routes[0]["id"].as_str().unwrap()
    ))
    .await;
    api.put(
        &format!("/api/connections/{c}"),
        json!({"name": "renamed", "settings": {"room": "other"}}),
    )
    .await;
    let (status, reply) = api.post("/api/config/import", json!({"yaml": yaml})).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{reply}");
    assert_eq!(api.get("/api/routes").await.1[0]["name"], "everything");
    let (_, connections) = api.get("/api/connections").await;
    assert_eq!(
        (
            connections[0]["name"].as_str(),
            connections[0]["has_credentials"].as_bool()
        ),
        (Some("ops"), Some(true))
    );
    let (_, test) = api
        .post(&format!("/api/connections/{c}/test"), json!({}))
        .await;
    assert_eq!(test["ok"], true);
    let sent = api.connector.sent.lock().unwrap().clone();
    assert_eq!(sent.last().unwrap().1.credentials["token"], "t0p-secret");
}

#[tokio::test]
async fn an_import_that_does_not_hold_together_is_refused_whole() {
    let api = Api::signed_in().await;
    let c = connection(&api, "ops").await;
    catch_all_route(&api, &c).await;
    for (yaml, reason) in [
        ("routes: [", "YAML"),
        (
            "routes:\n  - {id: r1, name: r, position: 1, enabled: true, stop: false, match: {expr: 'a =='}, targets: [{connection_id: x}]}\n",
            "expression",
        ),
        (
            "routes:\n  - {id: r1, name: r, position: 1, enabled: true, stop: false, match: {}, targets: [{connection_id: missing}]}\n",
            "missing",
        ),
        (
            "connections:\n  - {id: c9, kind: pigeon, name: p, settings: {}}\n",
            "pigeon",
        ),
        (
            "templates:\n  - {id: t1, name: t, title: '{{ x ', body: ''}\n",
            "title",
        ),
    ] {
        let (status, error) = api.post("/api/config/import", json!({"yaml": yaml})).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{yaml}");
        assert!(
            error["message"].as_str().unwrap().contains(reason),
            "{error}"
        );
    }
    assert_eq!(api.get("/api/routes").await.1.as_array().unwrap().len(), 1);
    assert_eq!(
        api.get("/api/connections")
            .await
            .1
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn a_rollback_keeps_the_credentials_a_connection_has_now() {
    let api = Api::signed_in().await;
    let c = connection(&api, "ops").await;
    let route = catch_all_route(&api, &c).await;
    let (_, revisions) = api.get("/api/config/revisions").await;
    let before_rotation = revisions[0]["id"].as_str().unwrap().to_owned();

    // The token is rotated, then an unrelated change is undone by rolling back.
    let rotated =
        json!({"name": "ops", "settings": {"room": "ops"}, "credentials": {"token": "new-token"}});
    assert_eq!(
        api.put(&format!("/api/connections/{c}"), rotated).await.0,
        StatusCode::OK
    );
    api.delete(&format!("/api/routes/{route}")).await;
    let (status, _) = api
        .post(
            &format!("/api/config/revisions/{before_rotation}/rollback"),
            json!({}),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    api.post(&format!("/api/connections/{c}/test"), json!({}))
        .await;
    let sent = api.connector.sent.lock().unwrap().clone();
    assert_eq!(sent.last().unwrap().1.credentials["token"], "new-token");

    // A connection that was deleted comes back with the credentials it had.
    api.delete(&format!("/api/routes/{route}")).await;
    api.delete(&format!("/api/connections/{c}")).await;
    api.post(
        &format!("/api/config/revisions/{before_rotation}/rollback"),
        json!({}),
    )
    .await;
    api.post(&format!("/api/connections/{c}/test"), json!({}))
        .await;
    let sent = api.connector.sent.lock().unwrap().clone();
    assert_eq!(sent.last().unwrap().1.credentials["token"], "t0p-secret");
}

#[tokio::test]
async fn the_route_tester_refuses_a_policy_that_could_not_be_saved() {
    let api = Api::signed_in().await;
    let c = connection(&api, "ops").await;
    let event = json!({"topic": "a", "title": "x"});
    let routes = json!([{"name": "r", "match": {}, "targets": [{"connection_id": c}], "shaping": {"rate_limit": {"max": 1, "window_s": 9_000_000_000_000_000_i64}}}]);
    let (status, error) = api
        .post(
            "/api/routes/test",
            json!({"event": event, "routes": routes}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{error}");
    assert!(
        error["message"].as_str().unwrap().contains("rate limit"),
        "{error}"
    );
}
