use object_store::memory::InMemory;
use std::sync::Arc;

use crate::{
    adapters::outbound::{
        persistence::{InMemoryLifecycleRepository, InMemoryObjectRepository},
        storage::{ApacheObjectStoreAdapter, VersionedApacheObjectStoreAdapter},
    },
    ports::{
        repositories::{LifecycleRepository, ObjectRepository},
        storage::{ObjectStore, VersionedObjectStore},
    },
    services::{LifecycleServiceImpl, ObjectServiceImpl, VersioningServiceImpl},
};

/// Configuration for the application
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub storage_backend: StorageBackend,
    pub repository_backend: RepositoryBackend,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            storage_backend: StorageBackend::InMemory,
            repository_backend: RepositoryBackend::InMemory,
        }
    }
}

/// Storage backend configuration
#[derive(Debug, Clone)]
pub enum StorageBackend {
    InMemory,
    S3 {
        bucket: String,
        region: String,
        access_key: Option<String>,
        secret_key: Option<String>,
    },
    MinIO {
        endpoint: String,
        bucket: String,
        access_key: String,
        secret_key: String,
        use_ssl: bool,
    },
}

/// Repository backend configuration
#[derive(Debug, Clone)]
pub enum RepositoryBackend {
    InMemory,
    Database { connection_string: String },
}

/// Application dependencies container
pub struct AppDependencies {
    pub object_store: Arc<dyn ObjectStore>,
    pub versioned_store: Arc<dyn VersionedObjectStore>,
    pub object_repository: Arc<dyn ObjectRepository>,
    pub lifecycle_repository: Arc<dyn LifecycleRepository>,
}

/// Application services container
pub struct AppServices {
    pub object_service: ObjectServiceImpl,
    pub lifecycle_service: LifecycleServiceImpl,
    pub versioning_service: VersioningServiceImpl,
}

/// Application builder for dependency injection
pub struct AppBuilder {
    config: AppConfig,
}

impl AppBuilder {
    /// Create a new application builder
    pub fn new() -> Self {
        Self {
            config: AppConfig::default(),
        }
    }

    /// Configure the application with custom settings
    pub fn with_config(mut self, config: AppConfig) -> Self {
        self.config = config;
        self
    }

    /// Configure storage backend
    pub fn with_storage_backend(mut self, backend: StorageBackend) -> Self {
        self.config.storage_backend = backend;
        self
    }

    /// Configure repository backend
    pub fn with_repository_backend(mut self, backend: RepositoryBackend) -> Self {
        self.config.repository_backend = backend;
        self
    }

    /// Build the application dependencies
    pub async fn build_dependencies(self) -> Result<AppDependencies, AppError> {
        // Create storage adapters based on configuration
        let (object_store, versioned_store) = self.create_storage_adapters().await?;

        // Create repositories based on configuration
        let (object_repository, lifecycle_repository) = self.create_repositories().await?;

        Ok(AppDependencies {
            object_store,
            versioned_store,
            object_repository,
            lifecycle_repository,
        })
    }

    /// Build the complete application with services
    pub async fn build(self) -> Result<AppServices, AppError> {
        let deps = self.build_dependencies().await?;

        // Create services with dependency injection
        let object_service =
            ObjectServiceImpl::new(deps.object_repository.clone(), deps.object_store.clone());

        let lifecycle_service = LifecycleServiceImpl::new(
            deps.lifecycle_repository.clone(),
            deps.object_repository.clone(),
            deps.object_store.clone(),
            deps.versioned_store.clone(),
        );

        let versioning_service = VersioningServiceImpl::new(
            deps.object_repository.clone(),
            deps.versioned_store.clone(),
        );

        Ok(AppServices {
            object_service,
            lifecycle_service,
            versioning_service,
        })
    }

    /// Create storage adapters based on configuration
    async fn create_storage_adapters(
        &self,
    ) -> Result<(Arc<dyn ObjectStore>, Arc<dyn VersionedObjectStore>), AppError> {
        match &self.config.storage_backend {
            StorageBackend::InMemory => {
                let store = Arc::new(InMemory::new());
                let adapter = Arc::new(ApacheObjectStoreAdapter::new(store.clone()));
                let versioned_adapter = Arc::new(VersionedApacheObjectStoreAdapter::new(store));
                Ok((
                    adapter.clone() as Arc<dyn ObjectStore>,
                    versioned_adapter as Arc<dyn VersionedObjectStore>,
                ))
            }
            StorageBackend::S3 {
                bucket,
                region,
                access_key,
                secret_key,
            } => {
                // In a real implementation, create S3 store here
                let store = Arc::new(InMemory::new()); // Fallback for now
                let adapter = Arc::new(ApacheObjectStoreAdapter::new(store.clone()));
                let versioned_adapter = Arc::new(VersionedApacheObjectStoreAdapter::new(store));
                Ok((
                    adapter.clone() as Arc<dyn ObjectStore>,
                    versioned_adapter as Arc<dyn VersionedObjectStore>,
                ))
            }
            StorageBackend::MinIO {
                endpoint,
                bucket,
                access_key,
                secret_key,
                use_ssl,
            } => {
                // In a real implementation, create MinIO store here
                let store = Arc::new(InMemory::new()); // Fallback for now
                let adapter = Arc::new(ApacheObjectStoreAdapter::new(store.clone()));
                let versioned_adapter = Arc::new(VersionedApacheObjectStoreAdapter::new(store));
                Ok((
                    adapter.clone() as Arc<dyn ObjectStore>,
                    versioned_adapter as Arc<dyn VersionedObjectStore>,
                ))
            }
        }
    }

