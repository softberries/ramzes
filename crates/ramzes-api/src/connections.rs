use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use ramzes_connectors::{ConnectionConfig, ConnectorDescriptor, SendOutcome};
use ramzes_core::{EventStatus, Message, Severity, new_id};
use ramzes_store::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use utoipa::ToSchema;

use crate::error::ErrorBody;
use crate::sources::clean_name;
use crate::{ApiError, AppState, Auth};

/// A connection as the API shows it: without its credentials.
#[derive(Serialize, ToSchema)]
pub(crate) struct ConnectionView {
    id: String,
    kind: String,
    name: String,
    /// The connector's non-secret settings.
    #[schema(value_type = Object)]
    settings: Value,
    /// Whether secret fields have been stored. Their values are never returned.
    has_credentials: bool,
    created_at: i64,
    updated_at: i64,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct NewConnection {
    /// A connector kind from `GET /api/connectors`.
    kind: String,
    name: String,
    /// Values for the connector's non-secret fields.
    #[schema(value_type = Object)]
    settings: Value,
    /// Values for the connector's secret fields.
    #[serde(default)]
    #[schema(value_type = Object)]
    credentials: Value,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct ConnectionUpdate {
    name: String,
    #[schema(value_type = Object)]
    settings: Value,
    /// Leave out to keep the stored credentials.
    #[serde(default)]
    #[schema(value_type = Option<Object>)]
    credentials: Option<Value>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct TestResult {
    ok: bool,
    /// HTTP status the service answered with, if it answered.
    status: Option<u16>,
    detail: Option<String>,
}

fn view(state: &AppState, c: Connection) -> ConnectionView {
    let has_credentials = state
        .key
        .open(&c.credentials)
        .is_ok_and(|opened| opened.as_object().is_some_and(|fields| !fields.is_empty()));
    ConnectionView {
        id: c.id,
        kind: c.kind,
        name: c.name,
        settings: c.settings,
        has_credentials,
        created_at: c.created_at,
        updated_at: c.updated_at,
    }
}

/// Checks the settings and credentials with the connector of the given kind.
fn validate(
    state: &AppState,
    kind: &str,
    settings: &Value,
    credentials: &Value,
) -> Result<(), ApiError> {
    let connector = state
        .registry
        .get(kind)
        .ok_or_else(|| ApiError::invalid(format!("there is no connector of kind {kind:?}")))?;
    if !settings.is_object() || !credentials.is_object() {
        return Err(ApiError::invalid(
            "settings and credentials must be JSON objects",
        ));
    }
    let config = ConnectionConfig {
        settings: settings.clone(),
        credentials: credentials.clone(),
    };
    connector.validate(&config).map_err(ApiError::invalid)
}

/// The connectors this binary has, with the fields each needs.
#[utoipa::path(get, path = "/api/connectors", tag = "connections", responses((status = 200, body = Vec<ConnectorDescriptor>)))]
pub(crate) async fn connectors(
    State(state): State<AppState>,
    _auth: Auth,
) -> Json<Vec<ConnectorDescriptor>> {
    Json(state.registry.descriptors())
}

/// All connections, without credentials.
#[utoipa::path(get, path = "/api/connections", tag = "connections", responses((status = 200, body = Vec<ConnectionView>)))]
pub(crate) async fn list(
    State(state): State<AppState>,
    auth: Auth,
) -> Result<Json<Vec<ConnectionView>>, ApiError> {
    let connections = state.store.list_connections(&auth.workspace_id).await?;
    Ok(Json(
        connections.into_iter().map(|c| view(&state, c)).collect(),
    ))
}

/// Create a connection. Credentials are encrypted before they are stored.
#[utoipa::path(
    post, path = "/api/connections", tag = "connections", request_body = NewConnection,
    responses((status = 201, body = ConnectionView), (status = 422, body = ErrorBody))
)]
pub(crate) async fn create(
    State(state): State<AppState>,
    auth: Auth,
    Json(body): Json<NewConnection>,
) -> Result<(StatusCode, Json<ConnectionView>), ApiError> {
    let name = clean_name(&body.name)?;
    let credentials = if body.credentials.is_null() {
        json!({})
    } else {
        body.credentials
    };
    validate(&state, &body.kind, &body.settings, &credentials)?;
    let now = state.clock.now_ms();
    let connection = Connection {
        id: new_id(),
        workspace_id: auth.workspace_id,
        kind: body.kind,
        name,
        settings: body.settings,
        credentials: state.key.seal(&credentials),
        created_at: now,
        updated_at: now,
    };
    state.store.create_connection(&connection).await?;
    Ok((StatusCode::CREATED, Json(view(&state, connection))))
}

/// Change a connection. Without `credentials` the stored ones are kept.
#[utoipa::path(
    put, path = "/api/connections/{id}", tag = "connections", params(("id" = String, Path)),
    request_body = ConnectionUpdate,
    responses((status = 200, body = ConnectionView), (status = 404, body = ErrorBody), (status = 422, body = ErrorBody))
)]
pub(crate) async fn update(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<String>,
    Json(body): Json<ConnectionUpdate>,
) -> Result<Json<ConnectionView>, ApiError> {
    let mut connection = state
        .store
        .get_connection(&auth.workspace_id, &id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let name = clean_name(&body.name)?;
    let credentials = match body.credentials {
        Some(credentials) => credentials,
        None => state
            .key
            .open(&connection.credentials)
            .map_err(|e| ApiError::internal("stored credentials could not be opened", e))?,
    };
    validate(&state, &connection.kind, &body.settings, &credentials)?;
    connection.name = name;
    connection.settings = body.settings;
    connection.credentials = state.key.seal(&credentials);
    connection.updated_at = state.clock.now_ms();
    state.store.update_connection(&connection).await?;
    Ok(Json(view(&state, connection)))
}

/// Delete a connection. Routes that target it will fail to deliver until they are changed.
#[utoipa::path(
    delete, path = "/api/connections/{id}", tag = "connections", params(("id" = String, Path)),
    responses((status = 204), (status = 404, body = ErrorBody))
)]
pub(crate) async fn remove(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state
        .store
        .delete_connection(&auth.workspace_id, &id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Send a test message through a connection and report what the service said.
#[utoipa::path(
    post, path = "/api/connections/{id}/test", tag = "connections", params(("id" = String, Path)),
    responses((status = 200, body = TestResult), (status = 404, body = ErrorBody))
)]
pub(crate) async fn test(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<String>,
) -> Result<Json<TestResult>, ApiError> {
    let connection = state
        .store
        .get_connection(&auth.workspace_id, &id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let connector = state.registry.get(&connection.kind).ok_or_else(|| {
        ApiError::invalid(format!(
            "there is no connector of kind {:?}",
            connection.kind
        ))
    })?;
    let credentials = state
        .key
        .open(&connection.credentials)
        .map_err(|e| ApiError::internal("stored credentials could not be opened", e))?;
    let message = Message {
        title: "Test message from ramzes".into(),
        body: Some("If you can read this, the connection works.".into()),
        severity: Severity::Info,
        status: EventStatus::Firing,
        topic: "ramzes.test".into(),
        fields: Vec::new(),
        link: None,
        event_ids: Vec::new(),
    };
    let config = ConnectionConfig {
        settings: connection.settings,
        credentials,
    };
    let result = match connector
        .send(&config, &message, None, state.clock.now_ms())
        .await
    {
        SendOutcome::Delivered { status, .. } => TestResult {
            ok: true,
            status,
            detail: None,
        },
        SendOutcome::Retry { status, detail, .. } | SendOutcome::Permanent { status, detail } => {
            TestResult {
                ok: false,
                status,
                detail: Some(detail),
            }
        }
    };
    Ok(Json(result))
}
