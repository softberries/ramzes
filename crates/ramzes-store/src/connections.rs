use sqlx::{AssertSqlSafe, Row};

use crate::{Result, Store, StoreError, with_db};

/// A configured channel instance. `credentials` is sealed by the caller; the store never sees secrets in the clear.
#[derive(Debug, Clone, PartialEq)]
pub struct Connection {
    pub id: String,
    pub workspace_id: String,
    pub kind: String,
    pub name: String,
    pub settings: serde_json::Value,
    pub credentials: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// How a connection's deliveries went over a period.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
pub struct ConnectionHealth {
    pub connection_id: String,
    pub delivered: i64,
    /// Rejected or given up.
    pub failed: i64,
    /// Waiting or being retried.
    pub pending: i64,
    /// The most recent failure in the period.
    pub last_error: Option<String>,
}

const CONNECTION_COLS: &str =
    "id, workspace_id, kind, name, settings, credentials, created_at, updated_at";

macro_rules! connection_from {
    ($row:expr) => {{
        let settings: String = $row.try_get("settings")?;
        let id: String = $row.try_get("id")?;
        Connection {
            settings: serde_json::from_str(&settings).map_err(|e| {
                StoreError::Data(format!("connection {id} has unreadable settings: {e}"))
            })?,
            id,
            workspace_id: $row.try_get("workspace_id")?,
            kind: $row.try_get("kind")?,
            name: $row.try_get("name")?,
            credentials: $row.try_get("credentials")?,
            created_at: $row.try_get("created_at")?,
            updated_at: $row.try_get("updated_at")?,
        }
    }};
}

impl Store {
    pub async fn create_connection(&self, c: &Connection) -> Result<()> {
        let sql = self.sql(
            "INSERT INTO connections (id, workspace_id, kind, name, settings, credentials, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        );
        with_db!(self, |pool| {
            sqlx::query(AssertSqlSafe(&*sql))
                .bind(&c.id)
                .bind(&c.workspace_id)
                .bind(&c.kind)
                .bind(&c.name)
                .bind(c.settings.to_string())
                .bind(&c.credentials)
                .bind(c.created_at)
                .bind(c.updated_at)
                .execute(pool)
                .await
                .map(|_| ())
        })?;
        Ok(())
    }

    /// Replaces the name, settings and credentials of an existing connection.
    pub async fn update_connection(&self, c: &Connection) -> Result<()> {
        let sql = self.sql(
            "UPDATE connections SET name = $1, settings = $2, credentials = $3, updated_at = $4 \
             WHERE workspace_id = $5 AND id = $6",
        );
        let changed = with_db!(self, |pool| {
            sqlx::query(AssertSqlSafe(&*sql))
                .bind(&c.name)
                .bind(c.settings.to_string())
                .bind(&c.credentials)
                .bind(c.updated_at)
                .bind(&c.workspace_id)
                .bind(&c.id)
                .execute(pool)
                .await
                .map(|r| r.rows_affected())
        })?;
        if changed == 0 {
            Err(StoreError::NotFound)
        } else {
            Ok(())
        }
    }

    pub async fn get_connection(&self, workspace_id: &str, id: &str) -> Result<Option<Connection>> {
        let sql = self
            .sql(&format!(
                "SELECT {CONNECTION_COLS} FROM connections WHERE workspace_id = $1 AND id = $2"
            ))
            .into_owned();
        with_db!(self, |pool| {
            let row = sqlx::query(AssertSqlSafe(sql.as_str()))
                .bind(workspace_id)
                .bind(id)
                .fetch_optional(pool)
                .await?;
            row.map(|row| Ok(connection_from!(row))).transpose()
        })
    }

    pub async fn list_connections(&self, workspace_id: &str) -> Result<Vec<Connection>> {
        let sql = self
            .sql(&format!(
                "SELECT {CONNECTION_COLS} FROM connections WHERE workspace_id = $1 ORDER BY id"
            ))
            .into_owned();
        with_db!(self, |pool| {
            let rows = sqlx::query(AssertSqlSafe(sql.as_str()))
                .bind(workspace_id)
                .fetch_all(pool)
                .await?;
            rows.iter().map(|row| Ok(connection_from!(row))).collect()
        })
    }

    pub async fn delete_connection(&self, workspace_id: &str, id: &str) -> Result<()> {
        let sql = self.sql("DELETE FROM connections WHERE workspace_id = $1 AND id = $2");
        let changed = with_db!(self, |pool| {
            sqlx::query(AssertSqlSafe(&*sql))
                .bind(workspace_id)
                .bind(id)
                .execute(pool)
                .await
                .map(|r| r.rows_affected())
        })?;
        if changed == 0 {
            Err(StoreError::NotFound)
        } else {
            Ok(())
        }
    }

    /// Delivery counts per connection for deliveries created at or after `since_ms`.
    pub async fn connection_health(
        &self,
        workspace_id: &str,
        since_ms: i64,
    ) -> Result<Vec<ConnectionHealth>> {
        let counts = self.sql(
            "SELECT connection_id, \
             COUNT(CASE WHEN state = 'delivered' THEN 1 END) AS delivered, \
             COUNT(CASE WHEN state IN ('failed', 'dead') THEN 1 END) AS failed, \
             COUNT(CASE WHEN state IN ('pending', 'in_progress') THEN 1 END) AS pending \
             FROM deliveries WHERE workspace_id = $1 AND created_at >= $2 GROUP BY connection_id ORDER BY connection_id",
        );
        let errors = self.sql(
            "SELECT d.connection_id, d.last_error FROM deliveries d WHERE d.id IN \
             (SELECT MAX(x.id) FROM deliveries x WHERE x.workspace_id = $1 AND x.created_at >= $2 \
              AND x.state IN ('failed', 'dead') GROUP BY x.connection_id)",
        );
        with_db!(self, |pool| {
            let rows = sqlx::query(AssertSqlSafe(&*counts))
                .bind(workspace_id)
                .bind(since_ms)
                .fetch_all(pool)
                .await?;
            let last: Vec<(String, Option<String>)> = sqlx::query_as(AssertSqlSafe(&*errors))
                .bind(workspace_id)
                .bind(since_ms)
                .fetch_all(pool)
                .await?;
            rows.iter()
                .map(|row| {
                    let connection_id: String = row.try_get("connection_id")?;
                    Ok(ConnectionHealth {
                        delivered: row.try_get("delivered")?,
                        failed: row.try_get("failed")?,
                        pending: row.try_get("pending")?,
                        last_error: last
                            .iter()
                            .find(|(id, _)| *id == connection_id)
                            .and_then(|(_, error)| error.clone()),
                        connection_id,
                    })
                })
                .collect()
        })
    }
}
