use ramzes_core::Event;
use ramzes_core::shaping::{HoldKind, NotificationState, Role};
use serde::Serialize;
use sqlx::{AssertSqlSafe, Row};

use crate::shaping::{INSERT_DELIVERY_FOR, INSERT_TIMER, NewTimer};
use crate::sql::BeginWrite;
use crate::{Result, Store, StoreError, with_db};

/// A notification to create for one matched target of a route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewDispatch {
    pub notification_id: String,
    /// The first delivery, for a notification that is sent at once.
    pub delivery_id: Option<String>,
    pub route_id: String,
    pub route_name: String,
    pub connection_id: String,
    pub destination: Option<String>,
    /// The template that words the message, when the target names one.
    pub template_id: Option<String>,
    /// Created by replaying an event rather than by its first routing.
    pub replay: bool,
    /// Identifies the route target: later events look up open notifications by it.
    pub target_key: String,
    pub state: NotificationState,
    pub reason: Option<String>,
    pub dedup_key: Option<String>,
    pub group_key: Option<String>,
    /// Why and until when it is held.
    pub hold: Option<(HoldKind, i64)>,
    pub silence_id: Option<String>,
    pub timer: Option<NewTimer>,
}

impl Default for NewDispatch {
    fn default() -> Self {
        NewDispatch {
            notification_id: String::new(),
            delivery_id: None,
            route_id: String::new(),
            route_name: String::new(),
            connection_id: String::new(),
            destination: None,
            template_id: None,
            replay: false,
            target_key: String::new(),
            state: NotificationState::Sent,
            reason: None,
            dedup_key: None,
            group_key: None,
            hold: None,
            silence_id: None,
            timer: None,
        }
    }
}

/// An event joining a notification that already exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Join {
    pub notification_id: String,
    pub role: Role,
    /// The event resolves the notification. The list holds the deliveries that tell those who
    /// had been told about it; empty when nobody had.
    pub resolve: Option<Vec<NewDelivery>>,
}

/// A delivery to add to a notification that already exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewDelivery {
    pub id: String,
    pub connection_id: String,
    pub destination: Option<String>,
    pub template_id: Option<String>,
    /// The escalation step it belongs to, counted from 0. `None` for the route's own target.
    pub step: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryState {
    Pending,
    InProgress,
    Delivered,
    /// Rejected by the service; not retried.
    Failed,
    /// Retries were exhausted.
    Dead,
}

impl DeliveryState {
    pub fn as_str(self) -> &'static str {
        match self {
            DeliveryState::Pending => "pending",
            DeliveryState::InProgress => "in_progress",
            DeliveryState::Delivered => "delivered",
            DeliveryState::Failed => "failed",
            DeliveryState::Dead => "dead",
        }
    }

    pub fn parse(s: &str) -> Option<DeliveryState> {
        match s {
            "pending" => Some(DeliveryState::Pending),
            "in_progress" => Some(DeliveryState::InProgress),
            "delivered" => Some(DeliveryState::Delivered),
            "failed" => Some(DeliveryState::Failed),
            "dead" => Some(DeliveryState::Dead),
            _ => None,
        }
    }
}

/// One send of a notification to one target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct Delivery {
    pub id: String,
    pub workspace_id: String,
    pub notification_id: String,
    pub connection_id: String,
    pub destination: Option<String>,
    pub state: DeliveryState,
    /// Attempts made so far.
    pub attempts: i64,
    pub next_attempt_at: i64,
    pub last_error: Option<String>,
    /// The service's id for the delivered message, when it gives one.
    pub external_ref: Option<String>,
    /// Something went wrong that did not stop the send, for example a template that failed.
    pub warning: Option<String>,
    /// The escalation step that sent it, counted from 0. Absent for the route's own targets.
    pub step: Option<i64>,
    /// The template that words it, when it differs from the notification's.
    pub template_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct Attempt {
    pub id: String,
    pub delivery_id: String,
    pub started_at: i64,
    pub duration_ms: i64,
    /// `delivered`, `retry` or `failed`.
    pub outcome: String,
    /// HTTP status, when the service answered.
    pub status: Option<i64>,
    pub detail: Option<String>,
}

