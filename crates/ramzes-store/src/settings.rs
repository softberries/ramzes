use sqlx::AssertSqlSafe;

use crate::{Result, Store, with_db};

impl Store {
    pub async fn get_setting(&self, workspace_id: &str, key: &str) -> Result<Option<String>> {
        let sql = self.sql("SELECT value FROM settings WHERE workspace_id = $1 AND key = $2");
        let row: Option<(String,)> = with_db!(self, |pool| {
            sqlx::query_as(AssertSqlSafe(&*sql))
                .bind(workspace_id)
                .bind(key)
                .fetch_optional(pool)
                .await
        })?;
        Ok(row.map(|(value,)| value))
    }

    pub async fn set_setting(&self, workspace_id: &str, key: &str, value: &str) -> Result<()> {
        let sql = self.sql(
            "INSERT INTO settings (workspace_id, key, value) VALUES ($1, $2, $3) \
             ON CONFLICT (workspace_id, key) DO UPDATE SET value = excluded.value",
        );
        with_db!(self, |pool| {
            sqlx::query(AssertSqlSafe(&*sql))
                .bind(workspace_id)
                .bind(key)
                .bind(value)
                .execute(pool)
                .await
                .map(|_| ())
        })?;
        Ok(())
    }
}
