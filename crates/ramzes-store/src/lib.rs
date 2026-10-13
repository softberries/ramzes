//! Storage for ramzes: one interface over SQLite and Postgres.
//!
//! Every query is written once with `$N` placeholders and the column types both databases share
//! (`TEXT` and `BIGINT`). [`with_db!`] expands the same body for each backend.

mod auth;
mod connections;
mod deliveries;
mod events;
mod notifications;
mod routes;
mod settings;
mod sources;
mod sql;

pub use auth::{ApiToken, User};
pub use connections::Connection;
pub use deliveries::{
    Attempt, Delivery, DeliveryState, FailureRow, Finish, NewDispatch, QueueDepth,
};
pub use events::{EventFilter, EventRow, InsertOutcome, Outcome};
pub use notifications::{Journey, JourneyDelivery, JourneyNotification};
pub use sources::{Source, SourceRequest};

use std::borrow::Cow;
use std::str::FromStr;
use std::time::Duration;

use sqlx::migrate::{MigrateError, Migrator};
use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions};

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("not found")]
    NotFound,
    #[error("{0}")]
    Conflict(String),
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("migration failed: {0}")]
    Migrate(String),
    #[error("stored data is invalid: {0}")]
    Data(String),
}

pub type Result<T> = std::result::Result<T, StoreError>;

#[derive(Clone)]
pub(crate) enum Db {
    Sqlite(SqlitePool),
    Postgres(PgPool),
}

#[derive(Clone)]
pub struct Store {
    pub(crate) db: Db,
}

/// Runs `$body` with `$pool` bound to the backend's pool. The body is compiled once per backend.
macro_rules! with_db {
    ($store:expr, |$pool:ident| $body:expr) => {
        match &$store.db {
            $crate::Db::Sqlite($pool) => $body,
            $crate::Db::Postgres($pool) => $body,
        }
    };
}
pub(crate) use with_db;

/// Rewrites `$N` placeholders to SQLite's `?N`.
fn to_sqlite(query: &str) -> String {
    let mut out = String::with_capacity(query.len());
    let mut chars = query.chars().peekable();
    while let Some(c) = chars.next() {
        let is_placeholder = c == '$' && chars.peek().is_some_and(char::is_ascii_digit);
        out.push(if is_placeholder { '?' } else { c });
    }
    out
}

impl Store {
    /// Connects to `sqlite://path` (created when missing) or `postgres://…`.
    pub async fn connect(url: &str) -> Result<Store> {
        let db = if url.starts_with("sqlite:") {
            let options = SqliteConnectOptions::from_str(url)?
                .create_if_missing(true)
                .journal_mode(SqliteJournalMode::Wal)
                .foreign_keys(true)
                .busy_timeout(Duration::from_secs(5));
            Db::Sqlite(
                SqlitePoolOptions::new()
                    .max_connections(8)
                    .connect_with(options)
                    .await?,
            )
        } else if url.starts_with("postgres:") || url.starts_with("postgresql:") {
            Db::Postgres(
                PgPoolOptions::new()
                    .max_connections(16)
                    .connect(url)
                    .await?,
            )
        } else {
            return Err(StoreError::Data(
                "database URL must start with sqlite:// or postgres://".into(),
            ));
        };
        Ok(Store { db })
    }

    /// Applies pending migrations. Fails when the database was migrated by a newer binary.
    pub async fn migrate(&self) -> Result<()> {
        with_db!(self, |pool| MIGRATOR.run(pool).await).map_err(|e| match e {
            MigrateError::VersionMissing(version) => StoreError::Migrate(format!(
                "the database schema is newer than this binary (migration {version} is unknown); upgrade ramzes"
            )),
            other => StoreError::Migrate(other.to_string()),
        })
    }

    pub fn backend(&self) -> &'static str {
        match self.db {
            Db::Sqlite(_) => "sqlite",
            Db::Postgres(_) => "postgres",
        }
    }

    /// The query text for this backend.
    pub(crate) fn sql<'q>(&self, query: &'q str) -> Cow<'q, str> {
        match self.db {
            Db::Sqlite(_) => Cow::Owned(to_sqlite(query)),
            Db::Postgres(_) => Cow::Borrowed(query),
        }
    }

    pub async fn ping(&self) -> Result<()> {
        with_db!(self, |pool| sqlx::query("SELECT 1")
            .execute(pool)
            .await
            .map(|_| ()))?;
        Ok(())
    }

    /// Returns the id of the instance's single workspace, creating it on first use.
    pub async fn ensure_workspace(&self, now_ms: i64) -> Result<String> {
        let select = self.sql("SELECT id FROM workspaces ORDER BY created_at LIMIT 1");
        let insert = self.sql("INSERT INTO workspaces (id, name, created_at) VALUES ($1, $2, $3)");
        with_db!(self, |pool| {
            let existing: Option<(String,)> = sqlx::query_as(sqlx::AssertSqlSafe(&*select))
                .fetch_optional(pool)
                .await?;
            if let Some((id,)) = existing {
                return Ok(id);
            }
            let id = ramzes_core::new_id();
            sqlx::query(sqlx::AssertSqlSafe(&*insert))
                .bind(&id)
                .bind("default")
                .bind(now_ms)
                .execute(pool)
                .await?;
            Ok(id)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::to_sqlite;

    #[test]
    fn placeholders_are_rewritten_for_sqlite() {
        assert_eq!(
            to_sqlite("SELECT $1, $2 WHERE a = $12 AND b = $1"),
            "SELECT ?1, ?2 WHERE a = ?12 AND b = ?1"
        );
    }

    #[test]
    fn text_without_placeholders_is_unchanged() {
        assert_eq!(to_sqlite("SELECT 'a' FROM t"), "SELECT 'a' FROM t");
    }

    #[test]
    fn a_dollar_not_followed_by_a_digit_is_kept() {
        assert_eq!(to_sqlite("SELECT '$' || $1"), "SELECT '$' || ?1");
    }
}
