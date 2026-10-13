use serde::Serialize;
use sqlx::{AssertSqlSafe, Row};

use crate::events::is_unique_violation;
use crate::sql::BeginWrite;
use crate::{Result, Store, StoreError, with_db};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub id: String,
    pub workspace_id: String,
    pub email: String,
    pub password_hash: String,
    pub created_at: i64,
}

/// An admin API token. The token itself is never stored, only its hash and a display prefix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct ApiToken {
    pub id: String,
    pub name: String,
    pub token_prefix: String,
    pub created_by: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
}

const USER_COLS: &str = "id, workspace_id, email, password_hash, created_at";
const INSERT_USER: &str = "INSERT INTO users (id, workspace_id, email, password_hash, created_at) VALUES ($1, $2, $3, $4, $5)";

macro_rules! user_from {
    ($row:expr) => {
        User {
            id: $row.try_get("id")?,
            workspace_id: $row.try_get("workspace_id")?,
            email: $row.try_get("email")?,
            password_hash: $row.try_get("password_hash")?,
            created_at: $row.try_get("created_at")?,
        }
    };
}

fn email_conflict(e: sqlx::Error) -> StoreError {
    if is_unique_violation(&e) {
        StoreError::Conflict("a user with this email already exists".into())
    } else {
        e.into()
    }
}

impl Store {
    pub async fn count_users(&self) -> Result<i64> {
        let (count,): (i64,) = with_db!(self, |pool| sqlx::query_as("SELECT COUNT(*) FROM users")
            .fetch_one(pool)
            .await)?;
        Ok(count)
    }

    pub async fn create_user(&self, u: &User) -> Result<()> {
        let sql = self.sql(INSERT_USER);
        with_db!(self, |pool| {
            sqlx::query(AssertSqlSafe(&*sql))
                .bind(&u.id)
                .bind(&u.workspace_id)
                .bind(&u.email)
                .bind(&u.password_hash)
                .bind(u.created_at)
                .execute(pool)
                .await
                .map(|_| ())
        })
        .map_err(email_conflict)
    }

    /// Creates the first user. Fails with a conflict once any user exists.
    pub async fn create_first_user(&self, u: &User) -> Result<()> {
        let sql = self.sql(INSERT_USER);
        with_db!(self, |pool| {
            let mut tx = pool.begin_write().await?;
            let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
                .fetch_one(&mut *tx)
                .await?;
            if count > 0 {
                return Err(StoreError::Conflict(
                    "setup has already been completed".into(),
                ));
            }
            sqlx::query(AssertSqlSafe(&*sql))
                .bind(&u.id)
                .bind(&u.workspace_id)
                .bind(&u.email)
                .bind(&u.password_hash)
                .bind(u.created_at)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            Ok(())
        })
    }

    pub async fn user_by_email(&self, email: &str) -> Result<Option<User>> {
        let sql = self
            .sql(&format!("SELECT {USER_COLS} FROM users WHERE email = $1"))
            .into_owned();
        with_db!(self, |pool| {
            let row = sqlx::query(AssertSqlSafe(sql.as_str()))
                .bind(email)
                .fetch_optional(pool)
                .await?;
            row.map(|row| Ok(user_from!(row))).transpose()
        })
    }

    pub async fn list_users(&self, workspace_id: &str) -> Result<Vec<User>> {
        let sql = self
            .sql(&format!(
                "SELECT {USER_COLS} FROM users WHERE workspace_id = $1 ORDER BY id"
            ))
            .into_owned();
        with_db!(self, |pool| {
            let rows = sqlx::query(AssertSqlSafe(sql.as_str()))
                .bind(workspace_id)
                .fetch_all(pool)
                .await?;
            rows.iter().map(|row| Ok(user_from!(row))).collect()
        })
    }

