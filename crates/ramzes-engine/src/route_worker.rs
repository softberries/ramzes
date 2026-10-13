use std::collections::HashMap;

use ramzes_core::{Event, Route, new_id, route_event};
use ramzes_store::NewDispatch;

use crate::{Change, Engine, Result};

/// Events routed per pass.
const BATCH: i64 = 100;

/// How often the same error of the same route is written again.
const ERROR_REWRITE_MS: i64 = 60_000;

/// The deliveries an event's routing asks for, and the routes whose expression failed.
pub(crate) fn dispatches(
    event: &Event,
    routes: &[Route],
    replay: bool,
) -> (Vec<NewDispatch>, Vec<(String, String)>) {
    let routed = route_event(event, routes);
    let dispatches = routed
        .matched
        .into_iter()
        .map(|m| NewDispatch {
            notification_id: new_id(),
            delivery_id: new_id(),
            route_id: m.route.id.clone(),
            route_name: m.route.name.clone(),
            connection_id: m.target.connection_id.clone(),
            destination: m.target.destination.clone(),
            template_id: m.target.template_id.clone(),
            replay,
        })
        .collect();
    (dispatches, routed.errors)
}

impl Engine {
    /// Routes the events that are waiting. Returns how many were handled.
    pub async fn route_once(&self) -> Result<usize> {
        let store = &self.config.store;
        let events = store
            .claim_unrouted(self.now(), self.config.lease_ms, BATCH)
            .await?;
        let mut routes_by_workspace: HashMap<String, Vec<Route>> = HashMap::new();
        let mut created_deliveries = false;
        for event in &events {
            if !routes_by_workspace.contains_key(&event.workspace_id) {
                let routes = store.list_routes(&event.workspace_id).await?;
                routes_by_workspace.insert(event.workspace_id.clone(), routes);
            }
            let routes = &routes_by_workspace[&event.workspace_id];
            let (dispatches, errors) = dispatches(event, routes, false);
            self.note_route_errors(&errors).await;
            // On failure the event keeps its lease and is routed again once it expires.
            if let Err(e) = store
                .write_routing(event, &dispatches, true, self.now())
                .await
            {
                tracing::error!(event_id = %event.id, error = %e, "could not write routing result");
                continue;
            }
            created_deliveries |= !dispatches.is_empty();
            self.announce(Change::EventRouted {
                event_id: event.id.clone(),
            });
        }
        if created_deliveries {
            self.delivery_wake.notify_one();
        }
        Ok(events.len())
    }

    /// Stores why routes failed to evaluate, so the UI can show it on the route.
    pub(crate) async fn note_route_errors(&self, errors: &[(String, String)]) {
        let now = self.now();
        for (route_id, reason) in errors {
            let is_news = {
                let mut known = self.route_errors.lock().unwrap_or_else(|p| p.into_inner());
                let stale = known
                    .get(route_id)
                    .is_none_or(|(last, at)| last != reason || now - at >= ERROR_REWRITE_MS);
                if stale {
                    known.insert(route_id.clone(), (reason.clone(), now));
                }
                stale
            };
            if !is_news {
                continue;
            }
            if let Err(e) = self
                .config
                .store
                .set_route_error(route_id, Some(reason), now)
                .await
            {
                tracing::warn!(route_id = %route_id, error = %e, "could not record a route error");
            }
        }
    }
}
