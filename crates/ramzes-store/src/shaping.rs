//! Storage behind shaping: open notifications, timers, and the operations that move a
//! notification between states.

use ramzes_core::Severity;
use ramzes_core::shaping::{HoldKind, NotificationState, OpenNotification};
use sqlx::{AssertSqlSafe, Row};

use crate::sql::BeginWrite;
use crate::{Result, Store, StoreError, with_db};

/// A timer to create.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTimer {
    /// `flush`, `release`, `repeat` or `escalate`.
    pub kind: String,
    pub due_at: i64,
    pub data: Option<String>,
}

/// Something to do for a notification at a given time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timer {
    pub id: String,
    pub workspace_id: String,
    pub kind: String,
    pub due_at: i64,
    pub notification_id: String,
    pub data: Option<String>,
}

/// What the workers need to know about a notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationCore {
    pub id: String,
    pub workspace_id: String,
    pub route_id: String,
    pub route_name: String,
    pub connection_id: String,
    pub destination: Option<String>,
    pub target_key: String,
    pub state: NotificationState,
    pub hold_kind: Option<HoldKind>,
    pub held_until: Option<i64>,
    pub dedup_key: Option<String>,
    pub group_key: Option<String>,
    pub sent_count: i64,
    pub event_count: i64,
    pub severity: Severity,
    pub template_id: Option<String>,
    pub acked_by: Option<String>,
}

pub(crate) fn state_of(id: &str, text: &str) -> Result<NotificationState> {
    NotificationState::parse(text)
        .ok_or_else(|| StoreError::Data(format!("notification {id} has state {text:?}")))
}

pub(crate) fn severity_of(id: &str, value: i64) -> Result<Severity> {
    Severity::from_i64(value)
        .ok_or_else(|| StoreError::Data(format!("notification {id} has severity {value}")))
}

/// A state change: the columns to set, the states it may start from, and what to say otherwise.
struct Change<'a> {
    set: &'a str,
    from: &'a str,
    refusal: &'a str,
}

const INSERT_DELIVERY_OF: &str = "INSERT INTO deliveries (id, workspace_id, notification_id, connection_id, destination, state, attempts, \
     next_attempt_at, created_at, updated_at) \
     SELECT $1, workspace_id, id, connection_id, destination, 'pending', 0, $2, $2, $2 FROM notifications WHERE id = $3";
/// Adds a delivery with its own connection, destination, template and escalation step.
pub(crate) const INSERT_DELIVERY_FOR: &str = "INSERT INTO deliveries (id, workspace_id, notification_id, connection_id, destination, state, attempts, \
     next_attempt_at, created_at, updated_at, template_id, step) VALUES ($1, $2, $3, $4, $5, 'pending', 0, $6, $6, $6, $7, $8)";
pub(crate) const INSERT_TIMER: &str = "INSERT INTO timers (id, workspace_id, kind, due_at, notification_id, data) VALUES ($1, $2, $3, $4, $5, $6)";

impl Store {
    /// The open notifications of a route target that an event may join or resolve: those still
    /// collecting or held, and those already sent that carry `dedup_key`. Oldest first.
    pub async fn open_notifications(
        &self,
        workspace_id: &str,
        target_key: &str,
        dedup_key: Option<&str>,
    ) -> Result<Vec<OpenNotification>> {
        // A member "fires" a key when it carries it and no resolved member of the same
        // notification carries it too.
        const FIRING: &str = "f.notification_id = n.id AND f.role <> 'resolved' AND NOT EXISTS \
             (SELECT 1 FROM notification_events r WHERE r.notification_id = n.id AND r.role = 'resolved' \
              AND r.dedup_key = f.dedup_key)";
        let sql = self
            .sql(&format!(
                "SELECT n.id, n.state, n.dedup_key, n.group_key, n.created_at, n.sent_count, n.hold_kind, \
                 CAST(CASE WHEN EXISTS (SELECT 1 FROM notification_events f WHERE {FIRING} AND f.dedup_key = $3) \
                      THEN 1 ELSE 0 END AS BIGINT) AS has_key, \
                 CAST(CASE WHEN EXISTS (SELECT 1 FROM notification_events f WHERE {FIRING} \
                      AND (f.dedup_key IS NULL OR f.dedup_key <> $3)) THEN 1 ELSE 0 END AS BIGINT) AS others_firing \
                 FROM notifications n WHERE n.workspace_id = $1 AND n.target_key = $2 \
                 AND (n.state IN ('grouping', 'held') OR (n.state IN ('sent', 'acknowledged') AND EXISTS \
                 (SELECT 1 FROM notification_events f WHERE {FIRING} AND f.dedup_key = $3))) \
                 ORDER BY n.id DESC LIMIT 100"
            ))
            .into_owned();
        let mut found: Vec<OpenNotification> = with_db!(self, |pool| {
            let rows = sqlx::query(AssertSqlSafe(sql.as_str()))
                .bind(workspace_id)
                .bind(target_key)
                .bind(dedup_key)
                .fetch_all(pool)
                .await?;
            rows.iter()
                .map(|row| {
                    let id: String = row.try_get("id")?;
                    let state: String = row.try_get("state")?;
                    let hold: Option<String> = row.try_get("hold_kind")?;
                    Ok(OpenNotification {
                        state: state_of(&id, &state)?,
                        id,
                        dedup_key: row.try_get("dedup_key")?,
                        group_key: row.try_get("group_key")?,
                        created_at: row.try_get("created_at")?,
                        sent: row.try_get::<i64, _>("sent_count")? > 0,
                        hold: hold.as_deref().and_then(HoldKind::parse),
                        has_key: row.try_get::<i64, _>("has_key")? != 0,
                        others_firing: row.try_get::<i64, _>("others_firing")? != 0,
                    })
                })
                .collect::<Result<Vec<_>>>()
        })?;
        found.reverse();
        Ok(found)
    }

