# Bridging Apache object_store with Axum: The complete Tower integration guide

Building a seamless bridge between Apache Arrow's object_store and Tokio Axum requires careful architecture decisions, especially when advanced features like bucket versioning and lifecycle management are needed. This comprehensive guide shows you how to create this integration using Tower middleware while extending object_store's capabilities.

## The landscape: Understanding the components

Apache Arrow's object_store crate (version 0.11.x, with 0.12.x expected April 2025) provides a uniform API for interacting with various object storage systems including Amazon S3, Google Cloud Storage, and Azure Blob Storage. While powerful, object_store doesn't natively implement bucket lifecycle management or full bucket versioning comparable to what cloud providers offer.

The object_store crate focuses on core primitive operations like get, put, and delete operations with some versioning support through its optimistic concurrency control, but lacks automatic version retention or lifecycle policies. We'll need to extend these capabilities.

Axum, meanwhile, is a modern web framework built on Tokio, Tower, and Hyper. Rather than implementing its own middleware system, Axum leverages the Tower ecosystem through the `Service` trait – this is precisely what makes it an excellent candidate for our integration.

## Creating the bridge: Service-based integration

The most elegant approach to bridging object_store with Axum is creating a service-based integration using Tower. Here's a step-by-step implementation:

```rust
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use object_store::{aws::AmazonS3Builder, path::Path as ObjectPath, ObjectStore};
use std::sync::Arc;
use tower::{Layer, Service};

// Our storage service
#[derive(Clone)]
struct ObjectStorageService {
    store: Arc<dyn ObjectStore>,
}

impl ObjectStorageService {
    pub fn new() -> Self {
        // Create S3-compatible store
        let store = AmazonS3Builder::new()
            .with_bucket_name("my-bucket")
            .with_region("us-east-1")
            .build()
            .expect("Failed to create object store");

        ObjectStorageService {
            store: Arc::new(store),
        }
    }

    // Core operations
    pub async fn get_object(&self, key: &str) -> Result<Vec<u8>, StatusCode> {
        let path = ObjectPath::from(key);
        self.store
            .get(&path)
            .await
            .map(|r| r.bytes().to_vec())
            .map_err(|_| StatusCode::NOT_FOUND)
    }

    pub async fn put_object(&self, key: &str, data: Vec<u8>) -> Result<(), StatusCode> {
        let path = ObjectPath::from(key);
        self.store
            .put(&path, data.into())
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
    }
}

// Application setup
#[tokio::main]
async fn main() {
    let storage_service = ObjectStorageService::new();
    
    let app = Router::new()
        .route("/objects/:key", get(get_object).post(put_object))
        .with_state(storage_service);

    axum::serve(
        tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap(),
        app,
    )
    .await
    .unwrap();
}

// Handler functions
async fn get_object(
    State(storage): State<ObjectStorageService>,
    Path(key): Path<String>,
) -> impl IntoResponse {
    match storage.get_object(&key).await {
        Ok(data) => Response::builder()
            .status(StatusCode::OK)
            .body(axum::body::Body::from(data))
            .unwrap(),
        Err(status) => Response::builder().status(status).body(axum::body::Body::empty()).unwrap(),
    }
}

async fn put_object(
    State(storage): State<ObjectStorageService>,
    Path(key): Path<String>,
    body: axum::extract::Json<Vec<u8>>,
) -> StatusCode {
    match storage.put_object(&key, body.0).await {
        Ok(_) => StatusCode::CREATED,
        Err(status) => status,
    }
}
```

## Adding automatic versioning with Tower middleware

While object_store provides basic versioning through its optimistic concurrency controls, we can implement **automatic versioning** with a Tower middleware layer. This middleware intercepts PUT requests to automatically version objects:

