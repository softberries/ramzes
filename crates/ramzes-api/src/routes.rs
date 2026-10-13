use std::collections::HashSet;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use ramzes_core::{Route, RouteMatch, Target, new_id};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::config::record;
use crate::error::ErrorBody;
use crate::sources::clean_name;
use crate::{ApiError, AppState, Auth};

fn enabled_by_default() -> bool {
    true
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct RouteInput {
    name: String,
    #[serde(default = "enabled_by_default")]
    enabled: bool,
    /// When this route matches, routes after it are not evaluated.
    #[serde(default)]
    stop: bool,
    r#match: RouteMatch,
    targets: Vec<Target>,
}

impl RouteInput {
    /// The route as given, without checking it against what is stored.
    pub(crate) fn into_unchecked_route(self, id: String) -> Route {
        Route {
            id,
            name: self.name,
            position: 0,
            enabled: self.enabled,
            stop: self.stop,
            r#match: self.r#match,
            targets: self.targets,
        }
    }
}

/// A route with the last failure of its expression, if any.
#[derive(Serialize, ToSchema)]
pub(crate) struct RouteView {
    #[serde(flatten)]
    route: Route,
    /// Why the route's expression last failed. Cleared when the route is saved.
    last_error: Option<String>,
    last_error_at: Option<i64>,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct Reorder {
    /// Every route id exactly once, in the new evaluation order.
    ids: Vec<String>,
}

/// Checks the input and turns it into a route with the given id.
async fn checked(
    state: &AppState,
    workspace_id: &str,
    id: String,
    input: RouteInput,
) -> Result<Route, ApiError> {
    let name = clean_name(&input.name)?;
    input
        .r#match
        .validate()
        .map_err(|e| ApiError::invalid(e.to_string()))?;
    if input.targets.is_empty() {
        return Err(ApiError::invalid("a route needs at least one target"));
    }
    let known: HashSet<String> = state
        .store
        .list_connections(workspace_id)
        .await?
        .into_iter()
        .map(|c| c.id)
        .collect();
    if let Some(unknown) = input
        .targets
        .iter()
        .find(|t| !known.contains(&t.connection_id))
    {
        return Err(ApiError::invalid(format!(
            "there is no connection with id {:?}",
            unknown.connection_id
        )));
    }
    let templates: HashSet<String> = state
        .store
        .list_templates(workspace_id)
        .await?
        .into_iter()
        .map(|t| t.id)
        .collect();
    if let Some(unknown) = input
        .targets
        .iter()
        .filter_map(|t| t.template_id.as_ref())
        .find(|id| !templates.contains(*id))
    {
        return Err(ApiError::invalid(format!(
            "there is no template with id {unknown:?}"
        )));
    }
    let mut route = input.into_unchecked_route(id);
    route.name = name;
    Ok(route)
}

async fn views(state: &AppState, workspace_id: &str) -> Result<Vec<RouteView>, ApiError> {
    let routes = state.store.list_routes(workspace_id).await?;
    let mut errors = state.store.route_errors(workspace_id).await?;
    Ok(routes
        .into_iter()
        .map(|route| {
            let error = errors
                .iter()
                .position(|(id, _, _)| *id == route.id)
                .map(|at| errors.swap_remove(at));
            RouteView {
                last_error: error.as_ref().map(|(_, reason, _)| reason.clone()),
                last_error_at: error.map(|(_, _, at)| at),
                route,
            }
        })
        .collect())
}

async fn stored(state: &AppState, workspace_id: &str, id: &str) -> Result<RouteView, ApiError> {
    views(state, workspace_id)
        .await?
        .into_iter()
        .find(|r| r.route.id == id)
        .ok_or_else(ApiError::not_found)
}

/// All routes in evaluation order.
#[utoipa::path(get, path = "/api/routes", tag = "routes", responses((status = 200, body = Vec<RouteView>)))]
pub(crate) async fn list(
    State(state): State<AppState>,
    auth: Auth,
) -> Result<Json<Vec<RouteView>>, ApiError> {
    Ok(Json(views(&state, &auth.workspace_id).await?))
}

/// Add a route after the existing ones.
#[utoipa::path(
    post, path = "/api/routes", tag = "routes", request_body = RouteInput,
    responses((status = 201, body = RouteView), (status = 422, body = ErrorBody))
)]
pub(crate) async fn create(
    State(state): State<AppState>,
    auth: Auth,
    Json(body): Json<RouteInput>,
) -> Result<(StatusCode, Json<RouteView>), ApiError> {
    let route = checked(&state, &auth.workspace_id, new_id(), body).await?;
    state
        .store
        .create_route(&auth.workspace_id, &route, state.clock.now_ms())
        .await?;
    record(&state, &auth, format!("added route {}", route.name)).await;
    Ok((
        StatusCode::CREATED,
        Json(stored(&state, &auth.workspace_id, &route.id).await?),
    ))
}

/// Replace a route. Its position is kept.
#[utoipa::path(
    put, path = "/api/routes/{id}", tag = "routes", params(("id" = String, Path)), request_body = RouteInput,
    responses((status = 200, body = RouteView), (status = 404, body = ErrorBody), (status = 422, body = ErrorBody))
)]
pub(crate) async fn update(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<String>,
    Json(body): Json<RouteInput>,
) -> Result<Json<RouteView>, ApiError> {
    let route = checked(&state, &auth.workspace_id, id, body).await?;
    state
        .store
        .update_route(&auth.workspace_id, &route, state.clock.now_ms())
        .await?;
    record(&state, &auth, format!("changed route {}", route.name)).await;
    Ok(Json(stored(&state, &auth.workspace_id, &route.id).await?))
}

/// Delete a route.
#[utoipa::path(
    delete, path = "/api/routes/{id}", tag = "routes", params(("id" = String, Path)),
    responses((status = 204), (status = 404, body = ErrorBody))
)]
pub(crate) async fn remove(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state.store.delete_route(&auth.workspace_id, &id).await?;
    record(&state, &auth, "deleted a route").await;
    Ok(StatusCode::NO_CONTENT)
}

/// Set the evaluation order of all routes.
#[utoipa::path(
    post, path = "/api/routes/reorder", tag = "routes", request_body = Reorder,
    responses((status = 204), (status = 409, body = ErrorBody))
)]
pub(crate) async fn reorder(
    State(state): State<AppState>,
    auth: Auth,
    Json(body): Json<Reorder>,
) -> Result<StatusCode, ApiError> {
    state
        .store
        .reorder_routes(&auth.workspace_id, &body.ids, state.clock.now_ms())
        .await?;
    record(&state, &auth, "reordered routes").await;
    Ok(StatusCode::NO_CONTENT)
}
