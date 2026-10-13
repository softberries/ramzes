use ramzes_store::Store;

crate::contract!(
    migrate_twice_is_a_no_op,
    ensure_workspace_is_stable,
    settings_round_trip_and_overwrite,
    missing_setting_is_none,
    ping_succeeds,
    backend_is_named,
);

async fn migrate_twice_is_a_no_op(store: Store) {
    store.migrate().await.unwrap();
    store.migrate().await.unwrap();
}

async fn ensure_workspace_is_stable(store: Store) {
    let first = store.ensure_workspace(1000).await.unwrap();
    let second = store.ensure_workspace(2000).await.unwrap();
    assert_eq!(first, second);
    assert!(!first.is_empty());
}

async fn settings_round_trip_and_overwrite(store: Store) {
    let ws = store.ensure_workspace(1).await.unwrap();
    store
        .set_setting(&ws, "retention_days", "30")
        .await
        .unwrap();
    assert_eq!(
        store
            .get_setting(&ws, "retention_days")
            .await
            .unwrap()
            .as_deref(),
        Some("30")
    );
    store.set_setting(&ws, "retention_days", "7").await.unwrap();
    assert_eq!(
        store
            .get_setting(&ws, "retention_days")
            .await
            .unwrap()
            .as_deref(),
        Some("7")
    );
}

async fn missing_setting_is_none(store: Store) {
    let ws = store.ensure_workspace(1).await.unwrap();
    assert_eq!(store.get_setting(&ws, "nope").await.unwrap(), None);
}

async fn ping_succeeds(store: Store) {
    store.ping().await.unwrap();
}

async fn backend_is_named(store: Store) {
    assert!(matches!(store.backend(), "sqlite" | "postgres"));
}

#[tokio::test]
async fn a_database_newer_than_the_binary_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let url = format!("sqlite://{}", dir.path().join("ramzes.db").display());
    let store = Store::connect(&url).await.unwrap();
    store.migrate().await.unwrap();
    let raw = sqlx::SqlitePool::connect(&url).await.unwrap();
    sqlx::query(
        "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) \
         VALUES (99990101000000, 'from the future', 1, x'00', 0)",
    )
    .execute(&raw)
    .await
    .unwrap();
    let err = store.migrate().await.unwrap_err();
    assert!(err.to_string().contains("newer"), "unexpected error: {err}");
}
