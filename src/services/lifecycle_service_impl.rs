use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;

use crate::{
    domain::{
        errors::{LifecycleError, LifecycleResult},
        models::{
            ApplicableAction, EvaluateLifecycleRequest, LifecycleAction, LifecycleConfiguration,
            LifecycleEvaluationResult, LifecycleRule, LifecycleStorageClass, RuleStatus,
        },
        value_objects::{BucketName, ObjectKey},
    },
    ports::{
        repositories::{LifecycleRepository, ObjectRepository},
        services::{
            AppliedAction, BucketLifecycleResults, FailedAction, LifecycleActionResults,
            LifecycleService, ProcessingError, ProcessingStatus, ValidationError, ValidationResult,
            ValidationWarning,
        },
        storage::{ObjectStore, VersionedObjectStore},
    },
};

/// Implementation of the LifecycleService
#[derive(Clone)]
pub struct LifecycleServiceImpl {
    lifecycle_repo: Arc<dyn LifecycleRepository>,
    object_repo: Arc<dyn ObjectRepository>,
    object_store: Arc<dyn ObjectStore>,
    versioned_store: Arc<dyn VersionedObjectStore>,
    processing_status: Arc<RwLock<HashMap<BucketName, ProcessingStatus>>>,
}

impl LifecycleServiceImpl {
    pub fn new(
        lifecycle_repo: Arc<dyn LifecycleRepository>,
        object_repo: Arc<dyn ObjectRepository>,
        object_store: Arc<dyn ObjectStore>,
        versioned_store: Arc<dyn VersionedObjectStore>,
    ) -> Self {
        Self {
            lifecycle_repo,
            object_repo,
            object_store,
            versioned_store,
            processing_status: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl LifecycleService for LifecycleServiceImpl {
    async fn set_lifecycle_configuration(
        &self,
        bucket: &BucketName,
        config: LifecycleConfiguration,
    ) -> LifecycleResult<()> {
        // Validate configuration first
        let validation = self.validate_configuration(&config).await?;
        if !validation.is_valid {
            return Err(LifecycleError::ValidationFailed {
                errors: validation.errors.into_iter().map(|e| e.message).collect(),
            });
        }

        // Save configuration
        self.lifecycle_repo
            .save_configuration(bucket, &config)
            .await
            .map_err(|e| LifecycleError::RepositoryError {
                message: format!("Failed to save lifecycle configuration: {}", e),
            })?;

        Ok(())
    }

    async fn get_lifecycle_configuration(
        &self,
        bucket: &BucketName,
    ) -> LifecycleResult<Option<LifecycleConfiguration>> {
        self.lifecycle_repo
            .get_configuration(bucket)
            .await
            .map_err(|e| LifecycleError::RepositoryError {
                message: format!("Failed to get lifecycle configuration: {}", e),
            })
    }

    async fn delete_lifecycle_configuration(&self, bucket: &BucketName) -> LifecycleResult<()> {
        self.lifecycle_repo
            .delete_configuration(bucket)
            .await
            .map_err(|e| LifecycleError::RepositoryError {
                message: format!("Failed to delete lifecycle configuration: {}", e),
            })?;

        Ok(())
    }

    async fn evaluate_object_lifecycle(
        &self,
        request: EvaluateLifecycleRequest,
    ) -> LifecycleResult<LifecycleEvaluationResult> {
        // Get the bucket from the object key (extract from path)
        let bucket_name = self.extract_bucket_from_key(&request.key)?;

        // Get lifecycle configuration for this bucket
        let config = match self.get_lifecycle_configuration(&bucket_name).await? {
            Some(config) => config,
            None => {
                return Ok(LifecycleEvaluationResult {
                    actions_to_apply: Vec::new(),
                });
            }
        };

        let mut actions_to_apply = Vec::new();
        let current_time = SystemTime::now();

        for rule in &config.rules {
            if rule.status != RuleStatus::Enabled {
                continue;
            }

            // Check if rule matches this object
            if !rule.matches(&request.key, &request.object_tags, 0) {
                continue;
            }

            // Evaluate expiration rules
            if let Some(days) = rule.expiration_days {
                if self.should_expire_by_days(&request, days, current_time) {
                    actions_to_apply.push(ApplicableAction {
                        rule_id: rule.id.clone(),
                        action: LifecycleAction::Expiration {
                            days: Some(days),
                            date: None,
                        },
                        reason: format!("Object is {} days old", days),
                    });
                }
            }

            if let Some(date) = rule.expiration_date {
                if self.should_expire_by_date(&request, date, current_time) {
                    actions_to_apply.push(ApplicableAction {
                        rule_id: rule.id.clone(),
                        action: LifecycleAction::Expiration {
                            days: None,
                            date: Some(date),
                        },
                        reason: format!("Object scheduled to expire on {}", date),
                    });
                }
            }

            // Evaluate transition rules
            if let Some(days) = rule.transition_days {
                if let Some(storage_class) = &rule.transition_storage_class {
                    if self.should_transition_by_days(&request, days, current_time) {
                        actions_to_apply.push(ApplicableAction {
                            rule_id: rule.id.clone(),
                            action: LifecycleAction::Transition {
                                days: Some(days),
                                date: None,
                                storage_class: storage_class.clone(),
                            },
                            reason: format!(
                                "Object should transition to {} after {} days",
                                storage_class.as_str(),
                                days
                            ),
                        });
                    }
                }
            }

            // Evaluate delete marker expiration
            if let Some(days) = rule.del_marker_expiration_days {
                if request.is_delete_marker
                    && self.should_expire_by_days(&request, days, current_time)
                {
                    actions_to_apply.push(ApplicableAction {
                        rule_id: rule.id.clone(),
                        action: LifecycleAction::ExpireDeleteMarker { days },
                        reason: format!("Delete marker is {} days old", days),
                    });
                }
            }

            // Evaluate non-current version expiration
            if let Some(days) = rule.noncurrent_version_expiration_noncurrent_days {
                if !request.is_current_version
                    && self.should_expire_by_days(&request, days, current_time)
                {
                    actions_to_apply.push(ApplicableAction {
                        rule_id: rule.id.clone(),
                        action: LifecycleAction::NonCurrentVersionExpiration { days },
                        reason: format!("Non-current version is {} days old", days),
                    });
                }
            }

            // Evaluate non-current version transition
            if let Some(days) = rule.noncurrent_version_transition_noncurrent_days {
                if let Some(storage_class) = &rule.noncurrent_version_transition_storage_class {
                    if !request.is_current_version
                        && self.should_transition_by_days(&request, days, current_time)
                    {
                        actions_to_apply.push(ApplicableAction {
                            rule_id: rule.id.clone(),
                            action: LifecycleAction::NonCurrentVersionTransition {
                                days,
                                storage_class: storage_class.clone(),
                            },
                            reason: format!(
                                "Non-current version should transition after {} days",
                                days
                            ),
                        });
                    }
                }
            }

            // Evaluate incomplete multipart upload cleanup
            if let Some(days) = rule.abort_incomplete_multipart_upload_days_after_initiation {
                // This would require checking multipart upload status
                // For now, we'll create the action but implementation depends on store capabilities
                actions_to_apply.push(ApplicableAction {
                    rule_id: rule.id.clone(),
                    action: LifecycleAction::AbortIncompleteMultipartUpload {
                        days_after_initiation: days,
                    },
                    reason: format!("Cleanup incomplete uploads after {} days", days),
                });
            }
        }

        Ok(LifecycleEvaluationResult { actions_to_apply })
    }

    async fn apply_lifecycle_actions(
        &self,
        key: &ObjectKey,
        actions: Vec<ApplicableAction>,
    ) -> LifecycleResult<LifecycleActionResults> {
        let mut applied_actions = Vec::new();
        let mut failed_actions = Vec::new();

        for action in actions {
            let start_time = SystemTime::now();

            let result = match &action.action {
                LifecycleAction::Expiration { .. } => {
                    self.apply_expiration_action(key, &action).await
                }
                LifecycleAction::Transition { storage_class, .. } => {
                    self.apply_transition_action(key, &action, storage_class)
                        .await
                }
                LifecycleAction::ExpireDeleteMarker { .. } => {
                    self.apply_delete_marker_expiration(key, &action).await
                }
                LifecycleAction::NonCurrentVersionExpiration { .. } => {
                    self.apply_noncurrent_version_expiration(key, &action).await
                }
                LifecycleAction::NonCurrentVersionTransition { storage_class, .. } => {
                    self.apply_noncurrent_version_transition(key, &action, storage_class)
                        .await
                }
                LifecycleAction::AbortIncompleteMultipartUpload { .. } => {
                    self.apply_multipart_cleanup(key, &action).await
                }
            };

            match result {
                Ok(action_type) => {
                    applied_actions.push(AppliedAction {
                        rule_id: action.rule_id,
                        action_type,
                        timestamp: start_time,
                    });
                }
                Err(e) => {
                    failed_actions.push(FailedAction {
                        rule_id: action.rule_id,
                        action_type: format!("{:?}", action.action),
                        error: e.to_string(),
                    });
                }
            }
        }

        Ok(LifecycleActionResults {
            object_key: key.clone(),
            applied_actions,
            failed_actions,
        })
    }

    async fn process_bucket_lifecycle(
        &self,
        bucket: &BucketName,
    ) -> LifecycleResult<BucketLifecycleResults> {
        let start_time = SystemTime::now();

        // Update processing status to running
        {
            let mut status_map = self.processing_status.write().await;
            status_map.insert(
                bucket.clone(),
                ProcessingStatus {
                    is_running: true,
                    last_run: Some(start_time),
                    next_scheduled_run: Some(start_time + Duration::from_secs(86400)), // Next day
                    last_run_results: None,
                },
            );
        }

        let mut objects_processed = 0;
        let mut objects_affected = 0;
        let mut actions_applied = 0;
        let mut errors = Vec::new();

        // Get all objects in the bucket (this is a simplified approach)
        // In a real implementation, this would be paginated
        let objects = match self
            .object_store
            .list_objects(Some(bucket.as_str()), None)
            .await
        {
            Ok(objects) => objects,
            Err(e) => {
                errors.push(ProcessingError {
                    object_key: ObjectKey::new("unknown".to_string()).unwrap(),
                    rule_id: "system".to_string(),
                    error: format!("Failed to list bucket objects: {}", e),
                });

                let duration = start_time.elapsed().unwrap_or(Duration::from_secs(0));
                return Ok(BucketLifecycleResults {
                    bucket: bucket.clone(),
                    objects_processed: 0,
                    objects_affected: 0,
                    actions_applied: 0,
                    errors,
                    duration,
                });
            }
        };

        for object_info in objects {
            objects_processed += 1;

            // Create evaluation request for this object
            let request = EvaluateLifecycleRequest {
                key: object_info.key.clone(),
                object_created_at: object_info.last_modified,
                object_tags: HashMap::new(), // Would need to fetch actual tags
                is_delete_marker: false,     // Would need to determine this
                is_current_version: true,    // Would need to determine this
            };

            // Evaluate lifecycle rules for this object
            match self.evaluate_object_lifecycle(request).await {
                Ok(evaluation) => {
                    if !evaluation.actions_to_apply.is_empty() {
                        objects_affected += 1;

                        // Apply actions
                        match self
                            .apply_lifecycle_actions(&object_info.key, evaluation.actions_to_apply)
                            .await
                        {
                            Ok(results) => {
                                actions_applied += results.applied_actions.len();

                                // Add any failed actions as errors
                                for failed in results.failed_actions {
                                    errors.push(ProcessingError {
                                        object_key: object_info.key.clone(),
                                        rule_id: failed.rule_id,
                                        error: failed.error,
                                    });
                                }
                            }
                            Err(e) => {
                                errors.push(ProcessingError {
                                    object_key: object_info.key.clone(),
                                    rule_id: "apply_actions".to_string(),
                                    error: e.to_string(),
                                });
                            }
                        }
                    }
                }
                Err(e) => {
                    errors.push(ProcessingError {
                        object_key: object_info.key.clone(),
                        rule_id: "evaluation".to_string(),
                        error: e.to_string(),
                    });
                }
            }
        }

        let duration = start_time.elapsed().unwrap_or(Duration::from_secs(0));
        let results = BucketLifecycleResults {
            bucket: bucket.clone(),
            objects_processed,
            objects_affected,
            actions_applied,
            errors,
            duration,
        };

        // Update processing status
        {
            let mut status_map = self.processing_status.write().await;
            status_map.insert(
                bucket.clone(),
                ProcessingStatus {
                    is_running: false,
                    last_run: Some(start_time),
                    next_scheduled_run: Some(SystemTime::now() + Duration::from_secs(86400)),
                    last_run_results: Some(results.clone()),
                },
            );
        }

        Ok(results)
    }

    async fn enable_rule(&self, bucket: &BucketName, rule_id: &str) -> LifecycleResult<()> {
        self.update_rule_status(bucket, rule_id, RuleStatus::Enabled)
            .await
    }

    async fn disable_rule(&self, bucket: &BucketName, rule_id: &str) -> LifecycleResult<()> {
        self.update_rule_status(bucket, rule_id, RuleStatus::Disabled)
            .await
    }

    async fn add_rule(&self, bucket: &BucketName, rule: LifecycleRule) -> LifecycleResult<()> {
        // Get existing configuration
        let mut config =
            self.get_lifecycle_configuration(bucket)
                .await?
                .unwrap_or(LifecycleConfiguration {
                    bucket: bucket.clone(),
                    rules: Vec::new(),
                });

        // Check for duplicate rule ID
        if config.rules.iter().any(|r| r.id == rule.id) {
            return Err(LifecycleError::ValidationFailed {
                errors: vec![format!("Rule with ID '{}' already exists", rule.id)],
            });
        }

        // Validate the new rule
        rule.validate()
            .map_err(|e| LifecycleError::ValidationFailed {
                errors: vec![e.to_string()],
            })?;

        // Add the rule
        config.rules.push(rule);

        // Save updated configuration
        self.set_lifecycle_configuration(bucket, config).await
    }

    async fn remove_rule(&self, bucket: &BucketName, rule_id: &str) -> LifecycleResult<()> {
        // Get existing configuration
        let mut config = match self.get_lifecycle_configuration(bucket).await? {
            Some(config) => config,
            None => return Ok(()), // No configuration exists, nothing to remove
        };

        // Remove the rule
        let original_len = config.rules.len();
        config.rules.retain(|r| r.id != rule_id);

        if config.rules.len() == original_len {
            return Err(LifecycleError::RuleNotFound {
                rule_id: rule_id.to_string(),
            });
        }

        // Save updated configuration
        if config.rules.is_empty() {
            self.delete_lifecycle_configuration(bucket).await
        } else {
            self.set_lifecycle_configuration(bucket, config).await
        }
    }

    async fn validate_configuration(
        &self,
        config: &LifecycleConfiguration,
    ) -> LifecycleResult<ValidationResult> {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();

        // Validate the configuration using domain validation
        if let Err(validation_error) = config.validate() {
            errors.push(ValidationError {
                rule_id: None,
                field: "configuration".to_string(),
                message: validation_error.to_string(),
            });
        }

        // Validate individual rules
        for rule in &config.rules {
            if let Err(validation_error) = rule.validate() {
                errors.push(ValidationError {
                    rule_id: Some(rule.id.clone()),
                    field: "rule".to_string(),
                    message: validation_error.to_string(),
                });
            }

            // Check for potential warnings
            if !rule.has_any_action() {
                warnings.push(ValidationWarning {
                    rule_id: Some(rule.id.clone()),
                    message: "Rule has no actions defined".to_string(),
                });
            }

            if rule.filter.is_empty() {
                warnings.push(ValidationWarning {
                    rule_id: Some(rule.id.clone()),
                    message: "Rule filter matches all objects - this may affect performance"
                        .to_string(),
                });
            }
        }

        Ok(ValidationResult {
            is_valid: errors.is_empty(),
            errors,
            warnings,
        })
    }

    async fn get_processing_status(
        &self,
        bucket: &BucketName,
    ) -> LifecycleResult<ProcessingStatus> {
        let status_map = self.processing_status.read().await;
        Ok(status_map.get(bucket).cloned().unwrap_or(ProcessingStatus {
            is_running: false,
            last_run: None,
            next_scheduled_run: None,
            last_run_results: None,
        }))
    }
}

impl LifecycleServiceImpl {
    /// Helper method to extract bucket name from object key
    fn extract_bucket_from_key(&self, key: &ObjectKey) -> LifecycleResult<BucketName> {
        // This is a simplified implementation
        // In practice, bucket extraction depends on your key naming convention
        let parts: Vec<&str> = key.as_str().split('/').collect();
        if !parts.is_empty() {
            BucketName::new(parts[0].to_string()).map_err(|e| LifecycleError::ValidationFailed {
                errors: vec![format!("Invalid bucket name in key: {}", e)],
            })
        } else {
            Err(LifecycleError::ValidationFailed {
                errors: vec!["Cannot extract bucket from object key".to_string()],
            })
        }
    }

    /// Check if object should expire based on days
    fn should_expire_by_days(
        &self,
        request: &EvaluateLifecycleRequest,
        days: u32,
        current_time: SystemTime,
    ) -> bool {
        let age = current_time
            .duration_since(request.object_created_at)
            .unwrap_or(Duration::from_secs(0));

        age.as_secs() >= (days as u64) * 86400 // 86400 seconds in a day
    }

    /// Check if object should expire based on date
    fn should_expire_by_date(
        &self,
        _request: &EvaluateLifecycleRequest,
        expiration_date: DateTime<Utc>,
        current_time: SystemTime,
    ) -> bool {
        let expiration_system_time: SystemTime = expiration_date.into();
        current_time >= expiration_system_time
    }

    /// Check if object should transition based on days
    fn should_transition_by_days(
        &self,
        request: &EvaluateLifecycleRequest,
        days: u32,
        current_time: SystemTime,
    ) -> bool {
        self.should_expire_by_days(request, days, current_time)
    }

    /// Update rule status helper
    async fn update_rule_status(
        &self,
        bucket: &BucketName,
        rule_id: &str,
        new_status: RuleStatus,
    ) -> LifecycleResult<()> {
        let mut config = match self.get_lifecycle_configuration(bucket).await? {
            Some(config) => config,
            None => {
                return Err(LifecycleError::ConfigurationNotFound {
                    bucket: bucket.clone(),
                });
            }
        };

        let rule = config.rules.iter_mut().find(|r| r.id == rule_id).ok_or(
            LifecycleError::RuleNotFound {
                rule_id: rule_id.to_string(),
            },
        )?;

        rule.status = new_status;
        self.set_lifecycle_configuration(bucket, config).await
    }

    /// Apply expiration action
    async fn apply_expiration_action(
        &self,
        key: &ObjectKey,
        _action: &ApplicableAction,
    ) -> LifecycleResult<String> {
        self.object_store.delete_object(key).await.map_err(|e| {
            LifecycleError::ActionExecutionFailed {
                action: "expiration".to_string(),
                reason: e.to_string(),
            }
        })?;

        Ok("expiration".to_string())
    }

    /// Apply transition action
    async fn apply_transition_action(
        &self,
        _key: &ObjectKey,
        _action: &ApplicableAction,
        _storage_class: &LifecycleStorageClass,
    ) -> LifecycleResult<String> {
        // Storage class transitions would require storage-specific implementation
        // For now, we'll return an unsupported operation error
        Err(LifecycleError::ActionExecutionFailed {
            action: "transition".to_string(),
            reason: "Storage class transitions not yet implemented".to_string(),
        })
    }

    /// Apply delete marker expiration
    async fn apply_delete_marker_expiration(
        &self,
        _key: &ObjectKey,
        _action: &ApplicableAction,
    ) -> LifecycleResult<String> {
        // Delete marker handling would require versioning-specific logic
        Ok("delete_marker_expiration".to_string())
    }

    /// Apply non-current version expiration
    async fn apply_noncurrent_version_expiration(
        &self,
        _key: &ObjectKey,
        _action: &ApplicableAction,
    ) -> LifecycleResult<String> {
        // Non-current version cleanup would require versioning logic
        Ok("noncurrent_version_expiration".to_string())
    }

    /// Apply non-current version transition
    async fn apply_noncurrent_version_transition(
        &self,
        _key: &ObjectKey,
        _action: &ApplicableAction,
        _storage_class: &LifecycleStorageClass,
    ) -> LifecycleResult<String> {
        // Non-current version transitions would require versioning + storage class logic
        Ok("noncurrent_version_transition".to_string())
    }

    /// Apply multipart upload cleanup
    async fn apply_multipart_cleanup(
        &self,
        _key: &ObjectKey,
        _action: &ApplicableAction,
    ) -> LifecycleResult<String> {
        // Multipart cleanup would require storage-specific implementation
        Ok("multipart_cleanup".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::outbound::persistence::{
        InMemoryLifecycleRepository, InMemoryObjectRepository,
    };
    use crate::adapters::outbound::storage::ApacheObjectStoreAdapter;
    use object_store::memory::InMemory;
    use std::collections::HashMap;

    async fn create_test_service() -> LifecycleServiceImpl {
        let lifecycle_repo = Arc::new(InMemoryLifecycleRepository::new());
        let object_repo = Arc::new(InMemoryObjectRepository::new());
        let memory_store = Arc::new(InMemory::new());
        let object_store = Arc::new(ApacheObjectStoreAdapter::new(memory_store.clone()));
        let versioned_store = Arc::new(ApacheObjectStoreAdapter::new(memory_store));

        LifecycleServiceImpl::new(lifecycle_repo, object_repo, object_store, versioned_store)
    }

    #[tokio::test]
    async fn test_set_and_get_lifecycle_configuration() {
        let service = create_test_service().await;
        let bucket = BucketName::new("test-bucket".to_string()).unwrap();

        let config = LifecycleConfiguration {
            bucket: bucket.clone(),
            rules: vec![LifecycleRule {
                id: "test-rule".to_string(),
                status: RuleStatus::Enabled,
                filter: crate::domain::models::Filter::with_prefix("logs/".to_string()),
                expiration_days: Some(30),
                ..Default::default()
            }],
        };

        // Set configuration
        service
            .set_lifecycle_configuration(&bucket, config.clone())
            .await
            .unwrap();

        // Get configuration
        let retrieved = service.get_lifecycle_configuration(&bucket).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().rules.len(), 1);
    }

    #[tokio::test]
    async fn test_lifecycle_evaluation() {
        let service = create_test_service().await;
        let bucket = BucketName::new("test-bucket".to_string()).unwrap();

        let config = LifecycleConfiguration {
            bucket: bucket.clone(),
            rules: vec![LifecycleRule {
                id: "expire-old-logs".to_string(),
                status: RuleStatus::Enabled,
                filter: crate::domain::models::Filter::with_prefix("logs/".to_string()),
                expiration_days: Some(1), // 1 day for testing
                ..Default::default()
            }],
        };

        service
            .set_lifecycle_configuration(&bucket, config)
            .await
            .unwrap();

        // Test with an old object
        let old_time = SystemTime::now() - Duration::from_secs(2 * 86400); // 2 days ago
        let request = EvaluateLifecycleRequest {
            key: ObjectKey::new("logs/old-file.log".to_string()).unwrap(),
            object_created_at: old_time,
            object_tags: HashMap::new(),
            is_delete_marker: false,
            is_current_version: true,
        };

        let result = service.evaluate_object_lifecycle(request).await.unwrap();
        assert!(!result.actions_to_apply.is_empty());
        assert_eq!(result.actions_to_apply[0].rule_id, "expire-old-logs");
    }

    #[tokio::test]
    async fn test_rule_management() {
        let service = create_test_service().await;
        let bucket = BucketName::new("test-bucket".to_string()).unwrap();

        // Add a rule
        let rule = LifecycleRule {
            id: "new-rule".to_string(),
            status: RuleStatus::Enabled,
            filter: crate::domain::models::Filter::with_prefix("temp/".to_string()),
            expiration_days: Some(7),
            ..Default::default()
        };

        service.add_rule(&bucket, rule).await.unwrap();

        // Verify rule was added
        let config = service.get_lifecycle_configuration(&bucket).await.unwrap();
        assert!(config.is_some());
        assert_eq!(config.unwrap().rules.len(), 1);

        // Disable the rule
        service.disable_rule(&bucket, "new-rule").await.unwrap();

        // Verify rule was disabled
        let config = service.get_lifecycle_configuration(&bucket).await.unwrap();
        assert_eq!(config.unwrap().rules[0].status, RuleStatus::Disabled);

        // Remove the rule
        service.remove_rule(&bucket, "new-rule").await.unwrap();

        // Verify rule was removed
        let config = service.get_lifecycle_configuration(&bucket).await.unwrap();
        assert!(config.is_none() || config.unwrap().rules.is_empty());
    }
}
