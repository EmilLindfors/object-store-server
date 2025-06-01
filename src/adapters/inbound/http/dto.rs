use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::domain::{
    errors::{LifecycleError, StorageError, ValidationError},
    models::{Filter, LifecycleConfiguration, LifecycleRule, LifecycleStorageClass, RuleStatus},
    value_objects::{BucketName, ObjectKey},
};

/// DTO for object information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectInfoDto {
    pub key: String,
    pub size: u64,
    pub last_modified: DateTime<Utc>,
    pub etag: Option<String>,
    pub storage_class: Option<String>,
    pub version_id: Option<String>,
}

/// DTO for creating/updating objects
#[derive(Debug, Clone, Deserialize)]
pub struct CreateObjectDto {
    pub key: String,
    pub content_type: Option<String>,
    pub metadata: Option<HashMap<String, String>>,
    pub storage_class: Option<String>,
}

/// DTO for listing objects
#[derive(Debug, Clone, Deserialize)]
pub struct ListObjectsDto {
    pub prefix: Option<String>,
    pub max_results: Option<usize>,
    pub continuation_token: Option<String>,
}

/// DTO for object list response
#[derive(Debug, Clone, Serialize)]
pub struct ListObjectsResponseDto {
    pub objects: Vec<ObjectInfoDto>,
    pub is_truncated: bool,
    pub next_continuation_token: Option<String>,
    pub total_count: usize,
}

/// DTO for lifecycle rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecycleRuleDto {
    pub id: String,
    pub status: String, // "Enabled" or "Disabled"
    pub filter: FilterDto,

    // Expiration settings
    pub expiration_days: Option<u32>,
    pub expiration_date: Option<DateTime<Utc>>,

    // Transition settings
    pub transition_days: Option<u32>,
    pub transition_storage_class: Option<String>,

    // Delete marker expiration
    pub del_marker_expiration_days: Option<u32>,

    // Non-current version settings
    pub noncurrent_version_expiration_noncurrent_days: Option<u32>,
    pub noncurrent_version_transition_noncurrent_days: Option<u32>,
    pub noncurrent_version_transition_storage_class: Option<String>,

    // Multipart upload cleanup
    pub abort_incomplete_multipart_upload_days_after_initiation: Option<u32>,
}

/// DTO for lifecycle rule filter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterDto {
    pub prefix: Option<String>,
    pub tags: Option<HashMap<String, String>>,
    pub object_size_greater_than: Option<u64>,
    pub object_size_less_than: Option<u64>,
}

/// DTO for lifecycle configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecycleConfigurationDto {
    pub bucket: String,
    pub rules: Vec<LifecycleRuleDto>,
}

/// DTO for lifecycle evaluation request
#[derive(Debug, Clone, Deserialize)]
pub struct EvaluateLifecycleDto {
    pub key: String,
    pub object_created_at: Option<DateTime<Utc>>,
    pub object_tags: Option<HashMap<String, String>>,
    pub is_delete_marker: Option<bool>,
    pub is_current_version: Option<bool>,
}

/// DTO for lifecycle evaluation response
#[derive(Debug, Clone, Serialize)]
pub struct LifecycleEvaluationResponseDto {
    pub actions_to_apply: Vec<ApplicableActionDto>,
}

/// DTO for applicable lifecycle action
#[derive(Debug, Clone, Serialize)]
pub struct ApplicableActionDto {
    pub rule_id: String,
    pub action_type: String,
    pub reason: String,
    pub details: HashMap<String, serde_json::Value>,
}

/// DTO for versioned object
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionedObjectDto {
    pub key: String,
    pub version_id: String,
    pub size: u64,
    pub last_modified: DateTime<Utc>,
    pub etag: Option<String>,
    pub is_latest: bool,
}

/// DTO for version list response
#[derive(Debug, Clone, Serialize)]
pub struct ListVersionsResponseDto {
    pub versions: Vec<VersionedObjectDto>,
    pub delete_markers: Vec<DeleteMarkerDto>,
    pub is_truncated: bool,
    pub next_key_marker: Option<String>,
    pub next_version_id_marker: Option<String>,
}

/// DTO for delete marker
#[derive(Debug, Clone, Serialize)]
pub struct DeleteMarkerDto {
    pub key: String,
    pub version_id: String,
    pub last_modified: DateTime<Utc>,
    pub is_latest: bool,
}

