use ramzes_core::shaping::Silence;
use serde::Serialize;
use sqlx::{AssertSqlSafe, Row};

use crate::{Result, Store, StoreError, with_db};

/// A silence with who created it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct SilenceRow {
    #[serde(flatten)]
    pub silence: Silence,
    pub author: String,
    pub created_at: i64,
}

macro_rules! silence_from {
    ($row:expr) => {{
        let id: String = $row.try_get("id")?;
        let match_json: String = $row.try_get("match_json")?;
        Silence {
            r#match: serde_json::from_str(&match_json).map_err(|e| {
                StoreError::Data(format!("silence {id} has an unreadable match: {e}"))
            })?,
            id,
            starts_at: $row.try_get("starts_at")?,
            ends_at: $row.try_get("ends_at")?,
            comment: $row.try_get("comment")?,
        }
    }};
}

impl Store {
    pub async fn create_silence(
        &self,
        workspace_id: &str,
        s: &Silence,
        author: &str,
        now_ms: i64,
    ) -> Result<()> {
        let sql = self.sql(
            "INSERT INTO silences (id, workspace_id, match_json, starts_at, ends_at, author, comment, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        );
        let match_json =
            serde_json::to_string(&s.r#match).map_err(|e| StoreError::Data(e.to_string()))?;
        with_db!(self, |pool| {
            sqlx::query(AssertSqlSafe(&*sql))
                .bind(&s.id)
                .bind(workspace_id)
                .bind(&match_json)
                .bind(s.starts_at)
                .bind(s.ends_at)
                .bind(author)
                .bind(&s.comment)
                .bind(now_ms)
                .execute(pool)
                .await
                .map(|_| ())
        })?;
        Ok(())
    }

    /// All silences, the one ending last first.
    pub async fn list_silences(&self, workspace_id: &str) -> Result<Vec<SilenceRow>> {
        let sql = self.sql(
            "SELECT id, match_json, starts_at, ends_at, author, comment, created_at FROM silences \
             WHERE workspace_id = $1 ORDER BY ends_at DESC, id DESC LIMIT 500",
        );
        with_db!(self, |pool| {
            let rows = sqlx::query(AssertSqlSafe(&*sql))
                .bind(workspace_id)
                .fetch_all(pool)
                .await?;
            rows.iter()
                .map(|row| {
                    Ok(SilenceRow {
                        silence: silence_from!(row),
                        author: row.try_get("author")?,
                        created_at: row.try_get("created_at")?,
                    })
                })
                .collect()
        })
    }

    /// The silences in force at `now_ms`.
    pub async fn active_silences(&self, workspace_id: &str, now_ms: i64) -> Result<Vec<Silence>> {
        let sql = self.sql(
            "SELECT id, match_json, starts_at, ends_at, comment FROM silences \
             WHERE workspace_id = $1 AND starts_at <= $2 AND ends_at > $2 ORDER BY id",
        );
        with_db!(self, |pool| {
            let rows = sqlx::query(AssertSqlSafe(&*sql))
                .bind(workspace_id)
                .bind(now_ms)
                .fetch_all(pool)
                .await?;
            rows.iter().map(|row| Ok(silence_from!(row))).collect()
        })
    }

    /// Ends a silence now. One that is already over is left as it is.
    pub async fn end_silence(&self, workspace_id: &str, id: &str, now_ms: i64) -> Result<()> {
        let sql = self.sql(
            "UPDATE silences SET ends_at = CASE WHEN ends_at > $1 THEN $1 ELSE ends_at END \
             WHERE workspace_id = $2 AND id = $3",
        );
        let changed = with_db!(self, |pool| {
            sqlx::query(AssertSqlSafe(&*sql))
                .bind(now_ms)
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

    pub async fn delete_silence(&self, workspace_id: &str, id: &str) -> Result<()> {
        let sql = self.sql("DELETE FROM silences WHERE workspace_id = $1 AND id = $2");
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
