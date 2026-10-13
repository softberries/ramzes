use std::collections::HashMap;

use ramzes_core::Event;
use serde::Serialize;
use sqlx::{AssertSqlSafe, Row};

use crate::deliveries::{Attempt, DELIVERY_COLS, Delivery, delivery_from};
use crate::events::{EVENT_COLS, RawEvent, raw_event};
use crate::sql::{Sql, bind_args};
use crate::{Result, Store, with_db};

/// Everything that happened to one event.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Journey {
    pub event: Event,
    pub routed: bool,
    pub route_count: i64,
    pub notifications: Vec<JourneyNotification>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JourneyNotification {
    pub id: String,
    pub route_id: String,
    pub route_name: String,
    pub connection_id: String,
    pub destination: Option<String>,
    pub state: String,
    pub reason: Option<String>,
    pub replay: bool,
    pub created_at: i64,
    pub deliveries: Vec<JourneyDelivery>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
             n.created_at FROM notifications n JOIN notification_events ne ON ne.notification_id = n.id \
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
