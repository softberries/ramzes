use std::collections::HashMap;

use ramzes_core::shaping::{
    Action, Decision, NotificationState, Role, ShapeInput, Silence, dedup_key, shape,
};
use ramzes_core::{Event, Matched, Route, new_id, route_event};
use ramzes_store::{Join, NewDispatch, NewTimer};

use crate::{Change, Engine, Result};

/// Events routed per pass.
const BATCH: i64 = 100;
/// How often the same error of the same route is written again.
const ERROR_REWRITE_MS: i64 = 60_000;

/// What routing one event asks the store to write.
#[derive(Default)]
pub(crate) struct Plan {
    pub dispatches: Vec<NewDispatch>,
    pub joins: Vec<Join>,
    /// Routes whose expression failed, with the reason.
    pub errors: Vec<(String, String)>,
}

impl Plan {
    fn creates_deliveries(&self) -> bool {
        self.dispatches.iter().any(|d| d.delivery_id.is_some())
            || self
                .joins
                .iter()
                .any(|j| matches!(j.resolve, Some(Some(_))))
    }

    fn notification_ids(&self) -> impl Iterator<Item = &String> {
        self.dispatches
            .iter()
            .map(|d| &d.notification_id)
            .chain(self.joins.iter().map(|j| &j.notification_id))
    }
}

/// Identifies one target of one route, so later events find what earlier ones opened.
pub(crate) fn target_key(m: &Matched<'_>) -> String {
    format!(
        "{}|{}|{}",
        m.route.id,
        m.target.connection_id,
        m.target.destination.as_deref().unwrap_or_default()
    )
}

fn dispatch(m: &Matched<'_>, replay: bool) -> NewDispatch {
    NewDispatch {
        notification_id: new_id(),
        route_id: m.route.id.clone(),
        route_name: m.route.name.clone(),
        connection_id: m.target.connection_id.clone(),
        destination: m.target.destination.clone(),
        template_id: m.target.template_id.clone(),
        target_key: target_key(m),
        replay,
        ..NewDispatch::default()
    }
}

/// A replay: every matched target is sent at once, whatever the route's shaping says.
pub(crate) fn unshaped(event: &Event, routes: &[Route]) -> Plan {
    let routed = route_event(event, routes);
    Plan {
        dispatches: routed
            .matched
            .iter()
            .map(|m| NewDispatch {
                delivery_id: Some(new_id()),
                dedup_key: event.dedup_key.clone(),
                ..dispatch(m, true)
            })
            .collect(),
        joins: Vec::new(),
        errors: routed.errors,
    }
}

impl Engine {
    /// What shaping decides for one event at one route target, given what is stored now, and
    /// the event's dedup key there. Reads only; the route tester uses it to explain a route.
    pub async fn decide(
        &self,
        event: &Event,
        m: &Matched<'_>,
        silences: &[Silence],
    ) -> Result<(Decision, Option<String>)> {
        let store = &self.config.store;
        let now = self.now();
        let shaping = &m.route.shaping;
        let key = target_key(m);
        let event_key = dedup_key(shaping, event, &mut Vec::new());
        let looks_back = shaping.group.is_some()
            || shaping.quiet_hours.is_some()
            || shaping.rate_limit.is_some()
            || event_key.is_some();
        let open = if looks_back {
            store
                .open_notifications(&event.workspace_id, &key, event_key.as_deref())
                .await?
        } else {
            Vec::new()
        };
        let (sends, oldest) = match &shaping.rate_limit {
            Some(rate) => {
                store
                    .sends_since(&event.workspace_id, &key, now - rate.window_s * 1000)
                    .await?
            }
            None => (0, None),
        };
        let decision = shape(&ShapeInput {
            event,
            shaping,
            silences,
            open: &open,
            sends_in_window: sends,
            oldest_send_in_window: oldest,
            now_ms: now,
        });
        Ok((decision, event_key))
    }