    /// The events a message about a notification covers. While the notification is open these
    /// are its firing events that no later event resolved; once it is resolved, everything
    /// that had fired.
    pub async fn sendable_events(&self, notification_id: &str) -> Result<Vec<ramzes_core::Event>> {
        let sql = self.sql(
            "SELECT ne.event_id FROM notification_events ne JOIN notifications n ON n.id = ne.notification_id \
             WHERE ne.notification_id = $1 AND ne.role <> 'resolved' AND (n.state = 'resolved' OR NOT EXISTS \
             (SELECT 1 FROM notification_events r WHERE r.notification_id = ne.notification_id \
              AND r.role = 'resolved' AND r.dedup_key = ne.dedup_key))",
        );
        let keep: Vec<(String,)> = with_db!(self, |pool| {
            sqlx::query_as(AssertSqlSafe(&*sql))
                .bind(notification_id)
                .fetch_all(pool)
                .await
        })?;
        let all = self.notification_events(notification_id).await?;
        // A notification made only of resolved events still needs something to say.
        if keep.is_empty() {
            return Ok(all);
        }
        Ok(all
            .into_iter()
            .filter(|e| keep.iter().any(|(id,)| *id == e.id))
            .collect())
    }

    /// How many notifications were sent to a target at or after `since_ms`, and the earliest.
    pub async fn sends_since(
        &self,
        workspace_id: &str,
        target_key: &str,
        since_ms: i64,
    ) -> Result<(i64, Option<i64>)> {
        let sql = self.sql(
            "SELECT COUNT(*), MIN(sent_at) FROM notifications \
             WHERE workspace_id = $1 AND target_key = $2 AND sent_at >= $3",
        );
        let found: (i64, Option<i64>) = with_db!(self, |pool| {
            sqlx::query_as(AssertSqlSafe(&*sql))
                .bind(workspace_id)
                .bind(target_key)
                .bind(since_ms)
                .fetch_one(pool)
                .await
        })?;
        Ok(found)
    }

    pub async fn notification_core(&self, id: &str) -> Result<Option<NotificationCore>> {
        let sql = self.sql(
            "SELECT id, workspace_id, route_id, route_name, connection_id, destination, target_key, state, hold_kind, \
             held_until, dedup_key, group_key, sent_count, event_count, severity, template_id, acked_by \
             FROM notifications WHERE id = $1",
        );
        with_db!(self, |pool| {
            let row = sqlx::query(AssertSqlSafe(&*sql))
                .bind(id)
                .fetch_optional(pool)
                .await?;
            row.map(|row| {
                let id: String = row.try_get("id")?;
                let state: String = row.try_get("state")?;
                let hold: Option<String> = row.try_get("hold_kind")?;
                Ok(NotificationCore {
                    state: state_of(&id, &state)?,
                    severity: severity_of(&id, row.try_get("severity")?)?,
                    id,
                    workspace_id: row.try_get("workspace_id")?,
                    route_id: row.try_get("route_id")?,
                    route_name: row.try_get("route_name")?,
                    connection_id: row.try_get("connection_id")?,
                    destination: row.try_get("destination")?,
                    target_key: row.try_get("target_key")?,
                    hold_kind: hold.as_deref().and_then(HoldKind::parse),
                    held_until: row.try_get("held_until")?,
                    dedup_key: row.try_get("dedup_key")?,
                    group_key: row.try_get("group_key")?,
                    sent_count: row.try_get("sent_count")?,
                    event_count: row.try_get("event_count")?,
                    template_id: row.try_get("template_id")?,
                    acked_by: row.try_get("acked_by")?,
                })
            })
            .transpose()
        })
    }

