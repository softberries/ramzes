//! Milestone 3 endpoints: shaping on routes, silences, notifications, overview, paused connections.

use axum::http::StatusCode;
use ramzes_connectors::SendOutcome;
use serde_json::{Value, json};
use std::sync::atomic::Ordering;

use crate::common::Api;
use crate::resources::{connection, ingest, process, source};

async fn shaped_route(api: &Api, connection_id: &str, shaping: Value) -> String {
    let body = json!({"name": "shaped", "match": {"topic": "orders.*"}, "targets": [{"connection_id": connection_id}], "shaping": shaping});
    let (status, created) = api.post("/api/routes", body).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    created["id"].as_str().unwrap().to_owned()
}

async fn settle(api: &Api) {
    for _ in 0..10 {
        let done = api.engine.route_once().await.unwrap()
            + api.engine.timers_once().await.unwrap()
            + api.engine.deliver_once().await.unwrap();
        if done == 0 {
            return;
        }
    }
    panic!("the engine did not settle");
}

#[tokio::test]
async fn a_route_carries_a_shaping_policy_that_is_validated() {
    let api = Api::signed_in().await;
    let c = connection(&api, "ops").await;
    let policy = json!({"dedup": {"window_s": 600}, "group": {"by": ["service"], "wait_s": 30},
        "quiet_hours": {"tz": "Europe/Warsaw", "start": "22:00", "end": "07:00", "days": ["sat", "sun"], "bypass_severity": "critical"},
        "rate_limit": {"max": 5, "window_s": 300}, "notify_resolved": false});
    let id = shaped_route(&api, &c, policy.clone()).await;
    let (_, routes) = api.get("/api/routes").await;
    assert_eq!(routes[0]["shaping"]["group"]["wait_s"], 30);
    assert_eq!(
        routes[0]["shaping"]["quiet_hours"]["days"],
        json!(["sat", "sun"])
    );
    assert_eq!(routes[0]["shaping"]["notify_resolved"], false);

    for (bad, reason) in [
        (
            json!({"quiet_hours": {"tz": "Mars/Olympus", "start": "22:00", "end": "07:00"}}),
            "time zone",
        ),
        (json!({"group": {"by": [], "wait_s": 0}}), "group"),
        (
            json!({"dedup": {"window_s": 60, "key": "a =="}}),
            "dedup key",
        ),
        (
            json!({"rate_limit": {"max": 0, "window_s": 60}}),
            "rate limit",
        ),
    ] {
        let body =
            json!({"name": "x", "match": {}, "targets": [{"connection_id": c}], "shaping": bad});
        let (status, error) = api.put(&format!("/api/routes/{id}"), body).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{error}");
        assert!(
            error["message"].as_str().unwrap().contains(reason),
            "{error}"
        );
    }
    // A route saved without a policy has the empty one.
    let body = json!({"name": "plain", "match": {}, "targets": [{"connection_id": c}]});
    let (_, plain) = api.put(&format!("/api/routes/{id}"), body).await;
    assert_eq!(plain["shaping"], json!({"notify_resolved": true}));
}

