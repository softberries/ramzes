//! Milestone 4: escalation on routes, the acknowledgement link, Slack buttons.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use hmac::{Hmac, KeyInit, Mac};
use serde_json::{Value, json};
use sha2::Sha256;
use std::sync::atomic::Ordering;
use tower::ServiceExt;

use crate::common::Api;
use crate::resources::{connection, ingest, source};

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

/// Sends a raw request and returns status and body text.
async fn raw(api: &Api, request: Request<Body>) -> (StatusCode, String) {
    let response = api.app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

/// One event sent through a route to `ops`; returns `(notification id, acknowledgement link path)`.
async fn notified(api: &Api, escalation: Value) -> (String, String) {
    api.put("/api/settings", json!({"retention_days": 30, "retention_overrides": {}, "public_url": "https://ramzes.example.com"})).await;
    let c = connection(api, "ops").await;
    let body = json!({"name": "paged", "match": {}, "targets": [{"connection_id": c}], "escalation": escalation});
    let (status, created) = api.post("/api/routes", body).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let (_, key) = source(api).await;
    ingest(
        api,
        &key,
        json!({"topic": "infra.db", "title": "Database <down>", "labels": {"host": "db-1"}}),
    )
    .await;
    settle(api).await;
    let sent = api.connector.sent.lock().unwrap().clone();
    let message = &sent.last().unwrap().0;
    let link = message.ack_link.clone().unwrap();
    (
        message.notification_id.clone().unwrap(),
        link.strip_prefix("https://ramzes.example.com")
            .unwrap()
            .to_owned(),
    )
}

#[tokio::test]
async fn a_route_carries_an_escalation_chain_that_is_validated() {
    let api = Api::signed_in().await;
    let c = connection(&api, "ops").await;
    let route = |escalation: Value| json!({"name": "paged", "match": {}, "targets": [{"connection_id": c}], "escalation": escalation});
    let (status, created) = api.post("/api/routes", route(json!([{"after_s": 300, "targets": [{"connection_id": c, "destination": "oncall"}]}]))).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["escalation"][0]["after_s"], 300);
    for (bad, reason) in [
        (
            json!([{"after_s": 0, "targets": [{"connection_id": c}]}]),
            "step 1",
        ),
        (json!([{"after_s": 60, "targets": []}]), "step 1"),
        (
            json!([{"after_s": 60, "targets": [{"connection_id": c}]}, {"after_s": 90, "targets": [{"connection_id": "nope"}]}]),
            "escalation step 2: there is no connection with id \"nope\"",
        ),
        (
            json!([{"after_s": 60, "targets": [{"connection_id": c, "template_id": "missing"}]}]),
            "escalation step 1: there is no template with id \"missing\"",
        ),
    ] {
        let (status, error) = api.post("/api/routes", route(bad)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{error}");
        assert!(
            error["message"].as_str().unwrap().contains(reason),
            "{error}"
        );
    }
}

#[tokio::test]
async fn the_link_shows_a_page_and_acknowledges_only_when_the_form_is_submitted() {
    let api = Api::signed_in().await;
    let (id, link) = notified(&api, json!([])).await;
    api.forget_cookie();

    // Opening the link, as a scanner or a preview would, changes nothing.
    let (status, page) = raw(&api, Request::get(&link).body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        page.contains("Database &lt;down&gt;") && !page.contains("<down>"),
        "{page}"
    );
    assert!(page.contains("<form") && page.contains("Acknowledge"));
    let state = api
        .store
        .notification_core(&id)
        .await
        .unwrap()
        .unwrap()
        .state;
    assert_eq!(state.as_str(), "sent");

    let token = link.split("t=").nth(1).unwrap();
    let submit = || {
        Request::post(format!("/ack/{id}"))
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(format!("t={token}")))
            .unwrap()
    };
    let (status, page) = raw(&api, submit()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("Acknowledged"), "{page}");
    let core = api.store.notification_core(&id).await.unwrap().unwrap();
    assert_eq!(
        (core.state.as_str(), core.acked_by.as_deref()),
        ("acknowledged", Some("link"))
    );

    // Submitting or opening it again is answered calmly.
    let (status, page) = raw(&api, submit()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("already"), "{page}");
    let (status, page) = raw(&api, Request::get(&link).body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!page.contains("<form"), "{page}");
}

