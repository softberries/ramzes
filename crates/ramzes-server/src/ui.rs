use axum::Json;
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;
use serde_json::json;

/// The built web UI, compiled into the binary.
#[derive(RustEmbed)]
#[folder = "../../ui/build"]
struct Assets;

/// The page that boots the single-page app for any route it owns.
const APP_SHELL: &str = "200.html";
/// Files under this prefix have content hashes in their names and never change.
const IMMUTABLE_PREFIX: &str = "_app/immutable/";

/// Serves an embedded file, or the app shell for a page route. Unknown API paths get a JSON 404.
pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path.starts_with("api/") || path.starts_with("v1/") {
        let body = Json(json!({"error": "not_found", "message": "no such endpoint"}));
        return (StatusCode::NOT_FOUND, body).into_response();
    }
    if let Some(file) = Assets::get(path) {
        let cache = if path.starts_with(IMMUTABLE_PREFIX) {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache"
        };
        let headers = [
            (header::CONTENT_TYPE, file.metadata.mimetype().to_owned()),
            (header::CACHE_CONTROL, cache.to_owned()),
        ];
        return (headers, file.data).into_response();
    }
    match Assets::get(APP_SHELL) {
        Some(shell) => {
            let headers = [
                (header::CONTENT_TYPE, "text/html; charset=utf-8"),
                (header::CACHE_CONTROL, "no-cache"),
            ];
            (headers, shell.data).into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            "the web UI is not part of this build",
        )
            .into_response(),
    }
}
