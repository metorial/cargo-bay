use crate::error::{ProxyError, Result};
use axum::{
    extract::{Request, State},
    http::HeaderMap,
    middleware::Next,
    response::Response,
};
use base64::{engine::general_purpose::STANDARD as base64_engine, Engine as _};
use jsonwebtoken::{decode, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub exp: Option<usize>,
    pub access: AccessLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum AccessLevel {
    All,
    Repositories { repos: Vec<String> },
}

impl AccessLevel {
    pub fn can_access(&self, repository: &str) -> bool {
        match self {
            AccessLevel::All => true,
            AccessLevel::Repositories { repos } => repos
                .iter()
                .any(|r| repository == r || repository.starts_with(&format!("{}/", r))),
        }
    }
}

pub struct AuthState {
    pub jwt_secret: String,
}

pub async fn auth_middleware(
    State(state): State<Arc<AuthState>>,
    headers: HeaderMap,
    mut request: Request,
    next: Next,
) -> Result<Response> {
    let token = extract_token(&headers).ok_or_else(|| {
        ProxyError::Unauthorized("Missing or invalid Authorization header".into())
    })?;

    let claims = validate_token(&token, &state.jwt_secret)?;

    request.extensions_mut().insert(claims);

    Ok(next.run(request).await)
}

fn extract_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get("Authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            if value.starts_with("Bearer ") {
                // Bearer token authentication
                Some(value[7..].to_string())
            } else if value.starts_with("Basic ") {
                // Basic authentication - decode and extract password (which should be the JWT)
                let encoded = &value[6..];
                base64_engine
                    .decode(encoded)
                    .ok()
                    .and_then(|decoded| String::from_utf8(decoded).ok())
                    .and_then(|credentials| {
                        // credentials format is "username:password"
                        // The password should be the JWT token
                        credentials.split_once(':').map(|(_, password)| password.to_string())
                    })
            } else {
                None
            }
        })
}

fn validate_token(token: &str, secret: &str) -> Result<Claims> {
    let mut validation = Validation::default();
    validation.required_spec_claims.clear();
    let decoding_key = DecodingKey::from_secret(secret.as_bytes());

    decode::<Claims>(token, &decoding_key, &validation)
        .map(|data| data.claims)
        .map_err(|e| ProxyError::Unauthorized(format!("Invalid token: {}", e)))
}

pub fn check_repository_access(claims: &Claims, repository: &str) -> Result<()> {
    if claims.access.can_access(repository) {
        Ok(())
    } else {
        Err(ProxyError::Forbidden(format!(
            "Access denied to repository: {}",
            repository
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{encode, EncodingKey, Header};

    #[test]
    fn test_access_level_all() {
        let access = AccessLevel::All;
        assert!(access.can_access("any/repository"));
        assert!(access.can_access("another/one"));
    }

    #[test]
    fn test_access_level_specific_repos() {
        let access = AccessLevel::Repositories {
            repos: vec!["myapp".to_string(), "team/app".to_string()],
        };

        assert!(access.can_access("myapp"));
        assert!(access.can_access("team/app"));
        assert!(access.can_access("team/app/subpath"));
        assert!(!access.can_access("other"));
        assert!(!access.can_access("team/other"));
    }

    #[test]
    fn test_token_validation() {
        let secret = "test-secret";
        let claims = Claims {
            sub: "user123".to_string(),
            exp: None,
            access: AccessLevel::All,
        };

        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap();

        let result = validate_token(&token, secret);
        assert!(result.is_ok());

        let decoded = result.unwrap();
        assert_eq!(decoded.sub, "user123");
    }

    #[test]
    fn test_invalid_token() {
        let result = validate_token("invalid.token.here", "secret");
        assert!(result.is_err());
    }

    #[test]
    fn test_check_repository_access() {
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

    #[test]
    fn test_extract_bearer_token() {
        use axum::http::HeaderValue;
        let mut headers = HeaderMap::new();
        headers.insert(
            "Authorization",
            HeaderValue::from_str("Bearer test-token-123").unwrap(),
        );

        let token = extract_token(&headers);
        assert_eq!(token, Some("test-token-123".to_string()));
    }

    #[test]
    fn test_extract_basic_auth_token() {
        use axum::http::HeaderValue;
        use base64::{engine::general_purpose::STANDARD as base64_engine, Engine as _};

        let mut headers = HeaderMap::new();
        // Basic auth with username "user" and password "jwt-token-here"
        let credentials = "user:jwt-token-here";
        let encoded = base64_engine.encode(credentials);
        headers.insert(
            "Authorization",
            HeaderValue::from_str(&format!("Basic {}", encoded)).unwrap(),
        );

        let token = extract_token(&headers);
        assert_eq!(token, Some("jwt-token-here".to_string()));
    }
}
