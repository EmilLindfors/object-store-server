use crate::domain::{
    errors::StorageResult,
    models::{ObjectMetadata, ObjectVersionInfo, ObjectVersionList},
    value_objects::{ObjectKey, VersionId},
};
use async_trait::async_trait;

/// Repository for managing object metadata and version information
/// This trait handles metadata persistence, not the actual object data
#[async_trait]
pub trait ObjectRepository: Send + Sync + 'static {
    /// Store metadata for a new object version
    async fn save_object_metadata(
        &self,
        key: &ObjectKey,
        version_id: &VersionId,
        metadata: &ObjectMetadata,
    ) -> StorageResult<()>;

    /// Retrieve metadata for a specific version
    async fn get_object_metadata(
        &self,
        key: &ObjectKey,
        version_id: Option<&VersionId>,
    ) -> StorageResult<Option<ObjectMetadata>>;

    /// List all versions for an object
    async fn list_object_versions(&self, key: &ObjectKey) -> StorageResult<ObjectVersionList>;

    /// Get information about a specific version
    async fn get_version_info(
        &self,
        key: &ObjectKey,
        version_id: &VersionId,
    ) -> StorageResult<Option<ObjectVersionInfo>>;

    /// Mark a version as deleted (soft delete)
    async fn mark_version_deleted(
        &self,
        key: &ObjectKey,
        version_id: &VersionId,
    ) -> StorageResult<()>;

    /// Delete a version's metadata (hard delete)
    async fn delete_version_metadata(
        &self,
        key: &ObjectKey,
        version_id: &VersionId,
    ) -> StorageResult<()>;

    /// Get the latest version ID for an object
    async fn get_latest_version_id(&self, key: &ObjectKey) -> StorageResult<Option<VersionId>>;

    /// List objects with a given prefix
    async fn list_objects_by_prefix(
        &self,
        prefix: &str,
        max_results: Option<usize>,
    ) -> StorageResult<Vec<ObjectKey>>;

    /// Update metadata for an existing version
    async fn update_object_metadata(
        &self,
        key: &ObjectKey,
        version_id: &VersionId,
        metadata: &ObjectMetadata,
    ) -> StorageResult<()>;

    /// Check if an object exists (any version)
    async fn object_exists(&self, key: &ObjectKey) -> StorageResult<bool>;
}
