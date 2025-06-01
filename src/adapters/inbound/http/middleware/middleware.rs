use axum::{
    Router,
    body::Body,
    extract::{Path, Query, State},
    http::{Request, Response, StatusCode},
    response::{IntoResponse, Json},
    routing::get,
};
use bytes::Bytes;
use futures::future::BoxFuture;
use http::HeaderMap;
use object_store::{ObjectStore, path::Path as ObjectPath};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::Arc,
    task::{Context, Poll},
};
use tower::{Layer, Service};
use uuid::Uuid;

use crate::adapters::outbound::storage::{
    error::StoreError,
    lifecycle::{LifecycleConfiguration, LifecycleManager},
    lifecycle_adapter,
    minio::MinioClient,
    versioning::VersionedStore,
};

// Responses

/// Response for successful PUT operation
#[derive(Debug, Serialize)]
pub struct PutObjectResponse {
    /// The version ID of the newly created object
    pub version_id: String,

    /// The ETag for the object
    pub etag: Option<String>,
}

/// Response for listing object versions
#[derive(Debug, Serialize)]
pub struct ListVersionsResponse {
    /// The object key
    pub key: String,

    /// List of versions
    pub versions: Vec<VersionInfo>,
}

/// Information about a specific version
#[derive(Debug, Serialize)]
pub struct VersionInfo {
    /// Version ID
    pub version_id: String,

    /// When this version was created
    pub created_at: String,

    /// Size of the object in bytes
    pub size: usize,

    /// ETag if available
    pub etag: Option<String>,
}

// Service and Layer

/// Type alias for an S3-compatible object store
pub type S3CompatibleStore<T> = VersionedStore<T>;

/// Core service for the object store
pub struct ObjectStoreService<T: ObjectStore + Send + Sync + 'static> {
    /// The versioned object store
    pub store: Arc<S3CompatibleStore<T>>,

    /// The lifecycle manager
    pub lifecycle_manager: Arc<LifecycleManager<T>>,
}

