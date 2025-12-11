use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub server: ServerConfig,
    pub auth: AuthConfig,
    pub cache: CacheConfig,
    #[serde(default)]
    pub registries: Vec<Registry>,
    #[serde(default)]
    pub repositories: Vec<Repository>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    #[serde(default = "default_bind_address")]
    pub bind_address: String,
    #[serde(default = "default_port")]
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuthConfig {
    pub jwt_secret: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CacheConfig {
    pub directory: PathBuf,
    pub max_size_bytes: u64,
    pub max_age_seconds: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Registry {
    pub id: String,
    pub url: String,
    pub auth: Option<UpstreamAuth>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Repository {
    pub name: String,
    pub registry_id: String,
    pub upstream_name: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UpstreamAuth {
    pub username: String,
    pub password: String,
}

pub struct ResolvedRepository {
    pub upstream_name: String,
    pub registry_url: String,
    pub auth: Option<UpstreamAuth>,
}

fn default_bind_address() -> String {
    "0.0.0.0".to_string()
}

fn default_port() -> u16 {
    5000
}

impl Config {
    pub fn from_file(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> anyhow::Result<()> {
        let registry_ids: std::collections::HashSet<_> =
            self.registries.iter().map(|r| &r.id).collect();

        for repo in &self.repositories {
            if !registry_ids.contains(&repo.registry_id) {
                anyhow::bail!(
                    "Repository '{}' references unknown registry_id '{}'",
                    repo.name,
                    repo.registry_id
                );
            }
        }

        Ok(())
    }

    pub fn resolve_repository(&self, repository_name: &str) -> Option<ResolvedRepository> {
        let repo = self
            .repositories
            .iter()
            .find(|r| r.name == repository_name)?;

        let registry = self.registries.iter().find(|r| r.id == repo.registry_id)?;

        Some(ResolvedRepository {
            upstream_name: repo.upstream_name.clone(),
            registry_url: registry.url.clone(),
            auth: registry.auth.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_config_parsing() {
        let config_toml = r#"
[server]
bind_address = "127.0.0.1"
port = 8080

[auth]
jwt_secret = "test-secret"

[cache]
directory = "/tmp/cache"
max_size_bytes = 1073741824
max_age_seconds = 86400

[[registries]]
id = "dockerhub"
url = "https://registry-1.docker.io"

[[registries]]
id = "private"
url = "https://private-registry.example.com"

[registries.auth]
username = "user"
password = "pass"

[[repositories]]
name = "myapp"
registry_id = "dockerhub"
upstream_name = "library/myapp"

[[repositories]]
name = "private/app"
registry_id = "private"
upstream_name = "team/app"
"#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(config_toml.as_bytes()).unwrap();
        temp_file.flush().unwrap();

        let config = Config::from_file(temp_file.path().to_str().unwrap()).unwrap();

        assert_eq!(config.server.bind_address, "127.0.0.1");
        assert_eq!(config.server.port, 8080);
        assert_eq!(config.auth.jwt_secret, "test-secret");
        assert_eq!(config.registries.len(), 2);
        assert_eq!(config.repositories.len(), 2);

        let resolved = config.resolve_repository("myapp").unwrap();
        assert_eq!(resolved.upstream_name, "library/myapp");
        assert_eq!(resolved.registry_url, "https://registry-1.docker.io");
    }

    #[test]
    fn test_validation_invalid_registry_id() {
        let config_toml = r#"
[server]
bind_address = "127.0.0.1"
port = 8080

[auth]
jwt_secret = "test-secret"

[cache]
directory = "/tmp/cache"
max_size_bytes = 1073741824
max_age_seconds = 86400

[[registries]]
id = "dockerhub"
url = "https://registry-1.docker.io"

[[repositories]]
name = "myapp"
registry_id = "nonexistent"
upstream_name = "library/myapp"
"#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(config_toml.as_bytes()).unwrap();
        temp_file.flush().unwrap();

        let result = Config::from_file(temp_file.path().to_str().unwrap());
        assert!(result.is_err());
    }
}
