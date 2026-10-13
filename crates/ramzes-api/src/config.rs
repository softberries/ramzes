//! Configuration as a whole: history, rollback, export and import.

use std::collections::HashSet;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use ramzes_connectors::{Definition, Registry};
use ramzes_core::Mapping;
use ramzes_store::{ConfigSnapshot, RevisionMeta, Store, StoreError};
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;

use crate::crypto::SecretKey;
use crate::error::ErrorBody;
use crate::{ApiError, AppState, Auth};

/// Who made a change, for the history.
fn author(auth: &Auth) -> String {
    match &auth.user {
        Some(user) => user.email.clone(),
        None => "API token".to_owned(),
    }
}

/// Stores the configuration as it is now, after a change. A failure is logged, not returned:
/// the change itself has already happened.
pub(crate) async fn record(state: &AppState, auth: &Auth, summary: impl AsRef<str>) {
    let result = async {
        let config = state.store.read_config(&auth.workspace_id).await?;
        let snapshot =
            serde_json::to_string(&config).map_err(|e| StoreError::Data(e.to_string()))?;
        state
            .store
            .add_revision(
                &auth.workspace_id,
                &author(auth),
                summary.as_ref(),
                &snapshot,
                state.clock.now_ms(),
            )
            .await
    };
    if let Err(e) = result.await {
        tracing::error!(error = %e, "could not record a configuration revision");
    }
}

/// Loads the connectors users defined into the registry. Definitions that no longer parse are
/// skipped with a warning.
pub async fn refresh_custom_connectors(
    store: &Store,
    registry: &Registry,
    workspace_id: &str,
) -> Result<(), StoreError> {
    let mut definitions = Vec::new();
    for row in store.list_definitions(workspace_id).await? {
        match Definition::parse(&row.definition) {
            Ok(definition) => definitions.push(definition),
            Err(reason) => {
                tracing::warn!(kind = %row.kind, %reason, "skipping an invalid connector definition")
            }
        }
    }
    registry.set_custom(definitions);
    Ok(())
}

/// The configuration as YAML, without credentials.
pub fn to_yaml(mut config: ConfigSnapshot) -> String {
    for connection in &mut config.connections {
        connection.credentials = None;
    }
    serde_norway::to_string(&config).unwrap_or_default()
}

/// Checks that a configuration holds together before any of it is applied.
pub fn validate_config(config: &ConfigSnapshot, registry: &Registry) -> Result<(), String> {
    let mut ids = HashSet::new();
    let mut custom_kinds = HashSet::new();
    for d in &config.definitions {
        let parsed = Definition::parse(&d.definition)
            .map_err(|e| format!("connector definition {:?}: {e}", d.name))?;
        if parsed.kind != d.kind {
            return Err(format!(
                "connector definition {:?}: its kind does not match its text",
                d.name
            ));
        }
        if !custom_kinds.insert(d.kind.as_str()) || !ids.insert(d.id.as_str()) {
            return Err(format!("connector definition {:?} appears twice", d.kind));
        }
    }
    let mut connections = HashSet::new();
    for c in &config.connections {
        if c.name.trim().is_empty() || !c.settings.is_object() {
            return Err(format!(
                "connection {:?}: it needs a name and settings",
                c.id
            ));
        }
        let known = custom_kinds.contains(c.kind.as_str())
            || registry
                .get(&c.kind)
                .is_some_and(|connector| !connector.descriptor().custom);
        if !known {
            return Err(format!(
                "connection {:?}: there is no connector of kind {:?}",
                c.name, c.kind
            ));
        }
        if !connections.insert(c.id.as_str()) {
            return Err(format!("connection {:?} appears twice", c.id));
        }
    }
    let mut templates = HashSet::new();
    for t in &config.templates {
        t.validate()
            .map_err(|e| format!("template {:?}: {e}", t.name))?;
        if !templates.insert(t.id.as_str()) {
            return Err(format!("template {:?} appears twice", t.id));
        }
    }
    let mut routes = HashSet::new();
    for r in &config.routes {
        if r.name.trim().is_empty() {
            return Err(format!("route {:?}: it needs a name", r.id));
        }
        r.r#match
            .validate()
            .map_err(|e| format!("route {:?}: {e}", r.name))?;
        r.shaping
            .validate()
            .map_err(|e| format!("route {:?}: {e}", r.name))?;
        if r.targets.is_empty() {
            return Err(format!("route {:?}: it needs at least one target", r.name));
        }
        ramzes_core::validate_escalation(&r.escalation)
            .map_err(|e| format!("route {:?}: {e}", r.name))?;
        for target in r
            .targets
            .iter()
            .chain(r.escalation.iter().flat_map(|step| step.targets.iter()))
        {
            if !connections.contains(target.connection_id.as_str()) {
                return Err(format!(
                    "route {:?}: there is no connection with id {:?}",
                    r.name, target.connection_id
                ));
            }
            if let Some(template) = &target.template_id
                && !templates.contains(template.as_str())
            {
                return Err(format!(
                    "route {:?}: there is no template with id {template:?}",
                    r.name
                ));
            }
        }
        if !routes.insert(r.id.as_str()) {
            return Err(format!("route {:?} appears twice", r.id));
        }
    }
    for s in &config.sources {
        if let Some(mapping) = &s.mapping {
            Mapping::parse(mapping).map_err(|e| format!("source {:?}: {e}", s.name))?;
        }
    }
    Ok(())
}

/// Why an import did not happen.
#[derive(Debug)]
pub enum ImportError {
    /// The text is not a usable configuration; nothing was changed.
    Invalid(String),
    Store(StoreError),
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportError::Invalid(reason) => f.write_str(reason),
            ImportError::Store(e) => write!(f, "{e}"),
        }
    }
}

