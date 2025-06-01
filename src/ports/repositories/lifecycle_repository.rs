use crate::domain::{
    errors::LifecycleResult,
    models::{LifecycleConfiguration, LifecycleRule},
    value_objects::BucketName,
};
use async_trait::async_trait;

/// Repository for managing lifecycle configurations
#[async_trait]
pub trait LifecycleRepository: Send + Sync + 'static {
    /// Save a lifecycle configuration for a bucket
    async fn save_configuration(
        &self,
        bucket: &BucketName,
        config: &LifecycleConfiguration,
    ) -> LifecycleResult<()>;

    /// Retrieve the lifecycle configuration for a bucket
    async fn get_configuration(
        &self,
        bucket: &BucketName,
    ) -> LifecycleResult<Option<LifecycleConfiguration>>;

    /// Delete the lifecycle configuration for a bucket
    async fn delete_configuration(&self, bucket: &BucketName) -> LifecycleResult<()>;

    /// Check if a lifecycle configuration exists for a bucket
    async fn configuration_exists(&self, bucket: &BucketName) -> LifecycleResult<bool>;

    /// Get a specific rule by ID
    async fn get_rule(
        &self,
        bucket: &BucketName,
        rule_id: &str,
    ) -> LifecycleResult<Option<LifecycleRule>>;

    /// Update a specific rule
    async fn update_rule(&self, bucket: &BucketName, rule: &LifecycleRule) -> LifecycleResult<()>;

    /// List all buckets with lifecycle configurations
    async fn list_configured_buckets(&self) -> LifecycleResult<Vec<BucketName>>;

    /// Get the last time lifecycle rules were processed for a bucket
    async fn get_last_processed_time(
        &self,
        bucket: &BucketName,
    ) -> LifecycleResult<Option<std::time::SystemTime>>;

    /// Update the last processed time for a bucket
    async fn set_last_processed_time(
        &self,
        bucket: &BucketName,
        time: std::time::SystemTime,
    ) -> LifecycleResult<()>;
}
