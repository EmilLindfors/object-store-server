use async_trait::async_trait;
use std::sync::Arc;

use crate::{
    domain::{
        errors::{StorageError, StorageResult},
        models::{CreateObjectRequest, GetObjectRequest, ObjectMetadata, StorageObject},
        value_objects::{ObjectKey, VersionId},
    },
    ports::{
        repositories::ObjectRepository,
        services::ObjectService,
        storage::{ObjectInfo, ObjectStore},
    },
};

/// Implementation of ObjectService for managing object storage operations
#[derive(Clone)]
pub struct ObjectServiceImpl {
    repository: Arc<dyn ObjectRepository>,
    store: Arc<dyn ObjectStore>,
}

impl ObjectServiceImpl {
    /// Create a new ObjectServiceImpl instance
    pub fn new(repository: Arc<dyn ObjectRepository>, store: Arc<dyn ObjectStore>) -> Self {
        Self { repository, store }
    }

    /// Calculate ETag for object data
    fn calculate_etag(&self, data: &[u8]) -> String {
        // Simple MD5 hash for ETag (in production, use proper hashing)
        format!("{:x}", md5::compute(data))
    }
}

#[async_trait]
impl ObjectService for ObjectServiceImpl {
    /// Create a new object
    async fn create_object(&self, request: CreateObjectRequest) -> StorageResult<StorageObject> {
        // Check if object already exists
        if self.repository.object_exists(&request.key).await? {
            return Err(StorageError::ObjectAlreadyExists {
                key: request.key.clone(),
            });
        }

        // Store the object data
        self.store
            .put_object(
                &request.key,
                request.data.clone(),
                request.content_type.as_deref(),
            )
            .await?;

        // Create metadata
        let metadata = ObjectMetadata {
            content_type: request.content_type.clone(),
            content_length: request.data.len() as u64,
            etag: Some(self.calculate_etag(&request.data)),
            last_modified: std::time::SystemTime::now(),
            custom_metadata: request.custom_metadata.clone(),
        };

        // Generate version ID for non-versioned object
        let version_id = VersionId::generate();

        // Save metadata
        self.repository
            .save_object_metadata(&request.key, &version_id, &metadata)
            .await?;

        Ok(StorageObject {
            key: request.key,
            data: request.data,
            metadata,
        })
    }

    /// Get an object
    async fn get_object(&self, request: GetObjectRequest) -> StorageResult<StorageObject> {
        // Get metadata first
        let metadata = self
            .repository
            .get_object_metadata(&request.key, request.version_id.as_ref())
            .await?
            .ok_or_else(|| StorageError::ObjectNotFound {
                key: request.key.clone(),
            })?;

        // Get object data from store
        let data = self.store.get_object(&request.key).await?;

        Ok(StorageObject {
            key: request.key,
            data,
            metadata,
        })
    }

    /// Delete an object
    async fn delete_object(&self, key: &ObjectKey) -> StorageResult<()> {
        // Check if object exists
        if !self.repository.object_exists(key).await? {
            return Err(StorageError::ObjectNotFound { key: key.clone() });
        }

        // Delete from store
        self.store.delete_object(key).await?;

        // Get latest version and mark as deleted
        if let Some(version_id) = self.repository.get_latest_version_id(key).await? {
            self.repository
                .mark_version_deleted(key, &version_id)
                .await?;
        }

        Ok(())
    }

    /// List objects with a prefix
    async fn list_objects(
        &self,
        prefix: Option<&str>,
        max_results: Option<usize>,
    ) -> StorageResult<Vec<ObjectInfo>> {
        self.store.list_objects(prefix, max_results).await
    }

    /// Copy an object
    async fn copy_object(
        &self,
        source_key: &ObjectKey,
        destination_key: &ObjectKey,
    ) -> StorageResult<StorageObject> {
        // Get source object
        let source = self
            .get_object(GetObjectRequest {
                key: source_key.clone(),
                version_id: None,
            })
            .await?;

        // Create new object at destination
        self.create_object(CreateObjectRequest {
            key: destination_key.clone(),
            data: source.data,
            content_type: source.metadata.content_type,
            custom_metadata: source.metadata.custom_metadata,
        })
        .await
    }

    /// Update object metadata
    async fn update_metadata(
        &self,
        key: &ObjectKey,
        metadata: ObjectMetadata,
    ) -> StorageResult<()> {
        // Get latest version
        let version_id = self
            .repository
            .get_latest_version_id(key)
            .await?
            .ok_or_else(|| StorageError::ObjectNotFound { key: key.clone() })?;

        // Update metadata
        self.repository
            .update_object_metadata(key, &version_id, &metadata)
            .await
    }

    /// Check if object exists
    async fn object_exists(&self, key: &ObjectKey) -> StorageResult<bool> {
        self.repository.object_exists(key).await
    }

    /// Get object size without retrieving data
    async fn get_object_size(&self, key: &ObjectKey) -> StorageResult<u64> {
        self.store.get_object_size(key).await
    }
}

/// Builder for ObjectServiceImpl
pub struct ObjectServiceBuilder {
    repository: Option<Arc<dyn ObjectRepository>>,
    store: Option<Arc<dyn ObjectStore>>,
}

impl ObjectServiceBuilder {
    pub fn new() -> Self {
        Self {
            repository: None,
            store: None,
        }
    }

    pub fn repository(mut self, repository: Arc<dyn ObjectRepository>) -> Self {
        self.repository = Some(repository);
        self
    }

    pub fn store(mut self, store: Arc<dyn ObjectStore>) -> Self {
        self.store = Some(store);
        self
    }

    pub fn build(self) -> Result<ObjectServiceImpl, &'static str> {
        let repository = self.repository.ok_or("Repository is required")?;
        let store = self.store.ok_or("Store is required")?;

        Ok(ObjectServiceImpl::new(repository, store))
    }
}
