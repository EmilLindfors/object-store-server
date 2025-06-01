use crate::{
    domain::{
        errors::StorageResult,
        models::{CreateObjectRequest, GetObjectRequest, StorageObject},
        value_objects::ObjectKey,
    },
    ports::storage::ObjectInfo,
};
use async_trait::async_trait;

/// Port for object storage service operations
/// This trait defines the business logic for object management
#[async_trait]
pub trait ObjectService: Send + Sync + 'static {
    /// Create a new object
    async fn create_object(&self, request: CreateObjectRequest) -> StorageResult<StorageObject>;

    /// Get an object
    async fn get_object(&self, request: GetObjectRequest) -> StorageResult<StorageObject>;

    /// Delete an object
    async fn delete_object(&self, key: &ObjectKey) -> StorageResult<()>;

    /// List objects with a prefix
    async fn list_objects(
        &self,
        prefix: Option<&str>,
        max_results: Option<usize>,
    ) -> StorageResult<Vec<ObjectInfo>>;

    /// Copy an object
    async fn copy_object(
        &self,
        source_key: &ObjectKey,
        destination_key: &ObjectKey,
    ) -> StorageResult<StorageObject>;

    /// Update object metadata
    async fn update_metadata(
        &self,
        key: &ObjectKey,
        metadata: crate::domain::models::ObjectMetadata,
    ) -> StorageResult<()>;

    /// Check if object exists
    async fn object_exists(&self, key: &ObjectKey) -> StorageResult<bool>;

    /// Get object size without retrieving data
    async fn get_object_size(&self, key: &ObjectKey) -> StorageResult<u64>;
}
