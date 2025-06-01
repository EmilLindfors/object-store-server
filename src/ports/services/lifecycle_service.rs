use crate::domain::{
    errors::LifecycleResult,
    models::{
        ApplicableAction, EvaluateLifecycleRequest, LifecycleConfiguration,
        LifecycleEvaluationResult, LifecycleRule,
    },
    value_objects::{BucketName, ObjectKey},
};
use async_trait::async_trait;

/// Service port for lifecycle management operations
#[async_trait]
pub trait LifecycleService: Send + Sync + 'static {
    /// Set lifecycle configuration for a bucket
    async fn set_lifecycle_configuration(
        &self,
        bucket: &BucketName,
        config: LifecycleConfiguration,
    ) -> LifecycleResult<()>;

    /// Get lifecycle configuration for a bucket
    async fn get_lifecycle_configuration(
        &self,
        bucket: &BucketName,
    ) -> LifecycleResult<Option<LifecycleConfiguration>>;

    /// Delete lifecycle configuration for a bucket
    async fn delete_lifecycle_configuration(&self, bucket: &BucketName) -> LifecycleResult<()>;

    /// Evaluate lifecycle rules for a specific object
    async fn evaluate_object_lifecycle(
        &self,
        request: EvaluateLifecycleRequest,
    ) -> LifecycleResult<LifecycleEvaluationResult>;

    /// Apply lifecycle actions to an object
    async fn apply_lifecycle_actions(
        &self,
        key: &ObjectKey,
        actions: Vec<ApplicableAction>,
    ) -> LifecycleResult<LifecycleActionResults>;

    /// Process lifecycle rules for a bucket
    async fn process_bucket_lifecycle(
        &self,
        bucket: &BucketName,
    ) -> LifecycleResult<BucketLifecycleResults>;

    /// Enable a specific lifecycle rule
    async fn enable_rule(&self, bucket: &BucketName, rule_id: &str) -> LifecycleResult<()>;

    /// Disable a specific lifecycle rule
    async fn disable_rule(&self, bucket: &BucketName, rule_id: &str) -> LifecycleResult<()>;

    /// Add a new rule to existing configuration
    async fn add_rule(&self, bucket: &BucketName, rule: LifecycleRule) -> LifecycleResult<()>;

    /// Remove a rule from configuration
    async fn remove_rule(&self, bucket: &BucketName, rule_id: &str) -> LifecycleResult<()>;

    /// Validate a lifecycle configuration
    async fn validate_configuration(
        &self,
        config: &LifecycleConfiguration,
    ) -> LifecycleResult<ValidationResult>;

    /// Get lifecycle processing status
    async fn get_processing_status(&self, bucket: &BucketName)
    -> LifecycleResult<ProcessingStatus>;
}

/// Results from applying lifecycle actions
#[derive(Debug, Clone)]
pub struct LifecycleActionResults {
    pub object_key: ObjectKey,
    pub applied_actions: Vec<AppliedAction>,
    pub failed_actions: Vec<FailedAction>,
}

#[derive(Debug, Clone)]
pub struct AppliedAction {
    pub rule_id: String,
    pub action_type: String,
    pub timestamp: std::time::SystemTime,
}

#[derive(Debug, Clone)]
pub struct FailedAction {
    pub rule_id: String,
    pub action_type: String,
    pub error: String,
}

/// Results from processing lifecycle for an entire bucket
#[derive(Debug, Clone)]
pub struct BucketLifecycleResults {
    pub bucket: BucketName,
    pub objects_processed: usize,
    pub objects_affected: usize,
    pub actions_applied: usize,
    pub errors: Vec<ProcessingError>,
    pub duration: std::time::Duration,
}

#[derive(Debug, Clone)]
pub struct ProcessingError {
    pub object_key: ObjectKey,
    pub rule_id: String,
    pub error: String,
}

/// Validation result for lifecycle configuration
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub errors: Vec<ValidationError>,
    pub warnings: Vec<ValidationWarning>,
}

#[derive(Debug, Clone)]
pub struct ValidationError {
    pub rule_id: Option<String>,
    pub field: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct ValidationWarning {
    pub rule_id: Option<String>,
    pub message: String,
}

/// Status of lifecycle processing
#[derive(Debug, Clone)]
pub struct ProcessingStatus {
    pub is_running: bool,
    pub last_run: Option<std::time::SystemTime>,
    pub next_scheduled_run: Option<std::time::SystemTime>,
    pub last_run_results: Option<BucketLifecycleResults>,
}
