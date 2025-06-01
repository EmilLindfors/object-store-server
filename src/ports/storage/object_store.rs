use crate::domain::{
    errors::StorageResult,
    value_objects::{ObjectKey, VersionId},
};
use async_trait::async_trait;

/// Port for object storage operations
/// This abstracts the actual storage backend (S3, Azure, etc.)
#[async_trait]
pub trait ObjectStore: Send + Sync + 'static {
    /// Store object data
    async fn put_object(
        &self,
        key: &ObjectKey,
        data: Vec<u8>,
        content_type: Option<&str>,
    ) -> StorageResult<()>;

    /// Retrieve object data
    async fn get_object(&self, key: &ObjectKey) -> StorageResult<Vec<u8>>;

    /// Delete object data
    async fn delete_object(&self, key: &ObjectKey) -> StorageResult<()>;

    /// Check if object exists
    async fn object_exists(&self, key: &ObjectKey) -> StorageResult<bool>;

    /// Get object size without retrieving data
    async fn get_object_size(&self, key: &ObjectKey) -> StorageResult<u64>;

    /// List objects with a given prefix
    async fn list_objects(
        &self,
        prefix: Option<&str>,
        max_results: Option<usize>,
    ) -> StorageResult<Vec<ObjectInfo>>;

    /// Copy an object to a new location
    async fn copy_object(
        &self,
        source_key: &ObjectKey,
        destination_key: &ObjectKey,
    ) -> StorageResult<()>;

    /// Get a pre-signed URL for object access
    async fn get_presigned_url(
        &self,
        key: &ObjectKey,
        expiration_seconds: u64,
    ) -> StorageResult<String>;

    /// Initiate a multipart upload
    async fn create_multipart_upload(&self, key: &ObjectKey) -> StorageResult<String>; // Returns upload ID

    /// Upload a part in a multipart upload
    async fn upload_part(
        &self,
        key: &ObjectKey,
        upload_id: &str,
        part_number: u32,
        data: Vec<u8>,
    ) -> StorageResult<String>; // Returns ETag

    /// Complete a multipart upload
    async fn complete_multipart_upload(
        &self,
        key: &ObjectKey,
        upload_id: &str,
        parts: Vec<CompletedPart>,
    ) -> StorageResult<()>;

    /// Abort a multipart upload
    async fn abort_multipart_upload(&self, key: &ObjectKey, upload_id: &str) -> StorageResult<()>;
}

/// Information about an object in storage
#[derive(Debug, Clone)]
pub struct ObjectInfo {
    pub key: ObjectKey,
    pub size: u64,
    pub last_modified: std::time::SystemTime,
    pub etag: Option<String>,
}

/// Completed part information for multipart uploads
#[derive(Debug, Clone)]
pub struct CompletedPart {
    pub part_number: u32,
    pub etag: String,
}

/// Port for versioned object storage operations
/// Extends the basic ObjectStore with version-aware operations
#[async_trait]
pub trait VersionedObjectStore: ObjectStore {
    /// Store object data with a specific version
    async fn put_versioned_object(
        &self,
        key: &ObjectKey,
        version_id: &VersionId,
        data: Vec<u8>,
        content_type: Option<&str>,
    ) -> StorageResult<()>;

    /// Retrieve object data for a specific version
    async fn get_versioned_object(
        &self,
        key: &ObjectKey,
        version_id: &VersionId,
    ) -> StorageResult<Vec<u8>>;

    /// Delete a specific version
    async fn delete_version(&self, key: &ObjectKey, version_id: &VersionId) -> StorageResult<()>;

    /// Check if a specific version exists
    async fn version_exists(&self, key: &ObjectKey, version_id: &VersionId) -> StorageResult<bool>;

    /// Copy a specific version to a new location
    async fn copy_version(
        &self,
        source_key: &ObjectKey,
        source_version: &VersionId,
        destination_key: &ObjectKey,
    ) -> StorageResult<VersionId>; // Returns new version ID
}
