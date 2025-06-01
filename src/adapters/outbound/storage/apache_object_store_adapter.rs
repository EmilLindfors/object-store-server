use async_trait::async_trait;
use object_store::{ObjectStore as ApacheObjectStore, PutPayload, path::Path as ObjectPath};
use std::sync::Arc;

use crate::{
    domain::{
        errors::{StorageError, StorageResult},
        value_objects::{ObjectKey, VersionId},
    },
    ports::storage::{CompletedPart, ObjectInfo, ObjectStore, VersionedObjectStore},
};

/// Adapter that implements our ObjectStore trait using Apache object_store
pub struct ApacheObjectStoreAdapter {
    inner: Arc<dyn ApacheObjectStore>,
}

impl ApacheObjectStoreAdapter {
    pub fn new(store: Arc<dyn ApacheObjectStore>) -> Self {
        Self { inner: store }
    }
}

#[async_trait]
impl ObjectStore for ApacheObjectStoreAdapter {
    async fn put_object(
        &self,
        key: &ObjectKey,
        data: Vec<u8>,
        _content_type: Option<&str>,
    ) -> StorageResult<()> {
        let path = ObjectPath::from(key.as_str());
        let payload = PutPayload::from(data);

        // Note: content_type handling would need to be done via PutOptions
        // or other mechanism depending on object_store version

        self.inner
            .put(&path, payload)
            .await
            .map_err(|e| StorageError::InfrastructureError {
                message: format!("Failed to put object: {}", e),
                source: Some(e.to_string()),
            })?;

        Ok(())
    }

    async fn get_object(&self, key: &ObjectKey) -> StorageResult<Vec<u8>> {
        let path = ObjectPath::from(key.as_str());

        let result = self.inner.get(&path).await.map_err(|e| match e {
            object_store::Error::NotFound { .. } => {
                StorageError::ObjectNotFound { key: key.clone() }
            }
            _ => StorageError::InfrastructureError {
                message: format!("Failed to get object: {}", e),
                source: Some(e.to_string()),
            },
        })?;

        let bytes = result
            .bytes()
            .await
            .map_err(|e| StorageError::InfrastructureError {
                message: format!("Failed to read object bytes: {}", e),
                source: Some(e.to_string()),
            })?;

        Ok(bytes.to_vec())
    }

    async fn delete_object(&self, key: &ObjectKey) -> StorageResult<()> {
        let path = ObjectPath::from(key.as_str());

        self.inner.delete(&path).await.map_err(|e| match e {
            object_store::Error::NotFound { .. } => {
                StorageError::ObjectNotFound { key: key.clone() }
            }
            _ => StorageError::InfrastructureError {
                message: format!("Failed to delete object: {}", e),
                source: Some(e.to_string()),
            },
        })?;

        Ok(())
    }

    async fn object_exists(&self, key: &ObjectKey) -> StorageResult<bool> {
        let path = ObjectPath::from(key.as_str());

        match self.inner.head(&path).await {
            Ok(_) => Ok(true),
            Err(object_store::Error::NotFound { .. }) => Ok(false),
            Err(e) => Err(StorageError::InfrastructureError {
                message: format!("Failed to check object existence: {}", e),
                source: Some(e.to_string()),
            }),
        }
    }

    async fn get_object_size(&self, key: &ObjectKey) -> StorageResult<u64> {
        let path = ObjectPath::from(key.as_str());

        let meta = self.inner.head(&path).await.map_err(|e| match e {
            object_store::Error::NotFound { .. } => {
                StorageError::ObjectNotFound { key: key.clone() }
            }
            _ => StorageError::InfrastructureError {
                message: format!("Failed to get object metadata: {}", e),
                source: Some(e.to_string()),
            },
        })?;

        Ok(meta.size)
    }

