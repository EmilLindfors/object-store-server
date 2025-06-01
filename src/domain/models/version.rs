use crate::domain::value_objects::{ObjectKey, VersionId};
use std::collections::HashMap;

/// Configuration for versioning behavior
#[derive(Debug, Clone, PartialEq)]
pub struct VersioningConfiguration {
    pub enabled: bool,
    pub max_versions: Option<u32>,
}

impl Default for VersioningConfiguration {
    fn default() -> Self {
        Self {
            enabled: true,
            max_versions: None,
        }
    }
}

/// Metadata specifically related to a version
#[derive(Debug, Clone, PartialEq)]
pub struct VersionMetadata {
    pub created_at: std::time::SystemTime,
    pub created_by: Option<String>,
    pub delete_marker: bool,
    pub tags: HashMap<String, String>,
}

/// Represents a version transition (for lifecycle policies)
#[derive(Debug, Clone, PartialEq)]
pub struct VersionTransition {
    pub version_id: VersionId,
    pub transition_date: std::time::SystemTime,
    pub storage_class: StorageClass,
}

/// Storage classes for object versions
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
}

/// Version retention policy
#[derive(Debug, Clone, PartialEq)]
pub struct VersionRetentionPolicy {
    pub mode: RetentionMode,
    pub retain_until: Option<std::time::SystemTime>,
    pub legal_hold: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RetentionMode {
    Governance,
    Compliance,
}

/// Request to delete a specific version
#[derive(Debug, Clone)]
pub struct DeleteVersionRequest {
    pub key: ObjectKey,
    pub version_id: VersionId,
}

/// Result of a version deletion
#[derive(Debug, Clone)]
pub struct DeleteVersionResult {
    pub key: ObjectKey,
    pub version_id: VersionId,
    pub delete_marker_created: bool,
}
