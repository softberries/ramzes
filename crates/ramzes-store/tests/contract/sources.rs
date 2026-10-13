use ramzes_store::{Store, StoreError};

crate::contract!(
    source_crud_and_lookup_by_key_hash,
    rotating_a_key_invalidates_the_old_hash,
    deleting_a_missing_source_is_not_found,
    only_the_newest_twenty_requests_are_kept,
);

async fn source_crud_and_lookup_by_key_hash(store: Store) {
    let ws = store.ensure_workspace(1).await.unwrap();
    let created = store
        .create_source(&ws, "orders-api", "native", "hash-1", "rz_src_abcd", 10)
        .await
        .unwrap();
    assert_eq!(created.name, "orders-api");
    assert_eq!(created.kind, "native");
    assert_eq!(created.key_prefix, "rz_src_abcd");
    assert_eq!(created.created_at, 10);

    assert_eq!(
        store.list_sources(&ws).await.unwrap(),
        vec![created.clone()]
    );
    assert_eq!(
        store.source_by_key_hash("hash-1").await.unwrap(),
        Some(created.clone())
    );
    assert_eq!(store.source_by_key_hash("other").await.unwrap(), None);

    store.delete_source(&ws, &created.id).await.unwrap();
    assert!(store.list_sources(&ws).await.unwrap().is_empty());
    assert_eq!(store.source_by_key_hash("hash-1").await.unwrap(), None);
}

async fn rotating_a_key_invalidates_the_old_hash(store: Store) {
    let ws = store.ensure_workspace(1).await.unwrap();
    let s = store
        .create_source(&ws, "a", "native", "old", "rz_src_old1", 1)
        .await
        .unwrap();
    store
        .rotate_source_key(&ws, &s.id, "new", "rz_src_new1")
        .await
        .unwrap();
    assert_eq!(store.source_by_key_hash("old").await.unwrap(), None);
    let found = store.source_by_key_hash("new").await.unwrap().unwrap();
    assert_eq!(found.id, s.id);
    assert_eq!(found.key_prefix, "rz_src_new1");
}

async fn deleting_a_missing_source_is_not_found(store: Store) {
    let ws = store.ensure_workspace(1).await.unwrap();
    assert!(matches!(
        store.delete_source(&ws, "nope").await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        store.rotate_source_key(&ws, "nope", "h", "p").await,
        Err(StoreError::NotFound)
    ));
}

async fn only_the_newest_twenty_requests_are_kept(store: Store) {
    let (ws, source) = crate::seed(&store).await;
    for i in 0..25 {
        let error = (i % 2 == 0).then_some("bad");
        store
            .record_source_request(&ws, &source, &format!("body {i}"), None, error, i)
            .await
            .unwrap();
    }
    let kept = store.list_source_requests(&ws, &source).await.unwrap();
    assert_eq!(kept.len(), 20);
    assert_eq!(kept[0].body, "body 24");
    assert_eq!(kept[0].error.as_deref(), Some("bad"));
    assert_eq!(kept[1].error, None);
    assert_eq!(kept[19].body, "body 5");
}