    async fn list_objects(
        &self,
        prefix: Option<&str>,
        max_results: Option<usize>,
    ) -> StorageResult<Vec<ObjectInfo>> {
        let prefix_path = prefix.map(ObjectPath::from);

        let mut stream = self.inner.list(prefix_path.as_ref());
        let mut objects = Vec::new();
        let mut count = 0;

        while let Some(result) = futures::StreamExt::next(&mut stream).await {
            if let Some(max) = max_results {
                if count >= max {
                    break;
                }
            }

            let meta = result.map_err(|e| StorageError::InfrastructureError {
                message: format!("Failed to list objects: {}", e),
                source: Some(e.to_string()),
            })?;

            let key = ObjectKey::new(meta.location.to_string()).map_err(|e| {
                StorageError::ValidationError {
                    message: format!("Invalid object key from store: {}", e),
                }
            })?;

            objects.push(ObjectInfo {
                key,
                size: meta.size,
                last_modified: meta.last_modified.into(),
                etag: meta.e_tag.clone(),
            });

            count += 1;
        }

        Ok(objects)
    }

    async fn copy_object(
        &self,
        source_key: &ObjectKey,
        destination_key: &ObjectKey,
    ) -> StorageResult<()> {
        let source_path = ObjectPath::from(source_key.as_str());
        let dest_path = ObjectPath::from(destination_key.as_str());

        self.inner
            .copy(&source_path, &dest_path)
            .await
            .map_err(|e| match e {
                object_store::Error::NotFound { .. } => StorageError::ObjectNotFound {
                    key: source_key.clone(),
                },
                _ => StorageError::InfrastructureError {
                    message: format!("Failed to copy object: {}", e),
                    source: Some(e.to_string()),
                },
            })?;

        Ok(())
    }

    async fn get_presigned_url(
        &self,
        key: &ObjectKey,
        _expiration_seconds: u64,
    ) -> StorageResult<String> {
        let _path = ObjectPath::from(key.as_str());

        // Not all object stores support presigned URLs
        // This is a fallback that would need to be implemented per-provider
        Err(StorageError::UnsupportedOperation {
            operation: "presigned_url".to_string(),
            reason: "Not supported by this adapter".to_string(),
        })
    }

    async fn create_multipart_upload(&self, key: &ObjectKey) -> StorageResult<String> {
        let _path = ObjectPath::from(key.as_str());

        // The object_store crate handles multipart uploads differently
        // This would need to be implemented differently per provider
        Err(StorageError::UnsupportedOperation {
            operation: "create_multipart_upload".to_string(),
            reason: "Apache object_store uses different multipart API".to_string(),
        })
    }

    async fn upload_part(
        &self,
        _key: &ObjectKey,
        _upload_id: &str,
        _part_number: u32,
        _data: Vec<u8>,
    ) -> StorageResult<String> {
        // This would need to be implemented using the multipart upload API
        // The exact implementation depends on the specific object store
        Err(StorageError::UnsupportedOperation {
            operation: "upload_part".to_string(),
            reason: "Multipart uploads need store-specific implementation".to_string(),
        })
    }

    async fn complete_multipart_upload(
        &self,
        _key: &ObjectKey,
        _upload_id: &str,
        _parts: Vec<CompletedPart>,
    ) -> StorageResult<()> {
        // This would need to be implemented using the multipart upload API
        Err(StorageError::UnsupportedOperation {
            operation: "complete_multipart_upload".to_string(),
            reason: "Multipart uploads need store-specific implementation".to_string(),
        })
    }

    async fn abort_multipart_upload(&self, _key: &ObjectKey, upload_id: &str) -> StorageResult<()> {
        // This would need to be implemented using the multipart upload API
        Err(StorageError::UnsupportedOperation {
            operation: "abort_multipart_upload".to_string(),
            reason: "Multipart uploads need store-specific implementation".to_string(),
        })
    }
}

/// Versioned wrapper around the Apache object store adapter
pub struct VersionedApacheObjectStoreAdapter {
    base: ApacheObjectStoreAdapter,
}

impl VersionedApacheObjectStoreAdapter {
    pub fn new(store: Arc<dyn ApacheObjectStore>) -> Self {
        Self {
            base: ApacheObjectStoreAdapter::new(store),
        }
    }

