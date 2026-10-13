use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use ramzes_core::{generate_key, hash_key, new_id};
use ramzes_store::{ApiToken, User};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::auth::{check_password, hash_password, normalize_email};
use crate::error::ErrorBody;
use crate::sources::clean_name;
use crate::{ApiError, AppState, Auth};

const TOKEN_PREFIX: &str = "rz_tok_";
const DEFAULT_RETENTION_DAYS: i64 = 30;
const MAX_RETENTION_DAYS: i64 = 3650;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub(crate) struct Settings {
    /// Days events are kept. Zero keeps them forever.
    retention_days: i64,
    /// Days per topic, overriding `retention_days`. Zero keeps that topic forever.
    retention_overrides: BTreeMap<String, i64>,
    /// Address of this ramzes instance, used for links in messages.
    public_url: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct UserView {
    id: String,
    email: String,
    created_at: i64,
}

impl From<User> for UserView {
    fn from(u: User) -> Self {
        UserView {
            id: u.id,
            email: u.email,
            created_at: u.created_at,
        }
    }
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct NewUser {
    email: String,
    password: String,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct NewToken {
    name: String,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct TokenCreated {
    #[serde(flatten)]
    token: ApiToken,
    /// The token. Shown only in this response.
    value: String,
}

async fn load(state: &AppState, workspace_id: &str) -> Result<Settings, ApiError> {
    let store = &state.store;
    let retention_days = match store.get_setting(workspace_id, "retention_days").await? {
        Some(value) => value.parse().unwrap_or(DEFAULT_RETENTION_DAYS),
        None => DEFAULT_RETENTION_DAYS,
    };
    let retention_overrides = match store
        .get_setting(workspace_id, "retention_overrides")
        .await?
    {
        Some(value) => serde_json::from_str(&value).unwrap_or_default(),
        None => BTreeMap::new(),
    };
    let public_url = store
        .get_setting(workspace_id, "public_url")
        .await?
        .filter(|url| !url.is_empty());
    Ok(Settings {
        retention_days,
        retention_overrides,
        public_url,
    })
}

/// Instance settings.
#[utoipa::path(get, path = "/api/settings", tag = "settings", responses((status = 200, body = Settings)))]
pub(crate) async fn get(
    State(state): State<AppState>,
    auth: Auth,
) -> Result<Json<Settings>, ApiError> {
    Ok(Json(load(&state, &auth.workspace_id).await?))
}

/// Replace the instance settings.
#[utoipa::path(
    put, path = "/api/settings", tag = "settings", request_body = Settings,
    responses((status = 200, body = Settings), (status = 422, body = ErrorBody))
)]
pub(crate) async fn put(
    State(state): State<AppState>,
    auth: Auth,
    Json(body): Json<Settings>,
) -> Result<Json<Settings>, ApiError> {
    let days_ok = |days: i64| (0..=MAX_RETENTION_DAYS).contains(&days);
    if !days_ok(body.retention_days) || !body.retention_overrides.values().copied().all(days_ok) {
        return Err(ApiError::invalid(format!(
            "retention must be 0 to {MAX_RETENTION_DAYS} days"
        )));
    }
    if body
        .retention_overrides
        .keys()
        .any(|topic| topic.trim().is_empty())
    {
        return Err(ApiError::invalid("a retention override needs a topic"));
    }
    let public_url = match body
        .public_url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
    {
        None => String::new(),
        Some(url) => {
            let is_http = url
                .strip_prefix("https://")
                .or_else(|| url.strip_prefix("http://"))
                .is_some_and(|rest| {
                    !rest.is_empty()
                        && !rest.starts_with('/')
                        && !rest.contains(char::is_whitespace)
                });
            if !is_http {
                return Err(ApiError::invalid(
                    "the public URL must start with http:// or https://",
                ));
            }
            url.trim_end_matches('/').to_owned()
        }
    };
    let overrides = serde_json::to_string(&body.retention_overrides)
        .map_err(|e| ApiError::internal("could not encode retention overrides", e))?;
    let ws = &auth.workspace_id;
    state
        .store
        .set_setting(ws, "retention_days", &body.retention_days.to_string())
        .await?;
    state
        .store
        .set_setting(ws, "retention_overrides", &overrides)
        .await?;
    state
        .store
        .set_setting(ws, "public_url", &public_url)
        .await?;
    crate::config::record(&state, &auth, "changed settings").await;
    Ok(Json(load(&state, ws).await?))
}

/// All users.
#[utoipa::path(get, path = "/api/users", tag = "settings", responses((status = 200, body = Vec<UserView>)))]
pub(crate) async fn users(
    State(state): State<AppState>,
    auth: Auth,
) -> Result<Json<Vec<UserView>>, ApiError> {
    let users = state.store.list_users(&auth.workspace_id).await?;
    Ok(Json(users.into_iter().map(UserView::from).collect()))
}

/// Add a user. Every user is an administrator.
#[utoipa::path(
    post, path = "/api/users", tag = "settings", request_body = NewUser,
    responses((status = 201, body = UserView), (status = 409, body = ErrorBody), (status = 422, body = ErrorBody))
)]
pub(crate) async fn create_user(
    State(state): State<AppState>,
    auth: Auth,
    Json(body): Json<NewUser>,
) -> Result<(StatusCode, Json<UserView>), ApiError> {
    let email = normalize_email(&body.email)?;
    check_password(&body.password)?;
    let user = User {
        id: new_id(),
        workspace_id: auth.workspace_id,
        email,
        password_hash: hash_password(body.password).await?,
        created_at: state.clock.now_ms(),
    };
    state.store.create_user(&user).await?;
    Ok((StatusCode::CREATED, Json(user.into())))
}

