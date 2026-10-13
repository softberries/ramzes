use std::collections::{BTreeMap, HashMap};

use ramzes_core::{Event, EventStatus, Severity};
use serde::Serialize;
use sqlx::{AssertSqlSafe, Row};

use crate::sql::{BeginWrite, Sql, bind_args, like_escape};
use crate::{Result, Store, StoreError, with_db};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InsertOutcome {
    pub ids: Vec<String>,
    /// The idempotency key had been used before; `ids` are those of the original request.
    pub duplicate: bool,
}

/// What became of an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Not routed yet, or has a delivery still to be sent.
    Pending,
    /// Every delivery arrived.
    Delivered,
    /// At least one delivery failed for good.
    Failed,
    /// No route matched.
    Unrouted,
}

#[derive(Debug, Clone, Default)]
pub struct EventFilter {
    /// Return only events older than this id (keyset paging).
    pub before_id: Option<String>,
    pub source_id: Option<String>,
    /// Exact topic, or a glob where `*` matches any run of characters.
    pub topic: Option<String>,
    pub min_severity: Option<Severity>,
    pub label: Option<(String, String)>,
    /// Case-insensitive substring of the title.
    pub title_contains: Option<String>,
    /// Received at or after this time.
    pub since_ms: Option<i64>,
    /// Received at or before this time.
    pub until_ms: Option<i64>,
    pub outcome: Option<Outcome>,
    pub limit: i64,
}

/// An event with a summary of what happened to it, for lists.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EventRow {
    #[serde(flatten)]
    pub event: Event,
    pub routed: bool,
    pub route_count: i64,
    pub deliveries_total: i64,
    pub deliveries_delivered: i64,
    pub deliveries_failed: i64,
}

pub(crate) const EVENT_COLS: &str = "id, workspace_id, source_id, topic, severity, status, title, body, payload, dedup_key, \
                          occurred_at, received_at";
const MAX_PAGE: i64 = 500;

/// Deliveries belonging to event `e`.
const DELIVERIES_OF_E: &str = "FROM notification_events ne JOIN deliveries d ON d.notification_id = ne.notification_id \
                               WHERE ne.event_id = e.id";

/// An event row before its text columns are parsed.
pub(crate) struct RawEvent {
    pub id: String,
    pub workspace_id: String,
    pub source_id: String,
    pub topic: String,
    pub severity: i64,
    pub status: String,
    pub title: String,
    pub body: Option<String>,
    pub payload: String,
    pub dedup_key: Option<String>,
    pub occurred_at: i64,
    pub received_at: i64,
}

macro_rules! raw_event {
    ($row:expr) => {
        $crate::events::RawEvent {
            id: $row.try_get("id")?,
            workspace_id: $row.try_get("workspace_id")?,
            source_id: $row.try_get("source_id")?,
            topic: $row.try_get("topic")?,
            severity: $row.try_get("severity")?,
            status: $row.try_get("status")?,
            title: $row.try_get("title")?,
            body: $row.try_get("body")?,
            payload: $row.try_get("payload")?,
            dedup_key: $row.try_get("dedup_key")?,
            occurred_at: $row.try_get("occurred_at")?,
            received_at: $row.try_get("received_at")?,
        }
    };
}

pub(crate) use raw_event;

impl RawEvent {
    pub(crate) fn into_event(self, labels: BTreeMap<String, String>) -> Result<Event> {
        Ok(Event {
            severity: Severity::from_i64(self.severity).ok_or_else(|| {
                StoreError::Data(format!("event {} has severity {}", self.id, self.severity))
            })?,
            status: EventStatus::parse(&self.status).ok_or_else(|| {
                StoreError::Data(format!("event {} has status {:?}", self.id, self.status))
            })?,
            payload: serde_json::from_str(&self.payload).map_err(|e| {
                StoreError::Data(format!("event {} has an unreadable payload: {e}", self.id))
            })?,
            id: self.id,
            workspace_id: self.workspace_id,
            source_id: self.source_id,
            topic: self.topic,
            title: self.title,
            body: self.body,
            labels,
            dedup_key: self.dedup_key,
            occurred_at: self.occurred_at,
            received_at: self.received_at,
        })
    }
}