impl<T: ObjectStore + Send + Sync + 'static> ObjectStoreService<T> {
    /// Create a new instance of the service
    pub fn new(store: T) -> Self {
        // Create versioned store
        let versioned_store = Arc::new(VersionedStore::new(store));

        // Create lifecycle manager
        let lifecycle_manager = Arc::new(LifecycleManager::new(versioned_store.clone()));

        ObjectStoreService {
            store: versioned_store,
            lifecycle_manager,
        }
    }

    /// Start the lifecycle manager
    pub async fn start_lifecycle_manager(&self) -> Result<(), StoreError> {
        self.lifecycle_manager.start().await
    }

    /// Get an object
    pub async fn get_object(&self, bucket: &str, key: &str) -> Result<Bytes, StoreError> {
        let path = self.make_path(bucket, key);

        let result = self.store.get(&path).await.map_err(|e| match e {
            object_store::Error::NotFound { .. } => StoreError::ObjectNotFound(key.to_string()),
            e => StoreError::ObjectStore(e),
        })?;

        Ok(result.bytes().await?)
    }

    /// Get a specific version of an object
    pub async fn get_object_version(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> Result<Bytes, StoreError> {
        let path = self.make_path(bucket, key);

        self.store.get_version(&path, version_id).await
    }

    /// Put an object
    pub async fn put_object(
        &self,
        bucket: &str,
        key: &str,
        data: Bytes,
        metadata: Option<HashMap<String, String>>,
    ) -> Result<PutObjectResponse, StoreError> {
        let path = self.make_path(bucket, key);

        // Generate version ID
        let version_id = Uuid::new_v4().to_string();

        // Store object
        self.store
            .put(&path, data.into())
            .await
            .map_err(StoreError::ObjectStore)?;

        // In a real implementation, we'd store the metadata and etag
        let etag = metadata.as_ref().and_then(|m| m.get("etag").cloned());

        Ok(PutObjectResponse { version_id, etag })
    }

    /// Delete an object
    pub async fn delete_object(&self, bucket: &str, key: &str) -> Result<(), StoreError> {
        let path = self.make_path(bucket, key);

        self.store
            .delete(&path)
            .await
            .map_err(StoreError::ObjectStore)
    }

    /// Delete a specific version of an object
    pub async fn delete_object_version(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> Result<(), StoreError> {
        println!(
            "Service: delete_object_version called with bucket={}, key={}, version_id={}",
            bucket, key, version_id
        );
        let path = self.make_path(bucket, key);
        println!("Service: created path: {} for deletion", path);

        // Additional debug info about the versioned store
        println!(
            "Service: using VersionedStore to delete version for path={}, version_id={}",
            path, version_id
        );

        let result = self.store.delete_version(&path, version_id).await;
        match &result {
            Ok(_) => println!(
                "Service: Successfully deleted version for path={}, version_id={}",
                path, version_id
            ),
            Err(e) => {
                println!(
                    "Service: Error deleting version for path={}, version_id={}, error={}",
                    path, version_id, e
                );
                println!("Service: Error type: {:?}", e);
            }
        }
        result
    }

    /// List versions of an object
    pub async fn list_object_versions(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<ListVersionsResponse, StoreError> {
        let path = self.make_path(bucket, key);

        let versions = self.store.list_versions(&path)?;

        let version_infos = versions
            .into_iter()
            .map(|v| VersionInfo {
                version_id: v.version_id,
                created_at: v.created_at.to_rfc3339(),
                size: v.size,
                etag: v.etag,
            })
            .collect();

        Ok(ListVersionsResponse {
            key: key.to_string(),
            versions: version_infos,
        })
    }

    /// Get lifecycle configuration
    pub fn get_lifecycle_config(
        &self,
        bucket: &str,
    ) -> Result<Option<LifecycleConfiguration>, StoreError> {
        self.lifecycle_manager.get_lifecycle_config(bucket)
    }

    /// Set lifecycle configuration
    pub fn set_lifecycle_config(
        &self,
        bucket: &str,
        config: LifecycleConfiguration,
    ) -> Result<(), StoreError> {
        self.lifecycle_manager.set_lifecycle_config(bucket, config)
    }

    /// Get lifecycle configuration directly from MinIO
    pub async fn get_minio_lifecycle_config(
        &self,
        bucket: &str,
        endpoint: &str,
        access_key: &str,
        secret_key: &str,
    ) -> Result<LifecycleConfiguration, StoreError> {
        let client = MinioClient::new(endpoint, access_key, secret_key, "");
        let domain_config = client.get_lifecycle_config(bucket).await?;

        // Convert domain config to service config
        let service_config = lifecycle_adapter::domain_to_service(&domain_config);

        // Also update the local manager's cache
        self.lifecycle_manager
            .set_lifecycle_config(bucket, service_config.clone())?;

        Ok(service_config)
    }

    /// Set lifecycle configuration directly in MinIO
    pub async fn set_minio_lifecycle_config(
        &self,
        bucket: &str,
        config: &LifecycleConfiguration,
        endpoint: &str,
        access_key: &str,
        secret_key: &str,
    ) -> Result<(), StoreError> {
        let client = MinioClient::new(endpoint, access_key, secret_key, "");

        // Convert service config to domain config
        let domain_config = lifecycle_adapter::service_to_domain(config);

        // Set in MinIO
        client.set_lifecycle_config(bucket, &domain_config).await?;

        // Also update the local manager's cache
        self.lifecycle_manager
            .set_lifecycle_config(bucket, config.clone())?;

        Ok(())
    }

    /// Delete lifecycle configuration directly from MinIO
    pub async fn delete_minio_lifecycle_config(
        &self,
        bucket: &str,
        endpoint: &str,
        access_key: &str,
        secret_key: &str,
    ) -> Result<(), StoreError> {
        let client = MinioClient::new(endpoint, access_key, secret_key, "");
        client.delete_lifecycle_config(bucket).await
    }

    /// Utility to create a path from bucket and key
    fn make_path(&self, _bucket: &str, key: &str) -> ObjectPath {
        // Don't include bucket in path as it's already specified in the S3 client config
        ObjectPath::from(key)
    }
}

/// The Tower layer for object store middleware
#[derive(Clone)]
pub struct ObjectStoreLayer<T: ObjectStore + Send + Sync + 'static> {
    service: Arc<ObjectStoreService<T>>,
}

impl<T: ObjectStore + Send + Sync + 'static> ObjectStoreLayer<T> {
    pub fn new(service: Arc<ObjectStoreService<T>>) -> Self {
        ObjectStoreLayer { service }
    }
}

