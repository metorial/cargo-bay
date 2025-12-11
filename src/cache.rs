use crate::config::CacheConfig;
use crate::error::{ProxyError, Result};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheEntry {
    digest: String,
    size: u64,
    last_accessed: DateTime<Utc>,
    created: DateTime<Utc>,
}

pub struct BlobCache {
    config: CacheConfig,
    db: Arc<sled::Db>,
    total_size: Arc<RwLock<u64>>,
}

impl BlobCache {
    pub async fn new(config: CacheConfig) -> Result<Self> {
        fs::create_dir_all(&config.directory)
            .await
            .map_err(|e| ProxyError::Cache(format!("Failed to create cache directory: {}", e)))?;

        let db_path = config.directory.join("metadata");
        let db = sled::open(db_path)
            .map_err(|e| ProxyError::Cache(format!("Failed to open cache database: {}", e)))?;

        let total_size = Self::calculate_total_size(&db)?;

        Ok(Self {
            config,
            db: Arc::new(db),
            total_size: Arc::new(RwLock::new(total_size)),
        })
    }

    fn calculate_total_size(db: &sled::Db) -> Result<u64> {
        let mut size = 0u64;
        for item in db.iter() {
            if let Ok((_, value)) = item {
                if let Ok(entry) = serde_json::from_slice::<CacheEntry>(&value) {
                    size += entry.size;
                }
            }
        }
        Ok(size)
    }

    pub async fn get(&self, digest: &str) -> Result<Option<Bytes>> {
        let key = digest.as_bytes();

        let entry_data = match self.db.get(key) {
            Ok(Some(data)) => data,
            Ok(None) => return Ok(None),
            Err(e) => {
                return Err(ProxyError::Cache(format!(
                    "Failed to read cache metadata: {}",
                    e
                )))
            }
        };

        let mut entry: CacheEntry = serde_json::from_slice(&entry_data)
            .map_err(|e| ProxyError::Cache(format!("Failed to parse cache entry: {}", e)))?;

        let blob_path = self.blob_path(digest);

        if !blob_path.exists() {
            warn!("Cache entry exists but blob file missing: {}", digest);
            let _ = self.db.remove(key);
            let mut total = self.total_size.write().await;
            *total = total.saturating_sub(entry.size);
            return Ok(None);
        }

        match fs::read(&blob_path).await {
            Ok(data) => {
                entry.last_accessed = Utc::now();
                if let Ok(updated) = serde_json::to_vec(&entry) {
                    let _ = self.db.insert(key, updated);
                }
                debug!("Cache hit for digest: {}", digest);
                Ok(Some(Bytes::from(data)))
            }
            Err(e) => {
                error!("Failed to read cached blob {}: {}", digest, e);
                Ok(None)
            }
        }
    }

    pub async fn put(&self, digest: &str, data: Bytes) -> Result<()> {
        let size = data.len() as u64;
        let blob_path = self.blob_path(digest);

        if let Some(parent) = blob_path.parent() {
            fs::create_dir_all(parent).await.map_err(|e| {
                ProxyError::Cache(format!("Failed to create cache subdirectory: {}", e))
            })?;
        }

        let mut file = fs::File::create(&blob_path)
            .await
            .map_err(|e| ProxyError::Cache(format!("Failed to create cache file: {}", e)))?;

        file.write_all(&data)
            .await
            .map_err(|e| ProxyError::Cache(format!("Failed to write cache file: {}", e)))?;

        file.sync_all()
            .await
            .map_err(|e| ProxyError::Cache(format!("Failed to sync cache file: {}", e)))?;

        let entry = CacheEntry {
            digest: digest.to_string(),
            size,
            last_accessed: Utc::now(),
            created: Utc::now(),
        };

        let entry_data = serde_json::to_vec(&entry)
            .map_err(|e| ProxyError::Cache(format!("Failed to serialize cache entry: {}", e)))?;

        self.db
            .insert(digest.as_bytes(), entry_data)
            .map_err(|e| ProxyError::Cache(format!("Failed to store cache metadata: {}", e)))?;

        let mut total = self.total_size.write().await;
        *total += size;

        debug!("Cached blob {} ({} bytes)", digest, size);

        Ok(())
    }

