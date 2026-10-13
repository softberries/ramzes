use std::time::Duration;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use futures::StreamExt;
use ramzes_connectors::SendOutcome;
use serde_json::{Value, json};

use crate::common::Api;

/// Creates a scripted connection and returns its id.
async fn connection(api: &Api, name: &str) -> String {
    let body = json!({"kind": "scripted", "name": name, "settings": {"room": "ops"}, "credentials": {"token": "t0p-secret"}});
    let (status, created) = api.post("/api/connections", body).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    created["id"].as_str().unwrap().to_owned()
}

async fn catch_all_route(api: &Api, connection_id: &str) -> String {
    let body =
        json!({"name": "everything", "match": {}, "targets": [{"connection_id": connection_id}]});
    let (status, created) = api.post("/api/routes", body).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    created["id"].as_str().unwrap().to_owned()
}

/// Creates a source and returns `(id, key)`.
async fn source(api: &Api) -> (String, String) {
    let (status, created) = api
        .post("/api/sources", json!({"name": "orders-api"}))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    (
        created["id"].as_str().unwrap().to_owned(),
        created["key"].as_str().unwrap().to_owned(),
    )
}

/// Sends events through the real ingest endpoint and returns their ids.
async fn ingest(api: &Api, key: &str, body: Value) -> Vec<String> {
    let request = Request::post("/v1/events")
        .header(header::AUTHORIZATION, format!("Bearer {key}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let (status, reply) = api.send(request).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{reply}");
    reply["ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect()
}

async fn process(api: &Api) {
    api.engine.route_once().await.unwrap();
    api.engine.deliver_once().await.unwrap();
}

async fn only(api: &Api, query: &str) -> Vec<String> {
    ids(&api.get(&format!("/api/events?{query}")).await.1)
}

fn ids(page: &Value) -> Vec<String> {
    page["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["id"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn every_endpoint_requires_authentication() {
    let api = Api::signed_in().await;
    api.forget_cookie();
    let endpoints = [
        (Method::GET, "/api/sources"),
        (Method::POST, "/api/sources"),
        (Method::POST, "/api/sources/x/rotate"),
        (Method::DELETE, "/api/sources/x"),
        (Method::GET, "/api/sources/x/requests"),
        (Method::GET, "/api/connectors"),
        (Method::GET, "/api/connections"),
        (Method::POST, "/api/connections"),
        (Method::PUT, "/api/connections/x"),
        (Method::DELETE, "/api/connections/x"),
        (Method::POST, "/api/connections/x/test"),
        (Method::GET, "/api/routes"),
        (Method::POST, "/api/routes"),
        (Method::PUT, "/api/routes/x"),
        (Method::DELETE, "/api/routes/x"),
        (Method::POST, "/api/routes/reorder"),
        (Method::GET, "/api/events"),
        (Method::GET, "/api/events/x"),
        (Method::POST, "/api/events/x/replay"),
        (Method::GET, "/api/failures"),
        (Method::POST, "/api/deliveries/x/replay"),
        (Method::POST, "/api/failures/replay"),
        (Method::GET, "/api/settings"),
        (Method::PUT, "/api/settings"),
        (Method::GET, "/api/users"),
        (Method::POST, "/api/users"),
        (Method::DELETE, "/api/users/x"),
        (Method::GET, "/api/tokens"),
        (Method::POST, "/api/tokens"),
        (Method::DELETE, "/api/tokens/x"),
        (Method::GET, "/api/stream"),
    ];
    for (method, path) in endpoints {
        let (status, body) = api.call(method.clone(), path, Some(json!({}))).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {path}: {body}");
    }
}

#[tokio::test]
async fn the_openapi_document_lists_every_path() {
    let api = Api::new().await;
    let paths = api.openapi["paths"].as_object().unwrap();
    let expected = [
        ("/api/setup", vec!["get", "post"]),
        ("/api/auth/login", vec!["post"]),
        ("/api/auth/logout", vec!["post"]),
        ("/api/auth/me", vec!["get"]),
        ("/api/sources", vec!["get", "post"]),
        ("/api/sources/{id}", vec!["delete"]),
        ("/api/sources/{id}/rotate", vec!["post"]),
        ("/api/sources/{id}/requests", vec!["get"]),
        ("/api/connectors", vec!["get"]),
        ("/api/connections", vec!["get", "post"]),
        ("/api/connections/{id}", vec!["put", "delete"]),
        ("/api/connections/{id}/test", vec!["post"]),
        ("/api/routes", vec!["get", "post"]),
        ("/api/routes/{id}", vec!["put", "delete"]),
        ("/api/routes/reorder", vec!["post"]),
        ("/api/events", vec!["get"]),
        ("/api/events/{id}", vec!["get"]),
        ("/api/events/{id}/replay", vec!["post"]),
        ("/api/failures", vec!["get"]),
        ("/api/failures/replay", vec!["post"]),
        ("/api/deliveries/{id}/replay", vec!["post"]),
        ("/api/settings", vec!["get", "put"]),
        ("/api/users", vec!["get", "post"]),
        ("/api/users/{id}", vec!["delete"]),
        ("/api/tokens", vec!["get", "post"]),
        ("/api/tokens/{id}", vec!["delete"]),
        ("/api/stream", vec!["get"]),
    ];
    for (path, methods) in expected {
        let item = paths
            .get(path)
            .unwrap_or_else(|| panic!("{path} is missing from the OpenAPI document"));
        for method in methods {
            assert!(item.get(method).is_some(), "{method} {path} is missing");
        }
    }
    assert_eq!(
        paths.len(),
        27,
        "undocumented or unexpected paths: {:?}",
        paths.keys().collect::<Vec<_>>()
    );
    let (status, served) = api.get("/api/openapi.json").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(served, api.openapi);
}

#[tokio::test]
async fn a_new_source_key_works_for_ingest_and_is_shown_only_once() {
    let api = Api::signed_in().await;
    let (id, key) = source(&api).await;
    assert!(key.starts_with("rz_src_"));
    ingest(&api, &key, json!({"topic": "a", "title": "t"})).await;

    let (_, listed) = api.get("/api/sources").await;
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(listed[0]["id"], json!(id));
    assert_eq!(listed[0]["key_prefix"], json!(key[..11]));
    assert!(!listed.to_string().contains(&key));
}

#[tokio::test]
async fn rotating_a_source_key_replaces_it() {
    let api = Api::signed_in().await;
    let (id, old_key) = source(&api).await;
    let (status, rotated) = api
        .post(&format!("/api/sources/{id}/rotate"), json!({}))
        .await;
    assert_eq!(status, StatusCode::OK);
    let new_key = rotated["key"].as_str().unwrap();
    assert_ne!(new_key, old_key);
    ingest(&api, new_key, json!({"topic": "a", "title": "t"})).await;

    let stale = Request::post("/v1/events")
        .header(header::AUTHORIZATION, format!("Bearer {old_key}"))
        .body(Body::from(json!({"topic": "a", "title": "t"}).to_string()))
        .unwrap();
    assert_eq!(api.send(stale).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn sources_validate_delete_and_show_rejected_requests() {
    let api = Api::signed_in().await;
    assert_eq!(
        api.post("/api/sources", json!({"name": "  "})).await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let (id, key) = source(&api).await;

    let bad = Request::post("/v1/events")
        .header(header::AUTHORIZATION, format!("Bearer {key}"))
        .body(Body::from("{not json"))
        .unwrap();
    api.send(bad).await;
    let (status, requests) = api.get(&format!("/api/sources/{id}/requests")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(requests[0]["body"], json!("{not json"));
    assert!(requests[0]["error"].as_str().unwrap().contains("JSON"));

    assert_eq!(
        api.delete(&format!("/api/sources/{id}")).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        api.delete(&format!("/api/sources/{id}")).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(api.get("/api/sources").await.1, json!([]));
}

#[tokio::test]
async fn connectors_are_described() {
    let api = Api::signed_in().await;
    let (status, connectors) = api.get("/api/connectors").await;
    assert_eq!(status, StatusCode::OK);
    let kinds: Vec<_> = connectors
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, vec!["scripted", "slack", "webhook"]);
    let slack = &connectors[1];
    assert_eq!(slack["fields"][0]["key"], json!("webhook_url"));
    assert_eq!(slack["fields"][0]["secret"], json!(true));
}

#[tokio::test]
async fn connection_credentials_never_leave_and_survive_an_update_without_them() {
    let api = Api::signed_in().await;
    let id = connection(&api, "ops room").await;

    let (_, listed) = api.get("/api/connections").await;
    assert_eq!(listed[0]["name"], json!("ops room"));
    assert_eq!(listed[0]["settings"], json!({"room": "ops"}));
    assert_eq!(listed[0]["has_credentials"], json!(true));
    assert!(listed[0].get("credentials").is_none());
    assert!(!listed.to_string().contains("t0p-secret"));

    let update = json!({"name": "renamed", "settings": {"room": "war-room"}});
    let (status, updated) = api.put(&format!("/api/connections/{id}"), update).await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["name"], json!("renamed"));
    assert!(!updated.to_string().contains("t0p-secret"));

    // The stored token is still the one given at creation.
    let (status, result) = api
        .post(&format!("/api/connections/{id}/test"), json!({}))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result["ok"], json!(true));
    let sent = api.connector.sent.lock().unwrap().clone();
    assert_eq!(sent[0].1.credentials, json!({"token": "t0p-secret"}));
    assert_eq!(sent[0].1.settings, json!({"room": "war-room"}));

    // And it is encrypted in the database.
    let stored = api
        .store
        .get_connection(&api.ws, &id)
        .await
        .unwrap()
        .unwrap();
    assert!(!stored.credentials.contains("t0p-secret"));
}

#[tokio::test]
async fn connections_are_validated() {
    let api = Api::signed_in().await;
    let cases = [
        json!({"kind": "carrier-pigeon", "name": "x", "settings": {}, "credentials": {}}),
        json!({"kind": "scripted", "name": "", "settings": {"room": "ops"}, "credentials": {"token": "t"}}),
        json!({"kind": "scripted", "name": "x", "settings": {}, "credentials": {"token": "t"}}),
        json!({"kind": "scripted", "name": "x", "settings": {"room": "ops"}, "credentials": {}}),
        json!({"kind": "scripted", "name": "x", "settings": ["not", "an", "object"], "credentials": {"token": "t"}}),
    ];
    for body in cases {
        let (status, reply) = api.post("/api/connections", body.clone()).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{body} -> {reply}"
        );
    }
    assert_eq!(api.get("/api/connections").await.1, json!([]));
    let update = json!({"name": "x", "settings": {"room": "ops"}});
    assert_eq!(
        api.put("/api/connections/nope", update).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        api.post("/api/connections/nope/test", json!({})).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        api.delete("/api/connections/nope").await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn the_connection_test_reports_failure_from_the_connector() {
    let api = Api::signed_in().await;
    let id = connection(&api, "ops").await;
    let rejected = SendOutcome::Permanent {
        status: Some(401),
        detail: "HTTP 401: invalid_token".into(),
    };
    api.connector.outcomes.lock().unwrap().push_back(rejected);
    let (status, result) = api
        .post(&format!("/api/connections/{id}/test"), json!({}))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        result,
        json!({"ok": false, "status": 401, "detail": "HTTP 401: invalid_token"})
    );
}

#[tokio::test]
async fn routes_crud_and_reorder() {
    let api = Api::signed_in().await;
    let c = connection(&api, "ops").await;
    let mut created = Vec::new();
    for name in ["a", "b", "c"] {
        let body = json!({
            "name": name, "stop": name == "b",
            "match": {"topic": "orders.*", "min_severity": "error",
                      "labels": [{"op": "equals", "key": "env", "value": "prod"}]},
            "targets": [{"connection_id": c, "destination": "#ops"}]
        });
        let (status, route) = api.post("/api/routes", body).await;
        assert_eq!(status, StatusCode::CREATED, "{route}");
        assert_eq!(route["enabled"], json!(true));
        created.push(route["id"].as_str().unwrap().to_owned());
    }
    let names = |list: &Value| {
        list.as_array()
            .unwrap()
            .iter()
            .map(|r| r["name"].clone())
            .collect::<Vec<_>>()
    };
    let (_, list) = api.get("/api/routes").await;
    assert_eq!(names(&list), vec![json!("a"), json!("b"), json!("c")]);
    assert_eq!(list[1]["stop"], json!(true));
    assert_eq!(
        list[0]["match"]["labels"][0],
        json!({"op": "equals", "key": "env", "value": "prod"})
    );
    assert_eq!(list[0]["targets"][0]["destination"], json!("#ops"));

    let reorder = json!({"ids": [created[2], created[0], created[1]]});
    assert_eq!(
        api.post("/api/routes/reorder", reorder).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        names(&api.get("/api/routes").await.1),
        vec![json!("c"), json!("a"), json!("b")]
    );
    let partial = json!({"ids": [created[0]]});
    assert_eq!(
        api.post("/api/routes/reorder", partial).await.0,
        StatusCode::CONFLICT
    );

    let update =
        json!({"name": "a2", "enabled": false, "match": {}, "targets": [{"connection_id": c}]});
    let (status, updated) = api
        .put(&format!("/api/routes/{}", created[0]), update)
        .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(
        (updated["name"].clone(), updated["enabled"].clone()),
        (json!("a2"), json!(false))
    );
    assert_eq!(
        names(&api.get("/api/routes").await.1),
        vec![json!("c"), json!("a2"), json!("b")]
    );

    assert_eq!(
        api.delete(&format!("/api/routes/{}", created[1])).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        api.delete(&format!("/api/routes/{}", created[1])).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        names(&api.get("/api/routes").await.1),
        vec![json!("c"), json!("a2")]
    );
}

#[tokio::test]
async fn routes_are_validated() {
    let api = Api::signed_in().await;
    let c = connection(&api, "ops").await;
    let target = json!([{"connection_id": c}]);
    let cases = [
        (json!({"name": "", "match": {}, "targets": target}), "name"),
        (json!({"name": "x", "match": {}, "targets": []}), "target"),
        (
            json!({"name": "x", "match": {}, "targets": [{"connection_id": "nope"}]}),
            "connection",
        ),
        (
            json!({"name": "x", "targets": target,
                   "match": {"labels": [{"op": "matches", "key": "k", "pattern": "("}]}}),
            "regular expression",
        ),
        (
            json!({"name": "x", "match": {"topic": ""}, "targets": target}),
            "topic",
        ),
    ];
    for (body, mentions) in cases {
        let (status, reply) = api.post("/api/routes", body.clone()).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{body} -> {reply}"
        );
        assert!(
            reply["message"].as_str().unwrap().contains(mentions),
            "{reply} should mention {mentions}"
        );
    }
    assert_eq!(api.get("/api/routes").await.1, json!([]));
}

#[tokio::test]
async fn the_feed_filters_and_pages() {
    let api = Api::signed_in().await;
    let (source_id, key) = source(&api).await;
    let sent = ingest(
        &api,
        &key,
        json!([
            {"topic": "orders.failed", "title": "Charge failed", "severity": "error", "labels": {"env": "prod"}},
            {"topic": "orders.created", "title": "Order created", "labels": {"env": "dev"}},
            {"topic": "deploy.finished", "title": "Deploy finished"}
        ]),
    )
    .await;
    let newest_first: Vec<String> = sent.iter().rev().cloned().collect();

    let (status, page) = api.get("/api/events").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ids(&page), newest_first);
    assert_eq!(page["next_before"], Value::Null);
    assert_eq!(page["events"][2]["labels"], json!({"env": "prod"}));
    assert_eq!(page["events"][2]["severity"], json!("error"));

    let (_, first) = api.get("/api/events?limit=2").await;
    assert_eq!(ids(&first), newest_first[..2]);
    let before = first["next_before"].as_str().unwrap();
    let (_, second) = api
        .get(&format!("/api/events?limit=2&before={before}"))
        .await;
    assert_eq!(ids(&second), newest_first[2..]);

    assert_eq!(
        only(&api, "topic=orders.*").await,
        vec![sent[1].clone(), sent[0].clone()]
    );
    assert_eq!(
        only(&api, "min_severity=error").await,
        vec![sent[0].clone()]
    );
    assert_eq!(only(&api, "label=env%3Dprod").await, vec![sent[0].clone()]);
    assert_eq!(only(&api, "q=deploy").await, vec![sent[2].clone()]);
    assert_eq!(only(&api, &format!("source={source_id}")).await.len(), 3);
    assert!(only(&api, "source=other").await.is_empty());
    assert_eq!(only(&api, "outcome=pending").await.len(), 3);
    assert!(only(&api, "outcome=delivered").await.is_empty());
    let now = 1_791_884_467_000_i64;
    assert_eq!(
        only(&api, &format!("since={now}&until={now}")).await.len(),
        3
    );
    assert!(only(&api, &format!("since={}", now + 1)).await.is_empty());

    for bad in [
        "min_severity=loud",
        "outcome=maybe",
        "label=no-equals-sign",
        "limit=0",
        "since=yesterday",
    ] {
        let (status, reply) = api.get(&format!("/api/events?{bad}")).await;
        assert!(status.is_client_error(), "{bad} -> {status} {reply}");
    }
}

#[tokio::test]
async fn the_journey_shows_route_notification_delivery_and_attempt() {
    let api = Api::signed_in().await;
    let c = connection(&api, "ops").await;
    let route = catch_all_route(&api, &c).await;
    let (_, key) = source(&api).await;
    let id = ingest(
        &api,
        &key,
        json!({"topic": "orders.failed", "title": "Charge failed"}),
    )
    .await
    .remove(0);
    process(&api).await;

    let (status, journey) = api.get(&format!("/api/events/{id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(journey["event"]["title"], json!("Charge failed"));
    assert_eq!(journey["routed"], json!(true));
    let n = &journey["notifications"][0];
    assert_eq!(
        (n["route_id"].clone(), n["route_name"].clone()),
        (json!(route), json!("everything"))
    );
    assert_eq!(n["connection_id"], json!(c));
    let d = &n["deliveries"][0];
    assert_eq!(d["state"], json!("delivered"));
    assert_eq!(d["attempts"].as_array().unwrap().len(), 1);
    assert_eq!(d["attempts"][0]["outcome"], json!("delivered"));
    assert_eq!(d["attempts"][0]["status"], json!(200));

    assert_eq!(
        ids(&api.get("/api/events?outcome=delivered").await.1),
        vec![id]
    );
    assert_eq!(api.get("/api/events/nope").await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn failures_are_listed_and_can_be_replayed() {
    let api = Api::signed_in().await;
    let c = connection(&api, "ops").await;
    catch_all_route(&api, &c).await;
    let (_, key) = source(&api).await;
    for _ in 0..2 {
        let rejected = SendOutcome::Permanent {
            status: Some(401),
            detail: "HTTP 401: invalid_token".into(),
        };
        api.connector.outcomes.lock().unwrap().push_back(rejected);
    }
    let sent = ingest(
        &api,
        &key,
        json!([{"topic": "a", "title": "first"}, {"topic": "a", "title": "second"}]),
    )
    .await;
    process(&api).await;

    let (status, failures) = api.get("/api/failures").await;
    assert_eq!(status, StatusCode::OK);
    let failures = failures.as_array().unwrap().clone();
    assert_eq!(failures.len(), 2);
    assert_eq!(failures[0]["state"], json!("failed"));
    assert_eq!(failures[0]["last_error"], json!("HTTP 401: invalid_token"));
    assert_eq!(failures[0]["connection_name"], json!("ops"));
    assert_eq!(failures[0]["route_name"], json!("everything"));
    assert!(sent.contains(&failures[0]["event_id"].as_str().unwrap().to_owned()));

    // Replay one, then the other through the bulk endpoint together with an unknown id.
    let one = failures[0]["id"].as_str().unwrap();
    let (status, fresh) = api
        .post(&format!("/api/deliveries/{one}/replay"), json!({}))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fresh["state"], json!("pending"));
    let bulk = json!({"delivery_ids": [failures[1]["id"], "nope"]});
    let (status, result) = api.post("/api/failures/replay", bulk).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result, json!({"replayed": 1, "missing": ["nope"]}));

    api.engine.deliver_once().await.unwrap();
    assert_eq!(api.connector.sent.lock().unwrap().len(), 4);
    assert_eq!(ids(&api.get("/api/events?outcome=failed").await.1).len(), 2);
    assert_eq!(
        api.post("/api/deliveries/nope/replay", json!({})).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn replaying_an_event_routes_it_through_current_routes() {
    let api = Api::signed_in().await;
    let (_, key) = source(&api).await;
    let id = ingest(&api, &key, json!({"topic": "a", "title": "t"}))
        .await
        .remove(0);
    process(&api).await;
    assert_eq!(
        ids(&api.get("/api/events?outcome=unrouted").await.1),
        vec![id.clone()]
    );

    let c = connection(&api, "ops").await;
    catch_all_route(&api, &c).await;
    let (status, result) = api
        .post(&format!("/api/events/{id}/replay"), json!({}))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result, json!({"deliveries": 1}));
    api.engine.deliver_once().await.unwrap();
    let (_, journey) = api.get(&format!("/api/events/{id}")).await;
    assert_eq!(journey["notifications"][0]["replay"], json!(true));
    assert_eq!(
        journey["notifications"][0]["deliveries"][0]["state"],
        json!("delivered")
    );
    assert_eq!(
        api.post("/api/events/nope/replay", json!({})).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn settings_round_trip_and_are_validated() {
    let api = Api::signed_in().await;
    let defaults = json!({"retention_days": 30, "retention_overrides": {}, "public_url": null});
    assert_eq!(api.get("/api/settings").await, (StatusCode::OK, defaults));

    let new = json!({"retention_days": 7, "retention_overrides": {"audit": 0, "noise": 1},
                     "public_url": "https://ramzes.example.com/"});
    let (status, saved) = api.put("/api/settings", new).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["public_url"], json!("https://ramzes.example.com"));
    assert_eq!(api.get("/api/settings").await.1, saved);

    for bad in [
        json!({"retention_days": -1, "retention_overrides": {}, "public_url": null}),
        json!({"retention_days": 30, "retention_overrides": {"a": -5}, "public_url": null}),
        json!({"retention_days": 30, "retention_overrides": {}, "public_url": "not a url"}),
        json!({"retention_days": 30, "retention_overrides": {}, "public_url": "ftp://example.com"}),
    ] {
        assert_eq!(
            api.put("/api/settings", bad.clone()).await.0,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{bad}"
        );
    }
    assert_eq!(api.get("/api/settings").await.1, saved);
}

#[tokio::test]
async fn users_can_be_added_signed_in_and_removed_but_never_all() {
    let api = Api::signed_in().await;
    let new = json!({"email": "Second@Example.com", "password": "another long password"});
    let (status, created) = api.post("/api/users", new.clone()).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["email"], json!("second@example.com"));
    assert!(created.get("password_hash").is_none());
    assert_eq!(api.post("/api/users", new).await.0, StatusCode::CONFLICT);
    let short = json!({"email": "third@example.com", "password": "short"});
    assert_eq!(
        api.post("/api/users", short).await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    let (_, users) = api.get("/api/users").await;
    assert_eq!(users.as_array().unwrap().len(), 2);
    assert!(!users.to_string().contains("argon2"));
    let first = users[0]["id"].as_str().unwrap().to_owned();
    let second = users[1]["id"].as_str().unwrap().to_owned();

    assert_eq!(
        api.delete(&format!("/api/users/{second}")).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        api.delete(&format!("/api/users/{first}")).await.0,
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn an_api_token_authenticates_without_cookie_or_request_header() {
    let api = Api::signed_in().await;
    let (status, created) = api.post("/api/tokens", json!({"name": "ci"})).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let value = created["value"].as_str().unwrap().to_owned();
    assert!(value.starts_with("rz_tok_"));
    assert_eq!(created["created_by"], json!("kris@example.com"));

    let (_, listed) = api.get("/api/tokens").await;
    assert_eq!(listed[0]["name"], json!("ci"));
    assert!(!listed.to_string().contains(&value));

    api.forget_cookie();
    let with_token = |method: Method, path: &str| {
        Request::builder()
            .method(method)
            .uri(path)
            .header(header::AUTHORIZATION, format!("Bearer {value}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json!({"name": "from-token"}).to_string()))
            .unwrap()
    };
    assert_eq!(
        api.send(with_token(Method::GET, "/api/sources")).await.0,
        StatusCode::OK
    );
    assert_eq!(
        api.send(with_token(Method::POST, "/api/sources")).await.0,
        StatusCode::CREATED
    );
    let (_, me) = api.send(with_token(Method::GET, "/api/auth/me")).await;
    assert_eq!(me, json!({"id": null, "email": null}));

    let id = created["id"].as_str().unwrap();
    assert_eq!(
        api.send(with_token(Method::DELETE, &format!("/api/tokens/{id}")))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        api.send(with_token(Method::GET, "/api/sources")).await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn the_stream_emits_routing_and_delivery_changes() {
    let api = Api::signed_in().await;
    let c = connection(&api, "ops").await;
    catch_all_route(&api, &c).await;
    let (_, key) = source(&api).await;

    let request = Request::get("/api/stream")
        .header(header::COOKIE, api.cookie().unwrap())
        .body(Body::empty())
        .unwrap();
    let response = tower::ServiceExt::oneshot(api.app.clone(), request)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "text/event-stream"
    );
    let mut stream = response.into_body().into_data_stream();

    let id = ingest(&api, &key, json!({"topic": "a", "title": "t"}))
        .await
        .remove(0);
    process(&api).await;

    let mut text = String::new();
    while !text.contains("event: delivery") {
        let chunk = tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .expect("no stream data");
        text.push_str(std::str::from_utf8(&chunk.unwrap().unwrap()).unwrap());
    }
    assert!(
        text.contains(&format!("event: event\ndata: {{\"event_id\":\"{id}\"}}")),
        "{text}"
    );
    assert!(text.contains("\"state\":\"delivered\""), "{text}");
    assert!(
        text.contains(&format!("\"event_ids\":[\"{id}\"]")),
        "{text}"
    );
}
