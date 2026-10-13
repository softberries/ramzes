use std::collections::HashSet;

use ramzes_core::Route;
use sqlx::{AssertSqlSafe, Row};

use crate::sql::BeginWrite;
use crate::{Result, Store, StoreError, with_db};

fn json<T: serde::Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|e| StoreError::Data(e.to_string()))
}

macro_rules! route_from {
    ($row:expr) => {{
        let id: String = $row.try_get("id")?;
        let match_json: String = $row.try_get("match_json")?;
        let targets_json: String = $row.try_get("targets_json")?;
        Route {
            r#match: serde_json::from_str(&match_json).map_err(|e| {
                StoreError::Data(format!("route {id} has an unreadable match: {e}"))
            })?,
            targets: serde_json::from_str(&targets_json)
                .map_err(|e| StoreError::Data(format!("route {id} has unreadable targets: {e}")))?,
            id,
            name: $row.try_get("name")?,
            position: $row.try_get("position")?,
            enabled: $row.try_get::<i64, _>("enabled")? != 0,
            stop: $row.try_get::<i64, _>("stop")? != 0,
        }
    }};
}

impl Store {
    /// Adds a route after all existing ones. `r.position` is ignored.
    pub async fn create_route(&self, workspace_id: &str, r: &Route, now_ms: i64) -> Result<()> {
        let sql = self.sql(
            "INSERT INTO routes (id, workspace_id, name, position, enabled, stop, match_json, targets_json, \
             shaping_json, escalation_json, created_at, updated_at) \
             VALUES ($1, $2, $3, (SELECT COALESCE(MAX(position), 0) + 1 FROM routes WHERE workspace_id = $2), \
             $4, $5, $6, $7, '{}', '[]', $8, $8)",
        );
        let (match_json, targets_json) = (json(&r.r#match)?, json(&r.targets)?);
        with_db!(self, |pool| {
            sqlx::query(AssertSqlSafe(&*sql))
                .bind(&r.id)
                .bind(workspace_id)
                .bind(&r.name)
                .bind(i64::from(r.enabled))
                .bind(i64::from(r.stop))
                .bind(&match_json)
                .bind(&targets_json)
                .bind(now_ms)
                .execute(pool)
                .await
                .map(|_| ())
        })?;
        Ok(())
    }

    /// Replaces everything about a route except its position, and forgets its last error.
    pub async fn update_route(&self, workspace_id: &str, r: &Route, now_ms: i64) -> Result<()> {
        let sql = self.sql(
            "UPDATE routes SET name = $1, enabled = $2, stop = $3, match_json = $4, targets_json = $5, updated_at = $6, \
             last_error = NULL, last_error_at = NULL \
             WHERE workspace_id = $7 AND id = $8",
        );
        let (match_json, targets_json) = (json(&r.r#match)?, json(&r.targets)?);
        let changed = with_db!(self, |pool| {
            sqlx::query(AssertSqlSafe(&*sql))
                .bind(&r.name)
                .bind(i64::from(r.enabled))
                .bind(i64::from(r.stop))
                .bind(&match_json)
                .bind(&targets_json)
                .bind(now_ms)
                .bind(workspace_id)
                .bind(&r.id)
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

    pub async fn delete_route(&self, workspace_id: &str, id: &str) -> Result<()> {
        let sql = self.sql("DELETE FROM routes WHERE workspace_id = $1 AND id = $2");
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

    /// All routes in evaluation order.
    pub async fn list_routes(&self, workspace_id: &str) -> Result<Vec<Route>> {
        let sql = self.sql(
            "SELECT id, name, position, enabled, stop, match_json, targets_json FROM routes \
             WHERE workspace_id = $1 ORDER BY position, id",
        );
        with_db!(self, |pool| {
            let rows = sqlx::query(AssertSqlSafe(&*sql))
                .bind(workspace_id)
                .fetch_all(pool)
                .await?;
            rows.iter().map(|row| Ok(route_from!(row))).collect()
        })
    }

    /// Sets the evaluation order. `ids` must list every route exactly once.
    pub async fn reorder_routes(
        &self,
        workspace_id: &str,
        ids: &[String],
        now_ms: i64,
    ) -> Result<()> {
        let select = self.sql("SELECT id FROM routes WHERE workspace_id = $1");
        let update = self.sql(
            "UPDATE routes SET position = $1, updated_at = $2 WHERE workspace_id = $3 AND id = $4",
        );
        let given: HashSet<&String> = ids.iter().collect();
        with_db!(self, |pool| {
            let mut tx = pool.begin_write().await?;
            let existing: Vec<(String,)> = sqlx::query_as(AssertSqlSafe(&*select))
                .bind(workspace_id)
                .fetch_all(&mut *tx)
                .await?;
            let is_permutation = given.len() == ids.len()
                && existing.len() == ids.len()
                && existing.iter().all(|(id,)| given.contains(id));
            if !is_permutation {
                return Err(StoreError::Conflict(
                    "the new order must list every route exactly once".into(),
                ));
            }
            for (position, id) in (1_i64..).zip(ids) {
                sqlx::query(AssertSqlSafe(&*update))
                    .bind(position)
                    .bind(now_ms)
                    .bind(workspace_id)
                    .bind(id)
                    .execute(&mut *tx)
                    .await?;
            }
            tx.commit().await?;
            Ok(())
        })
    }

    /// Records why a route's expression failed, or clears the record with `None`.
    pub async fn set_route_error(
        &self,
        route_id: &str,
        error: Option<&str>,
        now_ms: i64,
    ) -> Result<()> {
        let sql = self.sql("UPDATE routes SET last_error = $1, last_error_at = $2 WHERE id = $3");
        with_db!(self, |pool| {
            sqlx::query(AssertSqlSafe(&*sql))
                .bind(error)
                .bind(error.map(|_| now_ms))
                .bind(route_id)
                .execute(pool)
                .await
                .map(|_| ())
        })?;
        Ok(())
    }

    /// Routes whose expression last failed: `(route id, reason, when)`.
    pub async fn route_errors(&self, workspace_id: &str) -> Result<Vec<(String, String, i64)>> {
        let sql = self.sql(
            "SELECT id, last_error, last_error_at FROM routes WHERE workspace_id = $1 AND last_error IS NOT NULL \
             ORDER BY position, id",
        );
        let rows: Vec<(String, String, i64)> = with_db!(self, |pool| {
            sqlx::query_as(AssertSqlSafe(&*sql))
                .bind(workspace_id)
                .fetch_all(pool)
                .await
        })?;
        Ok(rows)
    }
}
