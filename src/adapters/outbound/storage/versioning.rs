use async_trait::async_trait;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures::stream::BoxStream;
use object_store::{MultipartUpload, PutPayload, PutResult};
use object_store::{ObjectMeta, ObjectStore, path::Path};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

use crate::adapters::outbound::storage::error::StoreError;

/// Metadata about a single version of an object
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionMetadata {
    /// Unique version ID
    pub version_id: String,

    /// When this version was created
    pub created_at: DateTime<Utc>,

    /// Size of the object in bytes
    pub size: usize,

    /// Optional ETag
    pub etag: Option<String>,

    /// Optional user-provided metadata
    pub user_metadata: Option<HashMap<String, String>>,
}

#[derive(Debug)]
/// An enhanced ObjectStore with automatic versioning support
pub struct VersionedStore<T: ObjectStore> {
    /// The underlying object store
    inner: T,

    /// In-memory cache of version metadata
    /// Maps object path -> list of versions (ordered by creation time)
    versions: Arc<RwLock<HashMap<String, Vec<VersionMetadata>>>>,

    /// Whether versioning is enabled
    versioning_enabled: bool,
}

impl<T: ObjectStore> std::fmt::Display for VersionedStore<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "VersionedStore")
    }
}

impl<T: ObjectStore> VersionedStore<T> {
    pub fn new(store: T) -> Self {
        VersionedStore {
            inner: store,
            versions: Arc::new(RwLock::new(HashMap::new())),
            versioning_enabled: true,
        }
    }

    pub fn enable_versioning(&mut self, enabled: bool) {
        self.versioning_enabled = enabled;
    }

    /// Create a versioned path by appending a version ID to the original path
    fn versioned_path(&self, path: &Path, version_id: &str) -> Path {
        let path_str = path.as_ref().to_string();
        let versioned_path = format!("{}.v_{}", path_str, version_id);
        println!(
            "Created versioned path: {} from original path: {}",
            versioned_path, path_str
        );
        Path::from(versioned_path)
    }

    /// Get a specific version of an object
    pub async fn get_version(&self, path: &Path, version_id: &str) -> Result<Bytes, StoreError> {
        let versioned_path = self.versioned_path(path, version_id);

        // Get the object from the inner store
        let result = self.inner.get(&versioned_path).await?;

        // Convert to bytes
        Ok(result.bytes().await?)
    }

    /// List all versions of an object
    pub fn list_versions(&self, path: &Path) -> Result<Vec<VersionMetadata>, StoreError> {
        let versions = self
            .versions
            .read()
            .map_err(|e| StoreError::Other(format!("Failed to acquire read lock: {}", e)))?;

        let path_str = path.as_ref().to_string();
        println!("Listing versions for path: {}", path_str);

        // Print all keys in the versions map
        println!(
            "All keys in versions map: {:?}",
            versions.keys().collect::<Vec<_>>()
        );

        // Return cloned versions if they exist, otherwise empty vec
        let result = versions
            .get(&path_str)
            .map(|v| v.clone())
            .unwrap_or_else(Vec::new);

        println!("Found {} versions for path {}", result.len(), path_str);
        Ok(result)
    }

    /// Get metadata for a specific version
    pub fn get_version_metadata(
        &self,
        path: &Path,
        version_id: &str,
    ) -> Result<Option<VersionMetadata>, StoreError> {
        let versions = self
            .versions
            .read()
            .map_err(|e| StoreError::Other(format!("Failed to acquire read lock: {}", e)))?;

        let path_str = path.as_ref().to_string();

        if let Some(version_list) = versions.get(&path_str) {
            return Ok(version_list
                .iter()
                .find(|v| v.version_id == version_id)
                .cloned());
        }

        Ok(None)
    }

