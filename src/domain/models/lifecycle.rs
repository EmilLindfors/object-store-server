use super::filter::Filter;
use crate::domain::value_objects::{BucketName, ObjectKey};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// Lifecycle configuration for a bucket
#[derive(Debug, Clone, PartialEq)]
pub struct LifecycleConfiguration {
    pub bucket: BucketName,
    pub rules: Vec<LifecycleRule>,
}

/// A single lifecycle rule with comprehensive MinIO-compatible features
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LifecycleRule {
    pub id: String,
    pub status: RuleStatus,
    pub filter: Filter,

    // Expiration settings
    pub expiration_days: Option<u32>,
    pub expiration_date: Option<DateTime<Utc>>,
    pub expiration_expired_object_delete_marker: Option<bool>,
    pub expiration_expired_object_all_versions: Option<bool>,

    // Delete marker expiration
    pub del_marker_expiration_days: Option<u32>,

    // All versions expiration
    pub all_versions_expiration_days: Option<u32>,
    pub all_versions_expiration_delete_marker: Option<bool>,

    // Transition settings
    pub transition_days: Option<u32>,
    pub transition_date: Option<DateTime<Utc>>,
    pub transition_storage_class: Option<StorageClass>,

    // Non-current version expiration
    pub noncurrent_version_expiration_noncurrent_days: Option<u32>,
    pub noncurrent_version_expiration_newer_versions: Option<u32>,

    // Non-current version transition
    pub noncurrent_version_transition_noncurrent_days: Option<u32>,
    pub noncurrent_version_transition_storage_class: Option<StorageClass>,
    pub noncurrent_version_transition_newer_versions: Option<u32>,

    // Abort incomplete multipart uploads
    pub abort_incomplete_multipart_upload_days_after_initiation: Option<u32>,
}

/// Status of a lifecycle rule
#[derive(Debug, Clone, PartialEq)]
pub enum RuleStatus {
    Enabled,
    Disabled,
}

impl Default for RuleStatus {
    fn default() -> Self {
        RuleStatus::Disabled
    }
}

/// Actions that can be taken by lifecycle rules
#[derive(Debug, Clone, PartialEq)]
pub enum LifecycleAction {
    /// Delete objects after a certain time
    Expiration {
        days: Option<u32>,
        date: Option<DateTime<Utc>>,
    },
    /// Transition objects to a different storage class
    Transition {
        days: Option<u32>,
        date: Option<DateTime<Utc>>,
        storage_class: StorageClass,
    },
    /// Abort incomplete multipart uploads
    AbortIncompleteMultipartUpload { days_after_initiation: u32 },
    /// Expire delete markers
    ExpireDeleteMarker { days: u32 },
    /// Handle non-current versions
    NonCurrentVersionExpiration { days: u32 },
    /// Transition non-current versions
    NonCurrentVersionTransition {
        days: u32,
        storage_class: StorageClass,
    },
}

/// Storage classes for lifecycle transitions
#[derive(Debug, Clone, PartialEq)]
pub enum StorageClass {
    Standard,
    InfrequentAccess,
    Glacier,
    DeepArchive,
    Custom(String),
}

impl StorageClass {
    pub fn as_str(&self) -> &str {
        match self {
            StorageClass::Standard => "STANDARD",
            StorageClass::InfrequentAccess => "STANDARD_IA",
            StorageClass::Glacier => "GLACIER",
            StorageClass::DeepArchive => "DEEP_ARCHIVE",
            StorageClass::Custom(s) => s,
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "STANDARD" => StorageClass::Standard,
            "STANDARD_IA" => StorageClass::InfrequentAccess,
            "GLACIER" => StorageClass::Glacier,
            "DEEP_ARCHIVE" => StorageClass::DeepArchive,
            _ => StorageClass::Custom(s.to_string()),
        }
    }
}

/// Request to evaluate lifecycle rules for an object
#[derive(Debug, Clone)]
pub struct EvaluateLifecycleRequest {
    pub key: ObjectKey,
    pub object_created_at: std::time::SystemTime,
    pub object_tags: HashMap<String, String>,
    pub is_delete_marker: bool,
    pub is_current_version: bool,
}

