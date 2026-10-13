use ramzes_core::Template;
use sqlx::{AssertSqlSafe, Row};

use crate::{Result, Store, StoreError, with_db};

macro_rules! template_from {
    ($row:expr) => {
        Template {
            id: $row.try_get("id")?,
            name: $row.try_get("name")?,
            title: $row.try_get("title")?,
            body: $row.try_get("body")?,
        }
    };
}

impl Store {
    pub async fn create_template(
        &self,
        workspace_id: &str,
        t: &Template,
        now_ms: i64,
    ) -> Result<()> {
        // `connector_kind` is '*': a template words a message for any connector.
        let sql = self.sql(
            "INSERT INTO templates (id, workspace_id, connector_kind, name, title, body, created_at, updated_at) \
             VALUES ($1, $2, '*', $3, $4, $5, $6, $6)",
        );
        with_db!(self, |pool| {
            sqlx::query(AssertSqlSafe(&*sql))
                .bind(&t.id)
                .bind(workspace_id)
                .bind(&t.name)
                .bind(&t.title)
                .bind(&t.body)
                .bind(now_ms)
                .execute(pool)
                .await
                .map(|_| ())
        })?;
        Ok(())
    }

    pub async fn update_template(
        &self,
        workspace_id: &str,
        t: &Template,
        now_ms: i64,
    ) -> Result<()> {
        let sql = self.sql(
            "UPDATE templates SET name = $1, title = $2, body = $3, updated_at = $4 WHERE workspace_id = $5 AND id = $6",
        );
        let changed = with_db!(self, |pool| {
            sqlx::query(AssertSqlSafe(&*sql))
                .bind(&t.name)
                .bind(&t.title)
                .bind(&t.body)
                .bind(now_ms)
                .bind(workspace_id)
                .bind(&t.id)
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

    pub async fn delete_template(&self, workspace_id: &str, id: &str) -> Result<()> {
        let sql = self.sql("DELETE FROM templates WHERE workspace_id = $1 AND id = $2");
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

    pub async fn get_template(&self, workspace_id: &str, id: &str) -> Result<Option<Template>> {
        let sql = self
            .sql("SELECT id, name, title, body FROM templates WHERE workspace_id = $1 AND id = $2");
        with_db!(self, |pool| {
            let row = sqlx::query(AssertSqlSafe(&*sql))
                .bind(workspace_id)
                .bind(id)
                .fetch_optional(pool)
                .await?;
            row.map(|row| Ok(template_from!(row))).transpose()
        })
    }

    /// All templates in order of creation.
    pub async fn list_templates(&self, workspace_id: &str) -> Result<Vec<Template>> {
        let sql = self
            .sql("SELECT id, name, title, body FROM templates WHERE workspace_id = $1 ORDER BY id");
        with_db!(self, |pool| {
            let rows = sqlx::query(AssertSqlSafe(&*sql))
                .bind(workspace_id)
                .fetch_all(pool)
                .await?;
            rows.iter().map(|row| Ok(template_from!(row))).collect()
        })
    }
}