    /// Queues a message for a notification. One that was collecting or held becomes `sent`;
    /// any other state is kept (a reminder, or the notice that it was resolved).
    pub async fn send_notification(
        &self,
        notification_id: &str,
        delivery_id: &str,
        timers: &[NewTimer],
        now_ms: i64,
    ) -> Result<()> {
        let update = self.sql(
            "UPDATE notifications SET \
             reason = CASE WHEN state IN ('grouping', 'held') THEN NULL ELSE reason END, \
             state = CASE WHEN state IN ('grouping', 'held') THEN 'sent' ELSE state END, \
             sent_at = $1, sent_count = sent_count + 1, held_until = NULL, hold_kind = NULL, updated_at = $1 \
             WHERE id = $2",
        );
        let delivery = self.sql(INSERT_DELIVERY_OF);
        let insert_timer = self.sql(INSERT_TIMER);
        let workspace = self.sql("SELECT workspace_id FROM notifications WHERE id = $1");
        with_db!(self, |pool| {
            let mut tx = pool.begin_write().await?;
            let changed = sqlx::query(AssertSqlSafe(&*update))
                .bind(now_ms)
                .bind(notification_id)
                .execute(&mut *tx)
                .await?
                .rows_affected();
            if changed == 0 {
                return Err(StoreError::NotFound);
            }
            sqlx::query(AssertSqlSafe(&*delivery))
                .bind(delivery_id)
                .bind(now_ms)
                .bind(notification_id)
                .execute(&mut *tx)
                .await?;
            if !timers.is_empty() {
                let (workspace_id,): (String,) = sqlx::query_as(AssertSqlSafe(&*workspace))
                    .bind(notification_id)
                    .fetch_one(&mut *tx)
                    .await?;
                for timer in timers {
                    sqlx::query(AssertSqlSafe(&*insert_timer))
                        .bind(ramzes_core::new_id())
                        .bind(&workspace_id)
                        .bind(&timer.kind)
                        .bind(timer.due_at)
                        .bind(notification_id)
                        .bind(&timer.data)
                        .execute(&mut *tx)
                        .await?;
                }
            }
            tx.commit().await?;
            Ok(())
        })
    }

    /// Adds deliveries to a notification without changing its state, and sets timers: an
    /// escalation step going out to its targets.
    pub async fn add_deliveries(
        &self,
        notification_id: &str,
        deliveries: &[crate::NewDelivery],
        timers: &[NewTimer],
        now_ms: i64,
    ) -> Result<()> {
        let workspace = self.sql("SELECT workspace_id FROM notifications WHERE id = $1");
        let insert = self.sql(INSERT_DELIVERY_FOR);
        let insert_timer = self.sql(INSERT_TIMER);
        with_db!(self, |pool| {
            let mut tx = pool.begin_write().await?;
            let row: Option<(String,)> = sqlx::query_as(AssertSqlSafe(&*workspace))
                .bind(notification_id)
                .fetch_optional(&mut *tx)
                .await?;
            let Some((workspace_id,)) = row else {
                return Err(StoreError::NotFound);
            };
            for d in deliveries {
                sqlx::query(AssertSqlSafe(&*insert))
                    .bind(&d.id)
                    .bind(&workspace_id)
                    .bind(notification_id)
                    .bind(&d.connection_id)
                    .bind(&d.destination)
                    .bind(now_ms)
                    .bind(&d.template_id)
                    .bind(d.step)
                    .execute(&mut *tx)
                    .await?;
            }
            for timer in timers {
                sqlx::query(AssertSqlSafe(&*insert_timer))
                    .bind(ramzes_core::new_id())
                    .bind(&workspace_id)
                    .bind(&timer.kind)
                    .bind(timer.due_at)
                    .bind(notification_id)
                    .bind(&timer.data)
                    .execute(&mut *tx)
                    .await?;
            }
            tx.commit().await?;
            Ok(())
        })
    }

