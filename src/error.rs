use axum::http::{header, StatusCode};
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
        let (status, error_message) = match &self {
            ProxyError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg.clone()),
            ProxyError::Forbidden(msg) => (StatusCode::FORBIDDEN, msg.clone()),
            ProxyError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            ProxyError::Upstream(e) => (
                StatusCode::BAD_GATEWAY,
                format!("Upstream registry error: {}", e),
            ),
            ProxyError::Cache(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
            ProxyError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
        };

        let body = Json(json!({
            "errors": [{
                "code": "PROXY_ERROR",
                "message": error_message,
            }]
        }));

        let mut response = (status, body).into_response();

        // Add WWW-Authenticate header for 401 responses per Docker Registry V2 API spec
        if matches!(self, ProxyError::Unauthorized(_)) {
            response.headers_mut().insert(
                header::WWW_AUTHENTICATE,
                header::HeaderValue::from_static(r#"Basic realm="Docker Registry Proxy""#),
            );
        }

        response
    }
}

pub type Result<T> = std::result::Result<T, ProxyError>;
