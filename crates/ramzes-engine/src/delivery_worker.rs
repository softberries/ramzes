use std::sync::Arc;

use futures::StreamExt;
use ramzes_connectors::SendOutcome;
use ramzes_core::shaping::NotificationState;
use ramzes_core::{EventStatus, Message, Next, Severity, new_id, next_attempt};
use ramzes_store::{Attempt, Delivery, DeliveryState, Finish, NotificationCore};
use tokio::sync::Semaphore;

use crate::system::{SYSTEM_TOPIC_PREFIX, SystemEvent};
use crate::{Change, Engine, Result};

/// Deliveries claimed per pass.
const BATCH: i64 = 32;
/// Deliveries sent at the same time.
const CONCURRENCY: usize = 16;
/// Requests one connection may have in flight, so ramzes does not flood a single service.
const PER_CONNECTION: usize = 4;
/// Failed attempts in a row after which a connection is paused.
const PAUSE_AFTER: i64 = 5;
const FIRST_PAUSE_MS: i64 = 60_000;
const LONGEST_PAUSE_MS: i64 = 15 * 60_000;

/// The text whose signature is the token of a notification's acknowledgement link.
pub fn ack_text(notification_id: &str) -> String {
    format!("ack:{notification_id}")
}

/// What one attempt produced, before it is written down.
struct Sent {
    outcome: SendOutcome,
    connector_kind: String,
    event_ids: Vec<String>,
    /// The connection, when it still exists: `(id, name)`.
    connection: Option<(String, String)>,
    /// The delivery carries one of ramzes's own events.
    about_ramzes: bool,
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

/// Whether an outcome says something about the connection rather than about one message:
/// `Some(true)` it works, `Some(false)` it is failing, `None` no conclusion.
fn connection_verdict(outcome: &SendOutcome) -> Option<bool> {
    match outcome {
        SendOutcome::Delivered { .. } => Some(true),
        SendOutcome::Retry { .. } => Some(false),
        // Wrong credentials or a missing endpoint fail every message; a 400 may be this one only.
        SendOutcome::Permanent {
            status: Some(401 | 403 | 404),
            ..
        } => Some(false),
        SendOutcome::Permanent { .. } => None,
    }
}

fn pause_ms(streak: i64) -> i64 {
    let doublings = u32::try_from(streak - PAUSE_AFTER).unwrap_or(0).min(10);
    FIRST_PAUSE_MS
        .saturating_mul(1_i64 << doublings)
        .min(LONGEST_PAUSE_MS)
}

impl Engine {
    /// Sends the deliveries that are due and updates messages whose notification changed.
    /// Returns how many things were done.
    pub async fn deliver_once(&self) -> Result<usize> {
        let refreshed = self.refresh_messages().await;
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
        Ok(count + refreshed)
    }

    async fn attempt(&self, delivery: &Delivery) -> Result<()> {
        let store = &self.config.store;
        let started_at = self.now();
        if started_at > delivery.created_at + self.config.policy.max_age_ms {
            return self.expire(delivery, started_at).await;
        }
        // A paused connection is left alone until its pause ends.
        let status = store.connection_status(&delivery.workspace_id).await?;
        let paused_until = status
            .get(&delivery.connection_id)
            .and_then(|s| s.paused_until)
            .filter(|until| *until > started_at);
        if let Some(until) = paused_until {
            return Ok(store.defer_delivery(&delivery.id, until).await?);
        }

        let mut sent = self.send(delivery).await?;
        sent.outcome = without_nul(sent.outcome);
        let finished_at = self.now();
        let attempts_done = delivery.attempts + 1;
        let verdict = connection_verdict(&sent.outcome);

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
            detail: detail.clone(),
        };
        store
            .finish_attempt(delivery, &attempt, &finish, finished_at)
            .await?;
        self.announce(Change::DeliveryUpdated {
            event_ids: sent.event_ids,
            delivery_id: delivery.id.clone(),
            state,
            connector_kind: sent.connector_kind,
            duration_ms: attempt.duration_ms,
        });

