use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::{
    domain::{
        errors::{StorageError, StorageResult},
        models::{ObjectMetadata, ObjectVersionInfo, ObjectVersionList},
        value_objects::{ObjectKey, VersionId},
    },
    ports::repositories::ObjectRepository,
};

/// In-memory implementation of ObjectRepository for testing and development
#[derive(Clone)]
pub struct InMemoryObjectRepository {
    data: Arc<RwLock<RepositoryData>>,
}

#[derive(Default)]
struct RepositoryData {
    // Map of object key -> version id -> metadata
    objects: HashMap<String, HashMap<String, StoredVersion>>,
    // Track latest version for each object
    latest_versions: HashMap<String, String>,
}

#[derive(Clone)]
struct StoredVersion {
    metadata: ObjectMetadata,
    deleted: bool,
}

impl InMemoryObjectRepository {
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(RepositoryData::default())),
        }
    }
}

#[async_trait]
impl ObjectRepository for InMemoryObjectRepository {
    async fn save_object_metadata(
        &self,
        key: &ObjectKey,
        version_id: &VersionId,
        metadata: &ObjectMetadata,
    ) -> StorageResult<()> {
        let mut data = self.data.write().await;

        let key_str = key.as_str().to_string();
        let version_str = version_id.as_str().to_string();

        // Get or create versions map for this object
        let versions = data
            .objects
            .entry(key_str.clone())
            .or_insert_with(HashMap::new);

        // Store the version
        versions.insert(
            version_str.clone(),
            StoredVersion {
                metadata: metadata.clone(),
                deleted: false,
            },
        );

        // Update latest version
        data.latest_versions.insert(key_str, version_str);

        Ok(())
    }

    async fn get_object_metadata(
        &self,
        key: &ObjectKey,
        version_id: Option<&VersionId>,
    ) -> StorageResult<Option<ObjectMetadata>> {
        let data = self.data.read().await;
        let key_str = key.as_str();

        // Get the version to retrieve
        let version_str = match version_id {
            Some(v) => v.as_str().to_string(),
            None => {
                // Get latest version
                match data.latest_versions.get(key_str) {
                    Some(v) => v.clone(),
                    None => return Ok(None),
                }
            }
        };

        // Get the metadata
        Ok(data
            .objects
            .get(key_str)
            .and_then(|versions| versions.get(&version_str))
            .filter(|v| !v.deleted)
            .map(|v| v.metadata.clone()))
    }