/// DTO for error responses
#[derive(Debug, Clone, Serialize)]
pub struct ErrorResponseDto {
    pub error: String,
    pub message: String,
    pub details: Option<HashMap<String, serde_json::Value>>,
    pub timestamp: DateTime<Utc>,
}

/// DTO for success responses
#[derive(Debug, Clone, Serialize)]
pub struct SuccessResponseDto {
    pub message: String,
    pub data: Option<serde_json::Value>,
    pub timestamp: DateTime<Utc>,
}

// Conversion implementations

impl TryFrom<CreateObjectDto> for ObjectKey {
    type Error = ValidationError;

    fn try_from(dto: CreateObjectDto) -> Result<Self, Self::Error> {
        ObjectKey::new(dto.key)
    }
}

impl TryFrom<&str> for BucketName {
    type Error = ValidationError;

    fn try_from(name: &str) -> Result<Self, Self::Error> {
        BucketName::new(name.to_string())
    }
}

impl From<FilterDto> for Filter {
    fn from(dto: FilterDto) -> Self {
        let mut filter = Filter::default();

        if let Some(prefix) = dto.prefix {
            filter = filter.with_prefix(prefix);
        }

        if let Some(tags) = dto.tags {
            filter = filter.with_tags(tags);
        }

        if let Some(size) = dto.object_size_greater_than {
            filter = filter.with_object_size_greater_than(size);
        }

        if let Some(size) = dto.object_size_less_than {
            filter = filter.with_object_size_less_than(size);
        }

        filter
    }
}

impl From<Filter> for FilterDto {
    fn from(filter: Filter) -> Self {
        FilterDto {
            prefix: filter.get_prefix().map(|s| s.to_string()),
            tags: filter.get_tags().cloned(),
            object_size_greater_than: filter.get_object_size_greater_than(),
            object_size_less_than: filter.get_object_size_less_than(),
        }
    }
}

impl TryFrom<LifecycleRuleDto> for LifecycleRule {
    type Error = ValidationError;

    fn try_from(dto: LifecycleRuleDto) -> Result<Self, Self::Error> {
        let status = match dto.status.as_str() {
            "Enabled" => RuleStatus::Enabled,
            "Disabled" => RuleStatus::Disabled,
            _ => {
                return Err(ValidationError::InvalidField {
                    field: "status".to_string(),
                    value: dto.status,
                    expected: "Enabled or Disabled".to_string(),
                });
            }
        };

        let transition_storage_class = dto
            .transition_storage_class
            .map(|s| LifecycleStorageClass::from_str(&s))
            .transpose()
            .map_err(|e| ValidationError::InvalidField {
                field: "transition_storage_class".to_string(),
                value: dto.transition_storage_class.unwrap_or_default(),
                expected: e.to_string(),
            })?;

        let noncurrent_version_transition_storage_class = dto
            .noncurrent_version_transition_storage_class
            .map(|s| LifecycleStorageClass::from_str(&s))
            .transpose()
            .map_err(|e| ValidationError::InvalidField {
                field: "noncurrent_version_transition_storage_class".to_string(),
                value: dto
                    .noncurrent_version_transition_storage_class
                    .unwrap_or_default(),
                expected: e.to_string(),
            })?;

        Ok(LifecycleRule {
            id: dto.id,
            status,
            filter: dto.filter.into(),
            expiration_days: dto.expiration_days,
            expiration_date: dto.expiration_date,
            transition_days: dto.transition_days,
            transition_storage_class,
            del_marker_expiration_days: dto.del_marker_expiration_days,
            noncurrent_version_expiration_noncurrent_days: dto
                .noncurrent_version_expiration_noncurrent_days,
            noncurrent_version_transition_noncurrent_days: dto
                .noncurrent_version_transition_noncurrent_days,
            noncurrent_version_transition_storage_class,
            abort_incomplete_multipart_upload_days_after_initiation: dto
                .abort_incomplete_multipart_upload_days_after_initiation,
        })
    }
}