impl<S, T> Layer<S> for ObjectStoreLayer<T>
where
    S: Service<Request<Body>, Response = Response<Body>> + Send + Clone + 'static,
    S::Future: Send + 'static,
    T: ObjectStore + Send + Sync + 'static,
{
    type Service = ObjectStoreMiddleware<S, T>;

    fn layer(&self, inner: S) -> Self::Service {
        ObjectStoreMiddleware {
            inner,
            service: self.service.clone(),
        }
    }
}

/// Middleware for handling object storage operations
pub struct ObjectStoreMiddleware<S, T: ObjectStore + Send + Sync + 'static> {
    inner: S,
    service: Arc<ObjectStoreService<T>>,
}

impl<S, T> Service<Request<Body>> for ObjectStoreMiddleware<S, T>
where
    S: Service<Request<Body>, Response = Response<Body>> + Send + Clone + 'static,
    S::Future: Send + 'static,
    T: ObjectStore + Send + Sync + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        // Clone the inner service since we'll move it into the async block
        let mut inner = self.inner.clone();
        let service = self.service.clone();

        // Extract important information before moving req
        let path = req.uri().path().to_string();
        let method = req.method().clone();
        let headers = req.headers().clone();

        println!(
            "Middleware received request: method={}, path={}",
            method, path
        );

        Box::pin(async move {
            // If this is a request to the object store API, handle it
            if path.starts_with("/storage/") {
                // Determine what kind of request this is
                if path.contains("/versions") && method == http::Method::GET {
                    // This is a request to list versions
                    if let Some(parts) = path.strip_prefix("/storage/").and_then(|p| {
                        let parts: Vec<&str> = p.split('/').collect();
                        if parts.len() >= 3 && parts[1] == "versions" {
                            Some((parts[0], parts[2]))
                        } else {
                            None
                        }
                    }) {
                        let (bucket, key) = parts;
                        match service.list_object_versions(bucket, key).await {
                            Ok(versions) => {
                                return Ok(Json(versions).into_response());
                            }
                            Err(e) => {
                                let status: StatusCode = e.into();
                                return Ok(status.into_response());
                            }
                        }
                    }
                } else if path.contains("/version/") && method == http::Method::DELETE {
                    // Delete a specific version
                    println!(
                        "Middleware: Processing DELETE for path containing /version/: {}",
                        path
                    );

                    if let Some((bucket, key, version)) = path.strip_prefix("/storage/").and_then(|p| {
                        let parts: Vec<&str> = p.split('/').collect();
                        println!("Middleware: Extracted path parts: {:?}", parts);

                        if parts.len() >= 4 && parts[1] == "version" {
                            println!("Middleware: Found version deletion pattern with bucket={}, key={}, version={}", 
                                    parts[0], parts[2], parts[3]);
                            Some((parts[0], parts[2], parts[3]))
                        } else {
                            println!("Middleware: Path parts don't match expected pattern for version deletion");
                            None
                        }
                    }) {
                        println!("Middleware: Calling delete_object_version with bucket={}, key={}, version={}", bucket, key, version);
                        match service.delete_object_version(bucket, key, version).await {
                            Ok(_) => {
                                println!("Middleware: Successfully processed version deletion");
                                return Ok(StatusCode::NO_CONTENT.into_response());
                            }
                            Err(e) => {
                                println!("Middleware: Error in version deletion: {:?}", e);
                                let status: StatusCode = e.into();
                                return Ok(status.into_response());
                            }
                        }
                    } else {
                        println!("Middleware: Could not extract version deletion components from path: {}", path);
                    }
                } else if path.contains("/version/") && method == http::Method::GET {
                    // Get a specific version
                    if let Some((bucket, key, version)) =
                        path.strip_prefix("/storage/").and_then(|p| {
                            let parts: Vec<&str> = p.split('/').collect();
                            if parts.len() >= 4 && parts[1] == "version" {
                                Some((parts[0], parts[2], parts[3]))
                            } else {
                                None
                            }
                        })
                    {
                        match service.get_object_version(bucket, key, version).await {
                            Ok(data) => {
                                return Ok(Response::builder()
                                    .status(StatusCode::OK)
                                    .body(Body::from(data))
                                    .unwrap());
                            }
                            Err(e) => {
                                let status: StatusCode = e.into();
                                return Ok(status.into_response());
                            }
                        }
                    }
                } else if path.contains("/version/") && method == http::Method::DELETE {
                    // Delete a specific version
                    println!(
                        "Middleware: Processing DELETE for path containing /version/: {}",
                        path
                    );

                    if let Some((bucket, key, version)) =
                        path.strip_prefix("/storage/").and_then(|p| {
                            let parts: Vec<&str> = p.split('/').collect();
                            println!("Middleware: Extracted path parts: {:?}", parts);
                            if parts.len() >= 4 && parts[1] == "version" {
                                Some((parts[0], parts[2], parts[3]))
                            } else {
                                None
                            }
                        })
                    {
                        println!(
                            "Middleware: Will delete version: bucket={}, key={}, version={}",
                            bucket, key, version
                        );
                        match service.delete_object_version(bucket, key, version).await {
                            Ok(_) => {
                                println!("Middleware: Successfully deleted version");
                                return Ok(StatusCode::NO_CONTENT.into_response());
                            }
                            Err(e) => {
                                println!("Middleware: Failed to delete version: {}", e);
                                let status: StatusCode = e.into();
                                return Ok(status.into_response());
                            }
                        }
                    } else {
                        println!(
                            "Middleware: Failed to parse DELETE version request path: {}",
                            path
                        );
                    }
                } else if path.contains("/lifecycle") {
                    // Handle lifecycle configuration requests
                    if let Some(bucket) = path.strip_prefix("/storage/").and_then(|p| {
                        let parts: Vec<&str> = p.split('/').collect();
                        if parts.len() >= 2 && parts[1] == "lifecycle" {
                            Some(parts[0])
                        } else {
                            None
                        }
                    }) {
                        if method == http::Method::GET {
                            // Get lifecycle configuration
                            match service.get_lifecycle_config(bucket) {
                                Ok(Some(config)) => {
                                    return Ok(Json(config).into_response());
                                }
                                Ok(None) => {
                                    // Create an empty config instead of returning 404
                                    let empty_config = LifecycleConfiguration::new();
                                    return Ok(Json(empty_config).into_response());
                                }
                                Err(e) => {
                                    let status: StatusCode = e.into();
                                    return Ok(status.into_response());
                                }
                            }
                        } else if method == http::Method::PUT {
                            // Read the request body and convert it to a LifecycleConfiguration
                            let body_bytes = axum::body::to_bytes(req.into_body(), usize::MAX)
                                .await
                                .unwrap_or_default();

                            // Parse the JSON to a LifecycleConfiguration
                            match serde_json::from_slice::<LifecycleConfiguration>(&body_bytes) {
                                Ok(config) => {
                                    // Set the configuration
                                    match service.set_lifecycle_config(bucket, config) {
                                        Ok(_) => return Ok(StatusCode::OK.into_response()),
                                        Err(e) => {
                                            let status: StatusCode = e.into();
                                            return Ok(status.into_response());
                                        }
                                    }
                                }
                                Err(_) => return Ok(StatusCode::BAD_REQUEST.into_response()),
                            }
                        }
                    }
                } else if method == http::Method::GET {
                    // Regular GET request
                    if let Some((bucket, key)) = path.strip_prefix("/storage/").and_then(|p| {
                        let parts: Vec<&str> = p.split('/').collect();
                        if parts.len() >= 2 {
                            Some((parts[0], parts[1]))
                        } else {
                            None
                        }
                    }) {
                        match service.get_object(bucket, key).await {
                            Ok(data) => {
                                return Ok(Response::builder()
                                    .status(StatusCode::OK)
                                    .body(Body::from(data))
                                    .unwrap());
                            }
                            Err(e) => {
                                let status: StatusCode = e.into();
                                return Ok(status.into_response());
                            }
                        }
                    }
                } else if method == http::Method::PUT || method == http::Method::POST {
                    // Upload request
                    if let Some((bucket, key)) = path.strip_prefix("/storage/").and_then(|p| {
                        let parts: Vec<&str> = p.split('/').collect();
                        if parts.len() >= 2 {
                            Some((parts[0], parts[1]))
                        } else {
                            None
                        }
                    }) {
                        // Extract metadata from headers
                        let metadata = extract_metadata_from_headers(&headers);

                        // Read the request body (we can safely move the body now)
                        let body_bytes = axum::body::to_bytes(req.into_body(), usize::MAX)
                            .await
                            .unwrap_or_default();

                        match service.put_object(bucket, key, body_bytes, metadata).await {
                            Ok(response) => {
                                return Ok((StatusCode::CREATED, Json(response)).into_response());
                            }
                            Err(e) => {
                                let status: StatusCode = e.into();
                                return Ok(status.into_response());
                            }
                        }
                    }
                } else if method == http::Method::DELETE {
                    // Delete request
                    if let Some((bucket, key)) = path.strip_prefix("/storage/").and_then(|p| {
                        let parts: Vec<&str> = p.split('/').collect();
                        if parts.len() >= 2 {
                            Some((parts[0], parts[1]))
                        } else {
                            None
                        }
                    }) {
                        match service.delete_object(bucket, key).await {
                            Ok(_) => {
                                return Ok(StatusCode::NO_CONTENT.into_response());
                            }
                            Err(e) => {
                                let status: StatusCode = e.into();
                                return Ok(status.into_response());
                            }
                        }
                    }
                }
            }

            // If we get here, it wasn't handled by our middleware, so pass it on
            inner.call(req).await
        })
    }
}

