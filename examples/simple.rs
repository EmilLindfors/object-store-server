// src/main.rs
use axum::{
    Router,
    extract::{Multipart, Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use object_store::aws::AmazonS3Builder;
use object_store_bridge::{
    MinioClient,
    domain::lifecycle::LifecycleConfig as MinioLifecycleConfig,
    lifecycle::{ExpirationConfig, LifecycleConfiguration, LifecycleRule, RuleStatus},
    lifecycle_adapter::{domain_to_service, service_to_domain},
    middleware::{ObjectStoreLayer, ObjectStoreService, create_object_store_router},
};
use std::sync::Arc;
use tower::ServiceBuilder;
use tower_http::{
    cors::CorsLayer,
    trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer},
};
use tracing::{Level, info};

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    info!("Starting Object Store Bridge service");

    // Create the S3 compatible store
    let store = AmazonS3Builder::new()
        .with_bucket_name("my-bucket")
        .with_access_key_id(std::env::var("MINIO_ACCESS_KEY_ID").unwrap())
        .with_secret_access_key(std::env::var("MINIO_SECRET_ACCESS_KEY").unwrap())
        .with_endpoint(std::env::var("MINIO_ENDPOINT").unwrap())
        .with_region("")
        .build()
        .expect("Failed to create S3 store");

    // Create the object store service
    let service = Arc::new(ObjectStoreService::new(store));

    // Set up a default lifecycle configuration
    let lifecycle_config = LifecycleConfiguration {
        rules: vec![LifecycleRule {
            id: "cleanup-docs".to_string(),
            prefix: "docs/".to_string(),
            status: RuleStatus::Enabled,
            expiration: Some(ExpirationConfig {
                days: Some(90),
                date: None,
                expired_object_delete_marker: None,
            }),
            transitions: None,
            filter_tags: None,
        }],
    };

    // Set the configuration for the bucket
    service
        .set_lifecycle_config("my-bucket", lifecycle_config.clone())
        .expect("Failed to set lifecycle config");

    // Start the lifecycle manager
    service
        .start_lifecycle_manager()
        .await
        .expect("Failed to start lifecycle manager");

    // OPTIONAL: If MinIO is available, also set the lifecycle configuration directly in MinIO
    if let Ok(endpoint) = std::env::var("MINIO_ENDPOINT") {
        if let (Ok(access_key), Ok(secret_key)) = (
            std::env::var("MINIO_ACCESS_KEY_ID"),
            std::env::var("MINIO_SECRET_ACCESS_KEY"),
        ) {
            info!("MinIO credentials found, attempting to set lifecycle configuration in MinIO");

            // Option 1: Using the service's convenience method
            match service
                .set_minio_lifecycle_config(
                    "my-bucket",
                    &lifecycle_config,
                    &endpoint,
                    &access_key,
                    &secret_key,
                )
                .await
            {
                Ok(_) => info!("Successfully set lifecycle configuration in MinIO using service"),
                Err(e) => info!("Failed to set lifecycle in MinIO using service: {}", e),
            }

            // Option 2: Using the MinIO client directly
            let minio_client = MinioClient::new(&endpoint, &access_key, &secret_key, "");

            // Convert service config to domain config
            let domain_config = service_to_domain(&lifecycle_config);

            match minio_client
                .set_lifecycle_config("my-bucket", &domain_config)
                .await
            {
                Ok(_) => info!("Successfully set lifecycle configuration in MinIO using client"),
                Err(e) => info!("Failed to set lifecycle in MinIO using client: {}", e),
            }

            // Retrieve the configuration
            match minio_client.get_lifecycle_config("my-bucket").await {
                Ok(config) => {
                    info!("Retrieved lifecycle configuration from MinIO:");
                    info!("Rules count: {}", config.rules.len());
                    for rule in &config.rules {
                        info!(
                            "Rule ID: {}, Status: {}",
                            rule.id,
                            if rule.status { "Enabled" } else { "Disabled" }
                        );
                    }
                }
                Err(e) => info!("Failed to get lifecycle from MinIO: {}", e),
            }
        }
    }

    // Create separate router with state for upload route
    let upload_routes = Router::new()
        .route("/upload", post(handle_multipart_upload))
        .with_state(service.clone());

    // Create the main router
    let app = Router::new()
        // Mount object storage routes
        .merge(create_object_store_router(service.clone()))
        // Additional application routes
        .route("/health", get(health_check))
        // Merge upload routes with state
        .merge(upload_routes)
        // Apply middleware
        .layer(
            ServiceBuilder::new()
                .layer(
                    TraceLayer::new_for_http()
                        .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                        .on_response(DefaultOnResponse::new().level(Level::INFO)),
                )
                .layer(CorsLayer::permissive())
                .layer(ObjectStoreLayer::new(service)),
        );

    // Start the server
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("Failed to bind to port");

    info!("Listening on http://0.0.0.0:3000");

    axum::serve(listener, app)
        .await
        .expect("Failed to start server");
}

// Handler for health check
async fn health_check() -> impl IntoResponse {
    StatusCode::OK
}

// Handler for multipart file uploads
async fn handle_multipart_upload(
    State(service): State<
        Arc<ObjectStoreService<impl object_store::ObjectStore + Send + Sync + 'static>>,
    >,
    Path(bucket): Path<String>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    // Process multipart form
    while let Some(field) = multipart.next_field().await.unwrap_or(None) {
        let _name = field.name().unwrap_or("file").to_string();
        let file_name = field.file_name().unwrap_or("unnamed.dat").to_string();

        // Convert field to bytes directly
        let data = field.bytes().await.unwrap_or_default();

        // Convert to Bytes type
        let bytes = bytes::Bytes::from(data);

        // Store the file with automatic versioning
        match service.put_object(&bucket, &file_name, bytes, None).await {
            Ok(_) => return StatusCode::CREATED,
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    StatusCode::BAD_REQUEST
}
