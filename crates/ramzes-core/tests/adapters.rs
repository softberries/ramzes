//! The bundled adapters against payloads shaped like each sender's documentation.

use std::collections::BTreeMap;

use ramzes_core::adapters::{mapping_for, source_kinds};
use ramzes_core::{EventStatus, NewEvent, Severity};
use serde_json::{Value, json};

fn apply(kind: &str, payload: Value, headers: &[(&str, &str)]) -> Vec<NewEvent> {
    let headers: BTreeMap<String, String> = headers
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let events = mapping_for(kind, None)
        .unwrap()
        .unwrap()
        .apply(&payload, &headers)
        .unwrap();
    for e in &events {
        e.validate().unwrap_or_else(|err| panic!("{kind}: {err}"));
    }
    events
}

fn alertmanager() -> Value {
    json!({
        "version": "4", "groupKey": "{}:{alertname=\"HighErrorRate\"}", "truncatedAlerts": 0, "status": "firing",
        "receiver": "ramzes", "groupLabels": {"alertname": "HighErrorRate"}, "commonLabels": {"alertname": "HighErrorRate"},
        "commonAnnotations": {}, "externalURL": "http://alertmanager:9093", "notification_reason": "new alert",
        "alerts": [
            {"status": "firing", "labels": {"alertname": "HighErrorRate", "severity": "critical", "service": "checkout"},
             "annotations": {"summary": "Error rate above 5%", "description": "checkout is failing 7% of requests"},
             "startsAt": "2026-10-13T09:41:07.123Z", "endsAt": "0001-01-01T00:00:00Z",
             "generatorURL": "http://prometheus:9090/graph?g0.expr=x", "fingerprint": "a1b2c3d4e5f60708"},
            {"status": "resolved", "labels": {"alertname": "DiskFull", "instance": "db-1:9100"},
             "annotations": {}, "startsAt": "2026-10-13T08:00:00Z", "endsAt": "2026-10-13T09:00:00Z",
             "generatorURL": "", "fingerprint": "0011223344556677"}
        ]
    })
}

#[test]
fn alertmanager_gives_one_event_per_alert() {
    let events = apply("alertmanager", alertmanager(), &[]);
    insta::assert_json_snapshot!("alertmanager", events, {"[].payload" => "[alert]"});
    assert_eq!(events[0].severity, Severity::Critical);
    assert_eq!(events[1].status, EventStatus::Resolved);
    assert_eq!(events[1].title, "DiskFull");
}

#[test]
fn grafana_gives_one_event_per_alert() {
    let payload = json!({
        "receiver": "ramzes", "status": "firing", "orgId": 1, "version": "1", "groupKey": "{}:{}", "truncatedAlerts": 0,
        "title": "[FIRING:1] High memory usage", "state": "alerting", "message": "**Firing**…",
        "groupLabels": {}, "commonLabels": {}, "commonAnnotations": {}, "externalURL": "https://grafana.example.com/",
        "alerts": [{
            "status": "firing", "labels": {"alertname": "High memory usage", "team": "blue", "zone": "us-1"},
            "annotations": {"description": "The system has high memory usage", "summary": "This alert was triggered for zone us-1"},
            "startsAt": "2021-10-12T09:51:03.157076+02:00", "endsAt": "0001-01-01T00:00:00Z", "values": {"B": 44.2, "C": 1},
            "generatorURL": "https://play.grafana.org/alerting/1afz29v7z/edit", "fingerprint": "c6eadffa33fcdf37",
            "silenceURL": "https://play.grafana.org/alerting/silence/new", "dashboardURL": "", "panelURL": "", "imageURL": ""
        }]
    });
    let events = apply("grafana", payload, &[]);
    insta::assert_json_snapshot!("grafana", events, {"[].payload" => "[alert]"});
    assert_eq!(events[0].topic, "grafana.High_memory_usage");
}

