use std::sync::Arc;
use async_trait::async_trait;
use bytes::Bytes;
use object_store::ObjectStore as ObjectStoreBackend;

use crate::{
    domain::{
        value_objects::{ObjectKey, VersionId},
        errors::StorageResult,
    },
    ports::storage::{VersionedObjectStore, ObjectInfo, StorageVersionMetadata, StorageVersionedObject},
    adapters::outbound::storage::s3::S3ObjectStoreAdapter,
};

/// Versioned S3 storage adapter that implements the VersionedObjectStore trait
#[derive(Clone)]
pub struct VersionedS3ObjectStoreAdapter {
    base_adapter: Arc<S3ObjectStoreAdapter>,
    store: Arc<dyn ObjectStoreBackend>,
}

impl VersionedS3ObjectStoreAdapter {
    /// Create a new versioned S3 adapter
    pub fn new(base_adapter: Arc<S3ObjectStoreAdapter>, store: Arc<dyn ObjectStoreBackend>) -> Self {
        Self {
            base_adapter,
            store,
        }
    }
}

#[async_trait]
impl VersionedObjectStore for VersionedS3ObjectStoreAdapter {
    async fn put_object_version(
        &self,
        key: &ObjectKey,
        data: Bytes,
        content_type: Option<&str>,
    ) -> StorageResult<ObjectInfo> {
        // S3 automatically handles versioning if enabled on the bucket
        // The version ID will be returned in the response
        self.base_adapter.put_object(key, data, content_type).await
    }

    async fn get_object_version(
        &self,
        key: &ObjectKey,
        version_id: &VersionId,
    ) -> StorageResult<Bytes> {
        // For S3, we would use GetObjectRequest with VersionId parameter
        // This is a simplified implementation that doesn't actually use versioning
        // In a real implementation, you would:
        // 1. Use the S3 SDK directly for version-specific operations
        // 2. Pass the version_id parameter to the get_object call
        
        // For now, fall back to getting the latest version
        self.base_adapter.get_object(key).await
    }

    async fn get_object_version_stream(
        &self,
        key: &ObjectKey,
        version_id: &VersionId,
    ) -> StorageResult<Box<dyn tokio::io::AsyncRead + Send + Unpin>> {
        // Similar to get_object_version, this would use version-specific streaming
        self.base_adapter.get_object_stream(key).await
    }

    async fn delete_object_version(
        &self,
        key: &ObjectKey,
        version_id: &VersionId,
    ) -> StorageResult<()> {
        // For S3, we would use DeleteObjectRequest with VersionId parameter
        // This would delete the specific version, not add a delete marker
        
        // For now, fall back to deleting the object (which adds a delete marker if versioning is enabled)
        self.base_adapter.delete_object(key).await
    }

    async fn list_object_versions(&self, key: &ObjectKey) -> StorageResult<Vec<StorageVersionMetadata>> {
        // For S3, we would use ListObjectVersions API
        // This would return all versions of the specified object
        
        // For now, return a mock version based on the current object
        match self.base_adapter.head_object(key).await {
            Ok(metadata) => {
                let version_metadata = StorageVersionMetadata {
                    version_id: VersionId::new("latest".to_string()).unwrap(),
                    key: key.clone(),
                    size: metadata.size,
                    last_modified: metadata.last_modified.into(),
                    etag: metadata.etag,
                    is_latest: true,
                    is_delete_marker: false,
                };
                Ok(vec![version_metadata])
            }
            Err(e) => Err(e),
        }
    }

    async fn head_object_version(
        &self,
        key: &ObjectKey,
        version_id: &VersionId,
    ) -> StorageResult<StorageVersionMetadata> {
        // For S3, we would use HeadObjectRequest with VersionId parameter
        
        // For now, fall back to head_object and create version metadata
        let metadata = self.base_adapter.head_object(key).await?;
        
        Ok(StorageVersionMetadata {
            version_id: version_id.clone(),
            key: key.clone(),
            size: metadata.size,
            last_modified: metadata.last_modified.into(),
            etag: metadata.etag,
            is_latest: true,
            is_delete_marker: false,
        })
    }

    async fn copy_object_version(
        &self,
        source_key: &ObjectKey,
        source_version_id: &VersionId,
        dest_key: &ObjectKey,
    ) -> StorageResult<ObjectInfo> {
        // For S3, we would use CopyObjectRequest with VersionId parameter for the source
        
        // For now, fall back to regular copy (copies the latest version)
        self.base_adapter.copy_object(source_key, dest_key).await
    }

    async fn restore_object_version(
        &self,
        key: &ObjectKey,
        version_id: &VersionId,
    ) -> StorageResult<ObjectInfo> {
        // For S3, restoring a version typically means copying the old version to become the new latest version
        
        // Get the specific version data
        let data = self.get_object_version(key, version_id).await?;
        
        // Put it as a new version (making it the latest)
        self.put_object_version(key, data, None).await
    }

    async fn get_latest_version(&self, key: &ObjectKey) -> StorageResult<StorageVersionedObject> {
        let metadata = self.base_adapter.head_object(key).await?;
        let data = self.base_adapter.get_object(key).await?;
        
        Ok(StorageVersionedObject {
            key: key.clone(),
            version_id: VersionId::new("latest".to_string()).unwrap(),
            data,
            metadata: StorageVersionMetadata {
                version_id: VersionId::new("latest".to_string()).unwrap(),
                key: key.clone(),
                size: metadata.size,
                last_modified: metadata.last_modified.into(),
                etag: metadata.etag,
                is_latest: true,
                is_delete_marker: false,
            },
        })
    }

    async fn version_exists(&self, key: &ObjectKey, version_id: &VersionId) -> StorageResult<bool> {
        // For S3, we would use HeadObjectRequest with VersionId parameter
        
        // For now, check if the object exists (simplified)
        self.base_adapter.object_exists(key).await
    }
}