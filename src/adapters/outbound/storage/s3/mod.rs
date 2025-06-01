//! S3 storage adapter implementation using the object_store crate
//! 
//! This module provides S3-compatible storage adapters that implement
//! the ObjectStore and VersionedObjectStore traits.

pub mod s3_adapter;
pub mod versioned_s3_adapter;

pub use s3_adapter::S3ObjectStoreAdapter;
pub use versioned_s3_adapter::VersionedS3ObjectStoreAdapter;

use object_store::{aws::AmazonS3Builder, ObjectStore as ObjectStoreBackend};
use std::sync::Arc;
use anyhow::{Context, Result};

/// Configuration for S3 storage backend
#[derive(Debug, Clone)]
pub struct S3Config {
    pub bucket: String,
    pub region: String,
    pub access_key: Option<String>,
    pub secret_key: Option<String>,
    pub endpoint: Option<String>,
}

/// Create an S3 store from configuration
pub fn create_s3_store(config: S3Config) -> Result<Arc<dyn ObjectStoreBackend>> {
    let mut builder = AmazonS3Builder::new()
        .with_bucket_name(&config.bucket)
        .with_region(&config.region);

    if let Some(access_key) = &config.access_key {
        builder = builder.with_access_key_id(access_key);
    }

    if let Some(secret_key) = &config.secret_key {
        builder = builder.with_secret_access_key(secret_key);
    }

    if let Some(endpoint) = &config.endpoint {
        builder = builder.with_endpoint(endpoint);
    }

    let store = builder.build()
        .context("Failed to build S3 store")?;

    Ok(Arc::new(store))
}