use futures::StreamExt;
use ramzes_connectors::{ConnectionConfig, SendOutcome};
use ramzes_core::{Next, build_message, new_id, next_attempt};
use ramzes_store::{Attempt, Delivery, DeliveryState, Finish};

use crate::{Change, Engine, Result};

/// Deliveries claimed per pass.
const BATCH: i64 = 32;
/// Deliveries sent at the same time.
const CONCURRENCY: usize = 8;

/// What one attempt produced, before it is written down.
struct Sent {
    outcome: SendOutcome,
    connector_kind: String,
    event_ids: Vec<String>,
}

fn rejected(detail: impl Into<String>) -> SendOutcome {
    SendOutcome::Permanent {
        status: None,
        detail: detail.into(),
    }
}

impl Engine {
    /// Sends the deliveries that are due. Returns how many were attempted.
    pub async fn deliver_once(&self) -> Result<usize> {
        let claimed = self
            .config
            .store
            .claim_due_deliveries(self.now(), self.config.lease_ms, BATCH)
            .await?;
        let count = claimed.len();
        futures::stream::iter(claimed)
            .for_each_concurrent(CONCURRENCY, |delivery| async move {
                // On failure the delivery keeps its lease and is attempted again once it expires.
                if let Err(e) = self.attempt(&delivery).await {
                    tracing::error!(delivery_id = %delivery.id, error = %e, "could not record delivery attempt");
                }
            })
            .await;
        Ok(count)
    }

    async fn attempt(&self, delivery: &Delivery) -> Result<()> {
        let started_at = self.now();
        let sent = self.send(delivery).await?;
        let finished_at = self.now();
        let attempts_done = delivery.attempts + 1;

        let (state, outcome, status, detail, finish) = match sent.outcome {
            SendOutcome::Delivered {
                external_ref,
                status,
            } => (
                DeliveryState::Delivered,
                "delivered",
                status,
                None,
                Finish::Delivered { external_ref },
            ),
            SendOutcome::Permanent { status, detail } => (
                DeliveryState::Failed,
                "failed",
                status,
                Some(detail.clone()),
                Finish::Failed { error: detail },
            ),
            SendOutcome::Retry {
                after_ms,
                status,
                detail,
            } => {
                let next = next_attempt(
                    &self.config.policy,
                    attempts_done,
                    delivery.created_at,
                    finished_at,
                    after_ms,
                    (self.config.jitter)(),
                );
                match next {
                    Next::At(at) => (
                        DeliveryState::Pending,
                        "retry",
                        status,
                        Some(detail.clone()),
                        Finish::Retry { at, error: detail },
                    ),
                    Next::GiveUp => {
                        let error = format!("gave up after {attempts_done} attempts: {detail}");
                        (
                            DeliveryState::Dead,
                            "retry",
                            status,
                            Some(detail),
                            Finish::Dead { error },
                        )
                    }
                }
            }
        };
        let attempt = Attempt {
            id: new_id(),
            delivery_id: delivery.id.clone(),
            started_at,
            duration_ms: finished_at - started_at,
            outcome: outcome.to_owned(),
            status: status.map(i64::from),
            detail,
        };
        self.config
            .store
            .finish_attempt(delivery, &attempt, &finish, finished_at)
            .await?;
        self.announce(Change::DeliveryUpdated {
            event_ids: sent.event_ids,
            delivery_id: delivery.id.clone(),
            state,
            connector_kind: sent.connector_kind,
            duration_ms: attempt.duration_ms,
        });
        Ok(())
    }

    /// Builds the message and hands it to the connector. Problems with the connection itself
    /// are reported as a permanent failure of the delivery.
    async fn send(&self, delivery: &Delivery) -> Result<Sent> {
        let store = &self.config.store;
        let events = store.notification_events(&delivery.notification_id).await?;
        let event_ids: Vec<String> = events.iter().map(|e| e.id.clone()).collect();
        let fail = |detail: String, kind: &str| Sent {
            outcome: rejected(detail),
            connector_kind: kind.to_owned(),
            event_ids: event_ids.clone(),
        };

        let Some(connection) = store
            .get_connection(&delivery.workspace_id, &delivery.connection_id)
            .await?
        else {
            return Ok(fail("the connection was deleted".into(), "unknown"));
        };
        let Some(connector) = self.config.registry.get(&connection.kind) else {
            return Ok(fail(
                format!(
                    "this version of ramzes has no connector of kind {:?}",
                    connection.kind
                ),
                &connection.kind,
            ));
        };
        let credentials = match self.config.opener.open(&connection.credentials) {
            Ok(credentials) => credentials,
            Err(e) => {
                return Ok(fail(
                    format!("the connection's credentials could not be read: {e}"),
                    &connection.kind,
                ));
            }
        };
        let Some(event) = events.first() else {
            return Ok(fail("the event was deleted".into(), &connection.kind));
        };

        let public_url = match &self.config.public_url {
            Some(url) => Some(url.clone()),
            None => store
                .get_setting(&delivery.workspace_id, "public_url")
                .await?
                .filter(|url| !url.is_empty()),
        };
        let message = build_message(event, public_url.as_deref());
        let config = ConnectionConfig {
            settings: connection.settings,
            credentials,
        };
        let outcome = connector
            .send(
                &config,
                &message,
                delivery.destination.as_deref(),
                self.now(),
            )
            .await;
        Ok(Sent {
            outcome,
            connector_kind: connection.kind,
            event_ids,
        })
    }
}
