pub mod adapters;
pub mod app;
pub mod domain;
pub mod ports;
pub mod services;

// Re-export key types for convenience

// Domain types - core business entities and value objects
pub use domain::{
    BucketName,
    DomainValidationError,

    Filter,

    LifecycleConfiguration,
    LifecycleError,
    LifecycleRule,
    // Value objects
    ObjectKey,
    // Models
    ObjectMetadata,
    // Errors
    StorageError,
    VersionId,
    VersionMetadata,
    VersionedObject,
    VersioningConfiguration,
};

// Port types - interfaces for external systems
pub use ports::{
    // Repository ports
    LifecycleRepository,
    ObjectRepository,
    // Service ports
    LifecycleService,
    VersioningService,
    // Storage ports
    storage::{
        CompletedPart,
        MultipartUpload,
        ObjectInfo,
        ObjectStore,
        PresignedUrlMethod,
        StorageVersionMetadata,
        StorageVersionedObject,
        VersionedObjectStore,
    },
};

// Service implementations - business logic
pub use services::{
    LifecycleServiceImpl, ObjectServiceBuilder, ObjectServiceImpl, VersioningServiceImpl,
};

// Application factory and configuration
pub use app::{
    AppBuilder, AppConfig, AppDependencies, AppError, AppServices, RepositoryBackend,
    StorageBackend, create_app_from_env, create_in_memory_app, create_minio_app, create_s3_app,
};

// Adapter types - infrastructure implementations
pub use adapters::outbound::storage::{
    S3ObjectStoreAdapter, VersionedS3ObjectStoreAdapter,
};

// Public facade for easy construction
pub mod prelude {
    pub use crate::{
        AppBuilder, AppServices, BucketName, LifecycleRepository,
        LifecycleService, LifecycleServiceImpl, ObjectKey, ObjectRepository, ObjectServiceImpl,
        ObjectStore, VersionId, S3ObjectStoreAdapter, VersionedS3ObjectStoreAdapter, VersionedObjectStore,
        VersioningService, create_in_memory_app, create_minio_app, create_s3_app,
    };
}
