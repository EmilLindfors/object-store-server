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
    CompletedPart,

    LifecycleRepository,

    LifecycleService,
    ObjectInfo,
    // Repository ports
    ObjectRepository,
    // Storage ports
    ObjectStore,
    VersionedObjectStore,
    // Service ports
    VersioningService,
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
    ApacheObjectStoreAdapter, VersionedApacheObjectStoreAdapter,
};

// Public facade for easy construction
pub mod prelude {
    pub use crate::{
        ApacheObjectStoreAdapter, AppBuilder, AppServices, BucketName, LifecycleRepository,
        LifecycleService, LifecycleServiceImpl, ObjectKey, ObjectRepository, ObjectServiceImpl,
        ObjectStore, VersionId, VersionedApacheObjectStoreAdapter, VersionedObjectStore,
        VersioningService, create_in_memory_app, create_minio_app, create_s3_app,
    };
}
