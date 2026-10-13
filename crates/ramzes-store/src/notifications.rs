use std::collections::HashMap;

use ramzes_core::shaping::NotificationState;
use ramzes_core::{Event, Severity};
use serde::Serialize;
use sqlx::{AssertSqlSafe, Row};

use crate::deliveries::{Attempt, DELIVERY_COLS, Delivery, delivery_from};
use crate::events::{EVENT_COLS, RawEvent, raw_event};
use crate::shaping::{severity_of, state_of};
use crate::sql::{Sql, bind_args};
use crate::{Result, Store, with_db};

/// Everything that happened to one event.
#[derive(Debug, Clone, PartialEq, Serialize, utoipa::ToSchema)]
pub struct Journey {
    pub event: Event,
    pub routed: bool,
    pub route_count: i64,
    pub notifications: Vec<JourneyNotification>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct JourneyNotification {
    pub id: String,
    pub route_id: String,
    pub route_name: String,
    pub connection_id: String,
    pub destination: Option<String>,
    pub state: String,
    pub reason: Option<String>,
    pub replay: bool,
    /// How this event relates to the notification: `first`, `duplicate`, `grouped` or `resolved`.
    pub role: String,
    /// Events the notification covers.
    pub event_count: i64,
    /// When a held notification is released.
    pub held_until: Option<i64>,
    pub created_at: i64,
    pub deliveries: Vec<JourneyDelivery>,
}

/// A notification in a list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct NotificationRow {
    pub id: String,
    pub route_id: String,
    pub route_name: String,
    pub connection_id: String,
    /// Absent when the connection was deleted.
    pub connection_name: Option<String>,
    pub destination: Option<String>,
    pub state: NotificationState,
    pub reason: Option<String>,
    /// The title of the event that opened it.
    pub title: String,
    /// The highest severity among its events.
    pub severity: Severity,
    pub event_count: i64,
    /// Messages sent for it so far.
    pub sent_count: i64,
    pub held_until: Option<i64>,
    /// When it is next due to be sent, released or repeated.
    pub next_timer_at: Option<i64>,
    pub acked_by: Option<String>,
    pub acked_at: Option<i64>,
    pub resolved_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Default)]
pub struct NotificationFilter {
    /// Only these states. Empty means all.
    pub states: Vec<NotificationState>,
    /// Return only notifications older than this id.
    pub before_id: Option<String>,
    /// Page size; 100 when zero.
    pub limit: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, utoipa::ToSchema)]
pub struct NotificationEvent {
    #[serde(flatten)]
    pub event: Event,
    pub role: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, utoipa::ToSchema)]
pub struct NotificationDetail {
    #[serde(flatten)]
    pub row: NotificationRow,
    pub events: Vec<NotificationEvent>,
    pub deliveries: Vec<JourneyDelivery>,
}

const ROW_COLS: &str = "n.id, n.route_id, n.route_name, n.connection_id, c.name AS connection_name, n.destination, \
     n.state, n.reason, n.title, n.severity, n.event_count, n.sent_count, n.held_until, n.acked_by, n.acked_at, \
     n.resolved_at, n.created_at, n.updated_at, \
     (SELECT MIN(t.due_at) FROM timers t WHERE t.notification_id = n.id) AS next_timer_at";

