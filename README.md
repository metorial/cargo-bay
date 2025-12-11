# Cargo Bay

A read-only Docker registry proxy with JWT authentication and local blob caching. Great for providing controlled access to upstream Docker registries for on-premise environments.

Published to GitHub Container Registry as `ghcr.io/metorial/cargo-bay`.

## Features

- JWT-based authentication with per-repository access control
- Local repository name mapping to upstream registries
- Automatic blob caching with size and age limits
- Support for authenticated upstream registries
- Read-only operations (only pull allowed; push is always denied)

## Configuration

The service is configured using a TOML file. Copy `config.example.toml` to `config.toml` and adjust:

### Server Settings

```toml
[server]
bind_address = "0.0.0.0"
port = 5000
```

### Authentication

```toml
[auth]
jwt_secret = "your-secret-key-change-this-in-production"
```

### Cache Configuration

```toml
[cache]
directory = "/var/cache/docker-registry-proxy"
max_size_bytes = 10737418240  # 10 GB
max_age_seconds = 604800       # 7 days
```

The cache automatically cleans up entries that exceed the age limit or when the total size exceeds the configured maximum.

### Registry Configuration

Define upstream registries that the proxy will connect to:

```toml
[[registries]]
id = "dockerhub"
url = "https://registry-1.docker.io"

[[registries]]
id = "private-registry"
url = "https://private-registry.example.com"

[registries.auth]
username = "registry-user"
password = "registry-password"
```

### Repository Mapping

Map local repository names to upstream registries:

```toml
[[repositories]]
name = "alpine"
registry_id = "dockerhub"
upstream_name = "library/alpine"

[[repositories]]
name = "myapp"
registry_id = "private-registry"
upstream_name = "team/application"
```

With this configuration, clients can pull `alpine` from Docker Hub using:
```bash
docker pull localhost:5000/alpine:latest
```

### Environment Variables

- `CONFIG_PATH`: Path to the configuration file (default: `config.toml`)
- `RUST_LOG`: Log level (default: `docker_registry_proxy=info`)

## Running the Service

### Using Docker

```bash
docker pull ghcr.io/metorial/cargo-bay:latest

docker run -d \
  -p 5000:5000 \
  -v $(pwd)/config.toml:/app/config.toml:ro \
  -v /var/cache/docker-registry-proxy:/var/cache/docker-registry-proxy \
  ghcr.io/metorial/cargo-bay:latest
```

## Authentication

Generate JWT tokens for Docker client authentication:

```bash
cargo run --example generate_jwt -- <secret> <username>

cargo run --example generate_jwt -- <secret> <username> alpine,nginx
```

Use the generated token with Docker:

```bash
docker login localhost:5000
# Username: <username>
# Password: <generated-jwt-token>

docker pull localhost:5000/alpine:latest
```

## API Endpoints

The proxy implements the Docker Registry HTTP API V2:

- `GET /v2/` - Version check and authentication
- `GET /v2/{repository}/manifests/{reference}` - Fetch image manifest
- `GET /v2/{repository}/blobs/{digest}` - Fetch blob (with caching)
- `HEAD /v2/{repository}/blobs/{digest}` - Check blob existence
- `GET /v2/{repository}/tags/list` - List available tags

Write operations (PUT, DELETE) return a 403 Forbidden response.

## License

Licensed under the Apache License, Version 2.0. See the [LICENSE](LICENSE) file for details.
