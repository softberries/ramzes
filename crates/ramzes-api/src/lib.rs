//! The admin API the web UI and automation talk to.

mod auth;
mod config;
mod connections;
mod crypto;
mod definitions;
mod error;
mod events;
mod notifications;
mod overview;
mod public;
mod routes;
mod settings;
mod silences;
mod sources;
mod stream;
mod templates;
mod tester;

use std::sync::Arc;

use axum::Router;
use ramzes_connectors::Registry;
use ramzes_engine::{Clock, Engine};
use ramzes_store::Store;
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

pub use auth::Auth;
pub use config::{ImportError, import_yaml, refresh_custom_connectors, to_yaml, validate_config};
pub use crypto::SecretKey;
pub use error::ApiError;

#[derive(Clone)]
pub struct AppState {
    pub store: Store,
    pub engine: Arc<Engine>,
    pub registry: Registry,
    pub key: Arc<SecretKey>,
    pub clock: Arc<dyn Clock>,
    /// The instance's single workspace.
    pub workspace_id: String,
    /// Mark the session cookie `Secure`. Set when ramzes is served over HTTPS.
    pub secure_cookies: bool,
}

#[derive(OpenApi)]
#[openapi(info(
    title = "ramzes admin API",
    description = "Everything the web UI can do. Authenticate with the session cookie or `Authorization: Bearer <API token>`."
))]
struct ApiDoc;

fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(auth::setup_status, auth::setup))
        .routes(routes!(auth::login))
        .routes(routes!(auth::logout))
        .routes(routes!(auth::me))
        .routes(routes!(sources::list, sources::create))
        .routes(routes!(sources::remove))
        .routes(routes!(sources::rotate))
        .routes(routes!(sources::requests))
        .routes(routes!(sources::kinds))
        .routes(routes!(sources::set_mapping))
        .routes(routes!(sources::preview_mapping))
        .routes(routes!(connections::connectors))
        .routes(routes!(connections::list, connections::create))
        .routes(routes!(connections::update, connections::remove))
        .routes(routes!(connections::test))
        .routes(routes!(connections::resume))
        .routes(routes!(definitions::list, definitions::create))
        .routes(routes!(definitions::update, definitions::remove))
        .routes(routes!(templates::list, templates::create))
        .routes(routes!(templates::preview))
        .routes(routes!(templates::update, templates::remove))
        .routes(routes!(routes::list, routes::create))
        .routes(routes!(routes::reorder))
        .routes(routes!(tester::test))
        .routes(routes!(routes::update, routes::remove))
        .routes(routes!(events::feed))
        .routes(routes!(events::journey))
        .routes(routes!(events::preview))
        .routes(routes!(events::replay_event))
        .routes(routes!(events::failures))
        .routes(routes!(events::replay_failures))
        .routes(routes!(events::replay_delivery))
        .routes(routes!(settings::get, settings::put))
        .routes(routes!(settings::users, settings::create_user))
        .routes(routes!(settings::delete_user))
        .routes(routes!(settings::tokens, settings::create_token))
        .routes(routes!(settings::delete_token))
        .routes(routes!(silences::list, silences::create))
        .routes(routes!(silences::end))
        .routes(routes!(silences::remove))
        .routes(routes!(notifications::list))
        .routes(routes!(notifications::detail))
        .routes(routes!(notifications::acknowledge))
        .routes(routes!(notifications::resolve))
        .routes(routes!(overview::overview))
        .routes(routes!(config::revisions))
        .routes(routes!(config::revision))
        .routes(routes!(config::rollback))
        .routes(routes!(config::export))
        .routes(routes!(config::import))
        .routes(routes!(stream::stream))
}

/// The OpenAPI document describing the admin API.
pub fn openapi() -> utoipa::openapi::OpenApi {
    routes().into_openapi()
}

/// The admin routes and the OpenAPI document that describes them.
///
/// The document itself is served at `/api/openapi.json`.
pub fn router(state: AppState) -> (Router, utoipa::openapi::OpenApi) {
    let (router, openapi) = routes().with_state(state.clone()).split_for_parts();
    let document = openapi.clone();
    let router = router
        .route(
            "/api/openapi.json",
            axum::routing::get(move || async move { axum::Json(document) }),
        )
        .merge(public::router(state));
    (router, openapi)
}