fn is_unique_violation(e: &sqlx::Error) -> bool {
    e.as_database_error()
        .is_some_and(|d| d.is_unique_violation())
}

impl Store {
    /// Stores the events in one transaction.
    ///
    /// With an idempotency key that this source has used before, nothing is stored and the ids
    /// of the original request are returned.
    pub async fn insert_events(
        &self,
        events: &[Event],
        idempotency_key: Option<&str>,
    ) -> Result<InsertOutcome> {
        let Some(first) = events.first() else {
            return Ok(InsertOutcome {
                ids: Vec::new(),
                duplicate: false,
            });
        };
        if let Some(key) = idempotency_key
            && let Some(ids) = self.ids_for_idempotency_key(&first.source_id, key).await?
        {
            return Ok(InsertOutcome {
                ids,
                duplicate: true,
            });
        }

        let insert_event = self.sql(
            "INSERT INTO events (id, workspace_id, source_id, topic, severity, status, title, body, payload, dedup_key, \
             idempotency_key, occurred_at, received_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
        );
        let insert_label = self.sql(
            "INSERT INTO event_labels (event_id, workspace_id, key, value) VALUES ($1, $2, $3, $4)",
        );
        let written: sqlx::Result<()> = with_db!(self, |pool| {
            async {
                let mut tx = pool.begin_write().await?;
                for (i, e) in events.iter().enumerate() {
                    sqlx::query(AssertSqlSafe(&*insert_event))
                        .bind(&e.id)
                        .bind(&e.workspace_id)
                        .bind(&e.source_id)
                        .bind(&e.topic)
                        .bind(e.severity.as_i64())
                        .bind(e.status.as_str())
                        .bind(&e.title)
                        .bind(&e.body)
                        .bind(e.payload.to_string())
                        .bind(&e.dedup_key)
                        .bind(idempotency_key.map(|key| format!("{key}#{i}")))
                        .bind(e.occurred_at)
                        .bind(e.received_at)
                        .execute(&mut *tx)
                        .await?;
                    for (key, value) in &e.labels {
                        sqlx::query(AssertSqlSafe(&*insert_label))
                            .bind(&e.id)
                            .bind(&e.workspace_id)
                            .bind(key)
                            .bind(value)
                            .execute(&mut *tx)
                            .await?;
                    }
                }
                tx.commit().await
            }
            .await
        });
        match written {
            Ok(()) => Ok(InsertOutcome {
                ids: events.iter().map(|e| e.id.clone()).collect(),
                duplicate: false,
            }),
            // A concurrent request with the same key committed first.
            Err(e) if is_unique_violation(&e) => {
                let key = idempotency_key.ok_or(StoreError::Db(e))?;
                match self.ids_for_idempotency_key(&first.source_id, key).await? {
                    Some(ids) => Ok(InsertOutcome {
                        ids,
                        duplicate: true,
                    }),
                    None => Err(StoreError::Conflict(
                        "an event with this id already exists".into(),
                    )),
                }
            }
            Err(e) => Err(e.into()),
        }
    }

    async fn ids_for_idempotency_key(
        &self,
        source_id: &str,
        key: &str,
    ) -> Result<Option<Vec<String>>> {
        let sql = self.sql(
            "SELECT id FROM events WHERE source_id = $1 AND idempotency_key LIKE $2 ESCAPE '\\' ORDER BY id",
        );
        let pattern = format!("{}#%", like_escape(key));
        let rows: Vec<(String,)> = with_db!(self, |pool| {
            sqlx::query_as(AssertSqlSafe(&*sql))
                .bind(source_id)
                .bind(&pattern)
                .fetch_all(pool)
                .await
        })?;
        Ok((!rows.is_empty()).then(|| rows.into_iter().map(|(id,)| id).collect()))
    }