        let Some((connection_id, connection_name)) = sent.connection else {
            return Ok(());
        };
        if let Some(works) = verdict {
            let streak = store
                .record_connection_result(&connection_id, works)
                .await?;
            if !works && streak >= PAUSE_AFTER {
                let until = finished_at + pause_ms(streak);
                store.pause_connection(&connection_id, until).await?;
                // Reported when the pause begins, not each time it is extended.
                if streak == PAUSE_AFTER && !sent.about_ramzes {
                    let reason = detail.clone().unwrap_or_default();
                    self.raise(
                        &delivery.workspace_id,
                        SystemEvent {
                            topic: "ramzes.connection.paused",
                            title: format!("Connection {connection_name} is paused after {streak} failed attempts"),
                            body: Some(format!(
                                "ramzes will try it again from time to time and resume when it answers. Last error: {reason}"
                            )),
                            severity: Severity::Error,
                            labels: &[("connection", &connection_name), ("connection_id", &connection_id)],
                            dedup_key: format!("connection-paused:{connection_id}"),
                        },
                    )
                    .await;
                }
            }
        }
        // A message that will never arrive is worth an event of its own, unless it was itself
        // one of ramzes's reports: that would feed on itself.
        if matches!(state, DeliveryState::Failed | DeliveryState::Dead) && !sent.about_ramzes {
            let outcome = if state == DeliveryState::Dead {
                "gave up"
            } else {
                "rejected"
            };
            let reason = detail.unwrap_or_default();
            self.raise(
                &delivery.workspace_id,
                SystemEvent {
                    topic: "ramzes.delivery.failed",
                    title: format!("A message to {connection_name} could not be delivered"),
                    body: Some(reason),
                    severity: Severity::Warning,
                    labels: &[
                        ("connection", &connection_name),
                        ("connection_id", &connection_id),
                        ("outcome", outcome),
                        ("delivery_id", &delivery.id),
                    ],
                    dedup_key: format!("delivery-failed:{connection_id}"),
                },
            )
            .await;
        }
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

    /// Fills in what ties a message to its notification: the key services pair messages by,
    /// the link that acknowledges it, and the earlier message a follow-up belongs under.
    async fn address(
        &self,
        message: &mut Message,
        delivery: &Delivery,
        notification: Option<&NotificationCore>,
    ) -> Result<()> {
        let id = &delivery.notification_id;
        message.notification_id = Some(id.clone());
        message.dedup_key = Some(
            notification
                .and_then(|n| n.dedup_key.clone())
                .unwrap_or_else(|| id.clone()),
        );
        message.thread_ref = self
            .config
            .store
            .first_external_ref(id, &delivery.connection_id, delivery.destination.as_deref())
            .await?;
        let open = notification.is_some_and(|n| {
            matches!(
                n.state,
                NotificationState::Sent | NotificationState::Grouping | NotificationState::Held
            )
        });
        if open && let Some(base) = self.public_url(&delivery.workspace_id).await? {
            let token = self.config.opener.sign(&ack_text(id));
            message.ack_link = Some(format!("{}/ack/{id}?t={token}", base.trim_end_matches('/')));
        }
        Ok(())
    }

    /// Rewrites the delivered messages of notifications that were acknowledged or resolved,
    /// where the service allows it. Returns how many notifications were handled.
    async fn refresh_messages(&self) -> usize {
        let ids: Vec<String> =
            std::mem::take(&mut *self.refreshes.lock().unwrap_or_else(|p| p.into_inner()));
        for id in &ids {
            if let Err(e) = self.refresh(id).await {
                tracing::warn!(notification_id = %id, error = %e, "could not update delivered messages");
            }
        }
        ids.len()
    }