#[tokio::test]
async fn silences_are_created_listed_ended_and_validated() {
    let api = Api::signed_in().await;
    let c = connection(&api, "ops").await;
    shaped_route(&api, &c, json!({})).await;
    let now = api.clock.0.load(Ordering::SeqCst);

    let (status, created) = api
        .post(
            "/api/silences",
            json!({"match": {"topic": "orders.*"}, "duration_s": 3600, "comment": "deploy"}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(
        (created["starts_at"].as_i64(), created["ends_at"].as_i64()),
        (Some(now), Some(now + 3_600_000))
    );
    assert_eq!(created["author"], crate::common::EMAIL);
    let id = created["id"].as_str().unwrap();

    let (_, key) = source(&api).await;
    ingest(
        &api,
        &key,
        json!({"topic": "orders.failed", "title": "silenced"}),
    )
    .await;
    settle(&api).await;
    assert!(api.connector.sent.lock().unwrap().is_empty());
    let (_, page) = api.get("/api/notifications?state=suppressed").await;
    assert_eq!(page["notifications"][0]["reason"], "silenced: deploy");

    assert_eq!(
        api.post(&format!("/api/silences/{id}/end"), json!({}))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    ingest(
        &api,
        &key,
        json!({"topic": "orders.failed", "title": "heard"}),
    )
    .await;
    settle(&api).await;
    assert_eq!(api.connector.sent.lock().unwrap().len(), 1);
    let (_, listed) = api.get("/api/silences").await;
    assert_eq!(listed[0]["ends_at"], now);

    for (body, reason) in [
        (json!({"match": {}, "duration_s": 60}), "everything"),
        (json!({"match": {"topic": "a"}, "duration_s": 0}), "end"),
        (
            json!({"match": {"topic": "a"}, "starts_at": now, "ends_at": now - 1}),
            "end",
        ),
        (
            json!({"match": {"expr": "a =="}, "duration_s": 60}),
            "expression",
        ),
        (json!({"match": {"topic": "a"}}), "duration"),
    ] {
        let (status, error) = api.post("/api/silences", body).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{error}");
        assert!(
            error["message"].as_str().unwrap().contains(reason),
            "{error}"
        );
    }
    assert_eq!(
        api.delete(&format!("/api/silences/{id}")).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        api.delete(&format!("/api/silences/{id}")).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn notifications_are_listed_shown_acknowledged_and_resolved() {
    let api = Api::signed_in().await;
    let c = connection(&api, "ops").await;
    shaped_route(&api, &c, json!({"group": {"by": [], "wait_s": 30}})).await;
    let (_, key) = source(&api).await;
    ingest(&api, &key, json!([{"topic": "orders.failed", "title": "one"}, {"topic": "orders.failed", "title": "two", "severity": "critical"}])).await;
    settle(&api).await;

    let (status, page) = api.get("/api/notifications?state=grouping,held").await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let rows = page["notifications"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    let n = &rows[0];
    assert_eq!(
        (
            n["state"].as_str(),
            n["event_count"].as_i64(),
            n["severity"].as_str()
        ),
        (Some("grouping"), Some(2), Some("critical"))
    );
    assert_eq!(
        (
            n["title"].as_str(),
            n["connection_name"].as_str(),
            n["route_name"].as_str()
        ),
        (Some("one"), Some("ops"), Some("shaped"))
    );
    assert!(n["next_timer_at"].is_i64());
    let id = n["id"].as_str().unwrap();
    assert_eq!(
        api.get("/api/notifications?state=nonsense").await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    let (status, detail) = api.get(&format!("/api/notifications/{id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["events"].as_array().unwrap().len(), 2);
    assert_eq!(detail["events"][1]["role"], "grouped");
    assert_eq!(
        api.get("/api/notifications/nope").await.0,
        StatusCode::NOT_FOUND
    );

    // Acknowledged while still collecting: it is never sent.
    assert_eq!(
        api.post(&format!("/api/notifications/{id}/acknowledge"), json!({}))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    api.clock.0.fetch_add(60_000, Ordering::SeqCst);
    settle(&api).await;
    assert!(api.connector.sent.lock().unwrap().is_empty());
    let (_, detail) = api.get(&format!("/api/notifications/{id}")).await;
    assert_eq!(
        (detail["state"].as_str(), detail["acked_by"].as_str()),
        (Some("acknowledged"), Some(crate::common::EMAIL))
    );
    assert_eq!(
        api.post(&format!("/api/notifications/{id}/acknowledge"), json!({}))
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        api.post(&format!("/api/notifications/{id}/resolve"), json!({}))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        api.post(&format!("/api/notifications/{id}/resolve"), json!({}))
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        api.post("/api/notifications/nope/resolve", json!({}))
            .await
            .0,
        StatusCode::NOT_FOUND
    );

    let (_, all) = api.get("/api/notifications?limit=1").await;
    assert_eq!(all["notifications"].as_array().unwrap().len(), 1);
    assert_eq!(all["next_before"], Value::Null);
}

#[tokio::test]
async fn the_route_tester_explains_the_shaping_of_each_target() {
    let api = Api::signed_in().await;
    let c = connection(&api, "ops").await;
    api.post("/api/silences", json!({"match": {"labels": [{"op": "equals", "key": "env", "value": "staging"}]}, "duration_s": 600})).await;
    let routes = json!([{"name": "grouped", "match": {}, "targets": [{"connection_id": c}], "shaping": {"group": {"by": ["service"], "wait_s": 30}}}]);
    let event = json!({"topic": "orders.failed", "title": "x", "labels": {"service": "pay"}});
    let (status, result) = api
        .post(
            "/api/routes/test",
            json!({"event": event, "routes": routes}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    let target = &result["results"][0]["targets"][0];
    assert_eq!(target["outcome"], "opens a group");
    assert_eq!(target["steps"][0]["step"], "group");

    let silenced = json!({"topic": "orders.failed", "title": "x", "labels": {"env": "staging"}});
    let (_, result) = api
        .post(
            "/api/routes/test",
            json!({"event": silenced, "routes": routes}),
        )
        .await;
    assert_eq!(result["results"][0]["targets"][0]["outcome"], "not sent");
    assert_eq!(
        result["results"][0]["targets"][0]["steps"][0]["step"],
        "silence"
    );
    // Nothing was written.
    let (_, page) = api.get("/api/notifications").await;
    assert!(page["notifications"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn a_paused_connection_is_shown_and_can_be_resumed() {
    let api = Api::signed_in().await;
    let c = connection(&api, "flaky").await;
    shaped_route(&api, &c, json!({})).await;
    let (_, key) = source(&api).await;
    for _ in 0..5 {
        api.connector
            .outcomes
            .lock()
            .unwrap()
            .push_back(SendOutcome::Retry {
                after_ms: None,
                status: Some(503),
                detail: "HTTP 503".into(),
            });
    }
    let events: Vec<Value> = (0..5)
        .map(|i| json!({"topic": "orders.failed", "title": format!("event {i}")}))
        .collect();
    ingest(&api, &key, json!(events)).await;
    process(&api).await;
    let (_, connections) = api.get("/api/connections").await;
    assert!(connections[0]["paused_until"].is_i64(), "{connections}");
    assert_eq!(connections[0]["failure_streak"], 5);

    assert_eq!(
        api.post(&format!("/api/connections/{c}/resume"), json!({}))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    let (_, connections) = api.get("/api/connections").await;
    assert_eq!(connections[0]["paused_until"], Value::Null);
    process(&api).await;
    assert_eq!(api.connector.sent.lock().unwrap().len(), 10);
    assert_eq!(
        api.post("/api/connections/nope/resume", json!({})).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn the_overview_summarises_the_last_hours() {
    let api = Api::signed_in().await;
    let c = connection(&api, "ops").await;
    shaped_route(&api, &c, json!({})).await;
    let (_, key) = source(&api).await;
    api.connector
        .outcomes
        .lock()
        .unwrap()
        .push_back(SendOutcome::Permanent {
            status: Some(400),
            detail: "HTTP 400".into(),
        });
    ingest(&api, &key, json!([{"topic": "orders.failed", "title": "a"}, {"topic": "orders.failed", "title": "b"}, {"topic": "billing.late", "title": "c"}])).await;
    settle(&api).await;
    let (status, overview) = api.get("/api/overview?hours=24").await;
    assert_eq!(status, StatusCode::OK, "{overview}");
    // Three sent in, plus ramzes's own report of the rejected delivery.
    assert_eq!(overview["events"], 4);
    assert_eq!(overview["deliveries_delivered"], 1);
    assert_eq!(overview["deliveries_failed"], 1);
    assert_eq!(
        overview["topics"][0],
        json!({"name": "orders.failed", "events": 2})
    );
    assert_eq!(overview["sources"][0]["name"], "orders-api");
    assert_eq!(overview["failing"][0]["name"], "ops");
    assert_eq!(overview["failing"][0]["failed"], 1);
    assert_eq!(overview["bucket_ms"], 3_600_000);
    assert!(overview["buckets"][0]["start"].is_i64() && overview["buckets"][0]["events"].is_i64());
    assert_eq!(
        api.get("/api/overview?hours=0").await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
}

#[tokio::test]
async fn a_journey_says_how_each_event_relates_to_its_notification() {
    let api = Api::signed_in().await;
    let c = connection(&api, "ops").await;
    shaped_route(&api, &c, json!({"dedup": {"window_s": 600}})).await;
    let (_, key) = source(&api).await;
    let body = json!({"topic": "orders.failed", "title": "same", "dedup_key": "k"});
    ingest(&api, &key, body.clone()).await;
    settle(&api).await;
    let ids = ingest(&api, &key, body).await;
    settle(&api).await;
    let (_, journey) = api.get(&format!("/api/events/{}", ids[0])).await;
    assert_eq!(journey["notifications"][0]["role"], "duplicate");
    assert_eq!(journey["notifications"][0]["event_count"], 2);
}

#[tokio::test]
async fn the_built_in_source_cannot_be_deleted_or_given_a_key() {
    let api = Api::signed_in().await;
    let built_in = api.store.system_source(&api.ws, 1).await.unwrap();
    let (status, error) = api.delete(&format!("/api/sources/{}", built_in.id)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{error}");
    let (status, _) = api
        .post(&format!("/api/sources/{}/rotate", built_in.id), json!({}))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(api.get("/api/sources").await.1.as_array().unwrap().len(), 1);
}