    /// Generate a versioned path for an object
    fn versioned_path(&self, key: &ObjectKey, version_id: &VersionId) -> String {
        format!("{}/.versions/{}", key.as_str(), version_id.as_str())
    }
}

#[async_trait]
impl ObjectStore for VersionedApacheObjectStoreAdapter {
    async fn put_object(
        &self,
        key: &ObjectKey,
        data: Vec<u8>,
        content_type: Option<&str>,
    ) -> StorageResult<()> {
        self.base.put_object(key, data, content_type).await
    }

    async fn get_object(&self, key: &ObjectKey) -> StorageResult<Vec<u8>> {
        self.base.get_object(key).await
    }

    async fn delete_object(&self, key: &ObjectKey) -> StorageResult<()> {
        self.base.delete_object(key).await
    }

    async fn object_exists(&self, key: &ObjectKey) -> StorageResult<bool> {
        self.base.object_exists(key).await
    }

    async fn get_object_size(&self, key: &ObjectKey) -> StorageResult<u64> {
        self.base.get_object_size(key).await
    }

    async fn list_objects(
        &self,
        prefix: Option<&str>,
        max_results: Option<usize>,
    ) -> StorageResult<Vec<ObjectInfo>> {
        self.base.list_objects(prefix, max_results).await
    }

    async fn copy_object(
        &self,
        source_key: &ObjectKey,
        destination_key: &ObjectKey,
    ) -> StorageResult<()> {
        self.base.copy_object(source_key, destination_key).await
    }

    async fn get_presigned_url(
        &self,
        key: &ObjectKey,
        expiration_seconds: u64,
    ) -> StorageResult<String> {
        self.base.get_presigned_url(key, expiration_seconds).await
    }

    async fn create_multipart_upload(&self, key: &ObjectKey) -> StorageResult<String> {
        self.base.create_multipart_upload(key).await
    }

    async fn upload_part(
        &self,
        key: &ObjectKey,
        upload_id: &str,
        part_number: u32,
        data: Vec<u8>,
    ) -> StorageResult<String> {
        self.base
            .upload_part(key, upload_id, part_number, data)
            .await
    }

    async fn complete_multipart_upload(
        &self,
        key: &ObjectKey,
        upload_id: &str,
        parts: Vec<CompletedPart>,
    ) -> StorageResult<()> {
        self.base
            .complete_multipart_upload(key, upload_id, parts)
            .await
    }

    async fn abort_multipart_upload(&self, key: &ObjectKey, upload_id: &str) -> StorageResult<()> {
        self.base.abort_multipart_upload(key, upload_id).await
    }
}

#[async_trait]
impl VersionedObjectStore for VersionedApacheObjectStoreAdapter {
    async fn put_versioned_object(
        &self,
        key: &ObjectKey,
        version_id: &VersionId,
        data: Vec<u8>,
        content_type: Option<&str>,
    ) -> StorageResult<()> {
        let versioned_path = self.versioned_path(key, version_id);
        let versioned_key =
            ObjectKey::new(versioned_path).map_err(|e| StorageError::ValidationError {
                message: format!("Invalid versioned key: {}", e),
            })?;

        self.base
            .put_object(&versioned_key, data, content_type)
            .await
    }

    async fn get_versioned_object(
        &self,
        key: &ObjectKey,
        version_id: &VersionId,
    ) -> StorageResult<Vec<u8>> {
        let versioned_path = self.versioned_path(key, version_id);
        let versioned_key =
            ObjectKey::new(versioned_path).map_err(|e| StorageError::ValidationError {
                message: format!("Invalid versioned key: {}", e),
            })?;

        self.base.get_object(&versioned_key).await
    }

    async fn delete_version(&self, key: &ObjectKey, version_id: &VersionId) -> StorageResult<()> {
        let versioned_path = self.versioned_path(key, version_id);
        let versioned_key =
            ObjectKey::new(versioned_path).map_err(|e| StorageError::ValidationError {
                message: format!("Invalid versioned key: {}", e),
            })?;

        self.base.delete_object(&versioned_key).await
    }