impl<S, T> Clone for ObjectStoreMiddleware<S, T>
where
    S: Clone,
    T: ObjectStore + Send + Sync + 'static,
{
    fn clone(&self) -> Self {
        ObjectStoreMiddleware {
            inner: self.inner.clone(),
            service: self.service.clone(),
        }
    }
}

/// Extract metadata from HTTP headers
fn extract_metadata_from_headers(headers: &HeaderMap) -> Option<HashMap<String, String>> {
    let mut metadata = HashMap::new();

    // Process metadata headers (typically prefixed with x-amz-meta-)
    for (name, value) in headers.iter() {
        let name_str = name.as_str();
        if name_str.starts_with("x-amz-meta-") {
            if let Ok(value_str) = value.to_str() {
                let key = name_str.trim_start_matches("x-amz-meta-").to_string();
                metadata.insert(key, value_str.to_string());
            }
        }
    }

    // Include Content-Type and ETag if present
    if let Some(content_type) = headers.get(http::header::CONTENT_TYPE) {
        if let Ok(value) = content_type.to_str() {
            metadata.insert("content-type".to_string(), value.to_string());
        }
    }

    if let Some(etag) = headers.get(http::header::ETAG) {
        if let Ok(value) = etag.to_str() {
            metadata.insert("etag".to_string(), value.to_string());
        }
    }

    if metadata.is_empty() {
        None
    } else {
        Some(metadata)
    }
}

