use crate::common::{Api, EMAIL, PASSWORD};
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use serde_json::json;

#[tokio::test]
async fn setup_is_needed_until_the_first_user_exists() {
    let api = Api::new().await;
    assert_eq!(
        api.get("/api/setup").await,
        (StatusCode::OK, json!({"needs_setup": true}))
    );

    let (status, body) = api
        .post(
            "/api/setup",
            json!({"email": " Kris@Example.com ", "password": PASSWORD}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["email"], json!(EMAIL));
    assert_eq!(api.get("/api/setup").await.1, json!({"needs_setup": false}));

    // Setup signs the new admin in.
    let (status, me) = api.get("/api/auth/me").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["email"], json!(EMAIL));
}

#[tokio::test]
async fn setup_can_only_be_done_once() {
    let api = Api::signed_in().await;
    let (status, body) = api
        .post(
            "/api/setup",
            json!({"email": "other@example.com", "password": PASSWORD}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], json!("conflict"));
}

#[tokio::test]
async fn setup_validates_email_and_password() {
    let api = Api::new().await;
    let (status, body) = api
        .post("/api/setup", json!({"email": EMAIL, "password": "short"}))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"], json!("invalid"));
    assert!(body["message"].as_str().unwrap().contains("10"));
    let (status, _) = api
        .post(
            "/api/setup",
            json!({"email": "not-an-email", "password": PASSWORD}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(api.get("/api/setup").await.1, json!({"needs_setup": true}));
}

#[tokio::test]
async fn the_session_cookie_is_http_only_and_same_site() {
    let api = Api::new().await;
    let request = Request::post("/api/setup")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({"email": EMAIL, "password": PASSWORD}).to_string(),
        ))
        .unwrap();
    let response = tower::ServiceExt::oneshot(api.app.clone(), request)
        .await
        .unwrap();
    let cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    assert!(cookie.starts_with("ramzes_session="), "{cookie}");
    for attribute in ["HttpOnly", "SameSite=Lax", "Path=/", "Max-Age=2592000"] {
        assert!(cookie.contains(attribute), "{cookie} lacks {attribute}");
    }
    assert!(!cookie.contains("Secure"));
}

#[tokio::test]
async fn login_works_and_wrong_credentials_look_the_same() {
    let api = Api::signed_in().await;
    api.forget_cookie();

    let wrong_password = api
        .post(
            "/api/auth/login",
            json!({"email": EMAIL, "password": "wrong password!"}),
        )
        .await;
    let unknown_email = api
        .post(
            "/api/auth/login",
            json!({"email": "nobody@example.com", "password": PASSWORD}),
        )
        .await;
    assert_eq!(wrong_password.0, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong_password, unknown_email);
    assert!(api.cookie().is_none());

    let (status, body) = api
        .post(
            "/api/auth/login",
            json!({"email": "KRIS@example.com", "password": PASSWORD}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["email"], json!(EMAIL));
    assert_eq!(api.get("/api/auth/me").await.0, StatusCode::OK);
}

#[tokio::test]
async fn me_requires_a_session() {
    let api = Api::new().await;
    let (status, body) = api.get("/api/auth/me").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], json!("unauthorized"));
}

#[tokio::test]
async fn logout_ends_the_session() {
    let api = Api::signed_in().await;
    let old_cookie = api.cookie().unwrap();
    assert_eq!(
        api.post("/api/auth/logout", json!({})).await.0,
        StatusCode::NO_CONTENT
    );
    assert!(api.cookie().is_none());

    // The old cookie no longer works even if a client kept it.
    let request = Request::get("/api/auth/me")
        .header(header::COOKIE, old_cookie)
        .body(Body::empty())
        .unwrap();
    assert_eq!(api.send(request).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn an_expired_session_is_rejected() {
    let api = Api::signed_in().await;
    api.clock
        .0
        .fetch_add(31 * 86_400_000, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(api.get("/api/auth/me").await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_cookie_request_that_changes_state_needs_the_request_header() {
    let api = Api::signed_in().await;
    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/auth/logout")
        .header(header::COOKIE, api.cookie().unwrap())
        .body(Body::empty())
        .unwrap();
    let (status, body) = api.send(request).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"], json!("missing_request_header"));
    // The session is still valid.
    assert_eq!(api.get("/api/auth/me").await.0, StatusCode::OK);
}

#[tokio::test]
async fn repeated_wrong_passwords_lock_sign_in_for_that_email_for_a_while() {
    use std::sync::atomic::Ordering;
    let api = Api::signed_in().await;
    api.forget_cookie();
    let attempt = |password: &'static str| {
        api.post(
            "/api/auth/login",
            json!({"email": EMAIL, "password": password}),
        )
    };
    for _ in 0..10 {
        assert_eq!(attempt("wrong password").await.0, StatusCode::UNAUTHORIZED);
    }
    // Even the right password is refused now, with a hint of how long to wait.
    let (status, body) = attempt(PASSWORD).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert!(
        body["message"].as_str().unwrap().contains("minutes"),
        "{body}"
    );
    // Another email is not affected.
    let (status, _) = api
        .post(
            "/api/auth/login",
            json!({"email": "other@example.com", "password": "x"}),
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    api.clock.0.fetch_add(15 * 60_000 + 1, Ordering::SeqCst);
    assert_eq!(attempt(PASSWORD).await.0, StatusCode::OK);
    // A success clears the count.
    api.forget_cookie();
    for _ in 0..9 {
        attempt("wrong password").await;
    }
    assert_eq!(attempt(PASSWORD).await.0, StatusCode::OK);
}