/// Replaces the configuration with the one in `yaml`. Connections keep their stored credentials;
/// a connection that is new starts without any.
pub async fn import_yaml(
    store: &Store,
    registry: &Registry,
    key: &SecretKey,
    workspace_id: &str,
    yaml: &str,
    now_ms: i64,
) -> Result<(), ImportError> {
    let mut config: ConfigSnapshot =
        serde_norway::from_str(yaml).map_err(|e| ImportError::Invalid(format!("YAML: {e}")))?;
    validate_config(&config, registry).map_err(ImportError::Invalid)?;
    let existing: HashSet<String> = store
        .list_connections(workspace_id)
        .await
        .map_err(ImportError::Store)?
        .into_iter()
        .map(|c| c.id)
        .collect();
    for connection in &mut config.connections {
        // An exported file carries no credentials; anything in this field is ignored.
        connection.credentials = (!existing.contains(&connection.id)).then(|| key.seal(&json!({})));
    }
    store
        .apply_config(workspace_id, &config, now_ms)
        .await
        .map_err(ImportError::Store)?;
    refresh_custom_connectors(store, registry, workspace_id)
        .await
        .map_err(ImportError::Store)
}

#[derive(Serialize, ToSchema)]
pub(crate) struct RevisionView {
    #[serde(flatten)]
    meta: RevisionMeta,
    /// The configuration after this change as YAML, without credentials.
    config: String,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct Exported {
    /// The configuration as YAML, without credentials or source keys.
    yaml: String,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct Import {
    yaml: String,
}

/// Configuration changes, newest first.
#[utoipa::path(get, path = "/api/config/revisions", tag = "config", responses((status = 200, body = Vec<RevisionMeta>)))]
pub(crate) async fn revisions(
    State(state): State<AppState>,
    auth: Auth,
) -> Result<Json<Vec<RevisionMeta>>, ApiError> {
    Ok(Json(state.store.list_revisions(&auth.workspace_id).await?))
}

/// The configuration as it was after one change.
#[utoipa::path(
    get, path = "/api/config/revisions/{id}", tag = "config", params(("id" = String, Path)),
    responses((status = 200, body = RevisionView), (status = 404, body = ErrorBody))
)]
pub(crate) async fn revision(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<String>,
) -> Result<Json<RevisionView>, ApiError> {
    let revision = state
        .store
        .get_revision(&auth.workspace_id, &id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let config: ConfigSnapshot = serde_json::from_str(&revision.snapshot)
        .map_err(|e| ApiError::internal("a stored revision could not be read", e))?;
    Ok(Json(RevisionView {
        meta: revision.meta,
        config: to_yaml(config),
    }))
}

/// Make the configuration what it was after a past change. Source keys are not affected, and
/// connections that still exist keep their current credentials.
#[utoipa::path(
    post, path = "/api/config/revisions/{id}/rollback", tag = "config", params(("id" = String, Path)),
    responses((status = 204), (status = 404, body = ErrorBody))
)]
pub(crate) async fn rollback(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let revision = state
        .store
        .get_revision(&auth.workspace_id, &id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let mut config: ConfigSnapshot = serde_json::from_str(&revision.snapshot)
        .map_err(|e| ApiError::internal("a stored revision could not be read", e))?;
    // A connection that still exists keeps the credentials it has now: they may have been
    // replaced since, and the old ones revoked. Only a connection that is brought back gets
    // the credentials it had.
    let existing: HashSet<String> = state
        .store
        .list_connections(&auth.workspace_id)
        .await?
        .into_iter()
        .map(|c| c.id)
        .collect();
    for connection in &mut config.connections {
        if existing.contains(&connection.id) {
            connection.credentials = None;
        }
    }
    state
        .store
        .apply_config(&auth.workspace_id, &config, state.clock.now_ms())
        .await?;
    refresh_custom_connectors(&state.store, &state.registry, &auth.workspace_id).await?;
    let summary = if revision.meta.summary.is_empty() {
        "rolled back".to_owned()
    } else {
        format!("rolled back to: {}", revision.meta.summary)
    };
    record(&state, &auth, summary).await;
    Ok(StatusCode::NO_CONTENT)
}

/// The whole configuration as YAML, for keeping in version control or moving to another instance.
#[utoipa::path(get, path = "/api/config/export", tag = "config", responses((status = 200, body = Exported)))]
pub(crate) async fn export(
    State(state): State<AppState>,
    auth: Auth,
) -> Result<Json<Exported>, ApiError> {
    let config = state.store.read_config(&auth.workspace_id).await?;
    Ok(Json(Exported {
        yaml: to_yaml(config),
    }))
}

/// Replace the configuration with an exported one. Stored credentials and source keys are kept.
#[utoipa::path(
    post, path = "/api/config/import", tag = "config", request_body = Import,
    responses((status = 204), (status = 422, body = ErrorBody))
)]
pub(crate) async fn import(
    State(state): State<AppState>,
    auth: Auth,
    Json(body): Json<Import>,
) -> Result<StatusCode, ApiError> {
    import_yaml(
        &state.store,
        &state.registry,
        &state.key,
        &auth.workspace_id,
        &body.yaml,
        state.clock.now_ms(),
    )
    .await
    .map_err(|e| match e {
        ImportError::Invalid(reason) => ApiError::invalid(reason),
        ImportError::Store(e) => e.into(),
    })?;
    record(&state, &auth, "imported a configuration").await;
    Ok(StatusCode::NO_CONTENT)
}