```rust
use std::task::{Context, Poll};
use tower::{Layer, Service};
use futures::future::BoxFuture;
use std::pin::Pin;

// Versioning middleware
struct VersioningLayer;

impl<S> Layer<S> for VersioningLayer {
    type Service = VersioningMiddleware<S>;

    fn layer(&self, service: S) -> Self::Service {
        VersioningMiddleware { inner: service }
    }
}

struct VersioningMiddleware<S> {
    inner: S,
}

impl<S, B> Service<axum::http::Request<B>> for VersioningMiddleware<S>
where
    S: Service<axum::http::Request<B>, Response = axum::http::Response<axum::body::Body>> + Send + 'static + Clone,
    S::Future: Send + 'static,
    B: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: axum::http::Request<B>) -> Self::Future {
        // Clone service since we'll move it into the async block
        let mut inner = self.inner.clone();
        
        Box::pin(async move {
            // Only handle PUT requests
            if req.method() == axum::http::Method::PUT {
                // Extract key from path for versioning
                if let Some(path) = req.uri().path().strip_prefix("/objects/") {
                    // In a real implementation, we'd:
                    // 1. Generate a version ID
                    let version_id = uuid::Uuid::new_v4().to_string();
                    
                    // 2. Add it to request extensions so handlers can access it
                    req.extensions_mut().insert(version_id.clone());
                    
                    // 3. Log versioning for clarity in this example
                    println!("Versioning object: {} with version: {}", path, version_id);
                }
            }
            
            // Continue with the request
            inner.call(req).await
        })
    }
}

// Use in app setup
let app = Router::new()
    .route("/objects/:key", get(get_object).post(put_object))
    .layer(VersioningLayer)
    .with_state(storage_service);
```

## Implementing bucket lifecycle configuration

To implement bucket lifecycle configuration, we'll create an extension to the ObjectStorageService and use Tower HTTP for the API:

```rust
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// Lifecycle configuration types
#[derive(Clone, Debug, Deserialize, Serialize)]
struct LifecycleRule {
    id: String,
    prefix: String,
    status: String, // "Enabled" or "Disabled"
    expiration_days: Option<u32>,
    transition_days: Option<u32>,
    transition_storage_class: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct LifecycleConfiguration {
    rules: Vec<LifecycleRule>,
}

// Extend our storage service
impl ObjectStorageService {
    // Storage for lifecycle configurations - would be persistent in production
    pub fn with_lifecycle_support() -> Self {
        let service = Self::new();
        service
    }
    
    // Get lifecycle configuration
    pub async fn get_lifecycle_configuration(&self, bucket: &str) -> Result<LifecycleConfiguration, StatusCode> {
        // In a real implementation, retrieve from persistent storage
        // This is a simplified example
        Ok(LifecycleConfiguration {
            rules: vec![
                LifecycleRule {
                    id: "rule1".to_string(),
                    prefix: "docs/".to_string(),
                    status: "Enabled".to_string(),
                    expiration_days: Some(365),
                    transition_days: Some(30),
                    transition_storage_class: Some("GLACIER".to_string()),
                }
            ]
        })
    }
    
    // Set lifecycle configuration
    pub async fn set_lifecycle_configuration(
        &self,
        bucket: &str,
        config: LifecycleConfiguration,
    ) -> Result<(), StatusCode> {
        // In a real implementation, save to persistent storage
        // This is a simplified example
        println!("Setting lifecycle configuration for bucket: {}", bucket);
        for rule in &config.rules {
            println!("Rule ID: {}, Prefix: {}, Status: {}", rule.id, rule.prefix, rule.status);
        }
        Ok(())
    }
}

// Add API routes
async fn get_lifecycle(
    State(storage): State<ObjectStorageService>,
    Path(bucket): Path<String>,
) -> impl IntoResponse {
    match storage.get_lifecycle_configuration(&bucket).await {
        Ok(config) => axum::Json(config).into_response(),
        Err(status) => status.into_response(),
    }
}

async fn set_lifecycle(
    State(storage): State<ObjectStorageService>,
    Path(bucket): Path<String>,
    axum::Json(config): axum::Json<LifecycleConfiguration>,
) -> StatusCode {
    match storage.set_lifecycle_configuration(&bucket, config).await {
        Ok(_) => StatusCode::OK,
        Err(status) => status,
    }
}

// Update router
let app = Router::new()
    .route("/objects/:key", get(get_object).post(put_object))
    .route("/buckets/:bucket/lifecycle", 
           get(get_lifecycle).put(set_lifecycle))
    .layer(VersioningLayer)
    .with_state(storage_service);
```

## Advanced: Using a lifecycle worker with Tower HTTP

To actually implement lifecycle policies, we need a background worker. We can integrate this with our Tower HTTP setup:

