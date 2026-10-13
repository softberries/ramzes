//! Acts on notifications when their time comes: sends a group, releases a hold, repeats a reminder.

use ramzes_core::Shaping;
use ramzes_core::new_id;
use ramzes_core::shaping::{
    HoldKind, NotificationState, ReleaseFrom, ReleaseInput, Released, release,
};
use ramzes_store::{NewTimer, NotificationCore, Timer};

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

    /// The shaping policy of the route a notification came from. A deleted route has none.
    async fn shaping_of(&self, n: &NotificationCore) -> Result<Shaping> {
        let routes = self.config.store.list_routes(&n.workspace_id).await?;
        Ok(routes
            .into_iter()
            .find(|r| r.id == n.route_id)
            .map(|r| r.shaping)
            .unwrap_or_default())
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
            ("repeat", NotificationState::Sent) if n.sent_count < MAX_SENDS => {
                let shaping = self.shaping_of(&n).await?;
                let again = shaping
                    .group
                    .and_then(|g| g.repeat_s)
                    .map(|repeat_s| NewTimer {
                        kind: "repeat".into(),
                        due_at: now + repeat_s * 1000,
                        data: None,
                    });
                store
                    .send_notification(&n.id, &new_id(), again.as_ref(), now)
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
        let shaping = self.shaping_of(n).await?;
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
                let repeat = shaping
                    .group
                    .as_ref()
                    .and_then(|g| g.repeat_s)
                    .map(|repeat_s| NewTimer {
                        kind: "repeat".into(),
                        due_at: now + repeat_s * 1000,
                        data: None,
                    });
                store
                    .send_notification(&n.id, &new_id(), repeat.as_ref(), now)
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
