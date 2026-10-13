use std::sync::OnceLock;

use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use axum::Json;
use axum::extract::{FromRequestParts, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use ramzes_core::{generate_key, hash_key, new_id};
use ramzes_store::User;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::ErrorBody;
use crate::{ApiError, AppState};

const SESSION_COOKIE: &str = "ramzes_session";
const SESSION_SECONDS: i64 = 30 * 86_400;
const PASSWORD_MIN: usize = 10;
const EMAIL_MAX: usize = 254;
/// Cookie-authenticated requests that change state must carry this header, which a form on
/// another site cannot add.
const REQUEST_HEADER: &str = "x-ramzes-request";

/// Who is making an admin request.
pub struct Auth {
    pub workspace_id: String,
    /// The signed-in user; `None` when an API token was used.
    pub user: Option<User>,
}

fn session_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|pair| {
            let (name, value) = pair.trim().split_once('=')?;
            (name == SESSION_COOKIE && !value.is_empty()).then_some(value)
        })
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::trim)
}

impl FromRequestParts<AppState> for Auth {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Auth, ApiError> {
        let now = state.clock.now_ms();
        if let Some(token) = bearer_token(&parts.headers) {
            let workspace_id = state
                .store
                .api_token_workspace(&hash_key(token), now)
                .await?
                .ok_or_else(ApiError::unauthorized)?;
            return Ok(Auth {
                workspace_id,
                user: None,
            });
        }
        let token = session_token(&parts.headers).ok_or_else(ApiError::unauthorized)?;
        let user = state
            .store
            .session_user(&hash_key(token), now)
            .await?
            .ok_or_else(ApiError::unauthorized)?;
        let changes_state = !matches!(parts.method, Method::GET | Method::HEAD | Method::OPTIONS);
        if changes_state && !parts.headers.contains_key(REQUEST_HEADER) {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "missing_request_header",
                "requests that change state must carry the header X-Ramzes-Request: 1",
            ));
        }
        Ok(Auth {
            workspace_id: user.workspace_id.clone(),
            user: Some(user),
        })
    }
}

fn cookie(state: &AppState, value: &str, max_age: i64) -> HeaderValue {
    let secure = if state.secure_cookies { "; Secure" } else { "" };
    let text = format!(
        "{SESSION_COOKIE}={value}; HttpOnly; SameSite=Lax; Path=/; Max-Age={max_age}{secure}"
    );
    HeaderValue::from_str(&text).expect("the cookie is built from ASCII parts")
}

pub(crate) async fn hash_password(password: String) -> Result<String, ApiError> {
    tokio::task::spawn_blocking(move || {
        Argon2::default()
            .hash_password(password.as_bytes())
            .map(|hash| hash.to_string())
    })
    .await
    .map_err(|e| ApiError::internal("password hashing task failed", e))?
    .map_err(|e| ApiError::internal("password hashing failed", e))
}

/// A hash to verify against when the email is unknown, so both failures take the same time.
fn dummy_hash() -> &'static str {
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| {
        Argon2::default()
            .hash_password(b"no user has this password")
            .map(|hash| hash.to_string())
            .expect("hashing a constant cannot fail")
    })
}

/// Checks a password against a stored hash. With no hash (unknown email) it still does the
/// work of a check and then fails.
async fn verify_password(hash: Option<String>, password: String) -> bool {
    tokio::task::spawn_blocking(move || {
        let known = hash.is_some();
        let hash = hash.unwrap_or_else(|| dummy_hash().to_owned());
        let matches = PasswordHash::new(&hash).is_ok_and(|parsed| {
            Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok()
        });
        known && matches
    })
    .await
    .unwrap_or(false)
}

pub(crate) fn normalize_email(email: &str) -> Result<String, ApiError> {
    let email = email.trim().to_lowercase();
    let well_formed = email
        .split_once('@')
        .is_some_and(|(local, domain)| !local.is_empty() && domain.contains('.'));
    if !well_formed || email.len() > EMAIL_MAX || email.contains(char::is_whitespace) {
        return Err(ApiError::invalid("enter a valid email address"));
    }
    Ok(email)
}

