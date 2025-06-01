use crate::domain::{
    errors::StorageResult,
    models::{
        CreateObjectRequest, DeleteVersionRequest, DeleteVersionResult, GetObjectRequest,
        ObjectVersionInfo, ObjectVersionList, VersionedObject, VersioningConfiguration,
    },
    value_objects::{BucketName, ObjectKey, VersionId},
};
use async_trait::async_trait;

/// Service port for version management operations
#[async_trait]
pub trait VersioningService: Send + Sync + 'static {
    /// Enable versioning for a bucket
    async fn enable_versioning(&self, bucket: &BucketName) -> StorageResult<()>;

    /// Disable versioning for a bucket
    async fn disable_versioning(&self, bucket: &BucketName) -> StorageResult<()>;

    /// Get versioning configuration for a bucket
    async fn get_versioning_configuration(
        &self,
        bucket: &BucketName,
    ) -> StorageResult<VersioningConfiguration>;

    /// Create a new versioned object
    async fn create_versioned_object(
        &self,
        request: CreateObjectRequest,
    ) -> StorageResult<VersionedObject>;

    /// Get an object (optionally by version)
    async fn get_object(&self, request: GetObjectRequest) -> StorageResult<VersionedObject>;

    /// List all versions of an object
    async fn list_versions(&self, key: &ObjectKey) -> StorageResult<ObjectVersionList>;

    /// Get information about a specific version
    async fn get_version_info(
        &self,
        key: &ObjectKey,
        version_id: &VersionId,
    ) -> StorageResult<ObjectVersionInfo>;

    /// Delete a specific version
    async fn delete_version(
        &self,
        request: DeleteVersionRequest,
    ) -> StorageResult<DeleteVersionResult>;

    /// Restore a previous version as the latest
    async fn restore_version(
        &self,
        key: &ObjectKey,
        version_id: &VersionId,
    ) -> StorageResult<VersionedObject>;

    /// Compare two versions
    async fn compare_versions(
        &self,
        key: &ObjectKey,
        version1: &VersionId,
        version2: &VersionId,
    ) -> StorageResult<VersionComparison>;

    /// Prune old versions based on retention policy
    async fn prune_versions(
        &self,
        key: &ObjectKey,
        keep_count: usize,
    ) -> StorageResult<Vec<VersionId>>; // Returns deleted version IDs

    /// Copy a specific version to a new location
    async fn copy_version(
        &self,
        source_key: &ObjectKey,
        source_version: &VersionId,
        destination_key: &ObjectKey,
    ) -> StorageResult<VersionId>;

    /// Check if a specific version exists
    async fn version_exists(&self, key: &ObjectKey, version_id: &VersionId) -> StorageResult<bool>;
}

/// Result of comparing two versions
#[derive(Debug, Clone)]
pub struct VersionComparison {
    pub key: ObjectKey,
    pub version1: VersionId,
    pub version2: VersionId,
    pub size_difference: i64,
    pub metadata_changes: Vec<MetadataChange>,
    pub content_identical: bool,
}

#[derive(Debug, Clone)]
pub struct MetadataChange {
    pub field: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
}
