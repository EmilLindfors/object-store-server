use crate::{
    domain::{
        errors::{StorageError, StorageResult},
        models::{
            CreateObjectRequest, DeleteVersionRequest, DeleteVersionResult, GetObjectRequest,
            ObjectMetadata, ObjectVersionInfo, ObjectVersionList, VersionedObject,
            VersioningConfiguration,
        },
        value_objects::{BucketName, ObjectKey, VersionId},
    },
    ports::{
        repositories::ObjectRepository,
        services::{MetadataChange, VersionComparison, VersioningService},
        storage::VersionedObjectStore,
    },
};
use async_trait::async_trait;
use std::sync::Arc;

/// Implementation of versioning service
#[derive(Clone)]
pub struct VersioningServiceImpl {
    repository: Arc<dyn ObjectRepository>,
    store: Arc<dyn VersionedObjectStore>,
    versioning_configs:
        Arc<tokio::sync::RwLock<std::collections::HashMap<BucketName, VersioningConfiguration>>>,
}

impl VersioningServiceImpl {
    pub fn new(
        repository: Arc<dyn ObjectRepository>,
        store: Arc<dyn VersionedObjectStore>,
    ) -> Self {
        Self {
            repository,
            store,
            versioning_configs: Arc::new(
                tokio::sync::RwLock::new(std::collections::HashMap::new()),
            ),
        }
    }
}

#[async_trait]
impl VersioningService for VersioningServiceImpl {
    async fn enable_versioning(&self, bucket: &BucketName) -> StorageResult<()> {
        let mut configs = self.versioning_configs.write().await;
        configs.insert(
            bucket.clone(),
            VersioningConfiguration {
                enabled: true,
                max_versions: None,
            },
        );
        Ok(())
    }

    async fn disable_versioning(&self, bucket: &BucketName) -> StorageResult<()> {
        let mut configs = self.versioning_configs.write().await;
        if let Some(config) = configs.get_mut(bucket) {
            config.enabled = false;
        }
        Ok(())
    }

    async fn get_versioning_configuration(
        &self,
        bucket: &BucketName,
    ) -> StorageResult<VersioningConfiguration> {
        let configs = self.versioning_configs.read().await;
        Ok(configs.get(bucket).cloned().unwrap_or_default())
    }