```rust
use tokio::time::{sleep, Duration};
use std::time::SystemTime;

struct LifecycleWorker {
    storage: ObjectStorageService,
    running: Arc<RwLock<bool>>,
}

impl LifecycleWorker {
    fn new(storage: ObjectStorageService) -> Self {
        LifecycleWorker {
            storage,
            running: Arc::new(RwLock::new(true)),
        }
    }
    
    async fn start(&self) {
        let storage = self.storage.clone();
        let running = self.running.clone();
        
        tokio::spawn(async move {
            // Check every day
            let check_interval = Duration::from_secs(86400);
            
            while *running.read().unwrap() {
                // Apply lifecycle rules
                if let Ok(config) = storage.get_lifecycle_configuration("my-bucket").await {
                    Self::apply_lifecycle_rules(&storage, &config).await;
                }
                
                sleep(check_interval).await;
            }
        });
    }
    
    async fn stop(&self) {
        let mut running = self.running.write().unwrap();
        *running = false;
    }
    
    async fn apply_lifecycle_rules(storage: &ObjectStorageService, config: &LifecycleConfiguration) {
        // For each rule, list matching objects and apply the rule
        for rule in &config.rules {
            if rule.status != "Enabled" {
                continue;
            }
            
            // This would use object_store's list API to find objects matching the prefix
            // And then apply the appropriate lifecycle actions
            println!("Applying lifecycle rule: {}", rule.id);
        }
    }
}

// Start the worker when the application starts
let worker = LifecycleWorker::new(storage_service.clone());
worker.start().await;
```

## Streaming uploads with Tower and Axum

For larger files, we need streaming support. Here's how to implement multipart uploads with Tower middleware:

```rust
use axum::extract::Multipart;
use bytes::Bytes;
use futures::{Stream, TryStreamExt};
use std::io;
use tokio_util::io::StreamReader;

async fn upload_multipart(
    State(storage): State<ObjectStorageService>,
    Path(key): Path<String>,
    mut multipart: Multipart,
) -> StatusCode {
    while let Ok(Some(field)) = multipart.next_field().await {
        if field.name() == Some("file") {
            // Convert the field into a byte stream
            let stream = field
                .map_err(|err| io::Error::new(io::ErrorKind::Other, err));
            
            // Perform multipart upload to S3
            let path = ObjectPath::from(key);
            
            // Using object_store's multipart API
            match storage.store.put_multipart(&path).await {
                Ok(mut writer) => {
                    // Stream data to the multipart writer
                    let mut stream_reader = StreamReader::new(stream);
                    if let Err(_) = tokio::io::copy(&mut stream_reader, &mut writer).await {
                        return StatusCode::INTERNAL_SERVER_ERROR;
                    }
                    
                    // Complete upload
                    if let Err(_) = writer.shutdown().await {
                        return StatusCode::INTERNAL_SERVER_ERROR;
                    }
                    
                    return StatusCode::CREATED;
                }
                Err(_) => return StatusCode::INTERNAL_SERVER_ERROR,
            }
        }
    }
    
    StatusCode::BAD_REQUEST
}
```

## Best practices for Rust-based integration

When building an Apache object_store to Axum bridge with Tower, follow these key practices:

1. **Use appropriate abstraction levels**: Tower offers multiple approaches to middleware - `middleware::from_fn` for simpler cases, full `Service` trait implementation for complex reusable libraries.

2. **Stream data instead of buffering**: With large files, always use streaming APIs:
   ```rust
   // For uploads
   let stream = axum::body::Body::wrap_stream(request_body)
       .map_ok(|chunk| chunk.to_vec())
       .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err));
   
   // For downloads
   let stream = storage_client.get_object_stream(key).await?;
   StreamBody::new(stream)
   ```

3. **Implement proper rate limiting and timeouts**:
   ```rust
   let app = Router::new()
       .route("/upload", post(handle_upload))
       .layer(
           ServiceBuilder::new()
               .layer(TimeoutLayer::new(Duration::from_secs(30)))
               .layer(RateLimitLayer::new(5, Duration::from_secs(1)))
       );
   ```

4. **Properly handle connection pooling**:
   ```rust
   let http_config = aws_smithy_runtime::client::http::hyper_ext::HttpConnectorBuilder::new()
       .pool_max_idle_per_host(16)
       .pool_idle_timeout(Duration::from_secs(30))
       .build();
   ```

5. **Implement graceful error handling** with custom error types that provide meaningful feedback.

6. **Use request extensions** to pass data between middleware and handlers.

## Building a complete object_store extension

