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
}

#[derive(Debug, Clone, Copy)]
pub struct Matched<'a> {
    pub route: &'a Route,
    pub target: &'a Target,
}

/// The outcome of checking an event against the routes.
#[derive(Debug, Clone, Default)]
pub struct Routed<'a> {
    /// Every target of every enabled route that matches, in route order.
    pub matched: Vec<Matched<'a>>,
    /// Routes whose expression failed, with the reason. They count as not matching.
    pub errors: Vec<(String, String)>,
}

pub fn route_event<'a>(e: &Event, routes: &'a [Route]) -> Routed<'a> {
    let mut ordered: Vec<&Route> = routes.iter().filter(|r| r.enabled).collect();
    ordered.sort_by_key(|r| r.position);
    let mut routed = Routed::default();
    for route in ordered {
        match route.r#match.evaluate(e) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(reason) => {
                routed.errors.push((route.id.clone(), reason));
                continue;
            }
        }
        routed
            .matched
            .extend(route.targets.iter().map(|target| Matched { route, target }));
        if route.stop {
            break;
        }
    }
    routed
}