macro_rules! notification_row {
    ($row:expr) => {{
        let id: String = $row.try_get("id")?;
        let state: String = $row.try_get("state")?;
        NotificationRow {
            state: state_of(&id, &state)?,
            severity: severity_of(&id, $row.try_get("severity")?)?,
            id,
            route_id: $row.try_get("route_id")?,
            route_name: $row.try_get("route_name")?,
            connection_id: $row.try_get("connection_id")?,
            connection_name: $row.try_get("connection_name")?,
            destination: $row.try_get("destination")?,
            reason: $row.try_get("reason")?,
            title: $row.try_get("title")?,
            event_count: $row.try_get("event_count")?,
            sent_count: $row.try_get("sent_count")?,
            held_until: $row.try_get("held_until")?,
            next_timer_at: $row.try_get("next_timer_at")?,
            acked_by: $row.try_get("acked_by")?,
            acked_at: $row.try_get("acked_at")?,
            resolved_at: $row.try_get("resolved_at")?,
            created_at: $row.try_get("created_at")?,
            updated_at: $row.try_get("updated_at")?,
        }
    }};
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct JourneyDelivery {
    #[serde(flatten)]
    pub delivery: Delivery,
    pub attempts: Vec<Attempt>,
}

impl Store {
    /// The events a notification covers, oldest first.
    pub async fn notification_events(&self, notification_id: &str) -> Result<Vec<Event>> {
        let cols = EVENT_COLS
            .split(", ")
            .map(|c| format!("e.{}", c.trim()))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = self
            .sql(&format!(
                "SELECT {cols} FROM events e JOIN notification_events ne ON ne.event_id = e.id \
                 WHERE ne.notification_id = $1 ORDER BY e.id"
            ))
            .into_owned();
        let raw: Vec<RawEvent> = with_db!(self, |pool| {
            let rows = sqlx::query(AssertSqlSafe(sql.as_str()))
                .bind(notification_id)
                .fetch_all(pool)
                .await?;
            rows.iter()
                .map(|row| Ok::<_, sqlx::Error>(raw_event!(row)))
                .collect::<sqlx::Result<Vec<_>>>()
        })?;
        self.with_labels(raw).await
    }

    pub async fn journey(&self, workspace_id: &str, event_id: &str) -> Result<Option<Journey>> {
        let Some(event) = self.get_event(workspace_id, event_id).await? else {
            return Ok(None);
        };
        let routing = self.sql("SELECT routed_at, route_count FROM events WHERE id = $1");
        let (routed_at, route_count): (Option<i64>, i64) =
            with_db!(self, |pool| sqlx::query_as(AssertSqlSafe(&*routing))
                .bind(event_id)
                .fetch_one(pool)
                .await)?;

        let notifications_sql = self.sql(
            "SELECT n.id, n.route_id, n.route_name, n.connection_id, n.destination, n.state, n.reason, n.replay, \
             n.created_at, ne.role, n.event_count, n.held_until FROM notifications n JOIN notification_events ne ON ne.notification_id = n.id \
             WHERE ne.event_id = $1 AND n.workspace_id = $2 ORDER BY n.id",
        );
        let mut notifications: Vec<JourneyNotification> = with_db!(self, |pool| {
            let rows = sqlx::query(AssertSqlSafe(&*notifications_sql))
                .bind(event_id)
                .bind(workspace_id)
                .fetch_all(pool)
                .await?;
            rows.iter()
                .map(|row| {
                    Ok::<_, sqlx::Error>(JourneyNotification {
                        id: row.try_get("id")?,
                        route_id: row.try_get("route_id")?,
                        route_name: row.try_get("route_name")?,
                        connection_id: row.try_get("connection_id")?,
                        destination: row.try_get("destination")?,
                        state: row.try_get("state")?,
                        reason: row.try_get("reason")?,
                        replay: row.try_get::<i64, _>("replay")? != 0,
                        role: row.try_get("role")?,
                        event_count: row.try_get("event_count")?,
                        held_until: row.try_get("held_until")?,
                        created_at: row.try_get("created_at")?,
                        deliveries: Vec::new(),
                    })
                })
                .collect::<sqlx::Result<Vec<_>>>()
        })?;

        let notification_ids: Vec<String> = notifications.iter().map(|n| n.id.clone()).collect();
        let deliveries = self.deliveries_of(&notification_ids).await?;
        let delivery_ids: Vec<String> = deliveries.iter().map(|d| d.id.clone()).collect();
        let mut attempts = self.attempts_of(&delivery_ids).await?;

        let mut by_notification: HashMap<String, Vec<JourneyDelivery>> = HashMap::new();
        for delivery in deliveries {
            let attempts = attempts.remove(&delivery.id).unwrap_or_default();
            by_notification
                .entry(delivery.notification_id.clone())
                .or_default()
                .push(JourneyDelivery { delivery, attempts });
        }
        for n in &mut notifications {
            n.deliveries = by_notification.remove(&n.id).unwrap_or_default();
        }
        Ok(Some(Journey {
            event,
            routed: routed_at.is_some(),
            route_count,
            notifications,
        }))
    }

    /// Notifications, newest first.
    pub async fn list_notifications(
        &self,
        workspace_id: &str,
        f: &NotificationFilter,
    ) -> Result<Vec<NotificationRow>> {
        let mut sql = Sql::new(&format!(
            "SELECT {ROW_COLS} FROM notifications n LEFT JOIN connections c ON c.id = n.connection_id \
             WHERE n.workspace_id = "
        ));
        sql.text(workspace_id);
        if !f.states.is_empty() {
            let states: Vec<String> = f.states.iter().map(|s| s.as_str().to_owned()).collect();
            sql.push(" AND n.state IN ").list(&states);
        }
        if let Some(before) = &f.before_id {
            sql.push(" AND n.id < ").text(before.as_str());
        }
        let limit = if f.limit <= 0 { 100 } else { f.limit.min(500) };
        sql.push(" ORDER BY n.id DESC LIMIT ").int(limit);
        let text = self.sql(&sql.text).into_owned();
        with_db!(self, |pool| {
            let rows = bind_args!(sqlx::query(AssertSqlSafe(text.as_str())), sql.args)
                .fetch_all(pool)
                .await?;
            rows.iter().map(|row| Ok(notification_row!(row))).collect()
        })
    }

    /// One notification with its events and deliveries.
    pub async fn get_notification(
        &self,
        workspace_id: &str,
        id: &str,
    ) -> Result<Option<NotificationDetail>> {
        let sql = self
            .sql(&format!(
                "SELECT {ROW_COLS} FROM notifications n LEFT JOIN connections c ON c.id = n.connection_id \
                 WHERE n.workspace_id = $1 AND n.id = $2"
            ))
            .into_owned();
        let row: Option<NotificationRow> = with_db!(self, |pool| {
            let row = sqlx::query(AssertSqlSafe(sql.as_str()))
                .bind(workspace_id)
                .bind(id)
                .fetch_optional(pool)
                .await?;
            row.map(|row| Ok::<_, crate::StoreError>(notification_row!(row)))
                .transpose()
        })?;
        let Some(row) = row else {
            return Ok(None);
        };
        let roles_sql =
            self.sql("SELECT event_id, role FROM notification_events WHERE notification_id = $1");
        let roles: Vec<(String, String)> = with_db!(self, |pool| {
            sqlx::query_as(AssertSqlSafe(&*roles_sql))
                .bind(id)
                .fetch_all(pool)
                .await
        })?;
        let roles: HashMap<String, String> = roles.into_iter().collect();
        let events = self
            .notification_events(id)
            .await?
            .into_iter()
            .map(|event| NotificationEvent {
                role: roles.get(&event.id).cloned().unwrap_or_default(),
                event,
            })
            .collect();
        let deliveries = self.deliveries_of(&[id.to_owned()]).await?;
        let delivery_ids: Vec<String> = deliveries.iter().map(|d| d.id.clone()).collect();
        let mut attempts = self.attempts_of(&delivery_ids).await?;
        Ok(Some(NotificationDetail {
            row,
            events,
            deliveries: deliveries
                .into_iter()
                .map(|delivery| JourneyDelivery {
                    attempts: attempts.remove(&delivery.id).unwrap_or_default(),
                    delivery,
                })
                .collect(),
        }))
    }

    async fn deliveries_of(&self, notification_ids: &[String]) -> Result<Vec<Delivery>> {
        if notification_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut sql = Sql::new(&format!(
            "SELECT {DELIVERY_COLS} FROM deliveries WHERE notification_id IN "
        ));
        sql.list(notification_ids).push(" ORDER BY id");
        let text = self.sql(&sql.text).into_owned();
        with_db!(self, |pool| {
            let rows = bind_args!(sqlx::query(AssertSqlSafe(text.as_str())), sql.args)
                .fetch_all(pool)
                .await?;
            rows.iter().map(|row| Ok(delivery_from!(row))).collect()
        })
    }

    async fn attempts_of(&self, delivery_ids: &[String]) -> Result<HashMap<String, Vec<Attempt>>> {
        let mut by_delivery: HashMap<String, Vec<Attempt>> = HashMap::new();
        if delivery_ids.is_empty() {
            return Ok(by_delivery);
        }
        let mut sql = Sql::new(
            "SELECT id, delivery_id, started_at, duration_ms, outcome, status, detail FROM delivery_attempts \
             WHERE delivery_id IN ",
        );
        sql.list(delivery_ids).push(" ORDER BY id");
        let text = self.sql(&sql.text).into_owned();
        let attempts: Vec<Attempt> = with_db!(self, |pool| {
            let rows = bind_args!(sqlx::query(AssertSqlSafe(text.as_str())), sql.args)
                .fetch_all(pool)
                .await?;
            rows.iter()
                .map(|row| {
                    Ok::<_, sqlx::Error>(Attempt {
                        id: row.try_get("id")?,
                        delivery_id: row.try_get("delivery_id")?,
                        started_at: row.try_get("started_at")?,
                        duration_ms: row.try_get("duration_ms")?,
                        outcome: row.try_get("outcome")?,
                        status: row.try_get("status")?,
                        detail: row.try_get("detail")?,
                    })
                })
                .collect::<sqlx::Result<Vec<_>>>()
        })?;
        for attempt in attempts {
            by_delivery
                .entry(attempt.delivery_id.clone())
                .or_default()
                .push(attempt);
        }
        Ok(by_delivery)
    }
}