    /// Add version metadata to the versions map
    fn add_version_metadata(
        &self,
        path: &Path,
        version_id: String,
        size: usize,
        user_metadata: Option<HashMap<String, String>>,
        etag: Option<String>,
    ) -> Result<(), StoreError> {
        let mut versions = self
            .versions
            .write()
            .map_err(|e| StoreError::Other(format!("Failed to acquire write lock: {}", e)))?;

        let path_str = path.as_ref().to_string();
        println!(
            "Adding version metadata for path: {}, version: {}",
            path_str, version_id
        );

        // Create metadata entry
        let metadata = VersionMetadata {
            version_id,
            created_at: Utc::now(),
            size,
            etag,
            user_metadata,
        };

        // Get or create version list for this path
        let version_list = versions.entry(path_str.clone()).or_insert_with(Vec::new);

        // Add the new version
        version_list.push(metadata);

        println!(
            "Current versions for path {}: {}",
            path_str,
            version_list.len()
        );

        Ok(())
    }

    /// Delete a specific version of an object
    pub async fn delete_version(&self, path: &Path, version_id: &str) -> Result<(), StoreError> {
        let versioned_path = self.versioned_path(path, version_id);
        println!(
            "Deleting version for path: {}, version: {}",
            path.as_ref(),
            version_id
        );

        // Delete from the underlying store
        println!("Deleting from underlying store: {}", versioned_path);
        let result = self.inner.delete(&versioned_path).await;
        match &result {
            Ok(_) => println!(
                "Successfully deleted from underlying store: {}",
                versioned_path
            ),
            Err(e) => println!(
                "Failed to delete from underlying store: {} - error: {}",
                versioned_path, e
            ),
        }
        result?;

        // Update metadata
        let mut versions = self
            .versions
            .write()
            .map_err(|e| StoreError::Other(format!("Failed to acquire write lock: {}", e)))?;

        let path_str = path.as_ref().to_string();
        println!(
            "All keys in versions map before deletion: {:?}",
            versions.keys().collect::<Vec<_>>()
        );

        if let Some(version_list) = versions.get_mut(&path_str) {
            println!(
                "Before deletion, versions for path {}: {:?}",
                path_str, version_list
            );

            // Remove the version from the list
            version_list.retain(|v| v.version_id != version_id);

            println!(
                "After deletion, versions for path {}: {:?}",
                path_str, version_list
            );

            // If no versions left, remove the entry
            if version_list.is_empty() {
                println!("Removing path from versions map: {}", path_str);
                versions.remove(&path_str);
            }
        } else {
            println!("Path not found in versions map: {}", path_str);
        }

        Ok(())
    }
}

// Implement ObjectStore for VersionedStore to allow it to be used as a regular store
#[async_trait]
impl<T: ObjectStore + Send + Sync> ObjectStore for VersionedStore<T> {
    async fn put(&self, location: &Path, bytes: PutPayload) -> object_store::Result<PutResult> {
        if self.versioning_enabled {
            // Generate a new version ID
            let version_id = Uuid::new_v4().to_string();

            // Store at versioned path
            let versioned_path = self.versioned_path(location, &version_id);
            self.inner.put(&versioned_path, bytes.clone()).await?;

            // Update version metadata
            self.add_version_metadata(location, version_id, bytes.content_length(), None, None)
                .map_err(|e| object_store::Error::Generic {
                    store: "versioned",
                    source: Box::new(e),
                })?;

            // Also store at the original path for compatibility
            self.inner.put(location, bytes).await
        } else {
            // If versioning is disabled, just pass through
            self.inner.put(location, bytes).await
        }
    }

    async fn put_opts(
        &self,
        location: &Path,
        bytes: PutPayload,
        options: object_store::PutOptions,
    ) -> object_store::Result<PutResult> {
        if self.versioning_enabled {
            // Generate a new version ID
            let version_id = Uuid::new_v4().to_string();

            // Store at versioned path
            let versioned_path = self.versioned_path(location, &version_id);
            self.inner
                .put_opts(&versioned_path, bytes.clone(), options.clone())
                .await?;

            // Update version metadata
            self.add_version_metadata(location, version_id, bytes.content_length(), None, None)
                .map_err(|e| object_store::Error::Generic {
                    store: "versioned",
                    source: Box::new(e),
                })?;

            // Also store at the original path for compatibility
            self.inner.put_opts(location, bytes, options).await
        } else {
            // If versioning is disabled, just pass
            self.inner.put_opts(location, bytes, options).await
        }
    }

