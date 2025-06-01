use std::collections::HashMap;
use async_trait::async_trait;
use bytes::Bytes;
use chrono::{DateTime, Utc};

use crate::domain::{
    errors::StorageResult,
    models::{ObjectMetadata, Filter},
    value_objects::{ObjectKey, VersionId},
};

/// Port for object storage operations
/// This abstracts the actual storage backend (S3, Azure, etc.)
/// Provides a comprehensive interface for production object storage systems
#[async_trait]
pub trait ObjectStore: Send + Sync + 'static {
    /// Store object data with optional metadata
    async fn put_object(
        &self,
        key: &ObjectKey,
        data: Bytes,
        content_type: Option<&str>,
    ) -> StorageResult<ObjectInfo>;

    /// Retrieve object data
    async fn get_object(&self, key: &ObjectKey) -> StorageResult<Bytes>;

    /// Retrieve object data as a stream for large objects
    async fn get_object_stream(
        &self,
        key: &ObjectKey,
    ) -> StorageResult<Box<dyn tokio::io::AsyncRead + Send + Unpin>>;

    /// Delete object data
    async fn delete_object(&self, key: &ObjectKey) -> StorageResult<()>;

    /// Check if object exists
    async fn object_exists(&self, key: &ObjectKey) -> StorageResult<bool>;

    /// Get object metadata without retrieving data (HEAD operation)
    async fn head_object(&self, key: &ObjectKey) -> StorageResult<ObjectMetadata>;

    /// List objects with advanced filtering
    async fn list_objects(&self, filter: &Filter) -> StorageResult<Vec<ObjectListItem>>;

    /// Copy an object to a new location
    async fn copy_object(
        &self,
        source_key: &ObjectKey,
        dest_key: &ObjectKey,
    ) -> StorageResult<ObjectInfo>;

    /// Get a pre-signed URL for object access
    async fn get_presigned_url(
        &self,
        key: &ObjectKey,
        expiration_seconds: u64,
        method: PresignedUrlMethod,
    ) -> StorageResult<String>;

    /// Initiate a multipart upload
    async fn initiate_multipart_upload(&self, key: &ObjectKey) -> StorageResult<String>; // Returns upload ID

    /// Upload a part in a multipart upload
    async fn upload_part(
        &self,
        key: &ObjectKey,
        upload_id: &str,
        part_number: u32,
        data: Bytes,
    ) -> StorageResult<CompletedPart>;

    /// Complete a multipart upload
    async fn complete_multipart_upload(
        &self,
        key: &ObjectKey,
        upload_id: &str,
        parts: Vec<CompletedPart>,
    ) -> StorageResult<ObjectInfo>;

    /// Abort a multipart upload
    async fn abort_multipart_upload(&self, key: &ObjectKey, upload_id: &str) -> StorageResult<()>;

    /// List ongoing multipart uploads
    async fn list_multipart_uploads(&self) -> StorageResult<Vec<MultipartUpload>>;

    /// Set object metadata (tags, custom metadata)
    async fn set_object_metadata(
        &self,
        key: &ObjectKey,
        metadata: HashMap<String, String>,
    ) -> StorageResult<()>;

    /// Get object metadata (tags, custom metadata)
    async fn get_object_metadata(&self, key: &ObjectKey) -> StorageResult<HashMap<String, String>>;
}

/// Port for versioned object storage operations
/// Extends the basic ObjectStore with version-aware operations
#[async_trait]
pub trait VersionedObjectStore: Send + Sync + 'static {
    /// Store object data and return version information
    async fn put_object_version(
        &self,
        key: &ObjectKey,
        data: Bytes,
        content_type: Option<&str>,
    ) -> StorageResult<ObjectInfo>;

    /// Retrieve object data for a specific version
    async fn get_object_version(
        &self,
        key: &ObjectKey,
        version_id: &VersionId,
    ) -> StorageResult<Bytes>;

    /// Retrieve object data as a stream for a specific version
    async fn get_object_version_stream(
        &self,
        key: &ObjectKey,
        version_id: &VersionId,
    ) -> StorageResult<Box<dyn tokio::io::AsyncRead + Send + Unpin>>;

    /// Delete a specific version
    async fn delete_object_version(
        &self,
        key: &ObjectKey,
        version_id: &VersionId,
    ) -> StorageResult<()>;

    /// List all versions of an object
    async fn list_object_versions(&self, key: &ObjectKey) -> StorageResult<Vec<StorageVersionMetadata>>;

    /// Get metadata for a specific version
    async fn head_object_version(
        &self,
        key: &ObjectKey,
        version_id: &VersionId,
    ) -> StorageResult<StorageVersionMetadata>;

    /// Copy a specific version to a new location
    async fn copy_object_version(
        &self,
        source_key: &ObjectKey,
        source_version_id: &VersionId,
        dest_key: &ObjectKey,
    ) -> StorageResult<ObjectInfo>;

    /// Restore a specific version (make it the latest)
    async fn restore_object_version(
        &self,
        key: &ObjectKey,
        version_id: &VersionId,
    ) -> StorageResult<ObjectInfo>;

    /// Get the latest version of an object
    async fn get_latest_version(&self, key: &ObjectKey) -> StorageResult<StorageVersionedObject>;

    /// Check if a specific version exists
    async fn version_exists(&self, key: &ObjectKey, version_id: &VersionId) -> StorageResult<bool>;
}

/// Enhanced object information with comprehensive metadata
#[derive(Debug, Clone)]
pub struct ObjectInfo {
    pub key: ObjectKey,
    pub size: u64,
    pub etag: Option<String>,
    pub version_id: Option<String>,
    pub last_modified: DateTime<Utc>,
}

/// Object information for listing operations
#[derive(Debug, Clone)]
pub struct ObjectListItem {
    pub key: ObjectKey,
    pub size: u64,
    pub etag: Option<String>,
    pub last_modified: DateTime<Utc>,
    pub content_type: Option<String>,
}

/// Completed part information for multipart uploads
#[derive(Debug, Clone)]
pub struct CompletedPart {
    pub part_number: u32,
    pub etag: String,
}

/// Information about an ongoing multipart upload
#[derive(Debug, Clone)]
pub struct MultipartUpload {
    pub upload_id: String,
    pub key: ObjectKey,
    pub initiated: DateTime<Utc>,
}

/// Version-specific metadata for storage operations
#[derive(Debug, Clone)]
pub struct StorageVersionMetadata {
    pub version_id: VersionId,
    pub key: ObjectKey,
    pub size: u64,
    pub last_modified: DateTime<Utc>,
    pub etag: Option<String>,
    pub is_latest: bool,
    pub is_delete_marker: bool,
}

/// A versioned object with its data from storage
#[derive(Debug, Clone)]
pub struct StorageVersionedObject {
    pub key: ObjectKey,
    pub version_id: VersionId,
    pub data: Bytes,
    pub metadata: StorageVersionMetadata,
}

/// Method for pre-signed URLs
#[derive(Debug, Clone, Copy)]
pub enum PresignedUrlMethod {
    Get,
    Put,
    Delete,
}

impl std::fmt::Display for PresignedUrlMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PresignedUrlMethod::Get => write!(f, "GET"),
            PresignedUrlMethod::Put => write!(f, "PUT"),
            PresignedUrlMethod::Delete => write!(f, "DELETE"),
        }
    }
}