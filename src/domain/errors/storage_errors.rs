use crate::domain::value_objects::{ObjectKey, VersionId};

/// Errors that can occur during storage operations
#[derive(Debug, Clone)]
pub enum StorageError {
    /// Object not found
    ObjectNotFound { key: ObjectKey },

    /// Version not found
    VersionNotFound {
        key: ObjectKey,
        version_id: VersionId,
    },

    /// Version conflict during concurrent operations
    VersionConflict {
        key: ObjectKey,
        expected_version: Option<VersionId>,
        actual_version: Option<VersionId>,
    },

    /// Storage quota exceeded
    QuotaExceeded { used: u64, limit: u64 },

    /// Invalid object size
    InvalidObjectSize {
        size: u64,
        min: Option<u64>,
        max: Option<u64>,
    },

    /// Access denied
    AccessDenied { key: ObjectKey, operation: String },

    /// Object already exists (when overwrite not allowed)
    ObjectAlreadyExists { key: ObjectKey },

    /// Invalid storage class
    InvalidStorageClass { class: String },

    /// Operation not supported
    OperationNotSupported { operation: String, reason: String },

    /// Validation error
    ValidationError { message: String },

    /// Unsupported operation
    UnsupportedOperation { operation: String, reason: String },

    /// Infrastructure error with external source
    InfrastructureError {
        message: String,
        source: Option<String>, // Store error as string to allow Clone
    },

    /// Generic storage error
    InternalError { message: String },

    /// Storage backend error
    StorageBackendError { message: String },
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageError::ObjectNotFound { key } => {
                write!(f, "Object not found: {}", key)
            }
            StorageError::VersionNotFound { key, version_id } => {
                write!(f, "Version '{}' not found for object: {}", version_id, key)
            }
            StorageError::VersionConflict {
                key,
                expected_version,
                actual_version,
            } => {
                write!(
                    f,
                    "Version conflict for object '{}': expected {:?}, actual {:?}",
                    key, expected_version, actual_version
                )
            }
            StorageError::QuotaExceeded { used, limit } => {
                write!(
                    f,
                    "Storage quota exceeded: {} bytes used of {} limit",
                    used, limit
                )
            }
            StorageError::InvalidObjectSize { size, min, max } => {
                let mut msg = format!("Invalid object size: {} bytes", size);
                if let Some(min) = min {
                    msg.push_str(&format!(" (min: {})", min));
                }
                if let Some(max) = max {
                    msg.push_str(&format!(" (max: {})", max));
                }
                write!(f, "{}", msg)
            }
            StorageError::AccessDenied { key, operation } => {
                write!(
                    f,
                    "Access denied for operation '{}' on object: {}",
                    operation, key
                )
            }
            StorageError::ObjectAlreadyExists { key } => {
                write!(f, "Object already exists: {}", key)
            }
            StorageError::InvalidStorageClass { class } => {
                write!(f, "Invalid storage class: {}", class)
            }
            StorageError::OperationNotSupported { operation, reason } => {
                write!(f, "Operation '{}' not supported: {}", operation, reason)
            }
            StorageError::ValidationError { message } => {
                write!(f, "Validation error: {}", message)
            }
            StorageError::UnsupportedOperation { operation, reason } => {
                write!(f, "Unsupported operation '{}': {}", operation, reason)
            }
            StorageError::InfrastructureError { message, .. } => {
                write!(f, "Infrastructure error: {}", message)
            }
            StorageError::InternalError { message } => {
                write!(f, "Internal storage error: {}", message)
            }
            StorageError::StorageBackendError { message } => {
                write!(f, "Storage backend error: {}", message)
            }
        }
    }
}

impl std::error::Error for StorageError {}

/// Result type for storage operations
pub type StorageResult<T> = Result<T, StorageError>;