// Router builder

/// Create an Axum router with object store routes
pub fn create_object_store_router<T: ObjectStore + Send + Sync + 'static>(
    service: Arc<ObjectStoreService<T>>,
) -> Router {
    Router::new()
        // Object routes
        .route(
            "/storage/:bucket/:key",
            get(get_object).put(put_object).delete(delete_object),
        )
        // Version management
        .route("/storage/:bucket/versions/:key", get(list_object_versions))
        .route(
            "/storage/:bucket/version/:key/:version_id",
            get(get_object_version).delete(delete_object_version),
        )
        // Lifecycle management
        .route(
            "/storage/:bucket/lifecycle",
            get(get_lifecycle_config).put(set_lifecycle_config),
        )
        // MinIO-specific lifecycle management
        .route(
            "/storage/:bucket/minio-lifecycle",
            get(get_minio_lifecycle_config)
                .put(set_minio_lifecycle_config)
                .delete(delete_minio_lifecycle_config),
        )
        .with_state(service)
}

// Handler functions

async fn get_object(
    State(service): State<Arc<ObjectStoreService<impl ObjectStore + Send + Sync + 'static>>>,
    Path((bucket, key)): Path<(String, String)>,
) -> impl IntoResponse {
    match service.get_object(&bucket, &key).await {
        Ok(data) => Response::builder()
            .status(StatusCode::OK)
            .body(Body::from(data))
            .unwrap(),
        Err(e) => {
            let status: StatusCode = e.into();
            status.into_response()
        }
    }
}

