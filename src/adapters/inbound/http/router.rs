use axum::{
    Router,
    routing::{delete, get, head, post, put},
};

use super::handlers::{
    add_lifecycle_rule,
    copy_object,
    copy_versioned_object,
    // Object handlers
    create_object,
    delete_lifecycle_configuration,
    delete_object,
    delete_versioned_object,
    disable_lifecycle_rule,
    enable_lifecycle_rule,
    evaluate_object_lifecycle,
    get_latest_object,
    get_lifecycle_configuration,
    get_object,
    get_versioned_object,
    head_object,
    head_versioned_object,
    list_object_versions,
    list_objects,
    process_bucket_lifecycle,
    // Versioning handlers
    put_versioned_object,
    remove_lifecycle_rule,
    restore_version,
    // Lifecycle handlers
    set_lifecycle_configuration,
};
use std::sync::Arc;

use crate::{
    ports::services::{LifecycleService, ObjectService, VersioningService},
    services::{LifecycleServiceImpl, ObjectServiceImpl, VersioningServiceImpl},
};

/// Application state containing all services
#[derive(Clone)]
pub struct AppState {
    pub object_service: Arc<dyn ObjectService>,
    pub lifecycle_service: Arc<dyn LifecycleService>,
    pub versioning_service: Arc<dyn VersioningService>,
}

/// Create the main application router with all endpoints
pub fn create_router(state: AppState) -> Router {
    Router::new()
        // Object operations
        .route("/objects", get(list_objects))
        .route("/objects/:key", put(create_object))
        .route("/objects/:key", get(get_object))
        .route("/objects/:key", delete(delete_object))
        .route("/objects/:key", head(head_object))
        .route("/objects/:source_key/copy/:dest_key", post(copy_object))
        // Versioned object operations
        .route("/versioned-objects/:key", put(put_versioned_object))
        .route("/versioned-objects/:key/latest", get(get_latest_object))
        .route(
            "/versioned-objects/:key/versions",
            get(list_object_versions),
        )
        .route(
            "/versioned-objects/:key/versions/:version_id",
            get(get_versioned_object),
        )
        .route(
            "/versioned-objects/:key/versions/:version_id",
            delete(delete_versioned_object),
        )
        .route(
            "/versioned-objects/:key/versions/:version_id",
            head(head_versioned_object),
        )
        .route(
            "/versioned-objects/:source_key/versions/:source_version_id/copy/:dest_key",
            post(copy_versioned_object),
        )
        .route(
            "/versioned-objects/:key/versions/:version_id/restore",
            post(restore_version),
        )
        // Lifecycle management
        .route(
            "/buckets/:bucket/lifecycle",
            put(set_lifecycle_configuration),
        )
        .route(
            "/buckets/:bucket/lifecycle",
            get(get_lifecycle_configuration),
        )
        .route(
            "/buckets/:bucket/lifecycle",
            delete(delete_lifecycle_configuration),
        )
        .route("/buckets/:bucket/lifecycle/rules", post(add_lifecycle_rule))
        .route(
            "/buckets/:bucket/lifecycle/rules/:rule_id",
            delete(remove_lifecycle_rule),
        )
        .route(
            "/buckets/:bucket/lifecycle/rules/:rule_id/enable",
            post(enable_lifecycle_rule),
        )
        .route(
            "/buckets/:bucket/lifecycle/rules/:rule_id/disable",
            post(disable_lifecycle_rule),
        )
        .route(
            "/buckets/:bucket/lifecycle/process",
            post(process_bucket_lifecycle),
        )
        .route("/lifecycle/evaluate", post(evaluate_object_lifecycle))
        // Add state for dependency injection
        .with_state(state)
}

/// Create a router with just object operations
pub fn create_object_router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_objects))
        .route("/:key", put(create_object))
        .route("/:key", get(get_object))
        .route("/:key", delete(delete_object))
        .route("/:key", head(head_object))
        .route("/:source_key/copy/:dest_key", post(copy_object))
}

/// Create a router with just lifecycle operations
pub fn create_lifecycle_router() -> Router<AppState> {
    Router::new()
        .route(
            "/buckets/:bucket/lifecycle",
            put(set_lifecycle_configuration),
        )
        .route(
            "/buckets/:bucket/lifecycle",
            get(get_lifecycle_configuration),
        )
        .route(
            "/buckets/:bucket/lifecycle",
            delete(delete_lifecycle_configuration),
        )
        .route("/buckets/:bucket/lifecycle/rules", post(add_lifecycle_rule))
        .route(
            "/buckets/:bucket/lifecycle/rules/:rule_id",
            delete(remove_lifecycle_rule),
        )
        .route(
            "/buckets/:bucket/lifecycle/rules/:rule_id/enable",
            post(enable_lifecycle_rule),
        )
        .route(
            "/buckets/:bucket/lifecycle/rules/:rule_id/disable",
            post(disable_lifecycle_rule),
        )
        .route(
            "/buckets/:bucket/lifecycle/process",
            post(process_bucket_lifecycle),
        )
        .route("/lifecycle/evaluate", post(evaluate_object_lifecycle))
}

/// Create a router with just versioning operations
pub fn create_versioning_router() -> Router<AppState> {
    Router::new()
        .route("/:key", put(put_versioned_object))
        .route("/:key/latest", get(get_latest_object))
        .route("/:key/versions", get(list_object_versions))
        .route("/:key/versions/:version_id", get(get_versioned_object))
        .route(
            "/:key/versions/:version_id",
            delete(delete_versioned_object),
        )
        .route("/:key/versions/:version_id", head(head_versioned_object))
        .route(
            "/:source_key/versions/:source_version_id/copy/:dest_key",
            post(copy_versioned_object),
        )
        .route("/:key/versions/:version_id/restore", post(restore_version))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        VersionedApacheObjectStoreAdapter,
        adapters::outbound::{
            persistence::{InMemoryLifecycleRepository, InMemoryObjectRepository},
            storage::ApacheObjectStoreAdapter,
        },
    };
    use axum_test::TestServer;
    use object_store::memory::InMemory;
    use std::sync::Arc;

    async fn create_test_app_state() -> AppState {
        let memory_store = Arc::new(InMemory::new());
        let object_store = Arc::new(ApacheObjectStoreAdapter::new(memory_store.clone()));
        let versioned_store = Arc::new(VersionedApacheObjectStoreAdapter::new(memory_store));
        let object_repo = Arc::new(InMemoryObjectRepository::new());
        let lifecycle_repo = Arc::new(InMemoryLifecycleRepository::new());

        let object_service = Arc::new(ObjectServiceImpl::new(
            object_repo.clone(),
            object_store.clone(),
        ));

        let lifecycle_service = Arc::new(LifecycleServiceImpl::new(
            lifecycle_repo,
            object_repo.clone(),
            object_store.clone(),
            versioned_store.clone(),
        ));

        let versioning_service = Arc::new(crate::services::VersioningServiceImpl::new(
            object_repo.clone(),
            versioned_store,
        ));

        AppState {
            object_service,
            lifecycle_service,
            versioning_service,
        }
    }

    #[tokio::test]
    async fn test_router_creation() {
        let state = create_test_app_state().await;
        let app = create_router(state);

        // This test just ensures the router can be created without panicking
        assert!(true);
    }

    #[tokio::test]
    async fn test_object_router() {
        let state = create_test_app_state().await;
        let object_router = create_object_router().with_state(state);

        // Test that we can create a test server with the router
        let _server = TestServer::new(object_router).unwrap();
        assert!(true);
    }
}