    /// The places escalation steps of a notification sent to: `(connection, destination, template)`.
    pub async fn escalated_targets(
        &self,
        notification_id: &str,
    ) -> Result<Vec<(String, Option<String>, Option<String>)>> {
        let sql = self.sql(
            "SELECT DISTINCT connection_id, destination, template_id FROM deliveries \
             WHERE notification_id = $1 AND step IS NOT NULL",
        );
        let rows: Vec<(String, Option<String>, Option<String>)> = with_db!(self, |pool| {
            sqlx::query_as(AssertSqlSafe(&*sql))
                .bind(notification_id)
                .fetch_all(pool)
                .await
        })?;
        Ok(rows)
    }

    /// The service's reference to the first message that arrived for a notification at one place.
    pub async fn first_external_ref(
        &self,
        notification_id: &str,
        connection_id: &str,
        destination: Option<&str>,
    ) -> Result<Option<String>> {
        let sql = self.sql(
            "SELECT external_ref FROM deliveries WHERE notification_id = $1 AND connection_id = $2 \
             AND COALESCE(destination, '') = COALESCE($3, '') AND external_ref IS NOT NULL ORDER BY id LIMIT 1",
        );
        let row: Option<(String,)> = with_db!(self, |pool| {
            sqlx::query_as(AssertSqlSafe(&*sql))
                .bind(notification_id)
                .bind(connection_id)
                .bind(destination)
                .fetch_optional(pool)
                .await
        })?;
        Ok(row.map(|(reference,)| reference))
    }

    /// Every message that arrived for a notification and has a reference:
    /// `(connection, destination, reference)`, oldest first.
    pub async fn message_refs(
        &self,
        notification_id: &str,
    ) -> Result<Vec<(String, Option<String>, String)>> {
        let sql = self.sql(
            "SELECT connection_id, destination, external_ref FROM deliveries \
             WHERE notification_id = $1 AND external_ref IS NOT NULL ORDER BY id",
        );
        let rows: Vec<(String, Option<String>, String)> = with_db!(self, |pool| {
            sqlx::query_as(AssertSqlSafe(&*sql))
                .bind(notification_id)
                .fetch_all(pool)
                .await
        })?;
        Ok(rows)
    }

    /// Where a notification's messages arrived: connection and destination, each pair once.
    pub async fn delivered_targets(
        &self,
        notification_id: &str,
    ) -> Result<Vec<(String, Option<String>)>> {
        let sql = self.sql(
            "SELECT DISTINCT connection_id, destination FROM deliveries \
             WHERE notification_id = $1 AND state = 'delivered' ORDER BY connection_id, destination",
        );
        let rows: Vec<(String, Option<String>)> = with_db!(self, |pool| {
            sqlx::query_as(AssertSqlSafe(&*sql))
                .bind(notification_id)
                .fetch_all(pool)
                .await
        })?;
        Ok(rows)
    }

    /// Puts a notification on hold until `until_ms`, replacing its timers with one that releases it.
    pub async fn hold_notification(
        &self,
        notification_id: &str,
        kind: HoldKind,
        until_ms: i64,
        reason: &str,
        now_ms: i64,
    ) -> Result<()> {
        let update = self.sql(
            "UPDATE notifications SET state = 'held', hold_kind = $1, held_until = $2, reason = $3, updated_at = $4 \
             WHERE id = $5 RETURNING workspace_id",
        );
        let cancel = self
            .sql("DELETE FROM timers WHERE notification_id = $1 AND kind IN ('flush', 'release')");
        let insert_timer = self.sql(INSERT_TIMER);
        with_db!(self, |pool| {
            let mut tx = pool.begin_write().await?;
            let row: Option<(String,)> = sqlx::query_as(AssertSqlSafe(&*update))
                .bind(kind.as_str())
                .bind(until_ms)
                .bind(reason)
                .bind(now_ms)
                .bind(notification_id)
                .fetch_optional(&mut *tx)
                .await?;
            let Some((workspace_id,)) = row else {
                return Err(StoreError::NotFound);
            };
            sqlx::query(AssertSqlSafe(&*cancel))
                .bind(notification_id)
                .execute(&mut *tx)
                .await?;
            sqlx::query(AssertSqlSafe(&*insert_timer))
                .bind(ramzes_core::new_id())
                .bind(&workspace_id)
                .bind("release")
                .bind(until_ms)
                .bind(notification_id)
                .bind(None::<String>)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            Ok(())
        })
    }

