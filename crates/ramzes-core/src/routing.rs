use serde::{Deserialize, Serialize};

use crate::event::Event;
use crate::matching::RouteMatch;

/// Where a route sends: a connection and an optional destination within it (for example a channel).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub connection_id: String,
    #[serde(default)]
    pub destination: Option<String>,
    /// The template that words the message for this target. Without one the event's own text is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Route {
    pub id: String,
    pub name: String,
    pub position: i64,
    pub enabled: bool,
    /// When this route matches, routes after it are not evaluated.
    pub stop: bool,
    pub r#match: RouteMatch,
    pub targets: Vec<Target>,
    /// What the route does besides sending: dedup, grouping, quiet hours, rate limit.
    #[serde(default)]
    pub shaping: crate::shaping::Shaping,
    /// Who else is told, and after how long, while nobody acknowledges.
    #[serde(default)]
    pub escalation: Vec<EscalationStep>,
}

/// One step of an escalation chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EscalationStep {
    /// Seconds after the previous step (or after the notification was first sent).
    pub after_s: i64,
    pub targets: Vec<Target>,
}

const STEPS_MAX: usize = 10;
const WEEK_S: i64 = 7 * 86_400;

/// Checks an escalation chain.
pub fn validate_escalation(steps: &[EscalationStep]) -> Result<(), String> {
    if steps.len() > STEPS_MAX {
        return Err(format!("escalation: at most {STEPS_MAX} steps are allowed"));
    }
    for (index, step) in steps.iter().enumerate() {
        let number = index + 1;
        if !(1..=WEEK_S).contains(&step.after_s) {
            return Err(format!(
                "escalation step {number}: the delay must be 1 second to 7 days"
            ));
        }
        if step.targets.is_empty() {
            return Err(format!(
                "escalation step {number}: it needs at least one target"
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub struct Matched<'a> {
    pub route: &'a Route,
    pub target: &'a Target,
    /// The event is a resolution less severe than the route asks for: it closes what the route
    /// opened and does nothing else there.
    pub closes_only: bool,
}

/// The outcome of checking an event against the routes.
#[derive(Debug, Clone, Default)]
pub struct Routed<'a> {
    /// Every target of every enabled route that matches, in route order.
    pub matched: Vec<Matched<'a>>,
    /// Routes whose expression failed, with the reason. They count as not matching.
    pub errors: Vec<(String, String)>,
}

/// What happened when an event was checked against one route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteOutcome {
    Matched,
    /// A resolved event that matches except for being less severe than the route asks for. It
    /// closes what the route opened and does not count as a match otherwise.
    ClosesOnly,
    NotMatched,
    /// The route is switched off.
    Disabled,
    /// A route before this one matched and stops evaluation.
    Skipped,
    /// The route's expression failed; the route counts as not matching.
    Error(String),
}

#[derive(Debug, Clone)]
pub struct RouteTrace<'a> {
    pub route: &'a Route,
    pub outcome: RouteOutcome,
}

/// Checks an event against every route in evaluation order and says what happened at each.
pub fn trace_routes<'a>(e: &Event, routes: &'a [Route]) -> Vec<RouteTrace<'a>> {
    let mut ordered: Vec<&Route> = routes.iter().collect();
    ordered.sort_by_key(|r| r.position);
    let mut stopped = false;
    ordered
        .into_iter()
        .map(|route| {
            let outcome = if !route.enabled {
                RouteOutcome::Disabled
            } else if stopped {
                RouteOutcome::Skipped
            } else {
                match route.r#match.evaluate(e) {
                    Ok(true) => {
                        stopped = route.stop;
                        RouteOutcome::Matched
                    }
                    Ok(false) => match route.r#match.closes(e) {
                        Ok(true) => RouteOutcome::ClosesOnly,
                        Ok(false) => RouteOutcome::NotMatched,
                        Err(reason) => RouteOutcome::Error(reason),
                    },
                    Err(reason) => RouteOutcome::Error(reason),
                }
            };
            RouteTrace { route, outcome }
        })
        .collect()
}

pub fn route_event<'a>(e: &Event, routes: &'a [Route]) -> Routed<'a> {
    let mut routed = Routed::default();
    for trace in trace_routes(e, routes) {
        match trace.outcome {
            RouteOutcome::Matched | RouteOutcome::ClosesOnly => {
                let route = trace.route;
                let closes_only = trace.outcome == RouteOutcome::ClosesOnly;
                routed
                    .matched
                    .extend(route.targets.iter().map(|target| Matched {
                        route,
                        target,
                        closes_only,
                    }));
            }
            RouteOutcome::Error(reason) => routed.errors.push((trace.route.id.clone(), reason)),
            RouteOutcome::NotMatched | RouteOutcome::Disabled | RouteOutcome::Skipped => {}
        }
    }
    routed
}
