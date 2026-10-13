//! The admin API the web UI and automation talk to.

mod auth;
mod crypto;
mod error;

use std::sync::Arc;

use axum::Router;
use ramzes_connectors::Registry;
use ramzes_engine::{Clock, Engine};
use ramzes_store::Store;
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

pub use auth::Auth;
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

/// The admin routes and the OpenAPI document that describes them.
pub fn router(state: AppState) -> (Router, utoipa::openapi::OpenApi) {
    OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(auth::setup_status, auth::setup))
        .routes(routes!(auth::login))
        .routes(routes!(auth::logout))
        .routes(routes!(auth::me))
        .with_state(state)
        .split_for_parts()
}