    /// Moves the events of `from` into `into` and removes `from` with its timers.
    pub async fn merge_notifications(&self, from: &str, into: &str, now_ms: i64) -> Result<()> {
        let q = |text: &str| self.sql(text).into_owned();
        let copy = q(
            "INSERT INTO notification_events (notification_id, event_id, workspace_id, role, dedup_key) \
             SELECT $1, event_id, workspace_id, CASE WHEN role = 'first' THEN 'grouped' ELSE role END, dedup_key \
             FROM notification_events WHERE notification_id = $2 \
             ON CONFLICT (notification_id, event_id) DO NOTHING",
        );
        let unlink = q("DELETE FROM notification_events WHERE notification_id = $1");
        let recount = q("UPDATE notifications SET updated_at = $1, \
             event_count = (SELECT COUNT(*) FROM notification_events WHERE notification_id = $2), \
             severity = (SELECT COALESCE(MAX(e.severity), 1) FROM events e \
                         JOIN notification_events ne ON ne.event_id = e.id WHERE ne.notification_id = $2) \
             WHERE id = $2");
        let timers = q("DELETE FROM timers WHERE notification_id = $1");
        let remove = q("DELETE FROM notifications WHERE id = $1");
        with_db!(self, |pool| {
            let mut tx = pool.begin_write().await?;
            sqlx::query(AssertSqlSafe(copy.as_str()))
                .bind(into)
                .bind(from)
                .execute(&mut *tx)
                .await?;
            sqlx::query(AssertSqlSafe(unlink.as_str()))
                .bind(from)
                .execute(&mut *tx)
                .await?;
            sqlx::query(AssertSqlSafe(recount.as_str()))
                .bind(now_ms)
                .bind(into)
                .execute(&mut *tx)
                .await?;
            sqlx::query(AssertSqlSafe(timers.as_str()))
                .bind(from)
                .execute(&mut *tx)
                .await?;
            sqlx::query(AssertSqlSafe(remove.as_str()))
                .bind(from)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            Ok(())
        })
    }

    pub async fn add_timer(
        &self,
        workspace_id: &str,
        notification_id: &str,
        kind: &str,
        due_at: i64,
        data: Option<&str>,
    ) -> Result<()> {
        let sql = self.sql(INSERT_TIMER);
        with_db!(self, |pool| {
            sqlx::query(AssertSqlSafe(&*sql))
                .bind(ramzes_core::new_id())
                .bind(workspace_id)
                .bind(kind)
                .bind(due_at)
                .bind(notification_id)
                .bind(data)
                .execute(pool)
                .await
                .map(|_| ())
        })?;
        Ok(())
    }

    /// Takes a lease on up to `limit` timers that are due, earliest first.
    pub async fn claim_due_timers(
        &self,
        now_ms: i64,
        lease_ms: i64,
        limit: i64,
    ) -> Result<Vec<Timer>> {
        const DUE: &str = "due_at <= $2 AND (claimed_until IS NULL OR claimed_until < $2)";
        let sql = self
            .sql(&format!(
                "UPDATE timers SET claimed_until = $1 WHERE id IN \
                 (SELECT id FROM timers WHERE {DUE} ORDER BY due_at, id LIMIT $3) AND {DUE} \
                 RETURNING id, workspace_id, kind, due_at, notification_id, data"
            ))
            .into_owned();
        let mut timers: Vec<Timer> = with_db!(self, |pool| {
            let rows = sqlx::query(AssertSqlSafe(sql.as_str()))
                .bind(now_ms + lease_ms)
                .bind(now_ms)
                .bind(limit)
                .fetch_all(pool)
                .await?;
            rows.iter()
                .map(|row| {
                    Ok::<_, sqlx::Error>(Timer {
                        id: row.try_get("id")?,
                        workspace_id: row.try_get("workspace_id")?,
                        kind: row.try_get("kind")?,
                        due_at: row.try_get("due_at")?,
                        notification_id: row.try_get("notification_id")?,
                        data: row.try_get("data")?,
                    })
                })
                .collect::<sqlx::Result<Vec<_>>>()
        })?;
        timers.sort_by(|a, b| (a.due_at, &a.id).cmp(&(b.due_at, &b.id)));
        Ok(timers)
    }

    pub async fn delete_timer(&self, id: &str) -> Result<()> {
        let sql = self.sql("DELETE FROM timers WHERE id = $1");
        with_db!(self, |pool| sqlx::query(AssertSqlSafe(&*sql))
            .bind(id)
            .execute(pool)
            .await
            .map(|_| ()))?;
        Ok(())
    }