#[test]
fn github_workflow_runs_carry_their_conclusion() {
    let run = |conclusion: &str| {
        json!({"action": "completed",
            "workflow_run": {"id": 1, "name": "CI", "display_title": "Fix tests", "html_url": "https://github.com/acme/shop/actions/runs/1",
                "status": "completed", "conclusion": conclusion, "head_branch": "main", "head_sha": "abc", "run_number": 12},
            "workflow": {"name": "CI"}, "repository": {"full_name": "acme/shop"}, "sender": {"login": "kris"}})
    };
    let failed = apply(
        "github",
        run("failure"),
        &[("x-github-event", "workflow_run")],
    );
    insta::assert_json_snapshot!("github_workflow_failed", failed, {"[].payload" => "[payload]"});
    let passed = apply(
        "github",
        run("success"),
        &[("x-github-event", "workflow_run")],
    );
    assert_eq!(passed[0].status, EventStatus::Resolved);
    assert_eq!(passed[0].severity, Severity::Info);
    assert_eq!(passed[0].dedup_key, failed[0].dedup_key);
}

#[test]
fn github_other_events_get_readable_titles() {
    let repo = json!({"full_name": "acme/shop"});
    let title = |event: &str, payload: Value| {
        apply("github", payload, &[("x-github-event", event)])[0]
            .title
            .clone()
    };
    assert_eq!(
        title(
            "pull_request",
            json!({"action": "closed", "number": 7, "repository": repo,
            "pull_request": {"title": "Add retries", "merged": true, "html_url": "https://github.com/acme/shop/pull/7"}})
        ),
        "acme/shop: pull request #7 merged: Add retries"
    );
    assert_eq!(
        title(
            "issues",
            json!({"action": "opened", "repository": repo, "issue": {"number": 3, "title": "Crash on start"}})
        ),
        "acme/shop: issue #3 opened: Crash on start"
    );
    assert_eq!(
        title(
            "push",
            json!({"ref": "refs/heads/main", "repository": repo, "pusher": {"name": "kris"},
            "commits": [{"id": "1"}, {"id": "2"}], "compare": "https://github.com/acme/shop/compare/a...b"})
        ),
        "acme/shop: push to main by kris (2 commits)"
    );
    assert_eq!(
        title(
            "release",
            json!({"action": "published", "repository": repo, "release": {"tag_name": "v1.2.0"}})
        ),
        "acme/shop: release v1.2.0 published"
    );
    assert_eq!(
        title("star", json!({"action": "created", "repository": repo})),
        "acme/shop: star created"
    );
    assert_eq!(
        title("fork", json!({"repository": repo})),
        "acme/shop: fork"
    );
}

#[test]
fn a_github_ping_produces_no_event() {
    let events = apply(
        "github",
        json!({"zen": "Keep it logically awesome.", "hook_id": 1}),
        &[("x-github-event", "ping")],
    );
    assert!(events.is_empty());
}

#[test]
fn gitlab_pipelines_merge_requests_and_pushes() {
    let project = json!({"name": "shop", "web_url": "https://gitlab.com/acme/shop", "path_with_namespace": "acme/shop"});
    let pipeline = apply(
        "gitlab",
        json!({"object_kind": "pipeline", "project": project, "user": {"username": "kris"},
        "object_attributes": {"id": 31, "iid": 3, "ref": "main", "status": "failed", "url": "https://gitlab.com/acme/shop/-/pipelines/31"}}),
        &[],
    );
    insta::assert_json_snapshot!("gitlab_pipeline_failed", pipeline, {"[].payload" => "[payload]"});
    let mr = apply(
        "gitlab",
        json!({"object_kind": "merge_request", "project": project, "user": {"username": "kris"},
        "object_attributes": {"iid": 9, "title": "Add retries", "state": "merged", "action": "merge", "url": "https://gitlab.com/acme/shop/-/merge_requests/9"}}),
        &[],
    );
    assert_eq!(
        mr[0].title,
        "acme/shop: merge request !9 merge: Add retries"
    );
    assert_eq!(mr[0].dedup_key, None);
    let push = apply(
        "gitlab",
        json!({"object_kind": "push", "ref": "refs/heads/main", "user_name": "Kris", "user_username": "kris",
        "project": project, "total_commits_count": 4}),
        &[],
    );
    assert_eq!(push[0].title, "acme/shop: push to main by Kris (4 commits)");
    assert_eq!(push[0].labels["user"], "kris");
}