Putting everything together, here's how we can create a comprehensive extension to object_store with full versioning and lifecycle support:

```rust
use object_store::{ObjectStore, path::Path};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use chrono::{DateTime, Utc};
use uuid::Uuid;

// First, create a versioned wrapper around object_store
struct VersionedObjectStore<T: ObjectStore> {
    inner: T,
    versions: Arc<RwLock<HashMap<String, Vec<String>>>>,
}

impl<T: ObjectStore> VersionedObjectStore<T> {
    pub fn new(store: T) -> Self {
        VersionedObjectStore {
            inner: store,
            versions: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    pub async fn put_versioned(&self, path: &Path, bytes: bytes::Bytes) -> object_store::Result<()> {
        // Generate version ID
        let version_id = Uuid::new_v4().to_string();
        
        // Create versioned path
        let versioned_path = Path::from(format!("{}.v_{}", path.as_ref(), version_id));
        
        // Store the versioned object
        self.inner.put(&versioned_path, bytes).await?;
        
        // Update version tracking
        let mut versions = self.versions.write().unwrap();
        let path_str = path.as_ref().to_string();
        let version_list = versions.entry(path_str).or_insert_with(Vec::new);
        version_list.push(version_id);
        
        Ok(())
    }
    
    pub async fn get_latest(&self, path: &Path) -> object_store::Result<bytes::Bytes> {
        let versions = self.versions.read().unwrap();
        let path_str = path.as_ref().to_string();
        
        if let Some(version_list) = versions.get(&path_str) {
            if let Some(latest_version) = version_list.last() {
                let versioned_path = Path::from(format!("{}.v_{}", path.as_ref(), latest_version));
                let result = self.inner.get(&versioned_path).await?;
                return Ok(result.bytes().to_vec().into());
            }
        }
        
        // Fall back to original path if no versions
        let result = self.inner.get(path).await?;
        Ok(result.bytes().to_vec().into())
    }
    
    pub async fn get_version(&self, path: &Path, version_id: &str) -> object_store::Result<bytes::Bytes> {
        let versioned_path = Path::from(format!("{}.v_{}", path.as_ref(), version_id));
        let result = self.inner.get(&versioned_path).await?;
        Ok(result.bytes().to_vec().into())
    }
    
    pub async fn list_versions(&self, path: &Path) -> Vec<String> {
        let versions = self.versions.read().unwrap();
        let path_str = path.as_ref().to_string();
        
        if let Some(version_list) = versions.get(&path_str) {
            return version_list.clone();
        }
        
        Vec::new()
    }
}

// Now add lifecycle management
struct LifecycleManager<T: ObjectStore> {
    store: VersionedObjectStore<T>,
    config: Arc<RwLock<LifecycleConfiguration>>,
}

impl<T: ObjectStore> LifecycleManager<T> {
    pub fn new(store: VersionedObjectStore<T>) -> Self {
        LifecycleManager {
            store,
            config: Arc::new(RwLock::new(LifecycleConfiguration::default())),
        }
    }
    
    pub fn get_lifecycle_config(&self) -> LifecycleConfiguration {
        self.config.read().unwrap().clone()
    }
    
    pub fn set_lifecycle_config(&self, config: LifecycleConfiguration) {
        let mut cfg = self.config.write().unwrap();
        *cfg = config;
    }
    
    pub async fn apply_lifecycle_rules(&self) {
        let config = self.config.read().unwrap().clone();
        
        for rule in &config.rules {
            if rule.status != "Enabled" {
                continue;
            }
            
            // Apply expiration and transition rules
            // This would be implemented using the underlying store's list and delete operations
        }
    }
}
```

## Conclusion: Creating a production-ready integration

Building a bridge between Apache object_store and Tokio Axum requires extending object_store's capabilities to support bucket lifecycles and versioning. While object_store lacks these features natively, we can implement them through Tower middleware and custom extension layers.

The approach outlined here provides a robust foundation for integrating object storage with Axum servers, with automatic versioning, lifecycle management, and efficient streaming operations. By following these patterns and best practices, you can create a high-performance object storage API that meets complex requirements while maintaining the ergonomic benefits of both the Axum framework and the object_store crate.

Remember that in a production environment, you'd need to ensure persistence of versioning and lifecycle data, implement proper error handling for all edge cases, and add authentication and authorization layers to secure your object storage API.