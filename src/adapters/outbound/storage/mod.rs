// Infrastructure error types
pub mod error;

// Storage implementations
pub mod bucket;
pub mod lifecycle;
pub mod lifecycle_adapter;
pub mod versioning;

// Provider-specific implementations
pub mod minio;
pub mod s3;

// Re-export key types
pub use s3::{S3ObjectStoreAdapter, VersionedS3ObjectStoreAdapter, S3Config, create_s3_store};
pub use error::StoreError;
pub use versioning::VersionedStore;
