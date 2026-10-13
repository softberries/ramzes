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

#[derive(Serialize, ToSchema)]
pub(crate) struct StepTarget {
    connection_id: String,
    /// Absent when the connection was deleted.
    connection_name: Option<String>,
    destination: Option<String>,
}

/// One step of the route's escalation chain and how far this notification is with it.
#[derive(Serialize, ToSchema)]
pub(crate) struct StepProgress {
    /// Seconds after the previous step.
    after_s: i64,
    targets: Vec<StepTarget>,
    /// When the step went out.
    sent_at: Option<i64>,
    /// When it will go out, if it is the next one and the notification is still unacknowledged.
    due_at: Option<i64>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct NotificationView {
    #[serde(flatten)]
    detail: NotificationDetail,
    /// The escalation chain of the route as it is now. Empty when the route has none.
    escalation: Vec<StepProgress>,
}

/// One notification with its events, deliveries and escalation progress.
#[utoipa::path(
    get, path = "/api/notifications/{id}", tag = "notifications", params(("id" = String, Path)),
    responses((status = 200, body = NotificationView), (status = 404, body = ErrorBody))
)]
pub(crate) async fn detail(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<String>,
) -> Result<Json<NotificationView>, ApiError> {
    let ws = &auth.workspace_id;
    let detail = state
        .store
        .get_notification(ws, &id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let routes = state.store.list_routes(ws).await?;
    let chain = routes
        .into_iter()
        .find(|r| r.id == detail.row.route_id)
        .map(|r| r.escalation)
        .unwrap_or_default();
    let mut escalation = Vec::with_capacity(chain.len());
    if !chain.is_empty() {
        let connections = state.store.list_connections(ws).await?;
        let timers = state.store.notification_timers(&id).await?;
        for (index, step) in (0_i64..).zip(chain) {
            let sent_at = detail
                .deliveries
                .iter()
                .filter(|d| d.delivery.step == Some(index))
                .map(|d| d.delivery.created_at)
                .min();
            let due_at = timers
                .iter()
                .find(|(kind, _, data)| {
                    kind == "escalate" && data.as_deref() == Some(index.to_string().as_str())
                })
                .map(|(_, due_at, _)| *due_at);
            escalation.push(StepProgress {
                after_s: step.after_s,
                targets: step
                    .targets
                    .into_iter()
                    .map(|t| StepTarget {
                        connection_name: connections
                            .iter()
                            .find(|c| c.id == t.connection_id)
                            .map(|c| c.name.clone()),
                        connection_id: t.connection_id,
                        destination: t.destination,
                    })
                    .collect(),
                sent_at,
                due_at,
            });
        }
    }
    Ok(Json(NotificationView { detail, escalation }))
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
    state.engine.request_refresh(&id);
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
    state.engine.request_refresh(&id);
    Ok(StatusCode::NO_CONTENT)
}
