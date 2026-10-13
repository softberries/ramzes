use serde::{Deserialize, Serialize};
use sqlx::{AssertSqlSafe, Row};

use crate::events::is_unique_violation;
use crate::{Result, Store, StoreError, with_db};

/// A connector definition added by a user. `definition` is the YAML text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DefinitionRow {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub definition: String,
    pub created_at: i64,
}

fn conflict(e: sqlx::Error, kind: &str) -> StoreError {
    if is_unique_violation(&e) {
        StoreError::Conflict(format!("there is already a connector of kind {kind:?}"))
    } else {
        StoreError::Db(e)
    }
}

impl Store {
    pub async fn create_definition(&self, workspace_id: &str, d: &DefinitionRow) -> Result<()> {
        let sql = self.sql(
            "INSERT INTO connector_definitions (id, workspace_id, kind, name, definition, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        );
        with_db!(self, |pool| {
            sqlx::query(AssertSqlSafe(&*sql))
                .bind(&d.id)
                .bind(workspace_id)
                .bind(&d.kind)
                .bind(&d.name)
                .bind(&d.definition)
                .bind(d.created_at)
                .execute(pool)
                .await
                .map(|_| ())
        })
        .map_err(|e| conflict(e, &d.kind))
    }

    /// Replaces the kind, name and text of a definition.
    pub async fn update_definition(&self, workspace_id: &str, d: &DefinitionRow) -> Result<()> {
        let sql = self.sql(
            "UPDATE connector_definitions SET kind = $1, name = $2, definition = $3 WHERE workspace_id = $4 AND id = $5",
        );
        let changed = with_db!(self, |pool| {
            sqlx::query(AssertSqlSafe(&*sql))
                .bind(&d.kind)
                .bind(&d.name)
                .bind(&d.definition)
                .bind(workspace_id)
                .bind(&d.id)
                .execute(pool)
                .await
                .map(|r| r.rows_affected())
        })
        .map_err(|e| conflict(e, &d.kind))?;
        if changed == 0 {
            Err(StoreError::NotFound)
        } else {
            Ok(())
        }
    }

    pub async fn delete_definition(&self, workspace_id: &str, id: &str) -> Result<()> {
        let sql = self.sql("DELETE FROM connector_definitions WHERE workspace_id = $1 AND id = $2");
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

    pub async fn list_definitions(&self, workspace_id: &str) -> Result<Vec<DefinitionRow>> {
        let sql = self.sql(
            "SELECT id, kind, name, definition, created_at FROM connector_definitions WHERE workspace_id = $1 ORDER BY id",
        );
        with_db!(self, |pool| {
            let rows = sqlx::query(AssertSqlSafe(&*sql))
                .bind(workspace_id)
                .fetch_all(pool)
                .await?;
            rows.iter()
                .map(|row| {
                    Ok(DefinitionRow {
                        id: row.try_get("id")?,
                        kind: row.try_get("kind")?,
                        name: row.try_get("name")?,
                        definition: row.try_get("definition")?,
                        created_at: row.try_get("created_at")?,
                    })
                })
                .collect()
        })
    }
}