#[test]
fn sentry_issues_alerts_and_metric_alerts() {
    let issue = apply(
        "sentry",
        json!({"action": "created", "installation": {"uuid": "u"}, "actor": {"type": "application", "id": "sentry", "name": "Sentry"},
        "data": {"issue": {"id": "1170820242", "shortId": "SHOP-1A", "title": "TypeError: x is undefined", "culprit": "checkout in pay",
            "level": "error", "status": "unresolved", "project": {"id": "1", "name": "shop", "slug": "shop"},
            "web_url": "https://sentry.io/organizations/acme/issues/1170820242/"}}}),
        &[("sentry-hook-resource", "issue")],
    );
    insta::assert_json_snapshot!("sentry_issue", issue, {"[].payload" => "[payload]"});
    let resolved = apply(
        "sentry",
        json!({"action": "resolved", "data": {"issue": {"id": "1170820242", "title": "TypeError", "level": "error"}}}),
        &[("sentry-hook-resource", "issue")],
    );
    assert_eq!(resolved[0].status, EventStatus::Resolved);
    assert_eq!(resolved[0].dedup_key, issue[0].dedup_key);
    let alert = apply(
        "sentry",
        json!({"action": "triggered", "data": {"triggered_rule": "Very Important Alert Rule!",
        "event": {"event_id": "e", "title": "This is an example Python exception", "culprit": "poll(../../sentry/scripts/views.js)",
            "level": "fatal", "issue_id": "1117540176", "web_url": "https://sentry.io/organizations/acme/issues/1117540176/events/e/"}}}),
        &[("sentry-hook-resource", "event_alert")],
    );
    assert_eq!(alert[0].severity, Severity::Critical);
    assert_eq!(alert[0].labels["rule"], "Very Important Alert Rule!");
    let metric = apply(
        "sentry",
        json!({"action": "critical", "data": {"description_title": "Critical: Too many errors",
        "description_text": "100 events in the last 5 minutes", "web_url": "https://sentry.io/alerts/1/", "metric_alert": {"id": "4"}}}),
        &[("sentry-hook-resource", "metric_alert")],
    );
    assert_eq!(
        (metric[0].title.as_str(), metric[0].severity),
        ("Critical: Too many errors", Severity::Critical)
    );
    assert_eq!(metric[0].dedup_key.as_deref(), Some("4"));
    assert!(
        apply(
            "sentry",
            json!({"action": "created", "data": {}}),
            &[("sentry-hook-resource", "installation")]
        )
        .is_empty()
    );
}

#[test]
fn every_kind_is_listed_with_setup_help() {
    let kinds = source_kinds();
    let names: Vec<&str> = kinds.iter().map(|k| k.kind.as_str()).collect();
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
    for k in &kinds {
        assert!(k.own_secret || k.setup.contains("{key}"), "{}", k.kind);
    }
    assert!(
        kinds
            .iter()
            .find(|k| k.kind == "sentry")
            .unwrap()
            .own_secret
    );
}

#[test]
fn an_any_json_source_uses_its_own_mapping_or_a_starter() {
    assert!(mapping_for("native", None).unwrap().is_none());
    let starter = mapping_for("json", None).unwrap().unwrap();
    let events = starter
        .apply(
            &json!({"message": "hello", "level": "warn"}),
            &BTreeMap::new(),
        )
        .unwrap();
    assert_eq!(
        (events[0].title.as_str(), events[0].severity),
        ("hello", Severity::Warning)
    );
    let own =
        "verify: {method: query, param: key}\nfields:\n  topic: \"'x.y'\"\n  title: payload.name\n";
    let mapping = mapping_for("json", Some(own)).unwrap().unwrap();
    assert_eq!(
        mapping
            .apply(&json!({"name": "n"}), &BTreeMap::new())
            .unwrap()[0]
            .topic,
        "x.y"
    );
    assert!(
        mapping_for("json", Some("fields: {topic: \"'x'\", title: \"(\"}"))
            .unwrap_err()
            .contains("fields.title")
    );
    assert!(mapping_for("carrier-pigeon", None).is_err());
    // A mapping survives being written back as YAML.
    assert_eq!(
        ramzes_core::Mapping::parse(&mapping.to_yaml()).unwrap(),
        mapping
    );
}