    async fn refresh(&self, notification_id: &str) -> Result<()> {
        let store = &self.config.store;
        let Some(n) = store.notification_core(notification_id).await? else {
            return Ok(());
        };
        let note = match (n.state, &n.acked_by) {
            (NotificationState::Resolved, _) => "Resolved".to_owned(),
            (NotificationState::Acknowledged, Some(by)) => format!("Acknowledged by {by}"),
            (NotificationState::Acknowledged, None) => "Acknowledged".to_owned(),
            _ => return Ok(()),
        };
        let refs = store.message_refs(notification_id).await?;
        if refs.is_empty() {
            return Ok(());
        }
        let mut events = store.notification_events(notification_id).await?;
        if events.iter().any(|e| e.status == EventStatus::Firing) {
            events.retain(|e| e.status == EventStatus::Firing);
        }
        let Some(mut composed) = self
            .compose_for(
                &n.workspace_id,
                notification_id,
                &events,
                n.template_id.as_deref(),
            )
            .await?
        else {
            return Ok(());
        };
        if n.state == NotificationState::Resolved {
            composed.message.status = EventStatus::Resolved;
        }
        composed.message.notification_id = Some(notification_id.to_owned());
        for (connection_id, _destination, reference) in refs {
            let Some(connection) = store
                .get_connection(&n.workspace_id, &connection_id)
                .await?
            else {
                continue;
            };
            let Ok((connector, config)) = self.connector_for(&connection) else {
                continue;
            };
            if let Err(reason) = connector
                .update(&config, &reference, &composed.message, &note)
                .await
            {
                tracing::warn!(connection = %connection.name, %reason, "a delivered message could not be updated");
            }
        }
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
        let mut events = store.notification_events(&delivery.notification_id).await?;
        let event_ids: Vec<String> = events.iter().map(|e| e.id.clone()).collect();
        let about_ramzes = events
            .iter()
            .any(|e| e.topic.starts_with(SYSTEM_TOPIC_PREFIX));
        let fail = |detail: String, kind: &str, connection: Option<(String, String)>| Sent {
            outcome: rejected(detail),
            connector_kind: kind.to_owned(),
            event_ids: event_ids.clone(),
            connection,
            about_ramzes,
        };

        let Some(connection) = store
            .get_connection(&delivery.workspace_id, &delivery.connection_id)
            .await?
        else {
            return Ok(fail("the connection was deleted".into(), "unknown", None));
        };
        let named = Some((connection.id.clone(), connection.name.clone()));
        let (connector, config) = match self.connector_for(&connection) {
            Ok(found) => found,
            Err(reason) => return Ok(fail(reason, &connection.kind, named)),
        };
        let notification = store.notification_core(&delivery.notification_id).await?;
        let resolved = notification
            .as_ref()
            .is_some_and(|n| n.state == NotificationState::Resolved);
        // The notice that something is resolved is about what was firing, not about the event
        // that reported the resolution.
        if resolved && events.iter().any(|e| e.status == EventStatus::Firing) {
            events.retain(|e| e.status == EventStatus::Firing);
        }
        let notification = notification;
        let template_id = delivery
            .template_id
            .clone()
            .or_else(|| notification.as_ref().and_then(|n| n.template_id.clone()));
        let Some(mut composed) = self
            .compose_for(
                &delivery.workspace_id,
                &delivery.notification_id,
                &events,
                template_id.as_deref(),
            )
            .await?
        else {
            return Ok(fail(
                "the event was deleted".into(),
                &connection.kind,
                named,
            ));
        };
        if resolved {
            composed.message.status = EventStatus::Resolved;
        } else if delivery.step.is_some() {
            composed.message.title = format!("Escalated: {}", composed.message.title);
        }
        self.address(&mut composed.message, delivery, notification.as_ref())
            .await?;
        if let Some(warning) = &composed.warning
            && delivery.warning.as_ref() != Some(warning)
        {
            store.set_delivery_warning(&delivery.id, warning).await?;
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
            connection: named,
            about_ramzes,
        })
    }
}
