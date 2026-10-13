use ramzes_core::new_id;
use ramzes_store::Delivery;

use crate::route_worker::dispatches;
use crate::{Engine, EngineError, Result};

impl Engine {
    /// Queues a new delivery of the same notification to the same target.
    pub async fn replay_delivery(&self, workspace_id: &str, delivery_id: &str) -> Result<Delivery> {
        let fresh = self
            .config
            .store
            .requeue_delivery(workspace_id, delivery_id, &new_id(), self.now())
            .await?;
        self.delivery_wake.notify_one();
        Ok(fresh)
    }

    /// Runs an event through the current routes again. Returns the number of deliveries queued.
    pub async fn replay_event(&self, workspace_id: &str, event_id: &str) -> Result<usize> {
        let store = &self.config.store;
        let event = store
            .get_event(workspace_id, event_id)
            .await?
            .ok_or(EngineError::NotFound)?;
        let routes = store.list_routes(workspace_id).await?;
        let dispatches = dispatches(&event, &routes, true);
        store
            .write_routing(&event, &dispatches, false, self.now())
            .await?;
        self.delivery_wake.notify_one();
        Ok(dispatches.len())
    }
}