/// Remove a user and end their sessions. The last user cannot be removed.
#[utoipa::path(
    delete, path = "/api/users/{id}", tag = "settings", params(("id" = String, Path)),
    responses((status = 204), (status = 404, body = ErrorBody), (status = 409, body = ErrorBody))
)]
pub(crate) async fn delete_user(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state.store.delete_user(&auth.workspace_id, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// All admin API tokens, without their values.
#[utoipa::path(get, path = "/api/tokens", tag = "settings", responses((status = 200, body = Vec<ApiToken>)))]
pub(crate) async fn tokens(
    State(state): State<AppState>,
    auth: Auth,
) -> Result<Json<Vec<ApiToken>>, ApiError> {
    Ok(Json(state.store.list_api_tokens(&auth.workspace_id).await?))
}

/// Create an admin API token. The response is the only time its value is shown.
#[utoipa::path(
    post, path = "/api/tokens", tag = "settings", request_body = NewToken,
    responses((status = 201, body = TokenCreated), (status = 422, body = ErrorBody))
)]
pub(crate) async fn create_token(
    State(state): State<AppState>,
    auth: Auth,
    Json(body): Json<NewToken>,
) -> Result<(StatusCode, Json<TokenCreated>), ApiError> {
    let name = clean_name(&body.name)?;
    let (value, token_prefix) = generate_key(TOKEN_PREFIX);
    let token = ApiToken {
        id: new_id(),
        name,
        token_prefix,
        created_by: auth
            .user
            .map_or_else(|| "an API token".to_owned(), |u| u.email),
        created_at: state.clock.now_ms(),
        last_used_at: None,
    };
    state
        .store
        .create_api_token(&auth.workspace_id, &token, &hash_key(&value))
        .await?;
    Ok((StatusCode::CREATED, Json(TokenCreated { token, value })))
}

/// Revoke an admin API token.
#[utoipa::path(
    delete, path = "/api/tokens/{id}", tag = "settings", params(("id" = String, Path)),
    responses((status = 204), (status = 404, body = ErrorBody))
)]
pub(crate) async fn delete_token(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state
        .store
        .delete_api_token(&auth.workspace_id, &id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