    pub(crate) async fn labels_for(
        &self,
        event_ids: &[String],
    ) -> Result<HashMap<String, BTreeMap<String, String>>> {
        let mut labels: HashMap<String, BTreeMap<String, String>> = HashMap::new();
        if event_ids.is_empty() {
            return Ok(labels);
        }
        let mut sql = Sql::new("SELECT event_id, key, value FROM event_labels WHERE event_id IN ");
        sql.list(event_ids);
        let text = self.sql(&sql.text).into_owned();
        let rows: Vec<(String, String, String)> = with_db!(self, |pool| {
            bind_args!(sqlx::query_as(AssertSqlSafe(text.as_str())), sql.args)
                .fetch_all(pool)
                .await
        })?;
        for (event_id, key, value) in rows {
            labels.entry(event_id).or_default().insert(key, value);
        }
        Ok(labels)
    }

    pub(crate) async fn with_labels(&self, raw: Vec<RawEvent>) -> Result<Vec<Event>> {
        let ids: Vec<String> = raw.iter().map(|r| r.id.clone()).collect();
        let mut labels = self.labels_for(&ids).await?;
        raw.into_iter()
            .map(|r| {
                let own = labels.remove(&r.id).unwrap_or_default();
                r.into_event(own)
            })
            .collect()
    }

    pub async fn get_event(&self, workspace_id: &str, id: &str) -> Result<Option<Event>> {
        let sql = self
            .sql(&format!(
                "SELECT {EVENT_COLS} FROM events WHERE workspace_id = $1 AND id = $2"
            ))
            .into_owned();
        let raw: Option<RawEvent> = with_db!(self, |pool| {
            let row = sqlx::query(AssertSqlSafe(sql.as_str()))
                .bind(workspace_id)
                .bind(id)
                .fetch_optional(pool)
                .await?;
            row.map(|row| Ok::<_, sqlx::Error>(raw_event!(row)))
                .transpose()
        })?;
        Ok(self.with_labels(raw.into_iter().collect()).await?.pop())
    }

