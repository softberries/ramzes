//! Helpers for the few queries whose text depends on their arguments.

use sqlx::postgres::PgPool;
use sqlx::sqlite::SqlitePool;

pub(crate) enum Arg {
    Text(String),
    Int(i64),
}

/// A query under construction: text plus the arguments its `$N` placeholders refer to.
#[derive(Default)]
pub(crate) struct Sql {
    pub text: String,
    pub args: Vec<Arg>,
}

impl Sql {
    pub fn new(text: &str) -> Sql {
        Sql {
            text: text.to_owned(),
            args: Vec::new(),
        }
    }

    pub fn push(&mut self, text: &str) -> &mut Sql {
        self.text.push_str(text);
        self
    }

    /// Appends a placeholder for `value`.
    pub fn text(&mut self, value: impl Into<String>) -> &mut Sql {
        self.args.push(Arg::Text(value.into()));
        self.text.push_str(&format!("${}", self.args.len()));
        self
    }

    pub fn int(&mut self, value: i64) -> &mut Sql {
        self.args.push(Arg::Int(value));
        self.text.push_str(&format!("${}", self.args.len()));
        self
    }

    /// Appends `($a, $b, …)` with one placeholder per value.
    pub fn list(&mut self, values: &[String]) -> &mut Sql {
        self.text.push('(');
        for (i, value) in values.iter().enumerate() {
            if i > 0 {
                self.text.push_str(", ");
            }
            self.text(value.as_str());
        }
        self.text.push(')');
        self
    }
}

/// Binds every argument of a [`Sql`] to a query, in order.
macro_rules! bind_args {
    ($query:expr, $args:expr) => {{
        let mut query = $query;
        for arg in $args.iter() {
            query = match arg {
                $crate::sql::Arg::Text(s) => query.bind(s.as_str()),
                $crate::sql::Arg::Int(i) => query.bind(*i),
            };
        }
        query
    }};
}
pub(crate) use bind_args;

/// Escapes `%`, `_` and `\` so the text matches literally in `LIKE … ESCAPE '\'`.
pub(crate) fn like_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Starts a transaction that will write. On SQLite it takes the write lock up front, so a
/// transaction that reads before writing cannot fail with a stale snapshot.
pub(crate) trait BeginWrite {
    type Db: sqlx::Database;
    async fn begin_write(&self) -> sqlx::Result<sqlx::Transaction<'static, Self::Db>>;
}

impl BeginWrite for SqlitePool {
    type Db = sqlx::Sqlite;
    async fn begin_write(&self) -> sqlx::Result<sqlx::Transaction<'static, sqlx::Sqlite>> {
        self.begin_with("BEGIN IMMEDIATE").await
    }
}

impl BeginWrite for PgPool {
    type Db = sqlx::Postgres;
    async fn begin_write(&self) -> sqlx::Result<sqlx::Transaction<'static, sqlx::Postgres>> {
        self.begin().await
    }
}

#[cfg(test)]
mod tests {
    use super::{Sql, like_escape};

    #[test]
    fn like_escape_escapes_wildcards_and_backslash() {
        assert_eq!(like_escape(r"50%_a\b"), r"50\%\_a\\b");
    }

    #[test]
    fn placeholders_are_numbered_in_order() {
        let mut sql = Sql::new("SELECT 1 WHERE a = ");
        sql.text("x")
            .push(" AND b = ")
            .int(2)
            .push(" AND c IN ")
            .list(&["p".into(), "q".into()]);
        assert_eq!(
            sql.text,
            "SELECT 1 WHERE a = $1 AND b = $2 AND c IN ($3, $4)"
        );
        assert_eq!(sql.args.len(), 4);
    }
}