/// How an attempt ended and what happens to the delivery next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finish {
    Delivered { external_ref: Option<String> },
    Retry { at: i64, error: String },
    Failed { error: String },
    Dead { error: String },
}

/// A failed or dead delivery with enough context to show in a list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct FailureRow {
    #[serde(flatten)]
    pub delivery: Delivery,
    pub event_id: String,
    pub event_title: String,
    pub route_name: String,
    /// `None` when the connection has been deleted.
    pub connection_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueDepth {
    pub unrouted_events: i64,
    pub pending_deliveries: i64,
}

pub(crate) const DELIVERY_COLS: &str = "id, workspace_id, notification_id, connection_id, destination, state, attempts, \
                                        next_attempt_at, last_error, external_ref, warning, step, template_id, created_at, updated_at";

macro_rules! delivery_from {
    ($row:expr) => {{
        let id: String = $row.try_get("id")?;
        let state: String = $row.try_get("state")?;
        $crate::deliveries::Delivery {
            state: $crate::deliveries::DeliveryState::parse(&state).ok_or_else(|| {
                $crate::StoreError::Data(format!("delivery {id} has state {state:?}"))
            })?,
            id,
            workspace_id: $row.try_get("workspace_id")?,
            notification_id: $row.try_get("notification_id")?,
            connection_id: $row.try_get("connection_id")?,
            destination: $row.try_get("destination")?,
            attempts: $row.try_get("attempts")?,
            next_attempt_at: $row.try_get("next_attempt_at")?,
            last_error: $row.try_get("last_error")?,
            external_ref: $row.try_get("external_ref")?,
            warning: $row.try_get("warning")?,
            step: $row.try_get("step")?,
            template_id: $row.try_get("template_id")?,
            created_at: $row.try_get("created_at")?,
            updated_at: $row.try_get("updated_at")?,
        }
    }};
}
pub(crate) use delivery_from;

