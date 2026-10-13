//! One suite of storage tests, run against SQLite and Postgres.
//!
//! Postgres tests need `RAMZES_TEST_POSTGRES_URL` (see `make test-pg`); without it they print a
//! notice and pass without running.

mod auth;
mod config;
mod deliveries;
mod events;
mod foundation;
mod sources;

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

/// A valid event for `source_id` with the given topic; other fields are fixed.
pub fn event(ws: &str, source_id: &str, topic: &str, received_at: i64) -> ramzes_core::Event {
    ramzes_core::Event {
        id: ramzes_core::new_id(),
        workspace_id: ws.into(),
        source_id: source_id.into(),
        topic: topic.into(),
        severity: ramzes_core::Severity::Info,
        status: ramzes_core::EventStatus::Firing,
        title: format!("{topic} happened"),
        body: None,
        labels: Default::default(),
        payload: serde_json::Value::Null,
        dedup_key: None,
        occurred_at: received_at,
        received_at,
    }
}

/// A workspace with one source; returns `(workspace_id, source_id)`.
pub async fn seed(store: &Store) -> (String, String) {
    let ws = store.ensure_workspace(1).await.unwrap();
    let source = store
        .create_source(
            &ws,
            "orders-api",
            "native",
            &ramzes_core::new_id(),
            "rz_src_abcd",
            1,
        )
        .await
        .unwrap();
    (ws, source.id)
}
