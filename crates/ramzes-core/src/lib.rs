//! Domain types and pure logic for ramzes. No I/O.

mod backoff;
mod event;
mod ids;
mod keys;
mod matching;
mod message;
mod routing;

pub use backoff::{Next, RetryPolicy, next_attempt};
pub use event::{Event, EventStatus, NewEvent, Severity, ValidationError};
pub use ids::new_id;
pub use keys::{generate_key, hash_key};
pub use matching::{LabelCond, MatchError, RouteMatch};
pub use message::{Message, build_message};
pub use routing::{Matched, Route, Target, route_event};