impl Store {
    /// Writes the result of routing one event in a single transaction: the notifications it
    /// opens (with a delivery, a timer, or neither) and the ones it joins or resolves. With
    /// `mark_routed` the event is also marked as routed.
    pub async fn write_routing(
        &self,
        event: &Event,
        dispatches: &[NewDispatch],
        joins: &[Join],
        mark_routed: bool,
        now_ms: i64,
    ) -> Result<()> {
        let q = |text: &str| self.sql(text).into_owned();
        let notification = q(
            "INSERT INTO notifications (id, workspace_id, route_id, route_name, connection_id, destination, dedup_key, \
             state, event_count, replay, created_at, updated_at, template_id, target_key, reason, group_key, \
             held_until, hold_kind, silence_id, title, severity, sent_at, sent_count) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 1, $9, $10, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21)",
        );
        let link = q(
            "INSERT INTO notification_events (notification_id, event_id, workspace_id, role) VALUES ($1, $2, $3, $4) \
             ON CONFLICT (notification_id, event_id) DO NOTHING",
        );
        let delivery = q(
            "INSERT INTO deliveries (id, workspace_id, notification_id, connection_id, destination, state, attempts, \
             next_attempt_at, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, 'pending', 0, $6, $6, $6)",
        );
        let timer = q(INSERT_TIMER);
        let joined = q(
            "UPDATE notifications SET event_count = event_count + 1, updated_at = $1, \
             severity = CASE WHEN severity < $2 THEN $2 ELSE severity END WHERE id = $3",
        );
        let resolved = q(
            "UPDATE notifications SET state = 'resolved', resolved_at = $1, updated_at = $1, \
             held_until = NULL, hold_kind = NULL WHERE id = $2",
        );
        let cancel = q("DELETE FROM timers WHERE notification_id = $1");
        let notice = q(INSERT_DELIVERY_FOR);
        let routed = q(
            "UPDATE events SET routed_at = $1, route_count = $2, claimed_until = NULL WHERE id = $3",
        );
        let count = i64::try_from(dispatches.len() + joins.len())
            .map_err(|e| StoreError::Data(e.to_string()))?;
        with_db!(self, |pool| {
            let mut tx = pool.begin_write().await?;
            for d in dispatches {
                let sent = d.delivery_id.is_some();
                sqlx::query(AssertSqlSafe(notification.as_str()))
                    .bind(&d.notification_id)
                    .bind(&event.workspace_id)
                    .bind(&d.route_id)
                    .bind(&d.route_name)
                    .bind(&d.connection_id)
                    .bind(&d.destination)
                    .bind(&d.dedup_key)
                    .bind(d.state.as_str())
                    .bind(i64::from(d.replay))
                    .bind(now_ms)
                    .bind(&d.template_id)
                    .bind(&d.target_key)
                    .bind(&d.reason)
                    .bind(&d.group_key)
                    .bind(d.hold.map(|(_, until)| until))
                    .bind(d.hold.map(|(kind, _)| kind.as_str()))
                    .bind(&d.silence_id)
                    .bind(&event.title)
                    .bind(event.severity.as_i64())
                    .bind(sent.then_some(now_ms))
                    .bind(i64::from(sent))
                    .execute(&mut *tx)
                    .await?;
                sqlx::query(AssertSqlSafe(link.as_str()))
                    .bind(&d.notification_id)
                    .bind(&event.id)
                    .bind(&event.workspace_id)
                    .bind(Role::First.as_str())
                    .execute(&mut *tx)
                    .await?;
                if let Some(delivery_id) = &d.delivery_id {
                    sqlx::query(AssertSqlSafe(delivery.as_str()))
                        .bind(delivery_id)
                        .bind(&event.workspace_id)
                        .bind(&d.notification_id)
                        .bind(&d.connection_id)
                        .bind(&d.destination)
                        .bind(now_ms)
                        .execute(&mut *tx)
                        .await?;
                }
                if let Some(t) = &d.timer {
                    sqlx::query(AssertSqlSafe(timer.as_str()))
                        .bind(ramzes_core::new_id())
                        .bind(&event.workspace_id)
                        .bind(&t.kind)
                        .bind(t.due_at)
                        .bind(&d.notification_id)
                        .bind(&t.data)
                        .execute(&mut *tx)
                        .await?;
                }
            }
            for j in joins {
                let added = sqlx::query(AssertSqlSafe(link.as_str()))
                    .bind(&j.notification_id)
                    .bind(&event.id)
                    .bind(&event.workspace_id)
                    .bind(j.role.as_str())
                    .execute(&mut *tx)
                    .await?
                    .rows_affected();
                // A routing pass that is repeated after a crash finds the event already attached.
                if added == 0 {
                    continue;
                }
                sqlx::query(AssertSqlSafe(joined.as_str()))
                    .bind(now_ms)
                    .bind(event.severity.as_i64())
                    .bind(&j.notification_id)
                    .execute(&mut *tx)
                    .await?;
                if let Some(notify) = &j.resolve {
                    sqlx::query(AssertSqlSafe(resolved.as_str()))
                        .bind(now_ms)
                        .bind(&j.notification_id)
                        .execute(&mut *tx)
                        .await?;
                    sqlx::query(AssertSqlSafe(cancel.as_str()))
                        .bind(&j.notification_id)
                        .execute(&mut *tx)
                        .await?;
                    for d in notify {
                        sqlx::query(AssertSqlSafe(notice.as_str()))
                            .bind(&d.id)
                            .bind(&event.workspace_id)
                            .bind(&j.notification_id)
                            .bind(&d.connection_id)
                            .bind(&d.destination)
                            .bind(now_ms)
                            .bind(&d.template_id)
                            .bind(d.step)
                            .execute(&mut *tx)
                            .await?;
                    }
                }
            }
            if mark_routed {
                sqlx::query(AssertSqlSafe(routed.as_str()))
                    .bind(now_ms)
                    .bind(count)
                    .bind(&event.id)
                    .execute(&mut *tx)
                    .await?;
            }
            tx.commit().await?;
            Ok(())
        })
    }