    async fn list_object_versions(&self, key: &ObjectKey) -> StorageResult<ObjectVersionList> {
        let data = self.data.read().await;
        let key_str = key.as_str();

        let versions = data
            .objects
            .get(key_str)
            .map(|versions| {
                versions
                    .iter()
                    .map(|(version_id, stored)| ObjectVersionInfo {
                        version_id: VersionId::new(version_id.clone()).unwrap(),
                        last_modified: stored.metadata.last_modified,
                        size: stored.metadata.content_length,
                        etag: stored.metadata.etag.clone(),
                        is_latest: data.latest_versions.get(key_str) == Some(version_id),
                        deleted: stored.deleted,
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(ObjectVersionList {
            key: key.clone(),
            versions,
        })
    }

    async fn get_version_info(
        &self,
        key: &ObjectKey,
        version_id: &VersionId,
    ) -> StorageResult<Option<ObjectVersionInfo>> {
        let data = self.data.read().await;
        let key_str = key.as_str();
        let version_str = version_id.as_str();

        Ok(data
            .objects
            .get(key_str)
            .and_then(|versions| versions.get(version_str))
            .map(|stored| ObjectVersionInfo {
                version_id: version_id.clone(),
                last_modified: stored.metadata.last_modified,
                size: stored.metadata.content_length,
                etag: stored.metadata.etag.clone(),
                is_latest: data.latest_versions.get(key_str) == Some(&version_str.to_string()),
                deleted: stored.deleted,
            }))
    }

    async fn mark_version_deleted(
        &self,
        key: &ObjectKey,
        version_id: &VersionId,
    ) -> StorageResult<()> {
        let mut data = self.data.write().await;
        let key_str = key.as_str();
        let version_str = version_id.as_str();

        if let Some(versions) = data.objects.get_mut(key_str) {
            if let Some(stored) = versions.get_mut(version_str) {
                stored.deleted = true;
                return Ok(());
            }
        }

        Err(StorageError::VersionNotFound {
            key: key.clone(),
            version_id: version_id.clone(),
        })
    }

    async fn delete_version_metadata(
        &self,
        key: &ObjectKey,
        version_id: &VersionId,
    ) -> StorageResult<()> {
        let mut data = self.data.write().await;
        let key_str = key.as_str();
        let version_str = version_id.as_str();

        let should_update_latest =
            data.latest_versions.get(key_str) == Some(&version_str.to_string());
        let mut should_remove_object = false;
        let mut new_latest: Option<String> = None;

        if let Some(versions) = data.objects.get_mut(key_str) {
            if versions.remove(version_str).is_some() {
                // Determine new latest version if needed
                if should_update_latest {
                    new_latest = versions
                        .iter()
                        .filter(|(_, v)| !v.deleted)
                        .max_by_key(|(_, v)| v.metadata.last_modified)
                        .map(|(k, _)| k.clone());
                }

                // Check if object should be removed
                should_remove_object = versions.is_empty();
            } else {
                return Err(StorageError::VersionNotFound {
                    key: key.clone(),
                    version_id: version_id.clone(),
                });
            }
        } else {
            return Err(StorageError::ObjectNotFound { key: key.clone() });
        }

        // Update latest version tracking
        if should_update_latest {
            match new_latest {
                Some(v) => {
                    data.latest_versions.insert(key_str.to_string(), v);
                }
                None => {
                    data.latest_versions.remove(key_str);
                }
            }
        }

        // Remove object entry if no versions left
        if should_remove_object {
            data.objects.remove(key_str);
            data.latest_versions.remove(key_str);
        }

        Ok(())
    }

    async fn get_latest_version_id(&self, key: &ObjectKey) -> StorageResult<Option<VersionId>> {
        let data = self.data.read().await;
        let key_str = key.as_str();

        Ok(data
            .latest_versions
            .get(key_str)
            .and_then(|v| VersionId::new(v.clone()).ok()))
    }

    async fn list_objects_by_prefix(
        &self,
        prefix: &str,
        max_results: Option<usize>,
    ) -> StorageResult<Vec<ObjectKey>> {
        let data = self.data.read().await;

        let mut keys: Vec<ObjectKey> = data
            .objects
            .keys()
            .filter(|k| k.starts_with(prefix))
            .filter_map(|k| ObjectKey::new(k.clone()).ok())
            .collect();

        // Sort for consistent results
        keys.sort_by(|a, b| a.as_str().cmp(b.as_str()));

        // Apply limit if specified
        if let Some(max) = max_results {
            keys.truncate(max);
        }

        Ok(keys)
    }

    async fn update_object_metadata(
        &self,
        key: &ObjectKey,
        version_id: &VersionId,
        metadata: &ObjectMetadata,
    ) -> StorageResult<()> {
        let mut data = self.data.write().await;
        let key_str = key.as_str();
        let version_str = version_id.as_str();

        if let Some(versions) = data.objects.get_mut(key_str) {
            if let Some(stored) = versions.get_mut(version_str) {
                stored.metadata = metadata.clone();
                return Ok(());
            }
        }

        Err(StorageError::VersionNotFound {
            key: key.clone(),
            version_id: version_id.clone(),
        })
    }

    async fn object_exists(&self, key: &ObjectKey) -> StorageResult<bool> {
        let data = self.data.read().await;
        let key_str = key.as_str();

        Ok(
            data.objects.contains_key(key_str)
                && data.objects[key_str].values().any(|v| !v.deleted),
        )
    }
}
