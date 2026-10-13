use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use ramzes_connectors::Definition;
use ramzes_core::new_id;
use ramzes_store::DefinitionRow;
use serde::Deserialize;
use utoipa::ToSchema;

use crate::config::{record, refresh_custom_connectors};
use crate::error::ErrorBody;
use crate::{ApiError, AppState, Auth};

#[derive(Deserialize, ToSchema)]
pub(crate) struct DefinitionInput {
    /// The connector definition as YAML.
    definition: String,
}

/// Names of the connections that use connectors of `kind`.
async fn users_of(
    state: &AppState,
    workspace_id: &str,
    kind: &str,
) -> Result<Vec<String>, ApiError> {
    Ok(state
        .store
        .list_connections(workspace_id)
        .await?
        .into_iter()
        .filter(|c| c.kind == kind)
        .map(|c| c.name)
        .collect())
}

fn in_use(names: &[String]) -> ApiError {
    ApiError::new(
        StatusCode::CONFLICT,
        "in_use",
        format!(
            "this connector is used by: {}. Delete those connections first.",
            names.join(", ")
        ),
    )
}

/// The connector definitions users added.
#[utoipa::path(get, path = "/api/connector-definitions", tag = "connections", responses((status = 200, body = Vec<DefinitionRow>)))]
pub(crate) async fn list(
    State(state): State<AppState>,
    auth: Auth,
) -> Result<Json<Vec<DefinitionRow>>, ApiError> {
    Ok(Json(
        state.store.list_definitions(&auth.workspace_id).await?,
    ))
}

/// Add a connector described by a YAML definition.
#[utoipa::path(
    post, path = "/api/connector-definitions", tag = "connections", request_body = DefinitionInput,
    responses((status = 201, body = DefinitionRow), (status = 409, body = ErrorBody), (status = 422, body = ErrorBody))
)]
pub(crate) async fn create(
    State(state): State<AppState>,
    auth: Auth,
    Json(body): Json<DefinitionInput>,
) -> Result<(StatusCode, Json<DefinitionRow>), ApiError> {
    let parsed = Definition::parse(&body.definition).map_err(ApiError::invalid)?;
    let row = DefinitionRow {
        id: new_id(),
        kind: parsed.kind,
        name: parsed.name,
        definition: body.definition,
        created_at: state.clock.now_ms(),
    };
    state
        .store
        .create_definition(&auth.workspace_id, &row)
        .await?;
    refresh_custom_connectors(&state.store, &state.registry, &auth.workspace_id).await?;
    record(&state, &auth, format!("added connector {}", row.name)).await;
    Ok((StatusCode::CREATED, Json(row)))
}

/// Replace a connector definition. Its kind can only change while no connection uses it.
#[utoipa::path(
    put, path = "/api/connector-definitions/{id}", tag = "connections", params(("id" = String, Path)),
    request_body = DefinitionInput,
    responses((status = 200, body = DefinitionRow), (status = 404, body = ErrorBody), (status = 409, body = ErrorBody), (status = 422, body = ErrorBody))
)]
pub(crate) async fn update(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<String>,
    Json(body): Json<DefinitionInput>,
) -> Result<Json<DefinitionRow>, ApiError> {
    let ws = &auth.workspace_id;
    let stored = state
        .store
        .list_definitions(ws)
        .await?
        .into_iter()
        .find(|d| d.id == id)
        .ok_or_else(ApiError::not_found)?;
    let parsed = Definition::parse(&body.definition).map_err(ApiError::invalid)?;
    if parsed.kind != stored.kind {
        let users = users_of(&state, ws, &stored.kind).await?;
        if !users.is_empty() {
            return Err(in_use(&users));
        }
    }
    let row = DefinitionRow {
        id,
        kind: parsed.kind,
        name: parsed.name,
        definition: body.definition,
        created_at: stored.created_at,
    };
    state.store.update_definition(ws, &row).await?;
    refresh_custom_connectors(&state.store, &state.registry, ws).await?;
    record(&state, &auth, format!("changed connector {}", row.name)).await;
    Ok(Json(row))
}

/// Delete a connector definition that no connection uses.
#[utoipa::path(
    delete, path = "/api/connector-definitions/{id}", tag = "connections", params(("id" = String, Path)),
    responses((status = 204), (status = 404, body = ErrorBody), (status = 409, body = ErrorBody))
)]
pub(crate) async fn remove(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let ws = &auth.workspace_id;
    let stored = state
        .store
        .list_definitions(ws)
        .await?
        .into_iter()
        .find(|d| d.id == id)
        .ok_or_else(ApiError::not_found)?;
    let users = users_of(&state, ws, &stored.kind).await?;
    if !users.is_empty() {
        return Err(in_use(&users));
    }
    state.store.delete_definition(ws, &id).await?;
    refresh_custom_connectors(&state.store, &state.registry, ws).await?;
    record(&state, &auth, format!("deleted connector {}", stored.name)).await;
    Ok(StatusCode::NO_CONTENT)
}
