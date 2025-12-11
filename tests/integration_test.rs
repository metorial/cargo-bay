use jsonwebtoken::{encode, EncodingKey, Header};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,
    exp: Option<usize>,
    access: AccessLevel,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum AccessLevel {
    All,
    Repositories { repos: Vec<String> },
}

#[test]
fn test_jwt_generation() {
    let secret = "test-secret";
    let claims = Claims {
        sub: "test-user".to_string(),
        exp: None,
        access: AccessLevel::All,
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    );

    assert!(token.is_ok());
    let token_str = token.unwrap();
    assert!(!token_str.is_empty());
    assert!(token_str.contains('.'));
}

#[test]
fn test_jwt_with_specific_repos() {
    let secret = "test-secret";
    let claims = Claims {
        sub: "test-user".to_string(),
        exp: None,
        access: AccessLevel::Repositories {
            repos: vec!["repo1".to_string(), "repo2".to_string()],
        },
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    );

    assert!(token.is_ok());
}
