use std::sync::Arc;
use async_trait::async_trait;
use object_store::{
    ObjectStore as ObjectStoreBackend, 
    path::Path as ObjectPath,
    PutPayload,
    ObjectMeta,
};

use crate::{
    domain::{
        models::{ObjectMetadata, Filter},
        value_objects::{ObjectKey, BucketName},
        errors::{StorageError, StorageResult},
    },
    ports::storage::{ObjectStore, ObjectInfo, ObjectListItem, CompletedPart, MultipartUpload, PresignedUrlMethod},
};
use std::collections::HashMap;
use bytes::Bytes;
use tokio_util::io::ReaderStream;

/// S3 storage adapter that implements the ObjectStore trait
#[derive(Clone)]
pub struct S3ObjectStoreAdapter {
    store: Arc<dyn ObjectStoreBackend>,
    bucket: BucketName,
}

impl S3ObjectStoreAdapter {
    /// Create a new S3 adapter
    pub fn new(store: Arc<dyn ObjectStoreBackend>, bucket: BucketName) -> Self {
        Self { store, bucket }
    }

    /// Convert ObjectKey to object_store Path
    fn to_object_path(&self, key: &ObjectKey) -> ObjectPath {
        ObjectPath::from(key.as_str())
    }

    /// Convert object_store ObjectMeta to our ObjectListItem
    fn to_object_list_item(&self, meta: ObjectMeta) -> ObjectListItem {
        let key = ObjectKey::new(meta.location.to_string()).unwrap();
        ObjectListItem {
            key,
            size: meta.size,
            etag: meta.e_tag,
            last_modified: meta.last_modified.into(),
            content_type: None, // S3 doesn't provide content-type in list operations
        }
    }

    /// Convert StorageError from object_store errors
    fn convert_error(err: object_store::Error) -> StorageError {
        match err {
            object_store::Error::NotFound { .. } => StorageError::ObjectNotFound {
                key: "unknown".to_string(),
            },
            object_store::Error::AlreadyExists { .. } => StorageError::ObjectAlreadyExists {
                key: "unknown".to_string(),
            },
            _ => StorageError::StorageBackendError {
                message: err.to_string(),
            },
        }
    }
}

#[async_trait]
impl ObjectStore for S3ObjectStoreAdapter {
    async fn put_object(
        &self,
        key: &ObjectKey,
        data: Bytes,
        content_type: Option<&str>,
    ) -> StorageResult<ObjectInfo> {
        let path = self.to_object_path(key);
        
        let mut payload = PutPayload::from(data);
        if let Some(ct) = content_type {
            payload = payload.with_content_type(ct);
        }

        let result = self.store
            .put(&path, payload)
            .await
            .map_err(Self::convert_error)?;

        Ok(ObjectInfo {
            key: key.clone(),
            size: data.len() as u64, // Use the data size we just uploaded
            etag: result.e_tag,
            version_id: result.version,
            last_modified: chrono::Utc::now(), // Use current time for new objects
        })
    }

    async fn get_object(&self, key: &ObjectKey) -> StorageResult<Bytes> {
        let path = self.to_object_path(key);
        
        let result = self.store
            .get(&path)
            .await
            .map_err(Self::convert_error)?;

        let bytes = result
            .bytes()
            .await
            .map_err(|e| StorageError::StorageBackendError {
                message: e.to_string(),
            })?;

        Ok(bytes)
    }

    async fn get_object_stream(
        &self,
        key: &ObjectKey,
    ) -> StorageResult<Box<dyn tokio::io::AsyncRead + Send + Unpin>> {
        // For now, convert to bytes and create a cursor - not ideal for large objects
        // In a real implementation, you'd use proper streaming
        let data = self.get_object(key).await?;
        let cursor = std::io::Cursor::new(data);
        Ok(Box::new(cursor))
    }

    async fn delete_object(&self, key: &ObjectKey) -> StorageResult<()> {
        let path = self.to_object_path(key);
        
        self.store
            .delete(&path)
            .await
            .map_err(Self::convert_error)?;

        Ok(())
    }

    async fn list_objects(&self, filter: &Filter) -> StorageResult<Vec<ObjectListItem>> {
        let prefix = filter.prefix.as_ref().map(|p| ObjectPath::from(p.as_str()));
        
        use futures::TryStreamExt;
        let mut list_stream = self.store.list(prefix.as_ref());
        let mut objects = Vec::new();

        while let Some(meta) = list_stream.try_next().await.map_err(Self::convert_error)? {
            // Apply client-side filtering
            let key_str = meta.location.to_string();
            
            // Apply prefix filter (already handled by object_store, but double-check)
            if let Some(prefix) = &filter.prefix {
                if !key_str.starts_with(prefix.as_str()) {
                    continue;
                }
            }

            // Apply suffix filter
            if let Some(suffix) = &filter.suffix {
                if !key_str.ends_with(suffix.as_str()) {
                    continue;
                }
            }

            // Apply max_keys limit
            if let Some(max_keys) = filter.max_keys {
                if objects.len() >= max_keys as usize {
                    break;
                }
            }

            objects.push(self.to_object_list_item(meta));
        }

        Ok(objects)
    }

