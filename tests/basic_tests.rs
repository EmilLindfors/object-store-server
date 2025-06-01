use async_trait::async_trait;
use bytes::Bytes;
use futures::{StreamExt, stream, stream::BoxStream};
use object_store::{Attributes, GetResultPayload};
use object_store::{
    GetOptions, GetResult, ListResult, MultipartUpload, ObjectMeta, ObjectStore, PutMultipartOpts,
    PutOptions, PutPayload, PutResult, Result, path::Path,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::{fmt, io};
use uuid::Uuid;

// Create a very basic in-memory store
#[derive(Debug, Clone)]
struct MemoryStore {
    data: Arc<tokio::sync::RwLock<HashMap<String, Bytes>>>,
}

impl MemoryStore {
    fn new() -> Self {
        Self {
            data: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }
}

impl fmt::Display for MemoryStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "MemoryStore")
    }
}

#[async_trait]
impl ObjectStore for MemoryStore {
    async fn put_opts(
        &self,
        location: &Path,
        bytes: PutPayload,
        _options: PutOptions,
    ) -> Result<PutResult> {
        self.put(location, bytes).await
    }

    async fn put_multipart_opts(
        &self,
        _location: &Path,
        _options: PutMultipartOpts,
    ) -> Result<Box<dyn MultipartUpload>> {
        Err(object_store::Error::NotSupported {
            source: Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Not implemented",
            )),
        })
    }

    async fn put(&self, location: &Path, bytes: PutPayload) -> Result<PutResult> {
        let mut data = self.data.write().await;
        data.insert(location.to_string(), bytes.into());
        Ok(PutResult {
            e_tag: Some("etag".to_string()),
            version: Some("1".to_string()),
        })
    }

    async fn get_opts(&self, location: &Path, _options: GetOptions) -> Result<GetResult> {
        self.get(location).await
    }

    async fn get(&self, location: &Path) -> Result<GetResult> {
        let objects = self.data.read().await;
        let (data, meta, range, attributes) = match objects.get(location.as_ref()) {
            Some(data) => {
                let meta = ObjectMeta {
                    location: location.clone(),
                    last_modified: chrono::Utc::now(),
                    size: data.len() as u64,
                    e_tag: None,
                    version: Some(Uuid::new_v4().to_string()),
                };

                // Create a new GetResult using the constructor method
                Ok((
                    data.clone(),
                    meta,
                    0..data.len() as u64,
                    Attributes::default(),
                ))
            }
            None => Err(object_store::Error::NotFound {
                path: location.to_string(),
                source: io::Error::new(io::ErrorKind::NotFound, "Object not found").into(),
            }),
        }?;

        // Return the result
        Ok(GetResult {
            payload: GetResultPayload::Stream(Box::pin(futures::stream::once(async move {
                Ok(data.clone())
            }))),
            meta,
            range,
            attributes,
        })
    }

    async fn delete(&self, location: &Path) -> Result<()> {
        let mut data = self.data.write().await;
        data.remove(&location.to_string());
        Ok(())
    }

    fn list(&self, _prefix: Option<&Path>) -> BoxStream<'static, Result<ObjectMeta>> {
        Box::pin(stream::empty())
    }

    async fn list_with_delimiter(&self, _prefix: Option<&Path>) -> Result<ListResult> {
        Ok(ListResult {
            common_prefixes: vec![],
            objects: vec![],
        })
    }

    async fn copy(&self, from: &Path, to: &Path) -> Result<()> {
        let data = self.data.read().await;
        let bytes = match data.get(&from.to_string()) {
            Some(bytes) => bytes.clone(),
            None => {
                return Err(object_store::Error::NotFound {
                    path: from.to_string(),
                    source: std::io::Error::new(std::io::ErrorKind::NotFound, "Not found").into(),
                });
            }
        };
        drop(data);

        let mut data = self.data.write().await;
        data.insert(to.to_string(), bytes);
        Ok(())
    }

    async fn copy_if_not_exists(&self, from: &Path, to: &Path) -> Result<()> {
        let mut data = self.data.write().await;
        if data.contains_key(&to.to_string()) {
            return Ok(());
        }

        let bytes = match data.get(&from.to_string()) {
            Some(bytes) => bytes.clone(),
            None => {
                return Err(object_store::Error::NotFound {
                    path: from.to_string(),
                    source: std::io::Error::new(std::io::ErrorKind::NotFound, "Not found").into(),
                });
            }
        };

        data.insert(to.to_string(), bytes);
        Ok(())
    }
}

// Tests
#[tokio::test]
async fn basic_put_get() {
    let store = MemoryStore::new();
    let path = Path::from("test.txt");
    let data = Bytes::from("hello world");

    // Put the data
    store.put(&path, data.clone().into()).await.unwrap();

    // Get it back
    let result = store.get(&path).await.unwrap();

    // Get the content as bytes for comparison
    let result_bytes = result.bytes().await.unwrap();

    // Verify - extract the bytes from the payload
    assert_eq!(result_bytes, data);
}

#[tokio::test]
async fn basic_delete() {
    let store = MemoryStore::new();
    let path = Path::from("to_delete.txt");
    let data = Bytes::from("delete me");

    // Put the data
    store.put(&path, data.clone().into()).await.unwrap();

    // Verify it exists
    let exists = store.get(&path).await.is_ok();
    assert!(exists);

    // Delete it
    store.delete(&path).await.unwrap();

    // Verify it's gone
    let result = store.get(&path).await;
    assert!(result.is_err());
}