    /// Takes a lease on up to `limit` deliveries that are due, oldest due first.
    ///
    /// A delivery that is not finished before its lease expires is handed out again.
    pub async fn claim_due_deliveries(
        &self,
        now_ms: i64,
        lease_ms: i64,
        limit: i64,
    ) -> Result<Vec<Delivery>> {
        const DUE: &str = "state IN ('pending', 'in_progress') AND next_attempt_at <= $2 \
                           AND (claimed_until IS NULL OR claimed_until < $2)";
        let sql = self
            .sql(&format!(
                "UPDATE deliveries SET state = 'in_progress', claimed_until = $1 WHERE id IN \
                 (SELECT id FROM deliveries WHERE {DUE} ORDER BY next_attempt_at, id LIMIT $3) AND {DUE} \
                 RETURNING {DELIVERY_COLS}"
            ))
            .into_owned();
        let mut claimed: Vec<Delivery> = with_db!(self, |pool| {
            let rows = sqlx::query(AssertSqlSafe(sql.as_str()))
                .bind(now_ms + lease_ms)
                .bind(now_ms)
                .bind(limit)
                .fetch_all(pool)
                .await?;
            rows.iter()
                .map(|row| Ok(delivery_from!(row)))
                .collect::<Result<Vec<_>>>()
        })?;
        claimed.sort_by(|a, b| (a.next_attempt_at, &a.id).cmp(&(b.next_attempt_at, &b.id)));
        Ok(claimed)
    }

