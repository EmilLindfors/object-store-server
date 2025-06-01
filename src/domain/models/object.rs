use std::collections::HashMap;

use crate::domain::value_objects::{ObjectKey, VersionId};

/// Represents metadata about an object in storage
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectMetadata {
    pub content_type: Option<String>,
    pub content_length: u64,
    pub etag: Option<String>,
    pub last_modified: std::time::SystemTime,
    pub custom_metadata: HashMap<String, String>,
}

/// Represents an object in the storage system
#[derive(Debug, Clone)]
pub struct StorageObject {
    pub key: ObjectKey,
    pub data: Vec<u8>,
    pub metadata: ObjectMetadata,
}

/// Represents a versioned object with its version information
#[derive(Debug, Clone)]
pub struct VersionedObject {
    pub key: ObjectKey,
    pub version_id: VersionId,
    pub data: Vec<u8>,
    pub metadata: ObjectMetadata,
    pub is_latest: bool,
    pub deleted: bool,
}

/// Request to create a new object
#[derive(Debug, Clone)]
pub struct CreateObjectRequest {
    pub key: ObjectKey,
    pub data: Vec<u8>,
    pub content_type: Option<String>,
    pub custom_metadata: HashMap<String, String>,
}

/// Request to retrieve an object
#[derive(Debug, Clone)]
pub struct GetObjectRequest {
    pub key: ObjectKey,
    pub version_id: Option<VersionId>,
}

/// Information about an object version
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectVersionInfo {
    pub version_id: VersionId,
    pub last_modified: std::time::SystemTime,
    pub size: u64,
    pub etag: Option<String>,
    pub is_latest: bool,
    pub deleted: bool,
}

/// List of versions for an object
#[derive(Debug, Clone)]
pub struct ObjectVersionList {
    pub key: ObjectKey,
    pub versions: Vec<ObjectVersionInfo>,
}