impl From<LifecycleRule> for LifecycleRuleDto {
    fn from(rule: LifecycleRule) -> Self {
        LifecycleRuleDto {
            id: rule.id,
            status: match rule.status {
                RuleStatus::Enabled => "Enabled".to_string(),
                RuleStatus::Disabled => "Disabled".to_string(),
            },
            filter: rule.filter.into(),
            expiration_days: rule.expiration_days,
            expiration_date: rule.expiration_date,
            transition_days: rule.transition_days,
            transition_storage_class: rule
                .transition_storage_class
                .map(|sc| sc.as_str().to_string()),
            del_marker_expiration_days: rule.del_marker_expiration_days,
            noncurrent_version_expiration_noncurrent_days: rule
                .noncurrent_version_expiration_noncurrent_days,
            noncurrent_version_transition_noncurrent_days: rule
                .noncurrent_version_transition_noncurrent_days,
            noncurrent_version_transition_storage_class: rule
                .noncurrent_version_transition_storage_class
                .map(|sc| sc.as_str().to_string()),
            abort_incomplete_multipart_upload_days_after_initiation: rule
                .abort_incomplete_multipart_upload_days_after_initiation,
        }
    }
}

impl TryFrom<LifecycleConfigurationDto> for LifecycleConfiguration {
    type Error = ValidationError;

    fn try_from(dto: LifecycleConfigurationDto) -> Result<Self, Self::Error> {
        let bucket = BucketName::new(dto.bucket)?;
        let mut rules = Vec::new();

        for rule_dto in dto.rules {
            rules.push(rule_dto.try_into()?);
        }

        Ok(LifecycleConfiguration { bucket, rules })
    }
}

impl From<LifecycleConfiguration> for LifecycleConfigurationDto {
    fn from(config: LifecycleConfiguration) -> Self {
        LifecycleConfigurationDto {
            bucket: config.bucket.as_str().to_string(),
            rules: config.rules.into_iter().map(|r| r.into()).collect(),
        }
    }
}

// Error response helpers

impl ErrorResponseDto {
    pub fn from_storage_error(error: StorageError) -> Self {
        let mut details = HashMap::new();

        match &error {
            StorageError::ObjectNotFound { key } => {
                details.insert(
                    "key".to_string(),
                    serde_json::Value::String(key.as_str().to_string()),
                );
            }
            StorageError::VersionNotFound { key, version_id } => {
                details.insert(
                    "key".to_string(),
                    serde_json::Value::String(key.as_str().to_string()),
                );
                details.insert(
                    "version_id".to_string(),
                    serde_json::Value::String(version_id.as_str().to_string()),
                );
            }
            StorageError::QuotaExceeded { used, limit } => {
                details.insert(
                    "used".to_string(),
                    serde_json::Value::Number((*used).into()),
                );
                details.insert(
                    "limit".to_string(),
                    serde_json::Value::Number((*limit).into()),
                );
            }
            _ => {}
        }

        ErrorResponseDto {
            error: "StorageError".to_string(),
            message: error.to_string(),
            details: if details.is_empty() {
                None
            } else {
                Some(details)
            },
            timestamp: Utc::now(),
        }
    }

    pub fn from_lifecycle_error(error: LifecycleError) -> Self {
        let mut details = HashMap::new();

        match &error {
            LifecycleError::ValidationFailed { errors } => {
                details.insert(
                    "validation_errors".to_string(),
                    serde_json::Value::Array(
                        errors
                            .iter()
                            .map(|e| serde_json::Value::String(e.clone()))
                            .collect(),
                    ),
                );
            }
            LifecycleError::RuleNotFound { rule_id } => {
                details.insert(
                    "rule_id".to_string(),
                    serde_json::Value::String(rule_id.clone()),
                );
            }
            _ => {}
        }

        ErrorResponseDto {
            error: "LifecycleError".to_string(),
            message: error.to_string(),
            details: if details.is_empty() {
                None
            } else {
                Some(details)
            },
            timestamp: Utc::now(),
        }
    }

    pub fn bad_request(message: &str) -> Self {
        ErrorResponseDto {
            error: "BadRequest".to_string(),
            message: message.to_string(),
            details: None,
            timestamp: Utc::now(),
        }
    }

    pub fn internal_error(message: &str) -> Self {
        ErrorResponseDto {
            error: "InternalServerError".to_string(),
            message: message.to_string(),
            details: None,
            timestamp: Utc::now(),
        }
    }
}

impl SuccessResponseDto {
    pub fn new(message: &str) -> Self {
        SuccessResponseDto {
            message: message.to_string(),
            data: None,
            timestamp: Utc::now(),
        }
    }

    pub fn with_data(message: &str, data: serde_json::Value) -> Self {
        SuccessResponseDto {
            message: message.to_string(),
            data: Some(data),
            timestamp: Utc::now(),
        }
    }
}