    /// A page of events, newest first.
    pub async fn list_events(&self, workspace_id: &str, f: &EventFilter) -> Result<Vec<EventRow>> {
        let cols = EVENT_COLS
            .split(", ")
            .map(|c| format!("e.{}", c.trim()))
            .collect::<Vec<_>>()
            .join(", ");
        let mut sql = Sql::new(&format!(
            "SELECT {cols}, e.routed_at, e.route_count, \
             (SELECT COUNT(*) {DELIVERIES_OF_E}) AS d_total, \
             (SELECT COUNT(*) {DELIVERIES_OF_E} AND d.state = 'delivered') AS d_delivered, \
             (SELECT COUNT(*) {DELIVERIES_OF_E} AND d.state IN ('failed', 'dead')) AS d_failed \
             FROM events e WHERE e.workspace_id = "
        ));
        sql.text(workspace_id);
        if let Some(before) = &f.before_id {
            sql.push(" AND e.id < ").text(before.as_str());
        }
        if let Some(source) = &f.source_id {
            sql.push(" AND e.source_id = ").text(source.as_str());
        }
        if let Some(topic) = &f.topic {
            if topic.contains('*') {
                sql.push(" AND e.topic LIKE ")
                    .text(like_escape(topic).replace('*', "%"))
                    .push(" ESCAPE '\\'");
            } else {
                sql.push(" AND e.topic = ").text(topic.as_str());
            }
        }
        if let Some(min) = f.min_severity {
            sql.push(" AND e.severity >= ").int(min.as_i64());
        }
        if let Some((key, value)) = &f.label {
            sql.push(
                " AND EXISTS (SELECT 1 FROM event_labels l WHERE l.event_id = e.id AND l.key = ",
            )
            .text(key.as_str())
            .push(" AND l.value = ")
            .text(value.as_str())
            .push(")");
        }
        if let Some(text) = &f.title_contains {
            sql.push(" AND LOWER(e.title) LIKE ")
                .text(format!("%{}%", like_escape(&text.to_lowercase())))
                .push(" ESCAPE '\\'");
        }
        if let Some(since) = f.since_ms {
            sql.push(" AND e.received_at >= ").int(since);
        }
        if let Some(until) = f.until_ms {
            sql.push(" AND e.received_at <= ").int(until);
        }
        match f.outcome {
            None => {}
            Some(Outcome::Pending) => {
                sql.push(&format!(
                    " AND (e.routed_at IS NULL OR EXISTS (SELECT 1 {DELIVERIES_OF_E} AND d.state IN ('pending', 'in_progress')))"
                ));
            }
            Some(Outcome::Delivered) => {
                sql.push(&format!(
                    " AND e.routed_at IS NOT NULL AND EXISTS (SELECT 1 {DELIVERIES_OF_E}) \
                     AND NOT EXISTS (SELECT 1 {DELIVERIES_OF_E} AND d.state <> 'delivered')"
                ));
            }
            Some(Outcome::Failed) => {
                sql.push(&format!(
                    " AND EXISTS (SELECT 1 {DELIVERIES_OF_E} AND d.state IN ('failed', 'dead'))"
                ));
            }
            Some(Outcome::Unrouted) => {
                sql.push(" AND e.routed_at IS NOT NULL AND e.route_count = 0");
            }
        }
        sql.push(" ORDER BY e.id DESC LIMIT ")
            .int(f.limit.clamp(1, MAX_PAGE));

        let text = self.sql(&sql.text).into_owned();
        struct Extra {
            routed: bool,
            route_count: i64,
            total: i64,
            delivered: i64,
            failed: i64,
        }
        let rows: Vec<(RawEvent, Extra)> = with_db!(self, |pool| {
            let rows = bind_args!(sqlx::query(AssertSqlSafe(text.as_str())), sql.args)
                .fetch_all(pool)
                .await?;
            rows.iter()
                .map(|row| {
                    let extra = Extra {
                        routed: row.try_get::<Option<i64>, _>("routed_at")?.is_some(),
                        route_count: row.try_get("route_count")?,
                        total: row.try_get("d_total")?,
                        delivered: row.try_get("d_delivered")?,
                        failed: row.try_get("d_failed")?,
                    };
                    Ok::<_, sqlx::Error>((raw_event!(row), extra))
                })
                .collect::<sqlx::Result<Vec<_>>>()
        })?;

        let (raw, extras): (Vec<RawEvent>, Vec<Extra>) = rows.into_iter().unzip();
        let events = self.with_labels(raw).await?;
        Ok(events
            .into_iter()
            .zip(extras)
            .map(|(event, x)| EventRow {
                event,
                routed: x.routed,
                route_count: x.route_count,
                deliveries_total: x.total,
                deliveries_delivered: x.delivered,
                deliveries_failed: x.failed,
            })
            .collect())
    }

    /// Takes a lease on up to `limit` events that have not been routed, oldest first.
    ///
    /// An event whose lease expires without [`Store::mark_routed`] is handed out again.
    pub async fn claim_unrouted(
        &self,
        now_ms: i64,
        lease_ms: i64,
        limit: i64,
    ) -> Result<Vec<Event>> {
        const FREE: &str = "routed_at IS NULL AND (claimed_until IS NULL OR claimed_until < $2)";
        let sql = self
            .sql(&format!(
                "UPDATE events SET claimed_until = $1 WHERE id IN \
                 (SELECT id FROM events WHERE {FREE} ORDER BY id LIMIT $3) AND {FREE} RETURNING {EVENT_COLS}"
            ))
            .into_owned();
        let mut raw: Vec<RawEvent> = with_db!(self, |pool| {
            let rows = sqlx::query(AssertSqlSafe(sql.as_str()))
                .bind(now_ms + lease_ms)
                .bind(now_ms)
                .bind(limit)
                .fetch_all(pool)
                .await?;
            rows.iter()
                .map(|row| Ok::<_, sqlx::Error>(raw_event!(row)))
                .collect::<sqlx::Result<Vec<_>>>()
        })?;
        raw.sort_by(|a, b| a.id.cmp(&b.id));
        self.with_labels(raw).await
    }