async fn get_object_version(
    State(service): State<Arc<ObjectStoreService<impl ObjectStore + Send + Sync + 'static>>>,
    Path((bucket, key, version_id)): Path<(String, String, String)>,
) -> impl IntoResponse {
    match service.get_object_version(&bucket, &key, &version_id).await {
        Ok(data) => Response::builder()
            .status(StatusCode::OK)
            .body(Body::from(data))
            .unwrap(),
        Err(e) => {
            let status: StatusCode = e.into();
            status.into_response()
        }
    }
}

async fn put_object(
    State(service): State<Arc<ObjectStoreService<impl ObjectStore + Send + Sync + 'static>>>,
    Path((bucket, key)): Path<(String, String)>,
    body: Bytes,
) -> impl IntoResponse {
    match service.put_object(&bucket, &key, body, None).await {
        Ok(response) => (StatusCode::CREATED, Json(response)).into_response(),
        Err(e) => {
            let status: StatusCode = e.into();
            status.into_response()
        }
    }
}

async fn delete_object(
    State(service): State<Arc<ObjectStoreService<impl ObjectStore + Send + Sync + 'static>>>,
    Path((bucket, key)): Path<(String, String)>,
) -> impl IntoResponse {
    match service.delete_object(&bucket, &key).await {
        Ok(_) => StatusCode::NO_CONTENT,
        Err(e) => e.into(),
    }
}