    async fn put_multipart_opts(
        &self,
        location: &Path,
        options: object_store::PutMultipartOpts,
    ) -> object_store::Result<Box<dyn MultipartUpload>> {
        // Pass through to inner store
        self.inner.put_multipart_opts(location, options).await
    }

    async fn get(&self, location: &Path) -> object_store::Result<object_store::GetResult> {
        // Always get from the original path
        self.inner.get(location).await
    }

    async fn get_opts(
        &self,
        location: &Path,
        options: object_store::GetOptions,
    ) -> object_store::Result<object_store::GetResult> {
        // Always get from the original path
        self.inner.get_opts(location, options).await
    }

    async fn get_range(&self, location: &Path, range: Range<u64>) -> object_store::Result<Bytes> {
        // Pass through to inner store
        self.inner.get_range(location, range).await
    }

    async fn head(&self, location: &Path) -> object_store::Result<ObjectMeta> {
        // Pass through to inner store
        self.inner.head(location).await
    }

    async fn delete(&self, location: &Path) -> object_store::Result<()> {
        if self.versioning_enabled {
            // In versioned mode, we don't actually delete - we create a delete marker
            // but this simplified implementation will delete the latest version

            // First, delete from the original path
            self.inner.delete(location).await?;

            // Update version metadata - in a real implementation, we might
            // add a delete marker instead of actually removing the version
            let versions = self
                .versions
                .read()
                .map_err(|e| object_store::Error::Generic {
                    store: "versioned",
                    source: Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Failed to acquire read lock: {}", e),
                    )),
                })?;

            let path_str = location.as_ref().to_string();

            // If this object has versions, we should mark them as deleted
            // but for simplicity in this example, we'll do nothing
            if versions.contains_key(&path_str) {
                // In a real implementation, we'd mark all versions as deleted
                // or apply the deletion policy based on lifecycle rules
            }

            Ok(())
        } else {
            // If versioning is disabled, just pass through
            self.inner.delete(location).await
        }
    }

    fn list(&self, prefix: Option<&Path>) -> BoxStream<'static, object_store::Result<ObjectMeta>> {
        // Pass through to inner store
        self.inner.list(prefix)
    }

    async fn list_with_delimiter(
        &self,
        prefix: Option<&Path>,
    ) -> object_store::Result<object_store::ListResult> {
        // Pass through to inner store
        self.inner.list_with_delimiter(prefix).await
    }

    async fn copy(&self, from: &Path, to: &Path) -> object_store::Result<()> {
        if self.versioning_enabled {
            // Generate a new version ID for the destination
            let version_id = Uuid::new_v4().to_string();

            // Get the content
            let content = self.inner.get(from).await?;
            let put_payload = content.bytes().await?;
            let put_len = put_payload.len();

            // Store at versioned path
            let versioned_path = self.versioned_path(to, &version_id);
            self.inner.put(&versioned_path, put_payload.into()).await?;

            // Update version metadata
            self.add_version_metadata(to, version_id, put_len, None, None)
                .map_err(|e| object_store::Error::Generic {
                    store: "versioned",
                    source: Box::new(e),
                })?;

            // Also store at the original path
            self.inner.copy(from, to).await
        } else {
            // If versioning is disabled, just pass through
            self.inner.copy(from, to).await
        }
    }

    async fn copy_if_not_exists(&self, from: &Path, to: &Path) -> object_store::Result<()> {
        // For simplicity, just call copy
        // In a real implementation, we would handle the conditional logic
        self.copy(from, to).await
    }
}