    /// Records that routing finished for an event that matched `route_count` routes.
    pub async fn mark_routed(&self, event_id: &str, route_count: i64, now_ms: i64) -> Result<()> {
        let sql = self.sql("UPDATE events SET routed_at = $1, route_count = $2, claimed_until = NULL WHERE id = $3");
        with_db!(self, |pool| {
            sqlx::query(AssertSqlSafe(&*sql))
                .bind(now_ms)
                .bind(route_count)
                .bind(event_id)
                .execute(pool)
                .await
                .map(|_| ())
        })?;
        Ok(())
    }

    /// Deletes up to `limit` events received before `cutoff_ms`, with everything that hangs off them.
    ///
    /// With `topic`, only that topic is deleted; `except_topics` are never deleted. Returns the
    /// number of events removed.
    pub async fn delete_events_before(
        &self,
        workspace_id: &str,
        cutoff_ms: i64,
        topic: Option<&str>,
        except_topics: &[String],
        limit: i64,
    ) -> Result<u64> {
        let mut select = Sql::new("SELECT id FROM events WHERE workspace_id = ");
        select
            .text(workspace_id)
            .push(" AND received_at < ")
            .int(cutoff_ms);
        if let Some(topic) = topic {
            select.push(" AND topic = ").text(topic);
        }
        if !except_topics.is_empty() {
            select.push(" AND topic NOT IN ").list(except_topics);
        }
        select.push(" ORDER BY id LIMIT ").int(limit);
        let text = self.sql(&select.text).into_owned();
        let ids: Vec<String> = with_db!(self, |pool| {
            let rows: Vec<(String,)> =
                bind_args!(sqlx::query_as(AssertSqlSafe(text.as_str())), select.args)
                    .fetch_all(pool)
                    .await?;
            Ok::<_, sqlx::Error>(rows.into_iter().map(|(id,)| id).collect())
        })?;
        if ids.is_empty() {
            return Ok(0);
        }

        // Statements run in this order inside one transaction. Each takes the event ids as its
        // only arguments; notifications are removed only once no other event refers to them.
        const LINKED: &str = "SELECT notification_id FROM notification_events WHERE event_id IN ";
        let statements: Vec<String> = [
            format!(
                "DELETE FROM delivery_attempts WHERE delivery_id IN (SELECT id FROM deliveries WHERE notification_id IN \
                 ({LINKED}{{ids}}) AND notification_id NOT IN \
                 (SELECT notification_id FROM notification_events WHERE event_id NOT IN {{ids}}))"
            ),
            format!(
                "DELETE FROM deliveries WHERE notification_id IN ({LINKED}{{ids}}) AND notification_id NOT IN \
                 (SELECT notification_id FROM notification_events WHERE event_id NOT IN {{ids}})"
            ),
            format!(
                "DELETE FROM notifications WHERE id IN ({LINKED}{{ids}}) AND id NOT IN \
                 (SELECT notification_id FROM notification_events WHERE event_id NOT IN {{ids}})"
            ),
            "DELETE FROM notification_events WHERE event_id IN {ids}".to_owned(),
            "DELETE FROM event_labels WHERE event_id IN {ids}".to_owned(),
            "DELETE FROM events WHERE id IN {ids}".to_owned(),
        ]
        .into_iter()
        .map(|template| {
            let mut list = Sql::default();
            list.list(&ids);
            self.sql(&template.replace("{ids}", &list.text)).into_owned()
        })
        .collect();

        let deleted = with_db!(self, |pool| {
            async {
                let mut tx = pool.begin_write().await?;
                let mut last = 0;
                for statement in &statements {
                    let mut query = sqlx::query(AssertSqlSafe(statement.as_str()));
                    for id in &ids {
                        query = query.bind(id);
                    }
                    last = query.execute(&mut *tx).await?.rows_affected();
                }
                tx.commit().await?;
                Ok::<_, sqlx::Error>(last)
            }
            .await
        })?;
        Ok(deleted)
    }
}
