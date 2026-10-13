//! Domain types and pure logic for ramzes. No I/O.

pub mod adapters;
mod backoff;
mod event;
pub mod expr;
mod ids;
mod keys;
mod mapping;
mod matching;
mod message;
mod routing;

pub use backoff::{Next, RetryPolicy, next_attempt};
pub use event::{Event, EventStatus, NewEvent, Severity, ValidationError};
pub use expr::event_context;
pub use ids::new_id;
pub use keys::{generate_key, hash_key};
pub use mapping::{FieldMap, Mapping, Verify};
pub use matching::{LabelCond, MatchError, RouteMatch};
pub use message::{Message, Template, apply_template, build_message};
pub use routing::{
    Matched, Route, RouteOutcome, RouteTrace, Routed, Target, route_event, trace_routes,
};
