use crate::domain::{
    errors::{LifecycleError, StorageError},
    value_objects::{ObjectKey, VersionId},
};
use std::io;
use thiserror::Error as ThisError;

#[derive(ThisError, Debug)]
pub enum StoreError {
    #[error("Object store error: {0}")]
    ObjectStore(#[from] object_store::Error),

    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Invalid lifecycle configuration: {0}")]
    InvalidLifecycleConfig(String),

    #[error("Version not found: {0}")]
    VersionNotFound(String),

    #[error("Bucket not found: {0}")]
    BucketNotFound(String),

    #[error("Object not found: {0}")]
    ObjectNotFound(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("HTTP error: {status} - {message}")]
    Http {
        status: http::StatusCode,
        message: String,
    },

    #[error("{0}")]
    Other(String),
}

impl From<StoreError> for http::StatusCode {
    fn from(err: StoreError) -> Self {
        match err {
            StoreError::ObjectNotFound(_) | StoreError::VersionNotFound(_) => {
                http::StatusCode::NOT_FOUND
            }
            StoreError::BucketNotFound(_) => http::StatusCode::NOT_FOUND,
            StoreError::InvalidLifecycleConfig(_) => http::StatusCode::BAD_REQUEST,
            StoreError::Http { status, .. } => status,
            _ => http::StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

/// Convert object_store errors to domain storage errors
impl From<object_store::Error> for StorageError {
    fn from(err: object_store::Error) -> Self {
        match err {
            object_store::Error::NotFound { path, .. } => {
                // Try to create ObjectKey, fallback to validation error if invalid
                match ObjectKey::new(path.to_string()) {
                    Ok(key) => StorageError::ObjectNotFound { key },
                    Err(_) => StorageError::ValidationError {
                        message: format!("Invalid object path from store: {}", path),
                    },
                }
            }
            object_store::Error::AlreadyExists { path, .. } => {
                match ObjectKey::new(path.to_string()) {
                    Ok(key) => StorageError::ObjectAlreadyExists { key },
                    Err(_) => StorageError::ValidationError {
                        message: format!("Invalid object path from store: {}", path),
                    },
                }
            }
            object_store::Error::Precondition { path, .. } => {
                match ObjectKey::new(path.to_string()) {
                    Ok(key) => StorageError::VersionConflict {
                        key,
                        expected_version: None,
                        actual_version: None,
                    },
                    Err(_) => StorageError::ValidationError {
                        message: format!("Invalid object path from store: {}", path),
                    },
                }
            }
            object_store::Error::NotSupported { .. } => StorageError::UnsupportedOperation {
                operation: "unknown".to_string(),
                reason: err.to_string(),
            },
            object_store::Error::NotModified { .. } => StorageError::InfrastructureError {
                message: "Object not modified".to_string(),
                source: Some(err.to_string()),
            },
            _ => StorageError::InfrastructureError {
                message: format!("Object store operation failed: {}", err),
                source: Some(err.to_string()),
            },
        }
    }
}

/// Convert infrastructure StoreError to domain StorageError
impl From<StoreError> for StorageError {
    fn from(err: StoreError) -> Self {
        match err {
            StoreError::ObjectStore(object_err) => object_err.into(),
            StoreError::ObjectNotFound(path) => match ObjectKey::new(path.clone()) {
                Ok(key) => StorageError::ObjectNotFound { key },
                Err(_) => StorageError::ValidationError {
                    message: format!("Invalid object path: {}", path),
                },
            },
            StoreError::VersionNotFound(version_info) => {
                // Version info might be in format "key:version" or just "version"
                if let Some((key_str, version_str)) = version_info.split_once(':') {
                    match (
                        ObjectKey::new(key_str.to_string()),
                        VersionId::new(version_str.to_string()),
                    ) {
                        (Ok(key), Ok(version_id)) => {
                            StorageError::VersionNotFound { key, version_id }
                        }
                        _ => StorageError::ValidationError {
                            message: format!("Invalid version info: {}", version_info),
                        },
                    }
                } else {
                    StorageError::ValidationError {
                        message: format!("Invalid version info format: {}", version_info),
                    }
                }
            }
            StoreError::BucketNotFound(bucket) => StorageError::ValidationError {
                message: format!("Bucket not found: {}", bucket),
            },
            StoreError::InvalidLifecycleConfig(msg) => {
                StorageError::ValidationError { message: msg }
            }
            StoreError::Serialization(serde_err) => StorageError::InfrastructureError {
                message: format!("Serialization failed: {}", serde_err),
                source: Some(serde_err.to_string()),
            },
            StoreError::Io(io_err) => StorageError::InfrastructureError {
                message: format!("IO operation failed: {}", io_err),
                source: Some(io_err.to_string()),
            },
            StoreError::Http { status, message } => {
                if status.is_client_error() {
                    StorageError::ValidationError { message }
                } else {
                    StorageError::InfrastructureError {
                        message: format!("HTTP error {}: {}", status, message),
                        source: Some(format!("{}: {}", status, message)),
                    }
                }
            }
            StoreError::Other(msg) => StorageError::InternalError { message: msg },
        }
    }
}

/// Convert infrastructure StoreError to domain LifecycleError
impl From<StoreError> for LifecycleError {
    fn from(err: StoreError) -> Self {
        match err {
            StoreError::InvalidLifecycleConfig(msg) => {
                LifecycleError::ValidationFailed { errors: vec![msg] }
            }
            StoreError::Serialization(serde_err) => LifecycleError::RepositoryError {
                message: format!("Configuration serialization failed: {}", serde_err),
            },
            StoreError::Io(io_err) => LifecycleError::RepositoryError {
                message: format!("IO operation failed: {}", io_err),
            },
            _ => LifecycleError::RepositoryError {
                message: format!("Infrastructure error: {}", err),
            },
        }
    }
}

/// Convert standard io::Error to domain errors
impl From<std::io::Error> for StorageError {
    fn from(err: std::io::Error) -> Self {
        match err.kind() {
            std::io::ErrorKind::NotFound => StorageError::ValidationError {
                message: "File or resource not found".to_string(),
            },
            std::io::ErrorKind::PermissionDenied => {
                StorageError::AccessDenied {
                    key: ObjectKey::new("unknown".to_string()).unwrap_or_else(|_| 
                        // This should never fail with "unknown"
                        panic!("Failed to create ObjectKey with 'unknown'")),
                    operation: "io_operation".to_string(),
                }
            }
            std::io::ErrorKind::OutOfMemory => StorageError::QuotaExceeded { used: 0, limit: 0 },
            _ => StorageError::InfrastructureError {
                message: format!("IO error: {}", err),
                source: Some(err.to_string()),
            },
        }
    }
}

/// Convert serde JSON errors to domain errors
impl From<serde_json::Error> for StorageError {
    fn from(err: serde_json::Error) -> Self {
        StorageError::ValidationError {
            message: format!("JSON serialization/deserialization error: {}", err),
        }
    }
}

impl From<serde_json::Error> for LifecycleError {
    fn from(err: serde_json::Error) -> Self {
        LifecycleError::RepositoryError {
            message: format!("JSON processing error: {}", err),
        }
    }
}

/// Convert domain StorageError to HTTP status codes for API responses
impl From<StorageError> for http::StatusCode {
    fn from(err: StorageError) -> Self {
        match err {
            StorageError::ObjectNotFound { .. } | StorageError::VersionNotFound { .. } => {
                http::StatusCode::NOT_FOUND
            }
            StorageError::VersionConflict { .. } => http::StatusCode::CONFLICT,
            StorageError::QuotaExceeded { .. } => http::StatusCode::INSUFFICIENT_STORAGE,
            StorageError::InvalidObjectSize { .. }
            | StorageError::InvalidStorageClass { .. }
            | StorageError::ValidationError { .. } => http::StatusCode::BAD_REQUEST,
            StorageError::AccessDenied { .. } => http::StatusCode::FORBIDDEN,
            StorageError::ObjectAlreadyExists { .. } => http::StatusCode::CONFLICT,
            StorageError::OperationNotSupported { .. }
            | StorageError::UnsupportedOperation { .. } => http::StatusCode::NOT_IMPLEMENTED,
            StorageError::InfrastructureError { .. } | StorageError::InternalError { .. } => {
                http::StatusCode::INTERNAL_SERVER_ERROR
            }
        }
    }
}

/// Convert domain LifecycleError to HTTP status codes for API responses
impl From<LifecycleError> for http::StatusCode {
    fn from(err: LifecycleError) -> Self {
        match err {
            LifecycleError::ValidationFailed { .. }
            | LifecycleError::InvalidRule { .. }
            | LifecycleError::ConflictingRules { .. }
            | LifecycleError::InvalidExpiration { .. }
            | LifecycleError::InvalidTransition { .. }
            | LifecycleError::TooManyRules { .. } => http::StatusCode::BAD_REQUEST,
            LifecycleError::ConfigurationNotFound { .. } | LifecycleError::RuleNotFound { .. } => {
                http::StatusCode::NOT_FOUND
            }
            LifecycleError::ActionFailed { .. }
            | LifecycleError::ActionExecutionFailed { .. }
            | LifecycleError::ProcessingError { .. }
            | LifecycleError::RepositoryError { .. } => http::StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}
