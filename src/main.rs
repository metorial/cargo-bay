mod auth;
mod cache;
mod config;
mod error;
mod registry;
mod upstream;

use crate::auth::{auth_middleware, AuthState};
use crate::cache::BlobCache;
use crate::config::Config;
use crate::registry::RegistryState;
use crate::upstream::UpstreamClient;
use axum::{
    middleware,
    routing::{get, put},
    Router,
};
use std::sync::Arc;
use tower_http::trace::TraceLayer;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "docker_registry_proxy=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config_path = std::env::var("CONFIG_PATH").unwrap_or_else(|_| "config.toml".to_string());
    let config = Config::from_file(&config_path)?;

    info!("Starting Docker Registry Proxy");
    info!("Cache directory: {:?}", config.cache.directory);
    info!(
        "Cache limits: max_size={} bytes, max_age={} seconds",
        config.cache.max_size_bytes, config.cache.max_age_seconds
    );
    info!("Configured {} upstream registries", config.registries.len());
    info!(
        "Configured {} repository mappings",
        config.repositories.len()
    );

    let cache = Arc::new(BlobCache::new(config.cache.clone()).await?);
    BlobCache::start_cleanup_task(cache.clone()).await;

    let upstream = UpstreamClient::new();

    let registry_state = Arc::new(RegistryState {
        config: config.clone(),
        upstream,
        cache,
    });

    let auth_state = Arc::new(AuthState {
        jwt_secret: config.auth.jwt_secret.clone(),
    });

    let app = Router::new()
        .route("/v2/", get(registry::handle_version_check))
        .route(
            "/v2/:repository/manifests/:reference",
            get(registry::handle_get_manifest)
                .put(registry::handle_unsupported_write)
                .delete(registry::handle_unsupported_write),
        )
        .route(
            "/v2/:repository/blobs/:digest",
            get(registry::handle_get_blob)
                .head(registry::handle_head_blob)
                .delete(registry::handle_unsupported_write),
        )
        .route(
            "/v2/:repository/blobs/uploads/",
            put(registry::handle_unsupported_write),
        )
        .route("/v2/:repository/tags/list", get(registry::handle_get_tags))
        .layer(middleware::from_fn_with_state(
            auth_state.clone(),
            auth_middleware,
        ))
        .layer(TraceLayer::new_for_http())
        .with_state(registry_state);

    let bind_addr = format!("{}:{}", config.server.bind_address, config.server.port);
    info!("Listening on {}", bind_addr);

    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
