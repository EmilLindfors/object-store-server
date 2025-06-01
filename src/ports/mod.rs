pub mod repositories;
pub mod services;
pub mod storage;

// Re-export all port traits for convenience
pub use repositories::{LifecycleRepository, ObjectRepository};
pub use services::{
    AppliedAction, BucketLifecycleResults, FailedAction, LifecycleActionResults, LifecycleService,
    MetadataChange, ProcessingError, ProcessingStatus, ValidationError, ValidationResult,
    ValidationWarning, VersionComparison, VersioningService,
};
pub use storage::{CompletedPart, ObjectInfo, ObjectStore, VersionedObjectStore};