    async fn create_versioned_object(
        &self,
        request: CreateObjectRequest,
    ) -> StorageResult<VersionedObject> {
        // Generate new version ID
        let version_id = VersionId::generate();

        // Store versioned object
        self.store
            .put_versioned_object(
                &request.key,
                &version_id,
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

        // Save metadata
        self.repository
            .save_object_metadata(&request.key, &version_id, &metadata)
            .await?;

        // Check if we need to prune old versions
        if let Some(bucket) = self.extract_bucket_from_key(&request.key) {
            let config = self.get_versioning_configuration(&bucket).await?;
            if let Some(max_versions) = config.max_versions {
                self.prune_versions(&request.key, max_versions as usize)
                    .await?;
            }
        }

        Ok(VersionedObject {
            key: request.key,
            version_id,
            data: request.data,
            metadata,
            is_latest: true,
            deleted: false,
        })
    }

    async fn get_object(&self, request: GetObjectRequest) -> StorageResult<VersionedObject> {
        let version_id = match request.version_id {
            Some(v) => v,
            None => self
                .repository
                .get_latest_version_id(&request.key)
                .await?
                .ok_or_else(|| StorageError::ObjectNotFound {
                    key: request.key.clone(),
                })?,
        };

        // Get metadata
        let metadata = self
            .repository
            .get_object_metadata(&request.key, Some(&version_id))
            .await?
            .ok_or_else(|| StorageError::VersionNotFound {
                key: request.key.clone(),
                version_id: version_id.clone(),
            })?;

        // Get object data
        let data = self
            .store
            .get_versioned_object(&request.key, &version_id)
            .await?;

        // Check if this is the latest version
        let latest_version = self.repository.get_latest_version_id(&request.key).await?;
        let is_latest = latest_version.as_ref() == Some(&version_id);

        Ok(VersionedObject {
            key: request.key,
            version_id,
            data,
            metadata,
            is_latest,
            deleted: false,
        })
    }

    async fn list_versions(&self, key: &ObjectKey) -> StorageResult<ObjectVersionList> {
        self.repository.list_object_versions(key).await
    }

    async fn get_version_info(
        &self,
        key: &ObjectKey,
        version_id: &VersionId,
    ) -> StorageResult<ObjectVersionInfo> {
        self.repository
            .get_version_info(key, version_id)
            .await?
            .ok_or_else(|| StorageError::VersionNotFound {
                key: key.clone(),
                version_id: version_id.clone(),
            })
    }

    async fn delete_version(
        &self,
        request: DeleteVersionRequest,
    ) -> StorageResult<DeleteVersionResult> {
        // Mark version as deleted
        self.repository
            .mark_version_deleted(&request.key, &request.version_id)
            .await?;

        // Delete from store
        self.store
            .delete_version(&request.key, &request.version_id)
            .await?;

        Ok(DeleteVersionResult {
            key: request.key,
            version_id: request.version_id,
            delete_marker_created: true,
        })
    }

    async fn restore_version(
        &self,
        key: &ObjectKey,
        version_id: &VersionId,
    ) -> StorageResult<VersionedObject> {
        // Get the version to restore
        let version = self
            .get_object(GetObjectRequest {
                key: key.clone(),
                version_id: Some(version_id.clone()),
            })
            .await?;

        // Create a new version with the same data
        self.create_versioned_object(CreateObjectRequest {
            key: key.clone(),
            data: version.data,
            content_type: version.metadata.content_type,
            custom_metadata: version.metadata.custom_metadata,
        })
        .await
    }

    async fn compare_versions(
        &self,
        key: &ObjectKey,
        version1: &VersionId,
        version2: &VersionId,
    ) -> StorageResult<VersionComparison> {
        // Get both versions
        let v1 = self.get_version_info(key, version1).await?;
        let v2 = self.get_version_info(key, version2).await?;

        // Compare metadata
        let mut metadata_changes = Vec::new();

        if v1.etag != v2.etag {
            metadata_changes.push(MetadataChange {
                field: "etag".to_string(),
                old_value: v1.etag.clone(),
                new_value: v2.etag.clone(),
            });
        }

        let size_difference = v2.size as i64 - v1.size as i64;
        let content_identical = v1.etag == v2.etag;

        Ok(VersionComparison {
            key: key.clone(),
            version1: version1.clone(),
            version2: version2.clone(),
            size_difference,
            metadata_changes,
            content_identical,
        })
    }

    async fn prune_versions(
        &self,
        key: &ObjectKey,
        keep_count: usize,
    ) -> StorageResult<Vec<VersionId>> {
        let version_list = self.list_versions(key).await?;
        let mut versions = version_list.versions;

        // Sort by last modified, newest first
        versions.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));

        // Keep only the specified number of versions
        let mut deleted_versions = Vec::new();

        if versions.len() > keep_count {
            for version in versions.iter().skip(keep_count) {
                // Delete the version
                self.delete_version(DeleteVersionRequest {
                    key: key.clone(),
                    version_id: version.version_id.clone(),
                })
                .await?;

                deleted_versions.push(version.version_id.clone());
            }
        }

        Ok(deleted_versions)
    }

    /// Copy a specific version to a new location
    async fn copy_version(
        &self,
        source_key: &ObjectKey,
        source_version: &VersionId,
        destination_key: &ObjectKey,
    ) -> StorageResult<VersionId> {
        // Use the versioned store's copy_version method
        self.store
            .copy_version(source_key, source_version, destination_key)
            .await
    }

    /// Check if a specific version exists
    async fn version_exists(&self, key: &ObjectKey, version_id: &VersionId) -> StorageResult<bool> {
        self.store.version_exists(key, version_id).await
    }
}

impl VersioningServiceImpl {
    fn calculate_etag(&self, data: &[u8]) -> String {
        format!("{:x}", md5::compute(data))
    }

    fn extract_bucket_from_key(&self, _key: &ObjectKey) -> Option<BucketName> {
        // In a real implementation, this would extract bucket from key
        // For now, return None
        None
    }
}
