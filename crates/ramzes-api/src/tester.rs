//! The route tester: what the routes would do with an event, without doing it.

use std::collections::HashMap;

use axum::Json;
use axum::extract::State;
use ramzes_core::shaping::{Action, Step};
use ramzes_core::{Event, Matched, NewEvent, Route, RouteOutcome, new_id, trace_routes};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::ErrorBody;
use crate::routes::RouteInput;
use crate::{ApiError, AppState, Auth};

#[derive(Deserialize, ToSchema)]
pub(crate) struct TestRequest {
    /// A stored event to test with.
    #[serde(default)]
    event_id: Option<String>,
    /// Or an event written for the test.
    #[serde(default)]
    event: Option<NewEvent>,
    /// The routes to test, in order, saved or not. Without them the stored routes are used.
    #[serde(default)]
    routes: Option<Vec<RouteInput>>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct RouteTestResult {
    event: Event,
    /// One entry per route, in evaluation order.
    results: Vec<RouteResult>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct RouteResult {
    /// Absent for a route that is not saved.
    route_id: Option<String>,
    name: String,
    /// `matched`, `not_matched`, `disabled`, `skipped` (an earlier route stopped evaluation) or `error`.
    outcome: &'static str,
    /// Why the route's expression failed.
    error: Option<String>,
    /// What each target would receive, when the route matched.
    targets: Vec<TargetResult>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct TargetResult {
    connection_id: String,
    /// Absent when the connection does not exist.
    connection_name: Option<String>,
    destination: Option<String>,
    title: String,
    body: Option<String>,
    /// For example: the template failed and the default wording would be sent.
    warning: Option<String>,
    /// What shaping would do right now: `sent`, `not sent`, `joins an open notification`,
    /// `opens a group`, `held` or `resolves an open notification`.
    outcome: &'static str,
    /// The shaping steps that led there.
    steps: Vec<Step>,
}

/// Evaluate an event against routes and show what each would send. Nothing is stored or sent.
#[utoipa::path(
    post, path = "/api/routes/test", tag = "routes", request_body = TestRequest,
    responses((status = 200, body = RouteTestResult), (status = 404, body = ErrorBody), (status = 422, body = ErrorBody))
)]
pub(crate) async fn test(
    State(state): State<AppState>,
    auth: Auth,
    Json(body): Json<TestRequest>,
) -> Result<Json<RouteTestResult>, ApiError> {
    let ws = &auth.workspace_id;
    let now = state.clock.now_ms();
    let event = match (body.event_id, body.event) {
        (Some(id), _) => state
            .store
            .get_event(ws, &id)
            .await?
            .ok_or_else(ApiError::not_found)?,
        (None, Some(new)) => {
            new.validate()
                .map_err(|e| ApiError::invalid(e.to_string()))?;
            let occurred_at = new
                .occurred_at(now)
                .map_err(|e| ApiError::invalid(e.to_string()))?;
            Event {
                id: new_id(),
                workspace_id: ws.clone(),
                source_id: String::new(),
                topic: new.topic,
                severity: new.severity,
                status: new.status,
                title: new.title,
                body: new.body,
                labels: new.labels,
                payload: new.payload,
                dedup_key: new.dedup_key,
                occurred_at,
                received_at: now,
            }
        }
        (None, None) => {
            return Err(ApiError::invalid(
                "give an event_id or an event to test with",
            ));
        }
    };
    let (routes, saved): (Vec<Route>, bool) = match body.routes {
        None => (state.store.list_routes(ws).await?, true),
        Some(inputs) => {
            let mut routes = Vec::with_capacity(inputs.len());
            for (position, input) in (1_i64..).zip(inputs) {
                let mut route = input.into_unchecked_route(format!("unsaved-{position}"));
                route
                    .r#match
                    .validate()
                    .map_err(|e| ApiError::invalid(format!("route {:?}: {e}", route.name)))?;
                route
                    .shaping
                    .validate()
                    .map_err(|e| ApiError::invalid(format!("route {:?}: {e}", route.name)))?;
                route.position = position;
                routes.push(route);
            }
            (routes, false)
        }
    };
    let names: HashMap<String, String> = state
        .store
        .list_connections(ws)
        .await?
        .into_iter()
        .map(|c| (c.id, c.name))
        .collect();

    let silences = state.store.active_silences(ws, now).await?;
    let mut results = Vec::with_capacity(routes.len());
    for trace in trace_routes(&event, &routes) {
        let (outcome, error) = match trace.outcome {
            RouteOutcome::Matched => ("matched", None),
            RouteOutcome::NotMatched => ("not_matched", None),
            RouteOutcome::Disabled => ("disabled", None),
            RouteOutcome::Skipped => ("skipped", None),
            RouteOutcome::Error(reason) => ("error", Some(reason)),
        };
        let mut targets = Vec::new();
        if outcome == "matched" {
            for target in &trace.route.targets {
                let composed = state
                    .engine
                    .compose_for(
                        ws,
                        "",
                        std::slice::from_ref(&event),
                        target.template_id.as_deref(),
                    )
                    .await?
                    .ok_or_else(|| {
                        ApiError::internal("route tester", "an event produced no message")
                    })?;
                let connection_name = names.get(&target.connection_id).cloned();
                let matched = Matched {
                    route: trace.route,
                    target,
                };
                let (decision, _) = state.engine.decide(&event, &matched, &silences).await?;
                targets.push(TargetResult {
                    outcome: match decision.action {
                        Action::Send { .. } => "sent",
                        Action::Suppress { .. } => "not sent",
                        Action::Attach { .. } => "joins an open notification",
                        Action::Resolve { .. } => "resolves an open notification",
                        Action::OpenGroup { .. } => "opens a group",
                        Action::Hold { .. } => "held",
                    },
                    steps: decision.steps,
                    warning: match (&connection_name, composed.warning) {
                        (None, _) => Some("this connection does not exist".to_owned()),
                        (Some(_), warning) => warning,
                    },
                    connection_id: target.connection_id.clone(),
                    connection_name,
                    destination: target.destination.clone(),
                    title: composed.message.title,
                    body: composed.message.body,
                });
            }
        }
        results.push(RouteResult {
            route_id: saved.then(|| trace.route.id.clone()),
            name: trace.route.name.clone(),
            outcome,
            error,
            targets,
        });
    }
    Ok(Json(RouteTestResult { event, results }))
}
