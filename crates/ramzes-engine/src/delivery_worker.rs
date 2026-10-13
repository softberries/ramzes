use std::sync::Arc;

use futures::StreamExt;
use ramzes_connectors::SendOutcome;
use ramzes_core::{Next, new_id, next_attempt};
use ramzes_store::{Attempt, Delivery, DeliveryState, Finish};
use tokio::sync::Semaphore;

use crate::{Change, Engine, Result};

/// Deliveries claimed per pass.
const BATCH: i64 = 32;
/// Deliveries sent at the same time.
const CONCURRENCY: usize = 16;
/// Requests one connection may have in flight, so ramzes does not flood a single service.
const PER_CONNECTION: usize = 4;

/// What one attempt produced, before it is written down.
struct Sent {
    outcome: SendOutcome,
    connector_kind: String,
    event_ids: Vec<String>,
}

/// Removes NUL characters from what a service said, since Postgres cannot store them as text.
fn without_nul(outcome: SendOutcome) -> SendOutcome {
    let clean = |detail: String| detail.replace('\0', "");
    match outcome {
        SendOutcome::Retry {
            after_ms,
            status,
            detail,
        } => SendOutcome::Retry {
            after_ms,
            status,
            detail: clean(detail),
        },
        SendOutcome::Permanent { status, detail } => SendOutcome::Permanent {
            status,
            detail: clean(detail),
        },
        SendOutcome::Delivered {
            external_ref,
            status,
        } => SendOutcome::Delivered {
            external_ref: external_ref.map(clean),
            status,
        },
    }
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
        if started_at > delivery.created_at + self.config.policy.max_age_ms {
            return self.expire(delivery, started_at).await;
        }
        let mut sent = self.send(delivery).await?;
        sent.outcome = without_nul(sent.outcome);
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

    /// Ends a delivery that outlived the time it may be retried, without sending it.
    ///
    /// The retry schedule normally gives up before this point. A delivery gets here only when
    /// its attempts could not be recorded, or ramzes was not running; this bounds both cases.
    async fn expire(&self, delivery: &Delivery, now: i64) -> Result<()> {
        let hours = self.config.policy.max_age_ms / 3_600_000;
        let error = format!("not sent: it could not be delivered within {hours} h of being queued");
        let attempt = Attempt {
            id: new_id(),
            delivery_id: delivery.id.clone(),
            started_at: now,
            duration_ms: 0,
            outcome: "expired".to_owned(),
            status: None,
            detail: Some(error.clone()),
        };
        self.config
            .store
            .finish_attempt(delivery, &attempt, &Finish::Dead { error }, now)
            .await?;
        let events = self
            .config
            .store
            .notification_events(&delivery.notification_id)
            .await?;
        self.announce(Change::DeliveryUpdated {
            event_ids: events.into_iter().map(|e| e.id).collect(),
            delivery_id: delivery.id.clone(),
            state: DeliveryState::Dead,
            connector_kind: "none".to_owned(),
            duration_ms: 0,
        });
        Ok(())
    }

    /// A permit to send through a connection; at most [`PER_CONNECTION`] are out at a time.
    async fn slot(&self, connection_id: &str) -> Option<tokio::sync::OwnedSemaphorePermit> {
        let semaphore = self
            .connection_slots
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .entry(connection_id.to_owned())
            .or_insert_with(|| Arc::new(Semaphore::new(PER_CONNECTION)))
            .clone();
        semaphore.acquire_owned().await.ok()
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
        let (connector, config) = match self.connector_for(&connection) {
            Ok(found) => found,
            Err(reason) => return Ok(fail(reason, &connection.kind)),
        };
        let template_id = store
            .notification_template(&delivery.notification_id)
            .await?;
        let Some(composed) = self
            .compose_for(&delivery.workspace_id, &events, template_id.as_deref())
            .await?
        else {
            return Ok(fail("the event was deleted".into(), &connection.kind));
        };
        if let Some(warning) = &composed.warning {
            if delivery.warning.as_ref() != Some(warning) {
                store.set_delivery_warning(&delivery.id, warning).await?;
            }
        }
        let _slot = self.slot(&connection.id).await;
        let outcome = connector
            .send(
                &config,
                &composed.message,
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
