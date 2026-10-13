use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use ramzes_core::adapters::{JSON, NATIVE, SourceKind, source_kinds, starter_mapping};
use ramzes_core::{Mapping, NewEvent, generate_key, hash_key};
use ramzes_store::{Source, SourceRequest};
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;

use crate::config::record;
use crate::error::ErrorBody;
use crate::{ApiError, AppState, Auth};

pub(crate) const SOURCE_KEY_PREFIX: &str = "rz_src_";
const NAME_MAX: usize = 100;
/// A key the user supplies must be at least this long.
const OWN_KEY_MIN: usize = 16;
const OWN_KEY_MAX: usize = 200;

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
    /// A kind from `GET /api/source-kinds`. Defaults to `native`.
    #[serde(default)]
    kind: Option<String>,
    /// Use this secret instead of a generated key, for senders that generate their own.
    #[serde(default)]
    key: Option<String>,
    /// The mapping of an any-JSON source, as YAML. A starter mapping is used when absent.
    #[serde(default)]
    mapping: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct Rotate {
    /// Use this secret instead of generating a key.
    #[serde(default)]
    key: Option<String>,
}

/// A source with the path its sender posts to.
#[derive(Serialize, ToSchema)]
pub(crate) struct SourceView {
    #[serde(flatten)]
    source: Source,
    /// `/v1/events` for native sources, `/v1/in/{id}` for the others.
    path: String,
}

impl From<Source> for SourceView {
    fn from(source: Source) -> Self {
        let path = if source.kind == NATIVE {
            "/v1/events".to_owned()
        } else {
            format!("/v1/in/{}", source.id)
        };
        SourceView { source, path }
    }
}

#[derive(Serialize, ToSchema)]
pub(crate) struct SourceCreated {
    #[serde(flatten)]
    source: SourceView,
    /// The ingest key. Shown only in this response.
    key: String,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct RotatedKey {
    /// The new ingest key. Shown only in this response.
    key: String,
    key_prefix: String,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct MappingInput {
    /// The mapping as YAML.
    mapping: String,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct MappingPreviewRequest {
    /// The mapping as YAML.
    mapping: String,
    /// A request body to try it on.
    body: String,
    /// The request's headers, lower-case names.
    #[serde(default)]
    headers: BTreeMap<String, String>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct MappingPreview {
    /// The events the request would produce.
    events: Vec<NewEvent>,
    /// Why the mapping or the body could not be used.
    error: Option<String>,
}

/// A key and its display prefix: the user's own secret, or a generated one.
fn key_from(own: Option<String>) -> Result<(String, String), ApiError> {
    let Some(own) = own.map(|k| k.trim().to_owned()).filter(|k| !k.is_empty()) else {
        return Ok(generate_key(SOURCE_KEY_PREFIX));
    };
    let length = own.chars().count();
    if !(OWN_KEY_MIN..=OWN_KEY_MAX).contains(&length) || own.chars().any(char::is_control) {
        return Err(ApiError::invalid(format!(
            "a key you supply must be {OWN_KEY_MIN} to {OWN_KEY_MAX} characters"
        )));
    }
    let prefix: String = own.chars().take(4).collect();
    Ok((own, prefix))
}

/// Stores the key sealed, so signatures made with it can be checked.
async fn keep_secret(
    state: &AppState,
    workspace_id: &str,
    id: &str,
    key: &str,
) -> Result<(), ApiError> {
    let sealed = state.key.seal(&json!({"key": key}));
    state
        .store
        .set_source_secret(workspace_id, id, &sealed)
        .await?;
    Ok(())
}

fn key_taken(e: ramzes_store::StoreError) -> ApiError {
    match e {
        ramzes_store::StoreError::Db(db)
            if db
                .as_database_error()
                .is_some_and(|d| d.is_unique_violation()) =>
        {
            ApiError::new(
                StatusCode::CONFLICT,
                "conflict",
                "another source already uses this key",
            )
        }
        other => other.into(),
    }
}

/// The built-in source that ramzes's own events come from has no key to replace, and deleting
/// it would only make ramzes create another.
async fn refuse_built_in(state: &AppState, id: &str) -> Result<(), ApiError> {
    match state.store.get_source(id).await? {
        Some(source) if source.kind == "system" => Err(ApiError::invalid(
            "this is the built-in source of ramzes's own events; it cannot be changed or deleted",
        )),
        _ => Ok(()),
    }
}

/// The kinds of source ramzes understands, with how to set each up.
#[utoipa::path(get, path = "/api/source-kinds", tag = "sources", responses((status = 200, body = Vec<SourceKind>)))]
pub(crate) async fn kinds(_auth: Auth) -> Json<Vec<SourceKind>> {
    Json(source_kinds())
}

/// All sources.
#[utoipa::path(get, path = "/api/sources", tag = "sources", responses((status = 200, body = Vec<SourceView>)))]
pub(crate) async fn list(
    State(state): State<AppState>,
    auth: Auth,
) -> Result<Json<Vec<SourceView>>, ApiError> {
    let sources = state.store.list_sources(&auth.workspace_id).await?;
    Ok(Json(sources.into_iter().map(SourceView::from).collect()))
}

/// Create a source. The response is the only time its key is shown.
#[utoipa::path(
    post, path = "/api/sources", tag = "sources", request_body = NewSource,
    responses((status = 201, body = SourceCreated), (status = 409, body = ErrorBody), (status = 422, body = ErrorBody))
)]
pub(crate) async fn create(
    State(state): State<AppState>,
    auth: Auth,
    Json(body): Json<NewSource>,
) -> Result<(StatusCode, Json<SourceCreated>), ApiError> {
    let ws = &auth.workspace_id;
    let name = clean_name(&body.name)?;
    let kind = body.kind.unwrap_or_else(|| NATIVE.to_owned());
    if !source_kinds().iter().any(|k| k.kind == kind) {
        return Err(ApiError::invalid(format!(
            "there is no source kind {kind:?}"
        )));
    }
    let mapping = match (kind.as_str(), body.mapping) {
        (JSON, Some(text)) => {
            Mapping::parse(&text).map_err(ApiError::invalid)?;
            Some(text)
        }
        (JSON, None) => Some(starter_mapping().to_yaml()),
        _ => None,
    };
    let (key, prefix) = key_from(body.key)?;
    let now = state.clock.now_ms();
    let mut source = state
        .store
        .create_source(ws, &name, &kind, &hash_key(&key), &prefix, now)
        .await
        .map_err(key_taken)?;
    keep_secret(&state, ws, &source.id, &key).await?;
    if mapping.is_some() {
        state
            .store
            .set_source_mapping(ws, &source.id, mapping.as_deref())
            .await?;
        source.mapping = mapping;
    }
    record(&state, &auth, format!("added source {name}")).await;
    Ok((
        StatusCode::CREATED,
        Json(SourceCreated {
            source: source.into(),
            key,
        }),
    ))
}

/// Replace a source's key. The old key stops working immediately.
#[utoipa::path(
    post, path = "/api/sources/{id}/rotate", tag = "sources", params(("id" = String, Path)),
    request_body(content = Option<Rotate>),
    responses((status = 200, body = RotatedKey), (status = 404, body = ErrorBody), (status = 422, body = ErrorBody))
)]
pub(crate) async fn rotate(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<String>,
    body: Option<Json<Rotate>>,
) -> Result<Json<RotatedKey>, ApiError> {
    refuse_built_in(&state, &id).await?;
    let (key, key_prefix) = key_from(body.and_then(|Json(body)| body.key))?;
    state
        .store
        .rotate_source_key(&auth.workspace_id, &id, &hash_key(&key), &key_prefix)
        .await
        .map_err(key_taken)?;
    keep_secret(&state, &auth.workspace_id, &id, &key).await?;
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
    refuse_built_in(&state, &id).await?;
    state.store.delete_source(&auth.workspace_id, &id).await?;
    record(&state, &auth, "deleted a source").await;
    Ok(StatusCode::NO_CONTENT)
}