    /// Decides, for each target of each matching route, what the event does there. Call with
    /// the shaping lock held: the decision depends on what earlier events wrote.
    async fn plan(&self, event: &Event, routes: &[Route], silences: &[Silence]) -> Result<Plan> {
        let routed = route_event(event, routes);
        let mut plan = Plan {
            errors: routed.errors,
            ..Plan::default()
        };
        for m in &routed.matched {
            let (decision, event_key) = self.decide(event, m, silences).await?;
            let fresh = dispatch(m, false);
            match decision.action {
                Action::Send {
                    dedup_key,
                    group_key,
                } => plan.dispatches.push(NewDispatch {
                    delivery_id: Some(new_id()),
                    dedup_key,
                    group_key,
                    ..fresh
                }),
                Action::Suppress { reason, silence_id } => plan.dispatches.push(NewDispatch {
                    state: NotificationState::Suppressed,
                    reason: Some(reason),
                    silence_id,
                    dedup_key: event_key,
                    ..fresh
                }),
                Action::Attach {
                    notification_id,
                    role,
                } => plan.joins.push(Join {
                    notification_id,
                    role,
                    resolve: None,
                }),
                Action::Resolve {
                    notification_id,
                    notify,
                } => plan.joins.push(Join {
                    notification_id,
                    role: Role::Resolved,
                    resolve: Some(notify.then(new_id)),
                }),
                Action::OpenGroup {
                    group_key,
                    dedup_key,
                    flush_at,
                } => plan.dispatches.push(NewDispatch {
                    state: NotificationState::Grouping,
                    group_key: Some(group_key),
                    dedup_key,
                    timer: Some(NewTimer {
                        kind: "flush".into(),
                        due_at: flush_at,
                        data: None,
                    }),
                    ..fresh
                }),
                Action::Hold {
                    existing: Some(notification_id),
                    ..
                } => plan.joins.push(Join {
                    notification_id,
                    role: Role::Grouped,
                    resolve: None,
                }),
                Action::Hold {
                    existing: None,
                    kind,
                    until,
                    reason,
                    dedup_key,
                    group_key,
                } => plan.dispatches.push(NewDispatch {
                    state: NotificationState::Held,
                    reason: Some(reason),
                    hold: Some((kind, until)),
                    dedup_key,
                    group_key,
                    timer: Some(NewTimer {
                        kind: "release".into(),
                        due_at: until,
                        data: None,
                    }),
                    ..fresh
                }),
            }
        }
        Ok(plan)
    }

    /// Routes the events that are waiting. Returns how many were handled.
    pub async fn route_once(&self) -> Result<usize> {
        let store = &self.config.store;
        let events = store
            .claim_unrouted(self.now(), self.config.lease_ms, BATCH)
            .await?;
        let mut by_workspace: HashMap<String, (Vec<Route>, Vec<Silence>)> = HashMap::new();
        let mut created_deliveries = false;
        for event in &events {
            if !by_workspace.contains_key(&event.workspace_id) {
                let routes = store.list_routes(&event.workspace_id).await?;
                let silences = store
                    .active_silences(&event.workspace_id, self.now())
                    .await?;
                by_workspace.insert(event.workspace_id.clone(), (routes, silences));
            }
            let (routes, silences) = &by_workspace[&event.workspace_id];
            // One event at a time: what this one writes decides what the next one does.
            let _shaping = self.shaping_lock.lock().await;
            let plan = match self.plan(event, routes, silences).await {
                Ok(plan) => plan,
                Err(e) => {
                    tracing::error!(event_id = %event.id, error = %e, "could not shape an event");
                    continue;
                }
            };
            self.note_route_errors(&plan.errors).await;
            // On failure the event keeps its lease and is routed again once it expires.
            if let Err(e) = store
                .write_routing(event, &plan.dispatches, &plan.joins, true, self.now())
                .await
            {
                tracing::error!(event_id = %event.id, error = %e, "could not write routing result");
                continue;
            }
            created_deliveries |= plan.creates_deliveries();
            self.announce(Change::EventRouted {
                event_id: event.id.clone(),
            });
            for notification_id in plan.notification_ids() {
                self.announce(Change::NotificationChanged {
                    notification_id: notification_id.clone(),
                });
            }
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
