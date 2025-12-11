use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum ProxyError {
    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Forbidden: {0}")]
    Forbidden(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Upstream error: {0}")]
    Upstream(#[from] reqwest::Error),

    #[error("Cache error: {0}")]
    Cache(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl IntoResponse for ProxyError {
    fn into_response(self) -> Response {
        let (status, error_message) = match self {
            ProxyError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg),
            ProxyError::Forbidden(msg) => (StatusCode::FORBIDDEN, msg),
            ProxyError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            ProxyError::Upstream(e) => (
                StatusCode::BAD_GATEWAY,
                format!("Upstream registry error: {}", e),
            ),
            ProxyError::Cache(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
            ProxyError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
        };

        let body = Json(json!({
            "errors": [{
                "code": "PROXY_ERROR",
                "message": error_message,
            }]
        }));

        (status, body).into_response()
    }
}

pub type Result<T> = std::result::Result<T, ProxyError>;