/// The source's recent requests, newest first: rejected ones for native sources, all of them
/// for the other kinds.
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

/// Replace the mapping of an any-JSON source.
#[utoipa::path(
    put, path = "/api/sources/{id}/mapping", tag = "sources", params(("id" = String, Path)), request_body = MappingInput,
    responses((status = 200, body = SourceView), (status = 404, body = ErrorBody), (status = 422, body = ErrorBody))
)]
pub(crate) async fn set_mapping(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<String>,
    Json(body): Json<MappingInput>,
) -> Result<Json<SourceView>, ApiError> {
    let ws = &auth.workspace_id;
    let mut source = state
        .store
        .get_source(&id)
        .await?
        .filter(|s| s.workspace_id == *ws)
        .ok_or_else(ApiError::not_found)?;
    if source.kind != JSON {
        return Err(ApiError::invalid(
            "only any-JSON sources have a mapping you can change",
        ));
    }
    Mapping::parse(&body.mapping).map_err(ApiError::invalid)?;
    state
        .store
        .set_source_mapping(ws, &id, Some(&body.mapping))
        .await?;
    source.mapping = Some(body.mapping);
    record(
        &state,
        &auth,
        format!("changed the mapping of source {}", source.name),
    )
    .await;
    Ok(Json(source.into()))
}

/// Try a mapping on a request body without saving anything.
#[utoipa::path(
    post, path = "/api/sources/preview-mapping", tag = "sources", request_body = MappingPreviewRequest,
    responses((status = 200, body = MappingPreview))
)]
pub(crate) async fn preview_mapping(
    _auth: Auth,
    Json(body): Json<MappingPreviewRequest>,
) -> Json<MappingPreview> {
    let result = Mapping::parse(&body.mapping).and_then(|mapping| {
        let payload: serde_json::Value = serde_json::from_str(&body.body)
            .map_err(|e| format!("the body is not valid JSON: {e}"))?;
        let events = mapping.apply(&payload, &body.headers)?;
        for event in &events {
            event.validate().map_err(|e| e.to_string())?;
        }
        Ok(events)
    });
    Json(match result {
        Ok(events) => MappingPreview {
            events,
            error: None,
        },
        Err(error) => MappingPreview {
            events: Vec::new(),
            error: Some(error),
        },
    })
}