pub(crate) fn check_password(password: &str) -> Result<(), ApiError> {
    if password.chars().count() < PASSWORD_MIN {
        return Err(ApiError::invalid(format!(
            "the password must be at least {PASSWORD_MIN} characters"
        )));
    }
    Ok(())
}

#[derive(Serialize, ToSchema)]
pub(crate) struct SetupStatus {
    /// True until the first user has been created.
    needs_setup: bool,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct Credentials {
    email: String,
    password: String,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct Me {
    /// Absent when the request used an API token.
    id: Option<String>,
    email: Option<String>,
}

async fn start_session(state: &AppState, user: &User) -> Result<Response, ApiError> {
    let (token, _) = generate_key("");
    let now = state.clock.now_ms();
    state
        .store
        .create_session(
            &hash_key(&token),
            &user.workspace_id,
            &user.id,
            now,
            now + SESSION_SECONDS * 1000,
        )
        .await?;
    let me = Me {
        id: Some(user.id.clone()),
        email: Some(user.email.clone()),
    };
    Ok((
        [(header::SET_COOKIE, cookie(state, &token, SESSION_SECONDS))],
        Json(me),
    )
        .into_response())
}

/// Whether the instance still needs its first user.
#[utoipa::path(get, path = "/api/setup", tag = "auth", responses((status = 200, body = SetupStatus)))]
pub(crate) async fn setup_status(
    State(state): State<AppState>,
) -> Result<Json<SetupStatus>, ApiError> {
    Ok(Json(SetupStatus {
        needs_setup: state.store.count_users().await? == 0,
    }))
}

/// Create the first user and sign them in. Works only while no user exists.
#[utoipa::path(
    post, path = "/api/setup", tag = "auth", request_body = Credentials,
    responses((status = 200, body = Me), (status = 409, body = ErrorBody), (status = 422, body = ErrorBody))
)]
pub(crate) async fn setup(
    State(state): State<AppState>,
    Json(body): Json<Credentials>,
) -> Result<Response, ApiError> {
    let email = normalize_email(&body.email)?;
    check_password(&body.password)?;
    let user = User {
        id: new_id(),
        workspace_id: state.workspace_id.clone(),
        email,
        password_hash: hash_password(body.password).await?,
        created_at: state.clock.now_ms(),
    };
    state.store.create_first_user(&user).await?;
    start_session(&state, &user).await
}

/// Sign in with email and password.
#[utoipa::path(
    post, path = "/api/auth/login", tag = "auth", request_body = Credentials,
    responses((status = 200, body = Me), (status = 401, body = ErrorBody))
)]
pub(crate) async fn login(
    State(state): State<AppState>,
    Json(body): Json<Credentials>,
) -> Result<Response, ApiError> {
    let user = state
        .store
        .user_by_email(&body.email.trim().to_lowercase())
        .await?;
    let hash = user.as_ref().map(|u| u.password_hash.clone());
    let verified = verify_password(hash, body.password).await;
    match user {
        Some(user) if verified => start_session(&state, &user).await,
        _ => Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "wrong email or password",
        )),
    }
}

/// End the current session.
#[utoipa::path(post, path = "/api/auth/logout", tag = "auth", responses((status = 204)))]
pub(crate) async fn logout(
    State(state): State<AppState>,
    _auth: Auth,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    if let Some(token) = session_token(&headers) {
        state.store.delete_session(&hash_key(token)).await?;
    }
    Ok((
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, cookie(&state, "", 0))],
    )
        .into_response())
}

/// The signed-in user.
#[utoipa::path(
    get, path = "/api/auth/me", tag = "auth",
    responses((status = 200, body = Me), (status = 401, body = ErrorBody))
)]
pub(crate) async fn me(auth: Auth) -> Json<Me> {
    let (id, email) = auth.user.map(|u| (u.id, u.email)).unzip();
    Json(Me { id, email })
}