    /// Deletes a user and their sessions. The last remaining user cannot be deleted.
    pub async fn delete_user(&self, workspace_id: &str, id: &str) -> Result<()> {
        let exists = self.sql("SELECT COUNT(*) FROM users WHERE workspace_id = $1 AND id = $2");
        let sessions = self.sql("DELETE FROM sessions WHERE user_id = $1");
        let user = self.sql("DELETE FROM users WHERE workspace_id = $1 AND id = $2");
        with_db!(self, |pool| {
            let mut tx = pool.begin_write().await?;
            let (found,): (i64,) = sqlx::query_as(AssertSqlSafe(&*exists))
                .bind(workspace_id)
                .bind(id)
                .fetch_one(&mut *tx)
                .await?;
            if found == 0 {
                return Err(StoreError::NotFound);
            }
            let (total,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
                .fetch_one(&mut *tx)
                .await?;
            if total <= 1 {
                return Err(StoreError::Conflict(
                    "the last user cannot be deleted".into(),
                ));
            }
            sqlx::query(AssertSqlSafe(&*sessions))
                .bind(id)
                .execute(&mut *tx)
                .await?;
            sqlx::query(AssertSqlSafe(&*user))
                .bind(workspace_id)
                .bind(id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            Ok(())
        })
    }

    pub async fn create_session(
        &self,
        token_hash: &str,
        workspace_id: &str,
        user_id: &str,
        now_ms: i64,
        expires_at: i64,
    ) -> Result<()> {
        let sql = self.sql(
            "INSERT INTO sessions (token_hash, workspace_id, user_id, created_at, expires_at) VALUES ($1, $2, $3, $4, $5)",
        );
        with_db!(self, |pool| {
            sqlx::query(AssertSqlSafe(&*sql))
                .bind(token_hash)
                .bind(workspace_id)
                .bind(user_id)
                .bind(now_ms)
                .bind(expires_at)
                .execute(pool)
                .await
                .map(|_| ())
        })?;
        Ok(())
    }

    /// The user a session belongs to, unless it has expired.
    pub async fn session_user(&self, token_hash: &str, now_ms: i64) -> Result<Option<User>> {
        let sql = self.sql(
            "SELECT u.id, u.workspace_id, u.email, u.password_hash, u.created_at FROM sessions s \
             JOIN users u ON u.id = s.user_id WHERE s.token_hash = $1 AND s.expires_at > $2",
        );
        with_db!(self, |pool| {
            let row = sqlx::query(AssertSqlSafe(&*sql))
                .bind(token_hash)
                .bind(now_ms)
                .fetch_optional(pool)
                .await?;
            row.map(|row| Ok(user_from!(row))).transpose()
        })
    }

    pub async fn delete_session(&self, token_hash: &str) -> Result<()> {
        let sql = self.sql("DELETE FROM sessions WHERE token_hash = $1");
        with_db!(self, |pool| sqlx::query(AssertSqlSafe(&*sql))
            .bind(token_hash)
            .execute(pool)
            .await
            .map(|_| ()))?;
        Ok(())
    }

    pub async fn create_api_token(
        &self,
        workspace_id: &str,
        t: &ApiToken,
        token_hash: &str,
    ) -> Result<()> {
        let sql = self.sql(
            "INSERT INTO api_tokens (id, workspace_id, name, token_hash, token_prefix, created_by, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        );
        with_db!(self, |pool| {
            sqlx::query(AssertSqlSafe(&*sql))
                .bind(&t.id)
                .bind(workspace_id)
                .bind(&t.name)
                .bind(token_hash)
                .bind(&t.token_prefix)
                .bind(&t.created_by)
                .bind(t.created_at)
                .execute(pool)
                .await
                .map(|_| ())
        })?;
        Ok(())
    }

    /// The workspace a token grants access to. Records the time of use.
    pub async fn api_token_workspace(
        &self,
        token_hash: &str,
        now_ms: i64,
    ) -> Result<Option<String>> {
        let sql = self.sql(
            "UPDATE api_tokens SET last_used_at = $1 WHERE token_hash = $2 RETURNING workspace_id",
        );
        let row: Option<(String,)> = with_db!(self, |pool| {
            sqlx::query_as(AssertSqlSafe(&*sql))
                .bind(now_ms)
                .bind(token_hash)
                .fetch_optional(pool)
                .await
        })?;
        Ok(row.map(|(workspace_id,)| workspace_id))
    }

    pub async fn list_api_tokens(&self, workspace_id: &str) -> Result<Vec<ApiToken>> {
        let sql = self.sql(
            "SELECT id, name, token_prefix, created_by, created_at, last_used_at FROM api_tokens \
             WHERE workspace_id = $1 ORDER BY id",
        );
        with_db!(self, |pool| {
            let rows = sqlx::query(AssertSqlSafe(&*sql))
                .bind(workspace_id)
                .fetch_all(pool)
                .await?;
            rows.iter()
                .map(|row| {
                    Ok(ApiToken {
                        id: row.try_get("id")?,
                        name: row.try_get("name")?,
                        token_prefix: row.try_get("token_prefix")?,
                        created_by: row.try_get("created_by")?,
                        created_at: row.try_get("created_at")?,
                        last_used_at: row.try_get("last_used_at")?,
                    })
                })
                .collect()
        })
    }

    pub async fn delete_api_token(&self, workspace_id: &str, id: &str) -> Result<()> {
        let sql = self.sql("DELETE FROM api_tokens WHERE workspace_id = $1 AND id = $2");
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