    /// Removes a notification's timers: those of one kind, or all of them.
    pub async fn cancel_timers(&self, notification_id: &str, kind: Option<&str>) -> Result<()> {
        let sql =
            self.sql("DELETE FROM timers WHERE notification_id = $1 AND ($2 IS NULL OR kind = $2)");
        with_db!(self, |pool| {
            sqlx::query(AssertSqlSafe(&*sql))
                .bind(notification_id)
                .bind(kind)
                .execute(pool)
                .await
                .map(|_| ())
        })?;
        Ok(())
    }

    /// The timers waiting for a notification: `(kind, due_at, data)`.
    pub async fn notification_timers(
        &self,
        notification_id: &str,
    ) -> Result<Vec<(String, i64, Option<String>)>> {
        let sql = self.sql(
            "SELECT kind, due_at, data FROM timers WHERE notification_id = $1 ORDER BY due_at",
        );
        let rows: Vec<(String, i64, Option<String>)> = with_db!(self, |pool| {
            sqlx::query_as(AssertSqlSafe(&*sql))
                .bind(notification_id)
                .fetch_all(pool)
                .await
        })?;
        Ok(rows)
    }

    /// When the notification's next timer is due.
    pub async fn next_timer(&self, notification_id: &str) -> Result<Option<i64>> {
        let sql = self.sql("SELECT MIN(due_at) FROM timers WHERE notification_id = $1");
        let (due,): (Option<i64>,) = with_db!(self, |pool| {
            sqlx::query_as(AssertSqlSafe(&*sql))
                .bind(notification_id)
                .fetch_one(pool)
                .await
        })?;
        Ok(due)
    }

    /// Applies a state change to a notification that is in one of the allowed states, and
    /// removes its timers.
    async fn transition(
        &self,
        workspace_id: &str,
        id: &str,
        change: Change<'_>,
        by: Option<&str>,
        now_ms: i64,
    ) -> Result<()> {
        let Change { set, from, refusal } = change;
        let update = self
            .sql(&format!(
                "UPDATE notifications SET {set}, updated_at = $1 WHERE workspace_id = $3 AND id = $4 AND state IN ({from})"
            ))
            .into_owned();
        let exists = self.sql("SELECT 1 FROM notifications WHERE workspace_id = $1 AND id = $2");
        let timers = self.sql("DELETE FROM timers WHERE notification_id = $1");
        with_db!(self, |pool| {
            let mut tx = pool.begin_write().await?;
            let changed = sqlx::query(AssertSqlSafe(update.as_str()))
                .bind(now_ms)
                .bind(by)
                .bind(workspace_id)
                .bind(id)
                .execute(&mut *tx)
                .await?
                .rows_affected();
            if changed == 0 {
                let found = sqlx::query(AssertSqlSafe(&*exists))
                    .bind(workspace_id)
                    .bind(id)
                    .fetch_optional(&mut *tx)
                    .await?;
                return Err(match found {
                    Some(_) => StoreError::Conflict(refusal.to_owned()),
                    None => StoreError::NotFound,
                });
            }
            sqlx::query(AssertSqlSafe(&*timers))
                .bind(id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            Ok(())
        })
    }

    /// Marks a notification as seen by a person. Reminders and anything still waiting stop.
    pub async fn acknowledge(
        &self,
        workspace_id: &str,
        id: &str,
        by: &str,
        now_ms: i64,
    ) -> Result<()> {
        let change = Change {
            set: "state = 'acknowledged', acked_by = $2, acked_at = $1, held_until = NULL, hold_kind = NULL",
            from: "'sent', 'grouping', 'held'",
            refusal: "this notification is already acknowledged or over",
        };
        self.transition(workspace_id, id, change, Some(by), now_ms)
            .await
    }

    /// Closes a notification by hand.
    pub async fn resolve_notification(
        &self,
        workspace_id: &str,
        id: &str,
        now_ms: i64,
    ) -> Result<()> {
        let change = Change {
            set: "state = 'resolved', resolved_at = $1, held_until = NULL, hold_kind = NULL, \
                  acked_by = COALESCE($2, acked_by)",
            from: "'sent', 'acknowledged', 'grouping', 'held'",
            refusal: "this notification is already over",
        };
        self.transition(workspace_id, id, change, None, now_ms)
            .await
    }
}
