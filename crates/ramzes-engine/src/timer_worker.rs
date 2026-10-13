//! Acts on notifications when their time comes: sends a group, releases a hold, repeats a reminder.

use ramzes_core::shaping::{
    HoldKind, NotificationState, ReleaseFrom, ReleaseInput, Released, release,
};
use ramzes_core::{Route, new_id};
use ramzes_store::{NewDelivery, NewTimer, NotificationCore, Timer};

use crate::route_worker::first_escalation;

use crate::{Change, Engine, Result};

/// Timers handled per pass.
const BATCH: i64 = 50;
/// Messages a notification may send in total, however long it stays open.
const MAX_SENDS: i64 = 20;

impl Engine {
    /// Handles the timers that are due. Returns how many there were.
    pub async fn timers_once(&self) -> Result<usize> {
        let store = &self.config.store;
        let timers = store
            .claim_due_timers(self.now(), self.config.lease_ms, BATCH)
            .await?;
        let mut sent = false;
        for timer in &timers {
            let _shaping = self.shaping_lock.lock().await;
            match self.fire(timer).await {
                Ok(queued) => sent |= queued,
                Err(e) => {
                    // The timer keeps its lease and fires again once it expires.
                    tracing::error!(timer_id = %timer.id, error = %e, "could not handle a timer");
                    continue;
                }
            }
            store.delete_timer(&timer.id).await?;
            self.announce(Change::NotificationChanged {
                notification_id: timer.notification_id.clone(),
            });
        }
        if sent {
            self.delivery_wake.notify_one();
        }
        Ok(timers.len())
    }

    /// The route a notification came from, as it is now. `None` when it was deleted.
    async fn route_of(&self, n: &NotificationCore) -> Result<Option<Route>> {
        let routes = self.config.store.list_routes(&n.workspace_id).await?;
        Ok(routes.into_iter().find(|r| r.id == n.route_id))
    }

    /// Sends an escalation step to its targets and arms the next one. Returns whether anything
    /// was queued.
    async fn escalate(
        &self,
        n: &NotificationCore,
        step_text: Option<&str>,
        now: i64,
    ) -> Result<bool> {
        // A deleted route, or a chain that was shortened meanwhile, ends the escalation.
        let Some(route) = self.route_of(n).await? else {
            return Ok(false);
        };
        let index = step_text.and_then(|t| t.parse::<usize>().ok()).unwrap_or(0);
        let Some(step) = route.escalation.get(index) else {
            return Ok(false);
        };
        let deliveries: Vec<NewDelivery> = step
            .targets
            .iter()
            .map(|target| NewDelivery {
                id: new_id(),
                connection_id: target.connection_id.clone(),
                destination: target.destination.clone(),
                template_id: target.template_id.clone(),
                step: i64::try_from(index).ok(),
            })
            .collect();
        let next: Vec<NewTimer> = route
            .escalation
            .get(index + 1)
            .map(|following| NewTimer {
                kind: "escalate".into(),
                due_at: now + following.after_s * 1000,
                data: Some((index + 1).to_string()),
            })
            .into_iter()
            .collect();
        self.config
            .store
            .add_deliveries(&n.id, &deliveries, &next, now)
            .await?;
        Ok(!deliveries.is_empty())
    }

    /// Acts on one timer. Returns whether a delivery was queued.
    async fn fire(&self, timer: &Timer) -> Result<bool> {
        let store = &self.config.store;
        // Retention may have removed the notification; the timer then has nothing to do.
        let Some(n) = store.notification_core(&timer.notification_id).await? else {
            return Ok(false);
        };
        let now = self.now();
        match (timer.kind.as_str(), n.state) {
            ("flush", NotificationState::Grouping) => {
                self.let_go(&n, ReleaseFrom::Group, now).await
            }
            ("release", NotificationState::Held) => self.let_go(&n, ReleaseFrom::Hold, now).await,
            ("escalate", NotificationState::Sent) => {
                self.escalate(&n, timer.data.as_deref(), now).await
            }
            ("repeat", NotificationState::Sent) if n.sent_count < MAX_SENDS => {
                let shaping = self
                    .route_of(&n)
                    .await?
                    .map(|r| r.shaping)
                    .unwrap_or_default();
                let again = shaping
                    .group
                    .and_then(|g| g.repeat_s)
                    .map(|repeat_s| NewTimer {
                        kind: "repeat".into(),
                        due_at: now + repeat_s * 1000,
                        data: None,
                    });
                store
                    .send_notification(&n.id, &new_id(), again.as_slice(), now)
                    .await?;
                Ok(true)
            }
            // Acknowledged, resolved or otherwise moved on: nothing to do.
            _ => Ok(false),
        }
    }

    /// Sends a notification whose wait is over, or holds it when quiet hours or the rate limit
    /// say so. Returns whether a delivery was queued.
    async fn let_go(&self, n: &NotificationCore, from: ReleaseFrom, now: i64) -> Result<bool> {
        let store = &self.config.store;
        let route = self.route_of(n).await?;
        let shaping = route
            .as_ref()
            .map(|r| r.shaping.clone())
            .unwrap_or_default();
        let (sends, oldest) = match &shaping.rate_limit {
            Some(rate) => {
                store
                    .sends_since(&n.workspace_id, &n.target_key, now - rate.window_s * 1000)
                    .await?
            }
            None => (0, None),
        };
        let released = release(&ReleaseInput {
            shaping: &shaping,
            max_severity: n.severity,
            from,
            sends_in_window: sends,
            oldest_send_in_window: oldest,
            now_ms: now,
        });
        match released {
            Released::Send => {
                let mut timers: Vec<NewTimer> = shaping
                    .group
                    .as_ref()
                    .and_then(|g| g.repeat_s)
                    .map(|repeat_s| NewTimer {
                        kind: "repeat".into(),
                        due_at: now + repeat_s * 1000,
                        data: None,
                    })
                    .into_iter()
                    .collect();
                // The chain starts when the notification first goes out.
                if n.sent_count == 0 {
                    timers.extend(route.as_ref().and_then(|r| first_escalation(r, now)));
                }
                store
                    .send_notification(&n.id, &new_id(), &timers, now)
                    .await?;
                Ok(true)
            }
            Released::Hold {
                kind,
                until,
                reason,
            } => {
                self.hold(n, kind, until, &reason, now).await?;
                Ok(false)
            }
        }
    }

    /// Holds a notification: merged into what is already held for the same target and reason,
    /// so that one digest goes out when the hold ends.
    async fn hold(
        &self,
        n: &NotificationCore,
        kind: HoldKind,
        until: i64,
        reason: &str,
        now: i64,
    ) -> Result<()> {
        let store = &self.config.store;
        let open = store
            .open_notifications(&n.workspace_id, &n.target_key, None)
            .await?;
        let existing = open
            .iter()
            .rev()
            .find(|o| o.id != n.id && o.state == NotificationState::Held && o.hold == Some(kind));
        match existing {
            Some(held) => store.merge_notifications(&n.id, &held.id, now).await?,
            None => {
                store
                    .hold_notification(&n.id, kind, until, reason, now)
                    .await?
            }
        }
        Ok(())
    }
}
