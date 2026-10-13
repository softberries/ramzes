use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use ramzes_core::shaping::NotificationState;
use ramzes_store::{NotificationDetail, NotificationFilter, NotificationRow};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::error::ErrorBody;
use crate::{ApiError, AppState, Auth};

const DEFAULT_PAGE: i64 = 50;
const MAX_PAGE: i64 = 200;

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct ListQuery {
    /// States to include, separated by commas: `grouping`, `held`, `sent`, `acknowledged`,
    /// `resolved`, `suppressed`. All when absent.
    state: Option<String>,
    /// Return notifications older than this id. Use `next_before` from the previous page.
    before: Option<String>,
    /// Page size, 1 to 200. Default 50.
    limit: Option<i64>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct NotificationPage {
    notifications: Vec<NotificationRow>,
    /// Pass as `before` to get the next page; absent on the last page.
    next_before: Option<String>,
}

/// A page of notifications, newest first.
#[utoipa::path(
    get, path = "/api/notifications", tag = "notifications", params(ListQuery),
    responses((status = 200, body = NotificationPage), (status = 422, body = ErrorBody))
)]
pub(crate) async fn list(
    State(state): State<AppState>,
    auth: Auth,
    Query(query): Query<ListQuery>,
) -> Result<Json<NotificationPage>, ApiError> {
    let mut states = Vec::new();
    for name in query
        .state
        .as_deref()
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
    {
        if name.is_empty() {
            continue;
        }
        states.push(
            NotificationState::parse(name).ok_or_else(|| {
                ApiError::invalid(format!("{name:?} is not a notification state"))
            })?,
        );
    }
    let limit = query.limit.unwrap_or(DEFAULT_PAGE).clamp(1, MAX_PAGE);
    let filter = NotificationFilter {
        states,
        before_id: query.before,
        limit: limit + 1,
    };
    let mut notifications = state
        .store
        .list_notifications(&auth.workspace_id, &filter)
        .await?;
    let has_more = notifications.len() as i64 > limit;
    notifications.truncate(limit as usize);
    let next_before = has_more
        .then(|| notifications.last().map(|n| n.id.clone()))
        .flatten();
    Ok(Json(NotificationPage {
        notifications,
        next_before,
    }))
}

/// One notification with its events and deliveries.
#[utoipa::path(
    get, path = "/api/notifications/{id}", tag = "notifications", params(("id" = String, Path)),
    responses((status = 200, body = NotificationDetail), (status = 404, body = ErrorBody))
)]
pub(crate) async fn detail(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<String>,
) -> Result<Json<NotificationDetail>, ApiError> {
    state
        .store
        .get_notification(&auth.workspace_id, &id)
        .await?
        .map(Json)
        .ok_or_else(ApiError::not_found)
}

/// Acknowledge a notification: reminders stop, and one still waiting is not sent.
#[utoipa::path(
    post, path = "/api/notifications/{id}/acknowledge", tag = "notifications", params(("id" = String, Path)),
    responses((status = 204), (status = 404, body = ErrorBody), (status = 409, body = ErrorBody))
)]
pub(crate) async fn acknowledge(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let by = auth.user.as_ref().map_or("API token", |u| u.email.as_str());
    state
        .store
        .acknowledge(&auth.workspace_id, &id, by, state.clock.now_ms())
        .await?;
    state.engine.notification_changed(&id);
    Ok(StatusCode::NO_CONTENT)
}

/// Close a notification by hand. Nothing is sent.
#[utoipa::path(
    post, path = "/api/notifications/{id}/resolve", tag = "notifications", params(("id" = String, Path)),
    responses((status = 204), (status = 404, body = ErrorBody), (status = 409, body = ErrorBody))
)]
pub(crate) async fn resolve(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state
        .store
        .resolve_notification(&auth.workspace_id, &id, state.clock.now_ms())
        .await?;
    state.engine.notification_changed(&id);
    Ok(StatusCode::NO_CONTENT)
}