    async fn version_exists(&self, key: &ObjectKey, version_id: &VersionId) -> StorageResult<bool> {
        let versioned_path = self.versioned_path(key, version_id);
        let versioned_key =
            ObjectKey::new(versioned_path).map_err(|e| StorageError::ValidationError {
                message: format!("Invalid versioned key: {}", e),
            })?;

        self.base.object_exists(&versioned_key).await
    }

    async fn copy_version(
        &self,
        source_key: &ObjectKey,
        source_version: &VersionId,
        destination_key: &ObjectKey,
    ) -> StorageResult<VersionId> {
        let source_versioned_path = self.versioned_path(source_key, source_version);
        let source_versioned_key =
            ObjectKey::new(source_versioned_path).map_err(|e| StorageError::ValidationError {
                message: format!("Invalid source versioned key: {}", e),
            })?;

        // Generate new version ID for destination
        let new_version_id = VersionId::new(uuid::Uuid::new_v4().to_string()).map_err(|e| {
            StorageError::ValidationError {
                message: format!("Failed to create version ID: {}", e),
            }
        })?;
        let dest_versioned_path = self.versioned_path(destination_key, &new_version_id);
        let dest_versioned_key =
            ObjectKey::new(dest_versioned_path).map_err(|e| StorageError::ValidationError {
                message: format!("Invalid destination versioned key: {}", e),
            })?;

        self.base
            .copy_object(&source_versioned_key, &dest_versioned_key)
            .await?;

        Ok(new_version_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use object_store::memory::InMemory;

    #[tokio::test]
    async fn test_basic_object_operations() {
        let store = Arc::new(InMemory::new());
        let adapter = ApacheObjectStoreAdapter::new(store);

        let key = ObjectKey::new("test/key".to_string()).unwrap();
        let data = b"test data".to_vec();

        // Test put
        adapter
            .put_object(&key, data.clone(), Some("text/plain"))
            .await
            .unwrap();

        // Test get
        let retrieved = adapter.get_object(&key).await.unwrap();
        assert_eq!(retrieved, data);

        // Test exists
        assert!(adapter.object_exists(&key).await.unwrap());

        // Test size
        let size = adapter.get_object_size(&key).await.unwrap();
        assert_eq!(size, data.len() as u64);

        // Test delete
        adapter.delete_object(&key).await.unwrap();
        assert!(!adapter.object_exists(&key).await.unwrap());
    }

    #[tokio::test]
    async fn test_versioned_operations() {
        let store = Arc::new(InMemory::new());
        let adapter = VersionedApacheObjectStoreAdapter::new(store);

        let key = ObjectKey::new("test/key".to_string()).unwrap();
        let version1 = VersionId::new();
        let version2 = VersionId::new();
        let data1 = b"test data v1".to_vec();
        let data2 = b"test data v2".to_vec();

        // Test put versioned
        adapter
            .put_versioned_object(&key, &version1, data1.clone(), None)
            .await
            .unwrap();
        adapter
            .put_versioned_object(&key, &version2, data2.clone(), None)
            .await
            .unwrap();

        // Test get versioned
        let retrieved1 = adapter.get_versioned_object(&key, &version1).await.unwrap();
        let retrieved2 = adapter.get_versioned_object(&key, &version2).await.unwrap();

        assert_eq!(retrieved1, data1);
        assert_eq!(retrieved2, data2);

        // Test version exists
        assert!(adapter.version_exists(&key, &version1).await.unwrap());
        assert!(adapter.version_exists(&key, &version2).await.unwrap());

        // Test delete version
        adapter.delete_version(&key, &version1).await.unwrap();
        assert!(!adapter.version_exists(&key, &version1).await.unwrap());
        assert!(adapter.version_exists(&key, &version2).await.unwrap());
    }
}