#[tokio::test]
async fn a_wrong_token_or_an_unknown_notification_gets_the_same_page_and_changes_nothing() {
    let api = Api::signed_in().await;
    let (id, link) = notified(&api, json!([])).await;
    let token = link.split("t=").nth(1).unwrap().to_owned();
    let wrong = format!("/ack/{id}?t={}", "0".repeat(token.len()));
    let (status_wrong, page_wrong) =
        raw(&api, Request::get(&wrong).body(Body::empty()).unwrap()).await;
    let (status_unknown, page_unknown) = raw(
        &api,
        Request::get(format!("/ack/nope?t={token}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(
        (status_wrong, status_unknown),
        (StatusCode::NOT_FOUND, StatusCode::NOT_FOUND)
    );
    assert_eq!(page_wrong, page_unknown);
    assert!(!page_wrong.contains("Database"));
    let post = Request::post(format!("/ack/{id}"))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("t=nope"))
        .unwrap();
    assert_eq!(raw(&api, post).await.0, StatusCode::NOT_FOUND);
    assert_eq!(
        api.store
            .notification_core(&id)
            .await
            .unwrap()
            .unwrap()
            .state
            .as_str(),
        "sent"
    );
}

#[tokio::test]
async fn the_notification_shows_how_far_its_escalation_has_come() {
    let api = Api::signed_in().await;
    let lead = connection(&api, "lead").await;
    let boss = connection(&api, "boss").await;
    let chain = json!([
        {"after_s": 300, "targets": [{"connection_id": lead, "destination": "oncall"}]},
        {"after_s": 600, "targets": [{"connection_id": boss}]}
    ]);
    let (id, _) = notified(&api, chain).await;
    let start = api.clock.0.load(Ordering::SeqCst);
    let (_, detail) = api.get(&format!("/api/notifications/{id}")).await;
    let steps = detail["escalation"].as_array().unwrap();
    assert_eq!(steps.len(), 2);
    assert_eq!(
        (
            steps[0]["after_s"].as_i64(),
            steps[0]["due_at"].as_i64(),
            steps[0]["sent_at"].as_i64()
        ),
        (Some(300), Some(start + 300_000), None)
    );
    assert_eq!(steps[0]["targets"][0]["connection_name"], "lead");
    assert_eq!(steps[1]["due_at"], Value::Null);

    api.clock.0.fetch_add(300_000, Ordering::SeqCst);
    settle(&api).await;
    let (_, detail) = api.get(&format!("/api/notifications/{id}")).await;
    let steps = detail["escalation"].as_array().unwrap();
    assert_eq!(steps[0]["sent_at"], start + 300_000);
    assert_eq!(steps[1]["due_at"], start + 900_000);
    assert_eq!(detail["deliveries"].as_array().unwrap().len(), 2);

    // Acknowledging through the API stops it and updates the delivered messages.
    assert_eq!(
        api.post(&format!("/api/notifications/{id}/acknowledge"), json!({}))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    settle(&api).await;
    let (_, detail) = api.get(&format!("/api/notifications/{id}")).await;
    assert_eq!(detail["escalation"][1]["due_at"], Value::Null);
    assert_eq!(api.connector.updates.lock().unwrap().len(), 2);
}

fn slack_signature(secret: &str, timestamp: i64, body: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(format!("v0:{timestamp}:{body}").as_bytes());
    format!("v0={}", hex::encode(mac.finalize().into_bytes()))
}

fn interaction(action: &str, notification_id: &str) -> String {
    let payload = json!({"type": "block_actions", "user": {"id": "U1", "username": "kris"},
        "actions": [{"action_id": action, "value": notification_id}]});
    serde_urlencoded::to_string([("payload", payload.to_string())]).unwrap()
}

async fn press(
    api: &Api,
    connection_id: &str,
    secret: &str,
    timestamp: i64,
    body: &str,
) -> StatusCode {
    let request = Request::post(format!("/v1/slack/{connection_id}/interactions"))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header("x-slack-request-timestamp", timestamp.to_string())
        .header(
            "x-slack-signature",
            slack_signature(secret, timestamp, body),
        )
        .body(Body::from(body.to_owned()))
        .unwrap();
    raw(api, request).await.0
}

#[tokio::test]
async fn slack_buttons_acknowledge_and_silence_when_the_request_is_really_from_slack() {
    let api = Api::signed_in().await;
    let (id, _) = notified(&api, json!([])).await;
    let body = json!({"kind": "slack_app", "name": "slack", "settings": {"channel": "C1"},
        "credentials": {"bot_token": "xoxb-1", "signing_secret": "slack-signing-secret"}});
    let (_, slack) = api.post("/api/connections", body).await;
    let slack = slack["id"].as_str().unwrap();
    let now_s = api.clock.0.load(Ordering::SeqCst) / 1000;
    let ack = interaction("ramzes_ack", &id);

    // Wrong secret, a stale timestamp, and a connection that is not a Slack app are refused.
    assert_eq!(
        press(&api, slack, "another-secret", now_s, &ack).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        press(&api, slack, "slack-signing-secret", now_s - 600, &ack).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        press(&api, "nope", "slack-signing-secret", now_s, &ack).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        api.store
            .notification_core(&id)
            .await
            .unwrap()
            .unwrap()
            .state
            .as_str(),
        "sent"
    );

    assert_eq!(
        press(&api, slack, "slack-signing-secret", now_s, &ack).await,
        StatusCode::OK
    );
    let core = api.store.notification_core(&id).await.unwrap().unwrap();
    assert_eq!(
        (core.state.as_str(), core.acked_by.as_deref()),
        ("acknowledged", Some("kris (Slack)"))
    );
    // Pressing again is not an error.
    assert_eq!(
        press(&api, slack, "slack-signing-secret", now_s, &ack).await,
        StatusCode::OK
    );

    let silence = interaction("ramzes_silence", &id);
    assert_eq!(
        press(&api, slack, "slack-signing-secret", now_s, &silence).await,
        StatusCode::OK
    );
    let (_, silences) = api.get("/api/silences").await;
    assert_eq!(silences[0]["match"]["topic"], "infra.db");
    assert_eq!(
        silences[0]["match"]["labels"][0],
        json!({"op": "equals", "key": "host", "value": "db-1"})
    );
    assert_eq!(
        silences[0]["ends_at"].as_i64().unwrap() - silences[0]["starts_at"].as_i64().unwrap(),
        3_600_000
    );
    assert_eq!(silences[0]["author"], "kris (Slack)");
    // An action ramzes does not know (the link button) is accepted and ignored.
    assert_eq!(
        press(
            &api,
            slack,
            "slack-signing-secret",
            now_s,
            &interaction("ramzes_open", &id)
        )
        .await,
        StatusCode::OK
    );
}