/// Result of lifecycle evaluation
#[derive(Debug, Clone)]
pub struct LifecycleEvaluationResult {
    pub actions_to_apply: Vec<ApplicableAction>,
}

/// An action that should be applied based on lifecycle rules
#[derive(Debug, Clone)]
pub struct ApplicableAction {
    pub rule_id: String,
    pub action: LifecycleAction,
    pub reason: String,
}

impl LifecycleRule {
    /// Check if this rule applies to an object
    pub fn matches(
        &self,
        key: &ObjectKey,
        tags: &HashMap<String, String>,
        object_size: u64,
    ) -> bool {
        if self.status != RuleStatus::Enabled {
            return false;
        }

        self.filter.matches(key.as_str(), tags, object_size)
    }

    /// Validate the rule for logical consistency
    pub fn validate(&self) -> Result<(), ValidationError> {
        // Rule ID validation
        if self.id.is_empty() {
            return Err(ValidationError::EmptyRuleId);
        }

        if self.id.len() > 255 {
            return Err(ValidationError::RuleIdTooLong(self.id.clone()));
        }

        // Check that expiration has either days or date, not both
        if self.expiration_days.is_some() && self.expiration_date.is_some() {
            return Err(ValidationError::ConflictingExpirationSettings);
        }

        // Check that transition has either days or date, not both
        if self.transition_days.is_some() && self.transition_date.is_some() {
            return Err(ValidationError::ConflictingTransitionSettings);
        }

        // Ensure the rule has at least one action defined
        if !self.has_any_action() {
            return Err(ValidationError::NoActionsInRule(self.id.clone()));
        }

        Ok(())
    }

    /// Check if the rule has any action defined
    pub fn has_any_action(&self) -> bool {
        self.expiration_days.is_some()
            || self.expiration_date.is_some()
            || self.expiration_expired_object_delete_marker.is_some()
            || self.expiration_expired_object_all_versions.is_some()
            || self.del_marker_expiration_days.is_some()
            || self.all_versions_expiration_days.is_some()
            || self.transition_days.is_some()
            || self.transition_date.is_some()
            || self.noncurrent_version_expiration_noncurrent_days.is_some()
            || self.noncurrent_version_transition_noncurrent_days.is_some()
            || self
                .abort_incomplete_multipart_upload_days_after_initiation
                .is_some()
    }
}

impl LifecycleConfiguration {
    /// Validate the lifecycle configuration
    pub fn validate(&self) -> Result<(), ValidationError> {
        // Check for duplicate rule IDs
        let mut seen_ids = std::collections::HashSet::new();
        for rule in &self.rules {
            if !seen_ids.insert(&rule.id) {
                return Err(ValidationError::DuplicateRuleId(rule.id.clone()));
            }

            // Validate rule ID length
            if rule.id.len() > 255 {
                return Err(ValidationError::RuleIdTooLong(rule.id.clone()));
            }

            // Ensure rule has at least one action
            if !rule.has_any_action() {
                return Err(ValidationError::NoActionsInRule(rule.id.clone()));
            }
        }

        Ok(())
    }
}

/// Validation errors for lifecycle configuration
#[derive(Debug, Clone, PartialEq)]
pub enum ValidationError {
    DuplicateRuleId(String),
    RuleIdTooLong(String),
    NoActionsInRule(String),
    EmptyRuleId,
    ConflictingExpirationSettings,
    ConflictingTransitionSettings,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationError::DuplicateRuleId(id) => write!(f, "Duplicate rule ID: {}", id),
            ValidationError::RuleIdTooLong(id) => write!(f, "Rule ID too long: {}", id),
            ValidationError::NoActionsInRule(id) => write!(f, "No actions defined in rule: {}", id),
            ValidationError::EmptyRuleId => write!(f, "Rule ID cannot be empty"),
            ValidationError::ConflictingExpirationSettings => {
                write!(f, "Expiration cannot specify both Days and Date")
            }
            ValidationError::ConflictingTransitionSettings => {
                write!(f, "Transition cannot specify both Days and Date")
            }
        }
    }
}

impl std::error::Error for ValidationError {}
