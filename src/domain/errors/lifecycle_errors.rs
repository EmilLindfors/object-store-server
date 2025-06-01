use crate::domain::value_objects::{BucketName, ObjectKey};

/// Errors specific to lifecycle management operations
#[derive(Debug, Clone)]
pub enum LifecycleError {
    /// Lifecycle configuration not found
    ConfigurationNotFound { bucket: BucketName },

    /// Invalid lifecycle rule configuration
    InvalidRule { rule_id: String, reason: String },

    /// Conflicting lifecycle rules
    ConflictingRules {
        rule1_id: String,
        rule2_id: String,
        conflict: String,
    },

    /// Lifecycle action failed
    ActionFailed {
        rule_id: String,
        action: String,
        key: ObjectKey,
        reason: String,
    },

    /// Invalid expiration configuration
    InvalidExpiration { rule_id: String, reason: String },

    /// Invalid transition configuration
    InvalidTransition { rule_id: String, reason: String },

    /// Lifecycle processing error
    ProcessingError { message: String },

    /// Maximum number of rules exceeded
    TooManyRules { count: usize, max: usize },

    /// Rule not found
    RuleNotFound { rule_id: String },

    /// Validation failed
    ValidationFailed { errors: Vec<String> },

    /// Repository error
    RepositoryError { message: String },

    /// Action execution failed
    ActionExecutionFailed { action: String, reason: String },
}

impl std::fmt::Display for LifecycleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LifecycleError::ConfigurationNotFound { bucket } => {
                write!(
                    f,
                    "Lifecycle configuration not found for bucket: {}",
                    bucket
                )
            }
            LifecycleError::InvalidRule { rule_id, reason } => {
                write!(f, "Invalid lifecycle rule '{}': {}", rule_id, reason)
            }
            LifecycleError::ConflictingRules {
                rule1_id,
                rule2_id,
                conflict,
            } => {
                write!(
                    f,
                    "Conflicting lifecycle rules '{}' and '{}': {}",
                    rule1_id, rule2_id, conflict
                )
            }
            LifecycleError::ActionFailed {
                rule_id,
                action,
                key,
                reason,
            } => {
                write!(
                    f,
                    "Lifecycle action '{}' failed for rule '{}' on object '{}': {}",
                    action, rule_id, key, reason
                )
            }
            LifecycleError::InvalidExpiration { rule_id, reason } => {
                write!(
                    f,
                    "Invalid expiration configuration in rule '{}': {}",
                    rule_id, reason
                )
            }
            LifecycleError::InvalidTransition { rule_id, reason } => {
                write!(
                    f,
                    "Invalid transition configuration in rule '{}': {}",
                    rule_id, reason
                )
            }
            LifecycleError::ProcessingError { message } => {
                write!(f, "Lifecycle processing error: {}", message)
            }
            LifecycleError::TooManyRules { count, max } => {
                write!(f, "Too many lifecycle rules: {} (max: {})", count, max)
            }
            LifecycleError::RuleNotFound { rule_id } => {
                write!(f, "Lifecycle rule not found: {}", rule_id)
            }
            LifecycleError::ValidationFailed { errors } => {
                write!(f, "Validation failed: {}", errors.join(", "))
            }
            LifecycleError::RepositoryError { message } => {
                write!(f, "Repository error: {}", message)
            }
            LifecycleError::ActionExecutionFailed { action, reason } => {
                write!(f, "Action '{}' execution failed: {}", action, reason)
            }
        }
    }
}

impl std::error::Error for LifecycleError {}

/// Result type for lifecycle operations
pub type LifecycleResult<T> = Result<T, LifecycleError>;
