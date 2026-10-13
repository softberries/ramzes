use std::collections::HashMap;

use ramzes_core::{Event, Route, new_id, route_event};
use ramzes_store::NewDispatch;

use crate::{Change, Engine, Result};

/// Events routed per pass.
const BATCH: i64 = 100;

pub(crate) fn dispatches(event: &Event, routes: &[Route], replay: bool) -> Vec<NewDispatch> {
    route_event(event, routes)
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
        .collect()
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
            let dispatches = dispatches(event, routes, false);
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
}
