use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use ramzes_core::{generate_key, hash_key};
use ramzes_store::{Source, SourceRequest};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::ErrorBody;
use crate::{ApiError, AppState, Auth};

pub(crate) const SOURCE_KEY_PREFIX: &str = "rz_src_";
const NAME_MAX: usize = 100;

/// Trims a display name and checks its length.
pub(crate) fn clean_name(name: &str) -> Result<String, ApiError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > NAME_MAX {
        return Err(ApiError::invalid(format!(
            "the name must be 1 to {NAME_MAX} characters"
        )));
    }
    Ok(name.to_owned())
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct NewSource {
    name: String,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct SourceCreated {
    #[serde(flatten)]
    source: Source,
    /// The ingest key. Shown only in this response.
    key: String,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct RotatedKey {
    /// The new ingest key. Shown only in this response.
    key: String,
    key_prefix: String,
}

/// All sources.
#[utoipa::path(get, path = "/api/sources", tag = "sources", responses((status = 200, body = Vec<Source>)))]
pub(crate) async fn list(
    State(state): State<AppState>,
    auth: Auth,
) -> Result<Json<Vec<Source>>, ApiError> {
    Ok(Json(state.store.list_sources(&auth.workspace_id).await?))
}

/// Create a source. The response is the only time its key is shown.
#[utoipa::path(
    post, path = "/api/sources", tag = "sources", request_body = NewSource,
    responses((status = 201, body = SourceCreated), (status = 422, body = ErrorBody))
)]
pub(crate) async fn create(
    State(state): State<AppState>,
    auth: Auth,
    Json(body): Json<NewSource>,
) -> Result<(StatusCode, Json<SourceCreated>), ApiError> {
    let name = clean_name(&body.name)?;
    let (key, prefix) = generate_key(SOURCE_KEY_PREFIX);
    let now = state.clock.now_ms();
    let source = state
        .store
        .create_source(
            &auth.workspace_id,
            &name,
            "native",
            &hash_key(&key),
            &prefix,
            now,
        )
        .await?;
    Ok((StatusCode::CREATED, Json(SourceCreated { source, key })))
}

/// Replace a source's key. The old key stops working immediately.
#[utoipa::path(
    post, path = "/api/sources/{id}/rotate", tag = "sources", params(("id" = String, Path)),
    responses((status = 200, body = RotatedKey), (status = 404, body = ErrorBody))
)]
pub(crate) async fn rotate(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<String>,
) -> Result<Json<RotatedKey>, ApiError> {
    let (key, key_prefix) = generate_key(SOURCE_KEY_PREFIX);
    state
        .store
        .rotate_source_key(&auth.workspace_id, &id, &hash_key(&key), &key_prefix)
        .await?;
    Ok(Json(RotatedKey { key, key_prefix }))
}

/// Delete a source. Its events are kept.
#[utoipa::path(
    delete, path = "/api/sources/{id}", tag = "sources", params(("id" = String, Path)),
    responses((status = 204), (status = 404, body = ErrorBody))
)]
pub(crate) async fn remove(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state.store.delete_source(&auth.workspace_id, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The source's most recent rejected requests, newest first.
#[utoipa::path(
    get, path = "/api/sources/{id}/requests", tag = "sources", params(("id" = String, Path)),
    responses((status = 200, body = Vec<SourceRequest>))
)]
pub(crate) async fn requests(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<String>,
) -> Result<Json<Vec<SourceRequest>>, ApiError> {
    Ok(Json(
        state
            .store
            .list_source_requests(&auth.workspace_id, &id)
            .await?,
    ))
}
