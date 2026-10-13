use ramzes_store::{ApiToken, Store, StoreError, User};

crate::contract!(
    create_first_user_succeeds_only_once,
    duplicate_email_conflicts,
    users_are_listed_and_found_by_email,
    the_last_user_cannot_be_deleted,
    a_session_resolves_to_its_user_until_it_expires,
    deleting_a_session_logs_it_out,
    deleting_a_user_ends_their_sessions,
    an_api_token_resolves_and_records_use,
    a_deleted_api_token_stops_working,
);

fn user(ws: &str, email: &str) -> User {
    User {
        id: ramzes_core::new_id(),
        workspace_id: ws.into(),
        email: email.into(),
        password_hash: "argon2-hash".into(),
        created_at: 5,
    }
}

async fn create_first_user_succeeds_only_once(store: Store) {
    let ws = store.ensure_workspace(1).await.unwrap();
    assert_eq!(store.count_users().await.unwrap(), 0);
    store
        .create_first_user(&user(&ws, "a@example.com"))
        .await
        .unwrap();
    assert_eq!(store.count_users().await.unwrap(), 1);
    let second = store.create_first_user(&user(&ws, "b@example.com")).await;
    assert!(matches!(second, Err(StoreError::Conflict(_))));
    assert_eq!(store.count_users().await.unwrap(), 1);
}

async fn duplicate_email_conflicts(store: Store) {
    let ws = store.ensure_workspace(1).await.unwrap();
    store
        .create_user(&user(&ws, "a@example.com"))
        .await
        .unwrap();
    assert!(matches!(
        store.create_user(&user(&ws, "a@example.com")).await,
        Err(StoreError::Conflict(_))
    ));
}

async fn users_are_listed_and_found_by_email(store: Store) {
    let ws = store.ensure_workspace(1).await.unwrap();
    let a = user(&ws, "a@example.com");
    let b = user(&ws, "b@example.com");
    store.create_user(&a).await.unwrap();
    store.create_user(&b).await.unwrap();
    assert_eq!(store.list_users(&ws).await.unwrap(), vec![a.clone(), b]);
    assert_eq!(store.user_by_email("a@example.com").await.unwrap(), Some(a));
    assert_eq!(
        store.user_by_email("nobody@example.com").await.unwrap(),
        None
    );
}

async fn the_last_user_cannot_be_deleted(store: Store) {
    let ws = store.ensure_workspace(1).await.unwrap();
    let a = user(&ws, "a@example.com");
    let b = user(&ws, "b@example.com");
    store.create_user(&a).await.unwrap();
    store.create_user(&b).await.unwrap();
    store.delete_user(&ws, &a.id).await.unwrap();
    assert!(matches!(
        store.delete_user(&ws, &b.id).await,
        Err(StoreError::Conflict(_))
    ));
    assert!(matches!(
        store.delete_user(&ws, "nope").await,
        Err(StoreError::NotFound)
    ));
    assert_eq!(store.count_users().await.unwrap(), 1);
}

async fn a_session_resolves_to_its_user_until_it_expires(store: Store) {
    let ws = store.ensure_workspace(1).await.unwrap();
    let a = user(&ws, "a@example.com");
    store.create_user(&a).await.unwrap();
    store
        .create_session("hash-1", &ws, &a.id, 100, 1000)
        .await
        .unwrap();
    assert_eq!(store.session_user("hash-1", 999).await.unwrap(), Some(a));
    assert_eq!(store.session_user("hash-1", 1000).await.unwrap(), None);
    assert_eq!(store.session_user("other", 500).await.unwrap(), None);
}

async fn deleting_a_session_logs_it_out(store: Store) {
    let ws = store.ensure_workspace(1).await.unwrap();
    let a = user(&ws, "a@example.com");
    store.create_user(&a).await.unwrap();
    store
        .create_session("hash-1", &ws, &a.id, 100, 1000)
        .await
        .unwrap();
    store.delete_session("hash-1").await.unwrap();
    assert_eq!(store.session_user("hash-1", 500).await.unwrap(), None);
}

async fn deleting_a_user_ends_their_sessions(store: Store) {
    let ws = store.ensure_workspace(1).await.unwrap();
    let a = user(&ws, "a@example.com");
    store.create_user(&a).await.unwrap();
    store
        .create_user(&user(&ws, "b@example.com"))
        .await
        .unwrap();
    store
        .create_session("hash-1", &ws, &a.id, 100, 1000)
        .await
        .unwrap();
    store.delete_user(&ws, &a.id).await.unwrap();
    assert_eq!(store.session_user("hash-1", 500).await.unwrap(), None);
}

async fn an_api_token_resolves_and_records_use(store: Store) {
    let ws = store.ensure_workspace(1).await.unwrap();
    let token = ApiToken {
        id: ramzes_core::new_id(),
        name: "ci".into(),
        token_prefix: "rz_tok_abcd".into(),
        created_by: "user-1".into(),
        created_at: 10,
        last_used_at: None,
    };
    store
        .create_api_token(&ws, &token, "token-hash")
        .await
        .unwrap();
    assert_eq!(
        store.list_api_tokens(&ws).await.unwrap(),
        vec![token.clone()]
    );

    assert_eq!(
        store.api_token_workspace("token-hash", 77).await.unwrap(),
        Some(ws.clone())
    );
    assert_eq!(store.api_token_workspace("wrong", 78).await.unwrap(), None);
    assert_eq!(
        store.list_api_tokens(&ws).await.unwrap()[0].last_used_at,
        Some(77)
    );
}

async fn a_deleted_api_token_stops_working(store: Store) {
    let ws = store.ensure_workspace(1).await.unwrap();
    let token = ApiToken {
        id: ramzes_core::new_id(),
        name: "ci".into(),
        token_prefix: "rz_tok_abcd".into(),
        created_by: "user-1".into(),
        created_at: 10,
        last_used_at: None,
    };
    store
        .create_api_token(&ws, &token, "token-hash")
        .await
        .unwrap();
    store.delete_api_token(&ws, &token.id).await.unwrap();
    assert_eq!(
        store.api_token_workspace("token-hash", 77).await.unwrap(),
        None
    );
    assert!(matches!(
        store.delete_api_token(&ws, &token.id).await,
        Err(StoreError::NotFound)
    ));
}