    pub async fn cleanup(&self) -> Result<()> {
        info!("Starting cache cleanup");

        let max_age = chrono::Duration::seconds(self.config.max_age_seconds as i64);
        let now = Utc::now();
        let mut entries_to_remove = Vec::new();
        let mut size_ordered_entries: Vec<CacheEntry> = Vec::new();

        for item in self.db.iter() {
            if let Ok((key, value)) = item {
                if let Ok(entry) = serde_json::from_slice::<CacheEntry>(&value) {
                    if now - entry.last_accessed > max_age {
                        entries_to_remove.push((key.to_vec(), entry));
                    } else {
                        size_ordered_entries.push(entry);
                    }
                }
            }
        }

        for (key, entry) in &entries_to_remove {
            if let Err(e) = self.remove_entry(key, entry).await {
                error!("Failed to remove expired entry {}: {}", entry.digest, e);
            } else {
                debug!("Removed expired entry: {}", entry.digest);
            }
        }

        let current_size = *self.total_size.read().await;
        if current_size > self.config.max_size_bytes {
            size_ordered_entries.sort_by_key(|e| e.last_accessed);

            let mut removed_size = 0u64;
            let target_size = (self.config.max_size_bytes as f64 * 0.9) as u64;

            for entry in size_ordered_entries {
                if current_size - removed_size <= target_size {
                    break;
                }

                if let Err(e) = self.remove_entry(entry.digest.as_bytes(), &entry).await {
                    error!("Failed to remove entry {}: {}", entry.digest, e);
                } else {
                    removed_size += entry.size;
                    debug!("Removed entry to free space: {}", entry.digest);
                }
            }

            info!(
                "Cache cleanup: removed {} bytes to meet size limit",
                removed_size
            );
        }

        let final_size = *self.total_size.read().await;
        info!(
            "Cache cleanup completed. Total size: {} bytes, entries removed: {}",
            final_size,
            entries_to_remove.len()
        );

        Ok(())
    }

    async fn remove_entry(&self, key: &[u8], entry: &CacheEntry) -> Result<()> {
        let blob_path = self.blob_path(&entry.digest);

        if blob_path.exists() {
            fs::remove_file(&blob_path)
                .await
                .map_err(|e| ProxyError::Cache(format!("Failed to remove blob file: {}", e)))?;
        }

        self.db
            .remove(key)
            .map_err(|e| ProxyError::Cache(format!("Failed to remove cache entry: {}", e)))?;

        let mut total = self.total_size.write().await;
        *total = total.saturating_sub(entry.size);

        Ok(())
    }

    fn blob_path(&self, digest: &str) -> PathBuf {
        let digest_clean = digest.replace(':', "_");
        let prefix = &digest_clean[..std::cmp::min(2, digest_clean.len())];
        self.config
            .directory
            .join("blobs")
            .join(prefix)
            .join(digest_clean)
    }

    pub async fn start_cleanup_task(cache: Arc<BlobCache>) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                if let Err(e) = cache.cleanup().await {
                    error!("Cache cleanup failed: {}", e);
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn create_test_cache() -> (BlobCache, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let config = CacheConfig {
            directory: temp_dir.path().to_path_buf(),
            max_size_bytes: 1024 * 1024,
            max_age_seconds: 3600,
        };
        let cache = BlobCache::new(config).await.unwrap();
        (cache, temp_dir)
    }

    #[tokio::test]
    async fn test_cache_put_and_get() {
        let (cache, _temp) = create_test_cache().await;
        let digest = "sha256:abc123";
        let data = Bytes::from("test data");

        cache.put(digest, data.clone()).await.unwrap();

        let retrieved = cache.get(digest).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap(), data);
    }

    #[tokio::test]
    async fn test_cache_miss() {
        let (cache, _temp) = create_test_cache().await;
        let result = cache.get("sha256:nonexistent").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_cache_cleanup_by_age() {
        let temp_dir = TempDir::new().unwrap();
        let config = CacheConfig {
            directory: temp_dir.path().to_path_buf(),
            max_size_bytes: 1024 * 1024,
            max_age_seconds: 1,
        };
        let cache = BlobCache::new(config).await.unwrap();

        let digest = "sha256:old";
        let data = Bytes::from("old data");
        cache.put(digest, data).await.unwrap();

        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        cache.cleanup().await.unwrap();

        let result = cache.get(digest).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_total_size_tracking() {
        let (cache, _temp) = create_test_cache().await;

        let data1 = Bytes::from(vec![0u8; 100]);
        let data2 = Bytes::from(vec![0u8; 200]);

        cache.put("sha256:test1", data1).await.unwrap();
        cache.put("sha256:test2", data2).await.unwrap();

        let total = *cache.total_size.read().await;
        assert_eq!(total, 300);
    }
}
