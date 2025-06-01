# Object Store Bridge

A Rust library that extends Apache Arrow's `object_store` crate with advanced features including:

1. **Versioning**: Automatic tracking and management of object versions
2. **Lifecycle Management**: Expiration and transition rules for objects
3. **Axum Web Framework Integration**: Tower middleware for HTTP API access

The library serves as a bridge between object storage systems (like S3, Azure Blob Storage, etc.) and web applications built with Axum, providing a RESTful API for object operations.

## Features

- Object CRUD operations with automatic versioning
- Version management (retrieve, list, delete specific versions)
- Lifecycle configuration (expiration and transition rules)
- Integration with Axum via Tower middleware
- Multipart upload support

## Usage

```rust
use axum::Router;
use object_store::aws::AmazonS3Builder;
use object_store_bridge::{
    lifecycle::{LifecycleConfiguration, LifecycleRule, RuleStatus, ExpirationConfig},
    middleware::{ObjectStoreLayer, ObjectStoreService, create_object_store_router},
};
use std::sync::Arc;

// Create an S3 compatible store
let store = AmazonS3Builder::new()
    .with_bucket_name("my-bucket")
    .with_access_key_id(std::env::var("AWS_ACCESS_KEY_ID").unwrap())
    .with_secret_access_key(std::env::var("AWS_SECRET_ACCESS_KEY").unwrap())
    .with_region("us-east-1")
    .build()
    .expect("Failed to create S3 store");

// Create the object store service
let service = Arc::new(ObjectStoreService::new(store));

// Set up a lifecycle configuration
let lifecycle_config = LifecycleConfiguration {
    rules: vec![LifecycleRule {
        id: "expire-temp".to_string(),
        prefix: "temp/".to_string(),
        status: RuleStatus::Enabled,
        expiration: Some(ExpirationConfig {
            days: Some(7),
            date: None,
            expired_object_delete_marker: None,
        }),
        transitions: None,
        filter_tags: None,
    }],
};

// Set the configuration
service.set_lifecycle_config("my-bucket", lifecycle_config).unwrap();

// Start the lifecycle manager
service.start_lifecycle_manager().await.unwrap();

// Create the API router
let app = Router::new()
    // Mount object storage routes
    .merge(create_object_store_router(service.clone()))
    // Add middleware
    .layer(ObjectStoreLayer::new(service));
```

## Testing with MinIO

The library includes integration tests that can be run against a MinIO instance:

1. Start a MinIO server:

```bash
docker run -p 9000:9000 -p 9001:9001 \
  --name minio \
  -e "MINIO_ROOT_USER=minioadmin" \
  -e "MINIO_ROOT_PASSWORD=minioadmin" \
  quay.io/minio/minio server /data --console-address ":9001"
```

2. Create a `.env` file with the following configuration (copy from `.env.example`):

```
MINIO_ENDPOINT=http://localhost:9000
MINIO_ACCESS_KEY_ID=minioadmin
MINIO_SECRET_ACCESS_KEY=minioadmin
MINIO_REGION=us-east-1
```

3. Run the integration tests:

```bash
cargo test --test minio_tests -- --ignored
```

## Project Status

The core library successfully compiles with the latest dependencies:
- object_store 0.12 with aws feature
- axum 0.7 with multipart feature
- Tower and hyper ecosystem

## Important Implementation Notes

### Path Handling

The library separates bucket and object key handling:

1. **API Routes**: Use paths in the format `/storage/{bucket}/{key}` for routing
2. **Internal Paths**: When constructing object paths, we **do not** include the bucket name as it's already configured in the object store client
   
This ensures objects are created at the correct path within the storage bucket rather than in nested directories.

## Building and Running

```bash
# Build the library
cargo build

# Run tests
cargo test

# Run the example
cargo run --example simple
```

## Documentation

Generate and view the documentation:

```bash
cargo doc --open
```

## License

MIT