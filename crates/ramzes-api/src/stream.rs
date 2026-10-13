use std::convert::Infallible;
use std::time::Duration;

use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::{Stream, StreamExt};
use ramzes_engine::Change;
use serde_json::json;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;

use crate::{AppState, Auth};

const KEEP_ALIVE: Duration = Duration::from_secs(15);

fn to_event(change: Result<Change, BroadcastStreamRecvError>) -> Event {
    match change {
        Ok(Change::EventRouted { event_id }) => Event::default()
            .event("event")
            .data(json!({"event_id": event_id}).to_string()),
        Ok(Change::NotificationChanged { notification_id }) => Event::default()
            .event("notification")
            .data(json!({"notification_id": notification_id}).to_string()),
        Ok(Change::DeliveryUpdated {
            event_ids,
            delivery_id,
            state,
            ..
        }) => Event::default().event("delivery").data(
            json!({"event_ids": event_ids, "delivery_id": delivery_id, "state": state}).to_string(),
        ),
        // The client fell behind and missed changes; it should reload what it shows.
        Err(BroadcastStreamRecvError::Lagged(_)) => Event::default().event("resync").data("{}"),
    }
}

/// Live changes as server-sent events.
///
/// `event` carries `{"event_id"}` when an event has been routed. `delivery` carries
/// `{"event_ids", "delivery_id", "state"}` after each delivery attempt. `notification` carries
/// `{"notification_id"}` when a notification changes state. `resync` means changes
/// were missed and the client should reload.
#[utoipa::path(
    get, path = "/api/stream", tag = "events",
    responses((status = 200, description = "A stream of server-sent events", content_type = "text/event-stream", body = String))
)]
pub(crate) async fn stream(
    State(state): State<AppState>,
    _auth: Auth,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let changes = BroadcastStream::new(state.engine.subscribe()).map(|change| Ok(to_event(change)));
    Sse::new(changes).keep_alive(KeepAlive::new().interval(KEEP_ALIVE))
}
