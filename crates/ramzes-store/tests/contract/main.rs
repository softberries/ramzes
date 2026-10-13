//! One suite of storage tests, run against SQLite and Postgres.
//!
//! Postgres tests need `RAMZES_TEST_POSTGRES_URL` (see `make test-pg`); without it they print a
//! notice and pass without running.

mod foundation;

use ramzes_store::Store;
use sqlx::Executor;

pub struct TestStore {
    pub store: Store,
    _dir: Option<tempfile::TempDir>,
}

pub async fn sqlite() -> TestStore {
    let dir = tempfile::tempdir().unwrap();
    let url = format!("sqlite://{}", dir.path().join("ramzes.db").display());
    let store = Store::connect(&url).await.unwrap();
    store.migrate().await.unwrap();
    TestStore {
        store,
        _dir: Some(dir),
    }
}

pub async fn postgres() -> Option<TestStore> {
    let Ok(url) = std::env::var("RAMZES_TEST_POSTGRES_URL") else {
        eprintln!("skipped: RAMZES_TEST_POSTGRES_URL is not set");
        return None;
    };
    let schema = format!("t_{}", ramzes_core::new_id().replace('-', ""));
    let admin = sqlx::PgPool::connect(&url).await.unwrap();
    admin
        .execute(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .await
        .unwrap();
    admin.close().await;
    let sep = if url.contains('?') { '&' } else { '?' };
    let store = Store::connect(&format!("{url}{sep}options=-c%20search_path%3D{schema}"))
        .await
        .unwrap();
    store.migrate().await.unwrap();
    Some(TestStore { store, _dir: None })
}

/// Declares each listed `async fn(Store)` as a test under `sqlite::` and `postgres::`.
#[macro_export]
macro_rules! contract {
    ($($name:ident),* $(,)?) => {
        mod sqlite {
            $(#[tokio::test]
            async fn $name() {
                let t = $crate::sqlite().await;
                super::$name(t.store.clone()).await;
            })*
        }
        mod postgres {
            $(#[tokio::test]
            async fn $name() {
                if let Some(t) = $crate::postgres().await {
                    super::$name(t.store.clone()).await;
                }
            })*
        }
    };
}
