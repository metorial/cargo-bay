use crate::config::{ResolvedRepository, UpstreamAuth};
use crate::error::{ProxyError, Result};
use bytes::Bytes;
use reqwest::{header, Client, Response, StatusCode};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AuthToken {
    token: Option<String>,
    access_token: Option<String>,
}

pub struct UpstreamClient {
    client: Client,
    tokens: Arc<RwLock<HashMap<String, String>>>,
}

impl UpstreamClient {
    pub fn new() -> Self {
        let client = Client::builder()
            .user_agent("docker-registry-proxy/0.1.0")
            .build()
            .unwrap_or_default();

        Self {
            client,
            tokens: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn get_manifest(
        &self,
        repo: &ResolvedRepository,
        reference: &str,
    ) -> Result<(Bytes, String)> {
        let url = format!(
            "{}/v2/{}/manifests/{}",
            repo.registry_url, repo.upstream_name, reference
        );

        let response = self.make_authenticated_request(repo, &url, true).await?;

        if response.status() == StatusCode::NOT_FOUND {
            return Err(ProxyError::NotFound(format!(
                "Manifest not found: {}",
                reference
            )));
        }

        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/vnd.docker.distribution.manifest.v2+json")
            .to_string();

        let bytes = response
            .bytes()
            .await
            .map_err(|e| ProxyError::Upstream(e))?;

        Ok((bytes, content_type))
    }

    pub async fn get_blob(&self, repo: &ResolvedRepository, digest: &str) -> Result<Bytes> {
        let url = format!(
            "{}/v2/{}/blobs/{}",
            repo.registry_url, repo.upstream_name, digest
        );

        let response = self.make_authenticated_request(repo, &url, false).await?;

        if response.status() == StatusCode::NOT_FOUND {
            return Err(ProxyError::NotFound(format!("Blob not found: {}", digest)));
        }

        response.bytes().await.map_err(|e| ProxyError::Upstream(e))
    }

    pub async fn get_tags(&self, repo: &ResolvedRepository) -> Result<Bytes> {
        let url = format!("{}/v2/{}/tags/list", repo.registry_url, repo.upstream_name);

        let response = self.make_authenticated_request(repo, &url, false).await?;

        response.bytes().await.map_err(|e| ProxyError::Upstream(e))
    }

    async fn make_authenticated_request(
        &self,
        repo: &ResolvedRepository,
        url: &str,
        include_manifest_headers: bool,
    ) -> Result<Response> {
        let mut request = self.client.get(url);

        if include_manifest_headers {
            request = request
                .header(
                    header::ACCEPT,
                    "application/vnd.docker.distribution.manifest.v2+json",
                )
                .header(
                    header::ACCEPT,
                    "application/vnd.docker.distribution.manifest.list.v2+json",
                )
                .header(header::ACCEPT, "application/vnd.oci.image.manifest.v1+json")
                .header(header::ACCEPT, "application/vnd.oci.image.index.v1+json");
        }

        let cache_key = format!("{}:{}", repo.registry_url, repo.upstream_name);

        {
            let tokens = self.tokens.read().await;
            if let Some(token) = tokens.get(&cache_key) {
                request = request.bearer_auth(token);
            }
        }

        let response = request.send().await?;

        if response.status() == StatusCode::UNAUTHORIZED {
            debug!("Received 401, attempting authentication");

            if let Some(auth_header) = response.headers().get(header::WWW_AUTHENTICATE) {
                let auth_str = auth_header
                    .to_str()
                    .map_err(|_| ProxyError::Internal("Invalid WWW-Authenticate header".into()))?;

                let token = self.authenticate(auth_str, repo.auth.as_ref()).await?;

                {
                    let mut tokens = self.tokens.write().await;
                    tokens.insert(cache_key, token.clone());
                }

                let mut retry_request = self.client.get(url).bearer_auth(&token);

                if include_manifest_headers {
                    retry_request = retry_request
                        .header(
                            header::ACCEPT,
                            "application/vnd.docker.distribution.manifest.v2+json",
                        )
                        .header(
                            header::ACCEPT,
                            "application/vnd.docker.distribution.manifest.list.v2+json",
                        )
                        .header(header::ACCEPT, "application/vnd.oci.image.manifest.v1+json")
                        .header(header::ACCEPT, "application/vnd.oci.image.index.v1+json");
                }

                return Ok(retry_request.send().await?);
            }
        }

        Ok(response)
    }

    async fn authenticate(
        &self,
        www_authenticate: &str,
        upstream_auth: Option<&UpstreamAuth>,
    ) -> Result<String> {
        let params = parse_www_authenticate(www_authenticate)?;

        let realm = params
            .get("realm")
            .ok_or_else(|| ProxyError::Internal("WWW-Authenticate header missing realm".into()))?;

        let mut auth_url = reqwest::Url::parse(realm)
            .map_err(|_| ProxyError::Internal("Invalid realm URL".into()))?;

        if let Some(service) = params.get("service") {
            auth_url.query_pairs_mut().append_pair("service", service);
        }

        if let Some(scope) = params.get("scope") {
            auth_url.query_pairs_mut().append_pair("scope", scope);
        }

        let mut request = self.client.get(auth_url);

        if let Some(auth) = upstream_auth {
            request = request.basic_auth(&auth.username, Some(&auth.password));
        }

        let response = request.send().await?;

        if !response.status().is_success() {
            return Err(ProxyError::Internal(format!(
                "Authentication failed with status: {}",
                response.status()
            )));
        }

        let auth_response: AuthToken = response.json().await?;

        auth_response
            .token
            .or(auth_response.access_token)
            .ok_or_else(|| ProxyError::Internal("No token in auth response".into()))
    }
}

fn parse_www_authenticate(header: &str) -> Result<HashMap<String, String>> {
    let mut params = HashMap::new();

    let header = header.trim();
    if !header.starts_with("Bearer ") {
        return Ok(params);
    }

    let params_str = &header[7..];

    for part in params_str.split(',') {
        let part = part.trim();
        if let Some(eq_pos) = part.find('=') {
            let key = part[..eq_pos].trim().to_string();
            let value = part[eq_pos + 1..].trim().trim_matches('"').to_string();
            params.insert(key, value);
        }
    }

    Ok(params)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_www_authenticate() {
        let header = r#"Bearer realm="https://auth.docker.io/token",service="registry.docker.io",scope="repository:library/alpine:pull""#;
        let params = parse_www_authenticate(header).unwrap();

        assert_eq!(params.get("realm").unwrap(), "https://auth.docker.io/token");
        assert_eq!(params.get("service").unwrap(), "registry.docker.io");
        assert_eq!(
            params.get("scope").unwrap(),
            "repository:library/alpine:pull"
        );
    }

    #[test]
    fn test_parse_www_authenticate_without_bearer() {
        let header = "Basic realm=\"test\"";
        let params = parse_www_authenticate(header).unwrap();
        assert!(params.is_empty());
    }
}