    /// Records an attempt and moves the delivery to its next state, in one transaction.
    pub async fn finish_attempt(
        &self,
        d: &Delivery,
        a: &Attempt,
        f: &Finish,
        now_ms: i64,
    ) -> Result<()> {
        let insert = self.sql(
            "INSERT INTO delivery_attempts (id, workspace_id, delivery_id, started_at, duration_ms, outcome, status, detail) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        );
        let update = self.sql(
            "UPDATE deliveries SET state = $1, attempts = attempts + 1, next_attempt_at = $2, last_error = $3, \
             external_ref = $4, claimed_until = NULL, updated_at = $5 WHERE id = $6",
        );
        let (state, next_at, error, external_ref) = match f {
            Finish::Delivered { external_ref } => (
                DeliveryState::Delivered,
                d.next_attempt_at,
                None,
                external_ref.as_deref(),
            ),
            Finish::Retry { at, error } => {
                (DeliveryState::Pending, *at, Some(error.as_str()), None)
            }
            Finish::Failed { error } => (
                DeliveryState::Failed,
                d.next_attempt_at,
                Some(error.as_str()),
                None,
            ),
            Finish::Dead { error } => (
                DeliveryState::Dead,
                d.next_attempt_at,
                Some(error.as_str()),
                None,
            ),
        };
        with_db!(self, |pool| {
            let mut tx = pool.begin_write().await?;
            sqlx::query(AssertSqlSafe(&*insert))
                .bind(&a.id)
                .bind(&d.workspace_id)
                .bind(&d.id)
                .bind(a.started_at)
                .bind(a.duration_ms)
                .bind(&a.outcome)
                .bind(a.status)
                .bind(&a.detail)
                .execute(&mut *tx)
                .await?;
            sqlx::query(AssertSqlSafe(&*update))
                .bind(state.as_str())
                .bind(next_at)
                .bind(error)
                .bind(external_ref)
                .bind(now_ms)
                .bind(&d.id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            Ok(())
        })
    }

    /// Failed and dead deliveries, newest first.
    pub async fn list_failures(&self, workspace_id: &str, limit: i64) -> Result<Vec<FailureRow>> {
        let cols = DELIVERY_COLS
            .split(", ")
            .map(|c| format!("d.{}", c.trim()))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = self
            .sql(&format!(
                "SELECT {cols}, e.id AS event_id, e.title AS event_title, n.route_name AS route_name, \
                 c.name AS connection_name \
                 FROM deliveries d \
                 JOIN notifications n ON n.id = d.notification_id \
                 JOIN events e ON e.id = \
                   (SELECT MIN(ne.event_id) FROM notification_events ne WHERE ne.notification_id = n.id) \
                 LEFT JOIN connections c ON c.id = d.connection_id \
                 WHERE d.workspace_id = $1 AND d.state IN ('failed', 'dead') ORDER BY d.id DESC LIMIT $2"
            ))
            .into_owned();
        with_db!(self, |pool| {
            let rows = sqlx::query(AssertSqlSafe(sql.as_str()))
                .bind(workspace_id)
                .bind(limit)
                .fetch_all(pool)
                .await?;
            rows.iter()
                .map(|row| {
                    Ok(FailureRow {
                        delivery: delivery_from!(row),
                        event_id: row.try_get("event_id")?,
                        event_title: row.try_get("event_title")?,
                        route_name: row.try_get("route_name")?,
                        connection_name: row.try_get("connection_name")?,
                    })
                })
                .collect()
        })
    }

    /// Queues a fresh delivery for the same notification and target as an existing one.
    pub async fn requeue_delivery(
        &self,
        workspace_id: &str,
        delivery_id: &str,
        new_id: &str,
        now_ms: i64,
    ) -> Result<Delivery> {
        let insert = self.sql(
            "INSERT INTO deliveries (id, workspace_id, notification_id, connection_id, destination, state, attempts, \
             next_attempt_at, created_at, updated_at) \
             SELECT $1, workspace_id, notification_id, connection_id, destination, 'pending', 0, $2, $2, $2 \
             FROM deliveries WHERE workspace_id = $3 AND id = $4",
        );
        let select = self
            .sql(&format!(
                "SELECT {DELIVERY_COLS} FROM deliveries WHERE id = $1"
            ))
            .into_owned();
        with_db!(self, |pool| {
            let inserted = sqlx::query(AssertSqlSafe(&*insert))
                .bind(new_id)
                .bind(now_ms)
                .bind(workspace_id)
                .bind(delivery_id)
                .execute(pool)
                .await?
                .rows_affected();
            if inserted == 0 {
                return Err(StoreError::NotFound);
            }
            let row = sqlx::query(AssertSqlSafe(select.as_str()))
                .bind(new_id)
                .fetch_one(pool)
                .await?;
            Ok(delivery_from!(row))
        })
    }

    pub async fn queue_depth(&self) -> Result<QueueDepth> {
        let sql = "SELECT (SELECT COUNT(*) FROM events WHERE routed_at IS NULL), \
                   (SELECT COUNT(*) FROM deliveries WHERE state IN ('pending', 'in_progress'))";
        let (unrouted_events, pending_deliveries): (i64, i64) =
            with_db!(self, |pool| sqlx::query_as(sql).fetch_one(pool).await)?;
        Ok(QueueDepth {
            unrouted_events,
            pending_deliveries,
        })
    }

    pub async fn set_delivery_warning(&self, delivery_id: &str, warning: &str) -> Result<()> {
        let sql = self.sql("UPDATE deliveries SET warning = $1 WHERE id = $2");
        with_db!(self, |pool| {
            sqlx::query(AssertSqlSafe(&*sql))
                .bind(warning)
                .bind(delivery_id)
                .execute(pool)
                .await
                .map(|_| ())
        })?;
        Ok(())
    }

    /// The template a notification's messages are worded with, if any.
    pub async fn notification_template(&self, notification_id: &str) -> Result<Option<String>> {
        let sql = self.sql("SELECT template_id FROM notifications WHERE id = $1");
        let row: Option<(Option<String>,)> = with_db!(self, |pool| {
            sqlx::query_as(AssertSqlSafe(&*sql))
                .bind(notification_id)
                .fetch_optional(pool)
                .await
        })?;
        Ok(row.and_then(|(id,)| id))
    }

    /// Puts a claimed delivery back to wait until `at_ms`, without counting an attempt.
    pub async fn defer_delivery(&self, delivery_id: &str, at_ms: i64) -> Result<()> {
        let sql = self.sql(
            "UPDATE deliveries SET state = 'pending', next_attempt_at = $1, claimed_until = NULL WHERE id = $2",
        );
        with_db!(self, |pool| {
            sqlx::query(AssertSqlSafe(&*sql))
                .bind(at_ms)
                .bind(delivery_id)
                .execute(pool)
                .await
                .map(|_| ())
        })?;
        Ok(())
    }
}
