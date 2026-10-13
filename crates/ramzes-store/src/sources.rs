use serde::Serialize;
use sqlx::{AssertSqlSafe, Row};

use crate::{Result, Store, StoreError, with_db};

/// A producer of events. The ingest key itself is never stored, only its hash and a display prefix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Source {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub kind: String,
    pub key_prefix: String,
    pub created_at: i64,
}

/// A request a source sent that ramzes rejected, kept for debugging.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceRequest {
    pub id: String,
    pub received_at: i64,
    pub body: String,
    pub error: Option<String>,
}

const SOURCE_COLS: &str = "id, workspace_id, name, kind, key_prefix, created_at";
const KEPT_REQUESTS: i64 = 20;

macro_rules! source_from {
    ($row:expr) => {
        Source {
            id: $row.try_get("id")?,
            workspace_id: $row.try_get("workspace_id")?,
            name: $row.try_get("name")?,
            kind: $row.try_get("kind")?,
            key_prefix: $row.try_get("key_prefix")?,
            created_at: $row.try_get("created_at")?,
        }
    };
}

impl Store {
    pub async fn create_source(
        &self,
        workspace_id: &str,
        name: &str,
        kind: &str,
        key_hash: &str,
        key_prefix: &str,
        now_ms: i64,
    ) -> Result<Source> {
        let source = Source {
            id: ramzes_core::new_id(),
            workspace_id: workspace_id.to_owned(),
            name: name.to_owned(),
            kind: kind.to_owned(),
            key_prefix: key_prefix.to_owned(),
            created_at: now_ms,
        };
        let sql = self.sql(
            "INSERT INTO sources (id, workspace_id, name, kind, key_hash, key_prefix, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        );
        with_db!(self, |pool| {
            sqlx::query(AssertSqlSafe(&*sql))
                .bind(&source.id)
                .bind(workspace_id)
                .bind(name)
                .bind(kind)
                .bind(key_hash)
                .bind(key_prefix)
                .bind(now_ms)
                .execute(pool)
                .await
                .map(|_| ())
        })?;
        Ok(source)
    }

    pub async fn list_sources(&self, workspace_id: &str) -> Result<Vec<Source>> {
        let sql = self
            .sql(&format!(
                "SELECT {SOURCE_COLS} FROM sources WHERE workspace_id = $1 ORDER BY id"
            ))
            .into_owned();
        with_db!(self, |pool| {
            let rows = sqlx::query(AssertSqlSafe(sql.as_str()))
                .bind(workspace_id)
                .fetch_all(pool)
                .await?;
            rows.iter().map(|row| Ok(source_from!(row))).collect()
        })
    }

    pub async fn source_by_key_hash(&self, key_hash: &str) -> Result<Option<Source>> {
        let sql = self
            .sql(&format!(
                "SELECT {SOURCE_COLS} FROM sources WHERE key_hash = $1"
            ))
            .into_owned();
        with_db!(self, |pool| {
            let row = sqlx::query(AssertSqlSafe(sql.as_str()))
                .bind(key_hash)
                .fetch_optional(pool)
                .await?;
            row.map(|row| Ok(source_from!(row))).transpose()
        })
    }

    pub async fn rotate_source_key(
        &self,
        workspace_id: &str,
        id: &str,
        key_hash: &str,
        key_prefix: &str,
    ) -> Result<()> {
        let sql = self.sql(
            "UPDATE sources SET key_hash = $1, key_prefix = $2 WHERE workspace_id = $3 AND id = $4",
        );
        let changed = with_db!(self, |pool| {
            sqlx::query(AssertSqlSafe(&*sql))
                .bind(key_hash)
                .bind(key_prefix)
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

    pub async fn delete_source(&self, workspace_id: &str, id: &str) -> Result<()> {
        let requests =
            self.sql("DELETE FROM source_requests WHERE workspace_id = $1 AND source_id = $2");
        let source = self.sql("DELETE FROM sources WHERE workspace_id = $1 AND id = $2");
        let changed = with_db!(self, |pool| {
            sqlx::query(AssertSqlSafe(&*requests))
                .bind(workspace_id)
                .bind(id)
                .execute(pool)
                .await?;
            sqlx::query(AssertSqlSafe(&*source))
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

    /// Records a rejected request and trims the source's history to the newest 20.
    pub async fn record_source_request(
        &self,
        workspace_id: &str,
        source_id: &str,
        body: &str,
        error: Option<&str>,
        now_ms: i64,
    ) -> Result<()> {
        let insert = self.sql(
            "INSERT INTO source_requests (id, workspace_id, source_id, received_at, body, error) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        );
        let trim = self.sql(
            "DELETE FROM source_requests WHERE source_id = $1 AND id NOT IN \
             (SELECT id FROM source_requests WHERE source_id = $1 ORDER BY id DESC LIMIT $2)",
        );
        let id = ramzes_core::new_id();
        with_db!(self, |pool| {
            sqlx::query(AssertSqlSafe(&*insert))
                .bind(&id)
                .bind(workspace_id)
                .bind(source_id)
                .bind(now_ms)
                .bind(body)
                .bind(error)
                .execute(pool)
                .await?;
            sqlx::query(AssertSqlSafe(&*trim))
                .bind(source_id)
                .bind(KEPT_REQUESTS)
                .execute(pool)
                .await
                .map(|_| ())
        })?;
        Ok(())
    }

    /// The source's recent rejected requests, newest first.
    pub async fn list_source_requests(
        &self,
        workspace_id: &str,
        source_id: &str,
    ) -> Result<Vec<SourceRequest>> {
        let sql = self.sql(
            "SELECT id, received_at, body, error FROM source_requests \
             WHERE workspace_id = $1 AND source_id = $2 ORDER BY id DESC",
        );
        with_db!(self, |pool| {
            let rows = sqlx::query(AssertSqlSafe(&*sql))
                .bind(workspace_id)
                .bind(source_id)
                .fetch_all(pool)
                .await?;
            rows.iter()
                .map(|row| {
                    Ok(SourceRequest {
                        id: row.try_get("id")?,
                        received_at: row.try_get("received_at")?,
                        body: row.try_get("body")?,
                        error: row.try_get("error")?,
                    })
                })
                .collect()
        })
    }
}