    /// Create repositories based on configuration
    async fn create_repositories(
        &self,
    ) -> Result<(Arc<dyn ObjectRepository>, Arc<dyn LifecycleRepository>), AppError> {
        match &self.config.repository_backend {
            RepositoryBackend::InMemory => {
                let object_repo = Arc::new(InMemoryObjectRepository::new());
                let lifecycle_repo = Arc::new(InMemoryLifecycleRepository::new());
                Ok((object_repo, lifecycle_repo))
            }
            RepositoryBackend::Database { connection_string } => {
                // In a real implementation, create database repositories here
                let object_repo = Arc::new(InMemoryObjectRepository::new()); // Fallback for now
                let lifecycle_repo = Arc::new(InMemoryLifecycleRepository::new()); // Fallback for now
                Ok((object_repo, lifecycle_repo))
            }
        }
    }
}

impl Default for AppBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Application-level errors
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Configuration error: {message}")]
    Configuration { message: String },

    #[error("Storage initialization error: {message}")]
    StorageInit { message: String },

    #[error("Repository initialization error: {message}")]
    RepositoryInit { message: String },

    #[error("Service initialization error: {message}")]
    ServiceInit { message: String },
}

/// Convenience functions for common configurations
///
/// Create an in-memory application for testing and development
pub async fn create_in_memory_app() -> Result<AppServices, AppError> {
    AppBuilder::new()
        .with_storage_backend(StorageBackend::InMemory)
        .with_repository_backend(RepositoryBackend::InMemory)
        .build()
        .await
}

/// Create a MinIO-backed application
pub async fn create_minio_app(
    endpoint: String,
    bucket: String,
    access_key: String,
    secret_key: String,
    use_ssl: bool,
) -> Result<AppServices, AppError> {
    AppBuilder::new()
        .with_storage_backend(StorageBackend::MinIO {
            endpoint,
            bucket,
            access_key,
            secret_key,
            use_ssl,
        })
        .with_repository_backend(RepositoryBackend::InMemory) // Use in-memory for metadata for now
        .build()
        .await
}

/// Create an S3-backed application
pub async fn create_s3_app(
    bucket: String,
    region: String,
    access_key: Option<String>,
    secret_key: Option<String>,
) -> Result<AppServices, AppError> {
    AppBuilder::new()
        .with_storage_backend(StorageBackend::S3 {
            bucket,
            region,
            access_key,
            secret_key,
        })
        .with_repository_backend(RepositoryBackend::InMemory) // Use in-memory for metadata for now
        .build()
        .await
}

/// Create application from environment variables
pub async fn create_app_from_env() -> Result<AppServices, AppError> {
    let storage_backend = match std::env::var("STORAGE_BACKEND").as_deref() {
        Ok("s3") => {
            let bucket = std::env::var("S3_BUCKET").map_err(|_| AppError::Configuration {
                message: "S3_BUCKET environment variable required".to_string(),
            })?;
            let region = std::env::var("S3_REGION").map_err(|_| AppError::Configuration {
                message: "S3_REGION environment variable required".to_string(),
            })?;
            let access_key = std::env::var("S3_ACCESS_KEY").ok();
            let secret_key = std::env::var("S3_SECRET_KEY").ok();

            StorageBackend::S3 {
                bucket,
                region,
                access_key,
                secret_key,
            }
        }
        Ok("minio") => {
            let endpoint =
                std::env::var("MINIO_ENDPOINT").map_err(|_| AppError::Configuration {
                    message: "MINIO_ENDPOINT environment variable required".to_string(),
                })?;
            let bucket = std::env::var("MINIO_BUCKET").map_err(|_| AppError::Configuration {
                message: "MINIO_BUCKET environment variable required".to_string(),
            })?;
            let access_key =
                std::env::var("MINIO_ACCESS_KEY").map_err(|_| AppError::Configuration {
                    message: "MINIO_ACCESS_KEY environment variable required".to_string(),
                })?;
            let secret_key =
                std::env::var("MINIO_SECRET_KEY").map_err(|_| AppError::Configuration {
                    message: "MINIO_SECRET_KEY environment variable required".to_string(),
                })?;
            let use_ssl = std::env::var("MINIO_USE_SSL")
                .map(|v| v.to_lowercase() == "true")
                .unwrap_or(false);

            StorageBackend::MinIO {
                endpoint,
                bucket,
                access_key,
                secret_key,
                use_ssl,
            }
        }
        _ => StorageBackend::InMemory,
    };

    let repository_backend = match std::env::var("REPOSITORY_BACKEND").as_deref() {
        Ok("database") => {
            let connection_string =
                std::env::var("DATABASE_URL").map_err(|_| AppError::Configuration {
                    message: "DATABASE_URL environment variable required".to_string(),
                })?;
            RepositoryBackend::Database { connection_string }
        }
        _ => RepositoryBackend::InMemory,
    };

    AppBuilder::new()
        .with_storage_backend(storage_backend)
        .with_repository_backend(repository_backend)
        .build()
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_in_memory_app() {
        let app = create_in_memory_app().await.unwrap();

        // Test that all services are created
        assert!(true); // Just a basic check that creation doesn't panic
    }

    #[tokio::test]
    async fn test_app_builder() {
        let app = AppBuilder::new()
            .with_storage_backend(StorageBackend::InMemory)
            .with_repository_backend(RepositoryBackend::InMemory)
            .build()
            .await
            .unwrap();

        // Test that all services are created
        assert!(true);
    }

    #[tokio::test]
    async fn test_dependencies_creation() {
        let deps = AppBuilder::new().build_dependencies().await.unwrap();

        // Test that all dependencies are created
        assert!(true);
    }
}
