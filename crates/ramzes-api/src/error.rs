use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use ramzes_engine::EngineError;
use ramzes_store::StoreError;
use serde::Serialize;
use utoipa::ToSchema;

/// The body of every error response.
#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorBody {
    /// A stable machine-readable code.
    pub error: &'static str,
    /// What went wrong, for people.
    pub message: String,
}

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> ApiError {
        ApiError {
            status,
            code,
            message: message.into(),
        }
    }

    pub fn unauthorized() -> ApiError {
        ApiError::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "sign in or send an API token",
        )
    }

    pub fn not_found() -> ApiError {
        ApiError::new(StatusCode::NOT_FOUND, "not_found", "no such resource")
    }

    pub fn invalid(message: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "invalid", message)
    }

    pub fn internal(context: &str, error: impl std::fmt::Display) -> ApiError {
        tracing::error!(%error, "{context}");
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "something went wrong inside ramzes; see its log",
        )
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorBody {
                error: self.code,
                message: self.message,
            }),
        )
            .into_response()
    }
}

impl From<StoreError> for ApiError {
    fn from(e: StoreError) -> Self {
        match e {
            StoreError::NotFound => ApiError::not_found(),
            StoreError::Conflict(message) => {
                ApiError::new(StatusCode::CONFLICT, "conflict", message)
            }
            other => ApiError::internal("storage error", other),
        }
    }
}

impl From<EngineError> for ApiError {
    fn from(e: EngineError) -> Self {
        match e {
            EngineError::NotFound => ApiError::not_found(),
            EngineError::Conflict(message) => {
                ApiError::new(StatusCode::CONFLICT, "conflict", message)
            }
            EngineError::Store(e) => e.into(),
        }
    }
}
