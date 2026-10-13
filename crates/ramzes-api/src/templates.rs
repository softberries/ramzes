use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use ramzes_connectors::Preview;
use ramzes_core::{Event, EventStatus, Severity, Template, apply_template, build_message, new_id};
use ramzes_store::EventFilter;
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;

use crate::config::record;
use crate::error::ErrorBody;
use crate::sources::clean_name;
use crate::{ApiError, AppState, Auth};

#[derive(Deserialize, ToSchema)]
pub(crate) struct TemplateInput {
    name: String,
    /// Template for the title. Empty keeps the event's title.
    title: String,
    /// Template for the body. Empty keeps the event's body.
    body: String,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct PreviewRequest {
    title: String,
    body: String,
    /// The event to word. Without it the newest event is used, or a sample when there is none.
    #[serde(default)]
    event_id: Option<String>,
    /// Also show the request this connection would send.
    #[serde(default)]
    connection_id: Option<String>,
    #[serde(default)]
    destination: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct TemplatePreview {
    title: String,
    body: Option<String>,
    /// Why the template could not be used; the default wording is shown in that case.
    error: Option<String>,
    /// The event the preview was made from.
    event_id: Option<String>,
    /// The request the chosen connection would send.
    request: Option<Preview>,
    request_error: Option<String>,
}

fn checked(id: String, input: TemplateInput) -> Result<Template, ApiError> {
    let template = Template {
        id,
        name: clean_name(&input.name)?,
        title: input.title,
        body: input.body,
    };
    if template.title.trim().is_empty() && template.body.trim().is_empty() {
        return Err(ApiError::invalid("a template needs a title or a body"));
    }
    template.validate().map_err(ApiError::invalid)?;
    Ok(template)
}

/// An event to show when the instance has none yet.
pub(crate) fn sample_event(workspace_id: &str, now_ms: i64) -> Event {
    Event {
        id: "sample".into(),
        workspace_id: workspace_id.to_owned(),
        source_id: "sample".into(),
        topic: "orders.payment.failed".into(),
        severity: Severity::Error,
        status: EventStatus::Firing,
        title: "Payment failed for order 1042".into(),
        body: Some("The card network timed out after 3 attempts.".into()),
        labels: [("env", "prod"), ("service", "checkout")]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect(),
        payload: json!({"order_id": 1042, "amount": 129.5, "currency": "EUR", "attempts": 3}),
        dedup_key: None,
        occurred_at: now_ms,
        received_at: now_ms,
    }
}

/// All templates.
#[utoipa::path(get, path = "/api/templates", tag = "templates", responses((status = 200, body = Vec<Template>)))]
pub(crate) async fn list(
    State(state): State<AppState>,
    auth: Auth,
) -> Result<Json<Vec<Template>>, ApiError> {
    Ok(Json(state.store.list_templates(&auth.workspace_id).await?))
}

/// Create a template: the wording of a message, usable by any route target.
#[utoipa::path(
    post, path = "/api/templates", tag = "templates", request_body = TemplateInput,
    responses((status = 201, body = Template), (status = 422, body = ErrorBody))
)]
pub(crate) async fn create(
    State(state): State<AppState>,
    auth: Auth,
    Json(body): Json<TemplateInput>,
) -> Result<(StatusCode, Json<Template>), ApiError> {
    let template = checked(new_id(), body)?;
    state
        .store
        .create_template(&auth.workspace_id, &template, state.clock.now_ms())
        .await?;
    record(&state, &auth, format!("added template {}", template.name)).await;
    Ok((StatusCode::CREATED, Json(template)))
}

/// Replace a template.
#[utoipa::path(
    put, path = "/api/templates/{id}", tag = "templates", params(("id" = String, Path)), request_body = TemplateInput,
    responses((status = 200, body = Template), (status = 404, body = ErrorBody), (status = 422, body = ErrorBody))
)]
pub(crate) async fn update(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<String>,
    Json(body): Json<TemplateInput>,
) -> Result<Json<Template>, ApiError> {
    let template = checked(id, body)?;
    state
        .store
        .update_template(&auth.workspace_id, &template, state.clock.now_ms())
        .await?;
    record(&state, &auth, format!("changed template {}", template.name)).await;
    Ok(Json(template))
}

/// Delete a template that no route uses.
#[utoipa::path(
    delete, path = "/api/templates/{id}", tag = "templates", params(("id" = String, Path)),
    responses((status = 204), (status = 404, body = ErrorBody), (status = 409, body = ErrorBody))
)]
pub(crate) async fn remove(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let routes = state.store.list_routes(&auth.workspace_id).await?;
    let users: Vec<&str> = routes
        .iter()
        .filter(|r| {
            r.targets
                .iter()
                .any(|t| t.template_id.as_deref() == Some(id.as_str()))
        })
        .map(|r| r.name.as_str())
        .collect();
    if !users.is_empty() {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "in_use",
            format!(
                "this template is used by: {}. Change those routes first.",
                users.join(", ")
            ),
        ));
    }
    state.store.delete_template(&auth.workspace_id, &id).await?;
    record(&state, &auth, "deleted a template").await;
    Ok(StatusCode::NO_CONTENT)
}

/// Word an event with a template that need not be saved, and optionally show the request a
/// connection would send for it.
#[utoipa::path(
    post, path = "/api/templates/preview", tag = "templates", request_body = PreviewRequest,
    responses((status = 200, body = TemplatePreview), (status = 404, body = ErrorBody))
)]
pub(crate) async fn preview(
    State(state): State<AppState>,
    auth: Auth,
    Json(body): Json<PreviewRequest>,
) -> Result<Json<TemplatePreview>, ApiError> {
    let ws = &auth.workspace_id;
    let now = state.clock.now_ms();
    let event = match &body.event_id {
        Some(id) => state
            .store
            .get_event(ws, id)
            .await?
            .ok_or_else(ApiError::not_found)?,
        None => {
            let newest = EventFilter {
                limit: 1,
                ..EventFilter::default()
            };
            match state
                .store
                .list_events(ws, &newest)
                .await?
                .into_iter()
                .next()
            {
                Some(row) => row.event,
                None => sample_event(ws, now),
            }
        }
    };
    let public_url = state.engine.public_url(ws).await?;
    let default = build_message(&event, public_url.as_deref());
    let template = Template {
        id: String::new(),
        name: String::new(),
        title: body.title,
        body: body.body,
    };
    let (message, error) = match apply_template(&template, &default, std::slice::from_ref(&event)) {
        Ok(message) => (message, None),
        Err(reason) => (default, Some(reason)),
    };
    let (mut request, mut request_error) = (None, None);
    if let Some(connection_id) = &body.connection_id {
        let connection = state
            .store
            .get_connection(ws, connection_id)
            .await?
            .ok_or_else(ApiError::not_found)?;
        let shown = state
            .engine
            .connector_for(&connection)
            .and_then(|(connector, config)| {
                connector.preview(&config, &message, body.destination.as_deref(), now)
            });
        match shown {
            Ok(preview) => request = Some(preview),
            Err(reason) => request_error = Some(reason),
        }
    }
    Ok(Json(TemplatePreview {
        title: message.title,
        body: message.body,
        error,
        event_id: (event.id != "sample").then_some(event.id),
        request,
        request_error,
    }))
}
