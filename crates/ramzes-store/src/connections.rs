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
}
