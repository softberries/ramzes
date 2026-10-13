use serde::{Deserialize, Serialize};

use crate::event::Event;
use crate::matching::RouteMatch;

/// Where a route sends: a connection and an optional destination within it (for example a channel).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub connection_id: String,
    #[serde(default)]
    pub destination: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Route {
    pub id: String,
    pub name: String,
    pub position: i64,
    pub enabled: bool,
    /// When this route matches, routes after it are not evaluated.
    pub stop: bool,
    pub r#match: RouteMatch,
    pub targets: Vec<Target>,
}

#[derive(Debug, Clone, Copy)]
pub struct Matched<'a> {
    pub route: &'a Route,
    pub target: &'a Target,
}

/// Every target of every enabled route that matches the event, in route order.
pub fn route_event<'a>(e: &Event, routes: &'a [Route]) -> Vec<Matched<'a>> {
    let mut ordered: Vec<&Route> = routes.iter().filter(|r| r.enabled).collect();
    ordered.sort_by_key(|r| r.position);
    let mut matched = Vec::new();
    for route in ordered {
        if !route.r#match.matches(e) {
            continue;
        }
        matched.extend(route.targets.iter().map(|target| Matched { route, target }));
        if route.stop {
            break;
        }
    }
    matched
}
