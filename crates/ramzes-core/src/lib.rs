//! Domain types and pure logic for ramzes. No I/O.

mod event;
mod ids;

pub use event::{Event, EventStatus, NewEvent, Severity, ValidationError};
pub use ids::new_id;
