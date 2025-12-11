use crate::auth::{check_repository_access, Claims};
use crate::cache::BlobCache;
use crate::config::Config;
use crate::error::{ProxyError, Result};
use crate::upstream::UpstreamClient;
use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde_json::json;
use std::sync::Arc;
use tracing::{debug, info};

pub struct RegistryState {
    pub config: Config,
    pub upstream: UpstreamClient,
    pub cache: Arc<BlobCache>,
}

pub async fn handle_version_check() -> impl IntoResponse {
    Json(json!({}))
}

pub async fn handle_get_manifest(
    State(state): State<Arc<RegistryState>>,
    Extension(claims): Extension<Claims>,
    Path((repository, reference)): Path<(String, String)>,
) -> Result<Response> {
    info!(
        "GET manifest request: repository={}, reference={}",
        repository, reference
    );

    check_repository_access(&claims, &repository)?;

    let resolved = state
        .config
        .resolve_repository(&repository)
        .ok_or_else(|| ProxyError::NotFound(format!("Repository not mapped: {}", repository)))?;

    let (manifest_data, content_type) = state.upstream.get_manifest(&resolved, &reference).await?;

    debug!(
        "Retrieved manifest for {}/{}: {} bytes",
        repository,
        reference,
        manifest_data.len()
    );

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, manifest_data.len())
        .body(Body::from(manifest_data))
        .unwrap())
}

pub async fn handle_get_blob(
    State(state): State<Arc<RegistryState>>,
    Extension(claims): Extension<Claims>,
    Path((repository, digest)): Path<(String, String)>,
) -> Result<Response> {
    info!(
        "GET blob request: repository={}, digest={}",
        repository, digest
    );

    check_repository_access(&claims, &repository)?;

    let resolved = state
        .config
        .resolve_repository(&repository)
        .ok_or_else(|| ProxyError::NotFound(format!("Repository not mapped: {}", repository)))?;

    if let Some(cached_data) = state.cache.get(&digest).await? {
        debug!("Serving blob {} from cache", digest);
        return Ok(Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .header(header::CONTENT_LENGTH, cached_data.len())
            .body(Body::from(cached_data))
            .unwrap());
    }

    debug!("Cache miss for blob {}, fetching from upstream", digest);

    let blob_data = state.upstream.get_blob(&resolved, &digest).await?;

    if let Err(e) = state.cache.put(&digest, blob_data.clone()).await {
        tracing::warn!("Failed to cache blob {}: {}", digest, e);
    }

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_LENGTH, blob_data.len())
        .body(Body::from(blob_data))
        .unwrap())
}

pub async fn handle_head_blob(
    State(state): State<Arc<RegistryState>>,
    Extension(claims): Extension<Claims>,
    Path((repository, digest)): Path<(String, String)>,
) -> Result<Response> {
    info!(
        "HEAD blob request: repository={}, digest={}",
        repository, digest
    );

    check_repository_access(&claims, &repository)?;

    let resolved = state
        .config
        .resolve_repository(&repository)
        .ok_or_else(|| ProxyError::NotFound(format!("Repository not mapped: {}", repository)))?;

    if let Some(cached_data) = state.cache.get(&digest).await? {
        debug!("Blob {} found in cache", digest);
        return Ok(Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .header(header::CONTENT_LENGTH, cached_data.len())
            .body(Body::empty())
            .unwrap());
    }

    let blob_data = state.upstream.get_blob(&resolved, &digest).await?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_LENGTH, blob_data.len())
        .body(Body::empty())
        .unwrap())
}

pub async fn handle_get_tags(
    State(state): State<Arc<RegistryState>>,
    Extension(claims): Extension<Claims>,
    Path(repository): Path<String>,
) -> Result<Response> {
    info!("GET tags request: repository={}", repository);

    check_repository_access(&claims, &repository)?;

    let resolved = state
        .config
        .resolve_repository(&repository)
        .ok_or_else(|| ProxyError::NotFound(format!("Repository not mapped: {}", repository)))?;

    let tags_data = state.upstream.get_tags(&resolved).await?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(tags_data))
        .unwrap())
}

pub async fn handle_unsupported_write() -> Result<Response> {
    Err(ProxyError::Forbidden(
        "Write operations are not supported by this proxy".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::AccessLevel;

    #[test]
    fn test_check_access_with_all_permission() {
        let claims = Claims {
            sub: "user".to_string(),
            exp: None,
            access: AccessLevel::All,
        };

        assert!(check_repository_access(&claims, "any/repo").is_ok());
    }

    #[test]
    fn test_check_access_with_specific_repos() {
        let claims = Claims {
            sub: "user".to_string(),
            exp: None,
            access: AccessLevel::Repositories {
                repos: vec!["allowed".to_string()],
            },
        };

        assert!(check_repository_access(&claims, "allowed").is_ok());
        assert!(check_repository_access(&claims, "denied").is_err());
    }
}