async fn delete_object_version(
    State(service): State<Arc<ObjectStoreService<impl ObjectStore + Send + Sync + 'static>>>,
    Path((bucket, key, version_id)): Path<(String, String, String)>,
) -> impl IntoResponse {
    println!(
        "Router handler: delete_object_version called with bucket={}, key={}, version_id={}",
        bucket, key, version_id
    );
    println!(
        "Router handler: Path components extracted from router path: bucket={}, key={}, version_id={}",
        bucket, key, version_id
    );

    // Logging the constructed path for comparison
    let object_path = ObjectPath::from(key.as_str());
    println!("Router handler: Constructed object path: {}", object_path);
    // Don't try to print object_path directly as it returns an iterator that doesn't implement Debug
    println!(
        "Router handler: Object path string: {}",
        object_path.to_string()
    );

    match service
        .delete_object_version(&bucket, &key, &version_id)
        .await
    {
        Ok(_) => {
            println!(
                "Router handler: Successfully deleted version: bucket={}, key={}, version_id={}",
                bucket, key, version_id
            );
            StatusCode::NO_CONTENT
        }
        Err(e) => {
            println!(
                "Router handler: Error deleting version: bucket={}, key={}, version_id={}, error={}",
                bucket, key, version_id, e
            );
            println!("Router handler: Error details: {:?}", e);
            e.into()
        }
    }
}

async fn list_object_versions(
    State(service): State<Arc<ObjectStoreService<impl ObjectStore + Send + Sync + 'static>>>,
    Path((bucket, key)): Path<(String, String)>,
) -> impl IntoResponse {
    match service.list_object_versions(&bucket, &key).await {
        Ok(versions) => Json(versions).into_response(),
        Err(e) => {
            let status: StatusCode = e.into();
            status.into_response()
        }
    }
}

async fn get_lifecycle_config(
    State(service): State<Arc<ObjectStoreService<impl ObjectStore + Send + Sync + 'static>>>,
    Path(bucket): Path<String>,
) -> impl IntoResponse {
    match service.get_lifecycle_config(&bucket) {
        Ok(Some(config)) => Json(config).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            let status: StatusCode = e.into();
            status.into_response()
        }
    }
}

async fn set_lifecycle_config(
    State(service): State<Arc<ObjectStoreService<impl ObjectStore + Send + Sync + 'static>>>,
    Path(bucket): Path<String>,
    Json(config): Json<LifecycleConfiguration>,
) -> impl IntoResponse {
    match service.set_lifecycle_config(&bucket, config) {
        Ok(_) => StatusCode::OK,
        Err(e) => e.into(),
    }
}

// MinIO-specific handlers

#[derive(Deserialize)]
struct MinioCredentials {
    endpoint: String,
    access_key: String,
    secret_key: String,
}

async fn get_minio_lifecycle_config(
    State(service): State<Arc<ObjectStoreService<impl ObjectStore + Send + Sync + 'static>>>,
    Path(bucket): Path<String>,
    Query(creds): Query<MinioCredentials>,
) -> impl IntoResponse {
    match service
        .get_minio_lifecycle_config(
            &bucket,
            &creds.endpoint,
            &creds.access_key,
            &creds.secret_key,
        )
        .await
    {
        Ok(config) => Json(config).into_response(),
        Err(e) => {
            let status: StatusCode = e.into();
            status.into_response()
        }
    }
}

async fn set_minio_lifecycle_config(
    State(service): State<Arc<ObjectStoreService<impl ObjectStore + Send + Sync + 'static>>>,
    Path(bucket): Path<String>,
    Query(creds): Query<MinioCredentials>,
    Json(config): Json<LifecycleConfiguration>,
) -> impl IntoResponse {
    match service
        .set_minio_lifecycle_config(
            &bucket,
            &config,
            &creds.endpoint,
            &creds.access_key,
            &creds.secret_key,
        )
        .await
    {
        Ok(_) => StatusCode::OK,
        Err(e) => e.into(),
    }
}

async fn delete_minio_lifecycle_config(
    State(service): State<Arc<ObjectStoreService<impl ObjectStore + Send + Sync + 'static>>>,
    Path(bucket): Path<String>,
    Query(creds): Query<MinioCredentials>,
) -> impl IntoResponse {
    match service
        .delete_minio_lifecycle_config(
            &bucket,
            &creds.endpoint,
            &creds.access_key,
            &creds.secret_key,
        )
        .await
    {
        Ok(_) => StatusCode::NO_CONTENT,
        Err(e) => e.into(),
    }
}
