//! Domain types and pure logic for ramzes. No I/O.

mod event;
mod ids;
mod matching;
mod routing;

pub use event::{Event, EventStatus, NewEvent, Severity, ValidationError};
pub use ids::new_id;
pub use matching::{LabelCond, MatchError, RouteMatch};
pub use routing::{Matched, Route, Target, route_event};
