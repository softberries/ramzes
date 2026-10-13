use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use ramzes_core::shaping::Silence;
use ramzes_core::{RouteMatch, new_id};
use ramzes_store::SilenceRow;
use serde::Deserialize;
use utoipa::ToSchema;

use crate::error::ErrorBody;
use crate::{ApiError, AppState, Auth};

const LONGEST_MS: i64 = 366 * 86_400_000;

#[derive(Deserialize, ToSchema)]
pub(crate) struct NewSilence {
    /// Which events to silence. At least one condition is required.
    r#match: RouteMatch,
    /// When it starts, unix milliseconds. Defaults to now.
    #[serde(default)]
    starts_at: Option<i64>,
    /// When it ends, unix milliseconds. Give this or `duration_s`.
    #[serde(default)]
    ends_at: Option<i64>,
    /// How long it lasts from its start, in seconds.
    #[serde(default)]
    duration_s: Option<i64>,
    #[serde(default)]
    comment: Option<String>,
}

/// Silences, the one ending last first. Ended ones are included.
#[utoipa::path(get, path = "/api/silences", tag = "silences", responses((status = 200, body = Vec<SilenceRow>)))]
pub(crate) async fn list(
    State(state): State<AppState>,
    auth: Auth,
) -> Result<Json<Vec<SilenceRow>>, ApiError> {
    Ok(Json(state.store.list_silences(&auth.workspace_id).await?))
}

/// Silence matching events for a period: they are recorded but sent nowhere.
#[utoipa::path(
    post, path = "/api/silences", tag = "silences", request_body = NewSilence,
    responses((status = 201, body = SilenceRow), (status = 422, body = ErrorBody))
)]
pub(crate) async fn create(
    State(state): State<AppState>,
    auth: Auth,
    Json(body): Json<NewSilence>,
) -> Result<(StatusCode, Json<SilenceRow>), ApiError> {
    let now = state.clock.now_ms();
    body.r#match
        .validate()
        .map_err(|e| ApiError::invalid(e.to_string()))?;
    if body.r#match == RouteMatch::default() {
        return Err(ApiError::invalid(
            "a silence without conditions would silence everything; add at least one condition",
        ));
    }
    let starts_at = body.starts_at.unwrap_or(now);
    let ends_at = match (body.ends_at, body.duration_s) {
        (Some(ends_at), _) => ends_at,
        (None, Some(seconds)) => starts_at.saturating_add(seconds.saturating_mul(1000)),
        (None, None) => {
            return Err(ApiError::invalid(
                "give the silence a duration or an end time",
            ));
        }
    };
    if ends_at <= starts_at || ends_at <= now {
        return Err(ApiError::invalid(
            "the silence must end after it starts, and in the future",
        ));
    }
    if ends_at - starts_at > LONGEST_MS {
        return Err(ApiError::invalid("a silence can last at most a year"));
    }
    let comment = body
        .comment
        .map(|c| c.trim().chars().take(500).collect::<String>())
        .filter(|c| !c.is_empty());
    let author = auth
        .user
        .as_ref()
        .map_or("API token", |u| u.email.as_str())
        .to_owned();
    let silence = Silence {
        id: new_id(),
        r#match: body.r#match,
        starts_at,
        ends_at,
        comment,
    };
    state
        .store
        .create_silence(&auth.workspace_id, &silence, &author, now)
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(SilenceRow {
            silence,
            author,
            created_at: now,
        }),
    ))
}

/// End a silence now.
#[utoipa::path(
    post, path = "/api/silences/{id}/end", tag = "silences", params(("id" = String, Path)),
    responses((status = 204), (status = 404, body = ErrorBody))
)]
pub(crate) async fn end(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state
        .store
        .end_silence(&auth.workspace_id, &id, state.clock.now_ms())
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Delete a silence from the list.
#[utoipa::path(
    delete, path = "/api/silences/{id}", tag = "silences", params(("id" = String, Path)),
    responses((status = 204), (status = 404, body = ErrorBody))
)]
pub(crate) async fn remove(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state.store.delete_silence(&auth.workspace_id, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}
