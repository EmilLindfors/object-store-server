// Infrastructure error types
pub mod error;

// Storage implementations
pub mod apache_object_store_adapter;
pub mod bucket;
pub mod lifecycle;
pub mod lifecycle_adapter;
pub mod versioning;

// Provider-specific implementations
pub mod minio;

// Re-export key types
pub use apache_object_store_adapter::{
    ApacheObjectStoreAdapter, VersionedApacheObjectStoreAdapter,
};
pub use error::StoreError;
pub use versioning::VersionedStore;