    async fn head_object(&self, key: &ObjectKey) -> StorageResult<ObjectMetadata> {
        let path = self.to_object_path(key);
        
        let meta = self.store
            .head(&path)
            .await
            .map_err(Self::convert_error)?;

        Ok(ObjectMetadata {
            content_type: None,
            content_length: meta.size,
            etag: meta.e_tag,
            last_modified: meta.last_modified,
            custom_metadata: HashMap::new(),
        })
    }

    async fn copy_object(
        &self,
        source_key: &ObjectKey,
        dest_key: &ObjectKey,
    ) -> StorageResult<ObjectInfo> {
        let source_path = self.to_object_path(source_key);
        let dest_path = self.to_object_path(dest_key);
        
        let result = self.store
            .copy(&source_path, &dest_path)
            .await
            .map_err(Self::convert_error)?;

        Ok(ObjectInfo {
            key: dest_key.clone(),
            size: 0, // We don't know the size from copy operation
            etag: result.e_tag,
            version_id: result.version,
            last_modified: chrono::Utc::now(),
        })
    }

    async fn object_exists(&self, key: &ObjectKey) -> StorageResult<bool> {
        match self.head_object(key).await {
            Ok(_) => Ok(true),
            Err(StorageError::ObjectNotFound { .. }) => Ok(false),
            Err(e) => Err(e),
        }
    }

    // Multipart upload methods - S3 native support
    async fn initiate_multipart_upload(&self, key: &ObjectKey) -> StorageResult<String> {
        // For now, return a simple upload ID. In a real implementation,
        // you would use the S3 multipart upload API
        Ok(format!("upload-{}", uuid::Uuid::new_v4()))
    }

    async fn upload_part(
        &self,
        key: &ObjectKey,
        upload_id: &str,
        part_number: u32,
        data: Bytes,
    ) -> StorageResult<CompletedPart> {
        // This is a simplified implementation. In reality, you would:
        // 1. Use S3's upload_part API
        // 2. Store the part ETag for later completion
        Ok(CompletedPart {
            part_number,
            etag: format!("etag-{}-{}", upload_id, part_number),
        })
    }

    async fn complete_multipart_upload(
        &self,
        key: &ObjectKey,
        upload_id: &str,
        parts: Vec<CompletedPart>,
    ) -> StorageResult<ObjectInfo> {
        // Simplified implementation. In reality, you would:
        // 1. Call S3's complete_multipart_upload API
        // 2. Provide the list of part ETags
        Ok(ObjectInfo {
            key: key.clone(),
            size: parts.iter().map(|_| 0u64).sum(), // Simplified - we don't track part sizes
            etag: Some(format!("final-etag-{}", upload_id)),
            version_id: None,
            last_modified: chrono::Utc::now(),
        })
    }

    async fn abort_multipart_upload(&self, key: &ObjectKey, upload_id: &str) -> StorageResult<()> {
        // Simplified implementation. In reality, you would:
        // 1. Call S3's abort_multipart_upload API
        // 2. Clean up any uploaded parts
        Ok(())
    }

    async fn get_presigned_url(
        &self,
        key: &ObjectKey,
        expiration_seconds: u64,
        method: PresignedUrlMethod,
    ) -> StorageResult<String> {
        // For now, return a placeholder URL. In a real implementation,
        // you would use the S3 SDK to generate pre-signed URLs
        Ok(format!(
            "https://s3.amazonaws.com/{}/{}?method={}&expires={}",
            self.bucket.as_str(),
            key.as_str(),
            method,
            expiration_seconds
        ))
    }

    async fn list_multipart_uploads(&self) -> StorageResult<Vec<MultipartUpload>> {
        // In a real implementation, you would call ListMultipartUploads
        Ok(Vec::new())
    }

    async fn set_object_metadata(
        &self,
        key: &ObjectKey,
        metadata: HashMap<String, String>,
    ) -> StorageResult<()> {
        // In a real implementation, you would use S3's object tagging or metadata APIs
        Ok(())
    }

    async fn get_object_metadata(&self, key: &ObjectKey) -> StorageResult<HashMap<String, String>> {
        // In a real implementation, you would retrieve S3 object tags and metadata
        Ok(HashMap::new())
    }
}