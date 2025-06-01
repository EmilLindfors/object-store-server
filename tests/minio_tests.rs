use bytes::Bytes;
use chrono::{Duration, Utc};
use object_store_server::{
    BucketName, ObjectKey,
    adapters::inbound::http::router::{AppState, create_router},
    create_minio_app,
    domain::models::{
        CreateObjectRequest, Filter, GetObjectRequest, LifecycleConfiguration, LifecycleRule,
        lifecycle::{RuleStatus, StorageClass},
    },
    ports::services::{LifecycleService, ObjectService, VersioningService},
};
use std::collections::HashMap;
use std::sync::Arc;

// Note: These tests require MinIO to be running and configured via environment variables:
// - MINIO_ENDPOINT (default: http://localhost:9000)
// - MINIO_ACCESS_KEY_ID (default: minioadmin)
// - MINIO_SECRET_ACCESS_KEY (default: minioadmin)
// - MINIO_BUCKET (default: test-bucket)

#[tokio::test]
#[ignore = "requires MinIO server to be running"]
async fn test_minio_object_operations() {
    // Get MinIO configuration from environment
    let endpoint =
        std::env::var("MINIO_ENDPOINT").unwrap_or_else(|_| "http://localhost:9000".to_string());
    let access_key =
        std::env::var("MINIO_ACCESS_KEY_ID").unwrap_or_else(|_| "minioadmin".to_string());
    let secret_key =
        std::env::var("MINIO_SECRET_ACCESS_KEY").unwrap_or_else(|_| "minioadmin".to_string());
    let bucket = std::env::var("MINIO_BUCKET").unwrap_or_else(|_| "test-bucket".to_string());

    println!("Connecting to MinIO at {} with bucket {}", endpoint, bucket);

    // Create MinIO-backed application
    let services = create_minio_app(
        endpoint, bucket, access_key, secret_key, true, // use_ssl = false for http
    )
    .await
    .unwrap();

    // Test object operations
    let key = ObjectKey::new("minio-test.txt".to_string()).unwrap();
    let content = Bytes::from("Hello from MinIO!");

    // Create object
    let create_request = CreateObjectRequest {
        key: key.clone(),
        data: content.to_vec(),
        content_type: Some("text/plain".to_string()),
        custom_metadata: HashMap::new(),
    };

    let created = services
        .object_service
        .create_object(create_request)
        .await
        .unwrap();

    println!("Created object with key: {:?}", created.key);

    // Get object
    let get_request = GetObjectRequest {
        key: key.clone(),
        version_id: None,
    };

    let retrieved = services
        .object_service
        .get_object(get_request)
        .await
        .unwrap();

    assert_eq!(retrieved.data, content.to_vec());

    // List objects
    let objects = services
        .object_service
        .list_objects(None, None)
        .await
        .unwrap();

    assert!(objects.iter().any(|o| o.key == key));

    // Delete object
    services.object_service.delete_object(&key).await.unwrap();

    // Verify deletion
    let get_after_delete = GetObjectRequest {
        key: key.clone(),
        version_id: None,
    };

    let result = services.object_service.get_object(get_after_delete).await;

    assert!(result.is_err());
}

#[tokio::test]
#[ignore = "requires MinIO server to be running"]
async fn test_minio_versioning() {
    let endpoint =
        std::env::var("MINIO_ENDPOINT").unwrap_or_else(|_| "http://localhost:9000".to_string());
    let access_key =
        std::env::var("MINIO_ACCESS_KEY_ID").unwrap_or_else(|_| "minioadmin".to_string());
    let secret_key =
        std::env::var("MINIO_SECRET_ACCESS_KEY").unwrap_or_else(|_| "minioadmin".to_string());
    let bucket =
        std::env::var("MINIO_BUCKET").unwrap_or_else(|_| "test-versioning-bucket".to_string());

    // Create MinIO-backed application
    let services = create_minio_app(endpoint, bucket.clone(), access_key, secret_key, true)
        .await
        .unwrap();

    // Enable versioning on the bucket
    let bucket_name = BucketName::new(bucket).unwrap();
    services
        .versioning_service
        .enable_versioning(&bucket_name)
        .await
        .unwrap();

    // Create multiple versions of an object
    let key = ObjectKey::new("versioned-doc.txt".to_string()).unwrap();

    // Version 1
    let v1_content = Bytes::from("Version 1 content");
    let create_v1 = CreateObjectRequest {
        key: key.clone(),
        data: v1_content.to_vec(),
        content_type: Some("text/plain".to_string()),
        custom_metadata: HashMap::new(),
    };

    let v1 = services
        .versioning_service
        .create_versioned_object(create_v1)
        .await
        .unwrap();

    println!("Created version 1 with ID: {:?}", v1.version_id);

    // Version 2
    let v2_content = Bytes::from("Version 2 content - updated");
    let create_v2 = CreateObjectRequest {
        key: key.clone(),
        data: v2_content.to_vec(),
        content_type: Some("text/plain".to_string()),
        custom_metadata: HashMap::new(),
    };

    let v2 = services
        .versioning_service
        .create_versioned_object(create_v2)
        .await
        .unwrap();

    println!("Created version 2 with ID: {:?}", v2.version_id);

    // List versions
    let versions = services
        .versioning_service
        .list_versions(&key)
        .await
        .unwrap();

    assert!(versions.versions.len() >= 2);
    println!("Found {} versions", versions.versions.len());

    // Get specific version
    let get_v1 = GetObjectRequest {
        key: key.clone(),
        version_id: Some(v1.version_id),
    };

    let retrieved_v1 = services
        .versioning_service
        .get_object(get_v1)
        .await
        .unwrap();

    assert_eq!(retrieved_v1.data, v1_content.to_vec());

    // Get latest version
    let get_latest = GetObjectRequest {
        key: key.clone(),
        version_id: None,
    };

    let latest = services
        .versioning_service
        .get_object(get_latest)
        .await
        .unwrap();

    assert_eq!(latest.data, v2_content.to_vec());
}

#[tokio::test]
#[ignore = "requires MinIO server to be running"]
async fn test_minio_lifecycle() {
    let endpoint =
        std::env::var("MINIO_ENDPOINT").unwrap_or_else(|_| "http://localhost:9000".to_string());
    let access_key =
        std::env::var("MINIO_ACCESS_KEY_ID").unwrap_or_else(|_| "minioadmin".to_string());
    let secret_key =
        std::env::var("MINIO_SECRET_ACCESS_KEY").unwrap_or_else(|_| "minioadmin".to_string());
    let bucket =
        std::env::var("MINIO_BUCKET").unwrap_or_else(|_| "test-lifecycle-bucket".to_string());

    // Create MinIO-backed application
    let services = create_minio_app(endpoint, bucket.clone(), access_key, secret_key, true)
        .await
        .unwrap();

    let bucket_name = BucketName::new(bucket).unwrap();

    // Create lifecycle configuration
    let mut filter = Filter::new();
    filter.prefix = Some("temp/".to_string());

    let config = LifecycleConfiguration {
        bucket: bucket_name.clone(),
        rules: vec![
            LifecycleRule {
                id: "delete-temp-files".to_string(),
                status: RuleStatus::Enabled,
                filter,
                expiration_days: Some(1),
                ..Default::default()
            },
            LifecycleRule {
                id: "transition-old-logs".to_string(),
                status: RuleStatus::Enabled,
                filter: {
                    let mut f = Filter::new();
                    f.prefix = Some("logs/".to_string());
                    f
                },
                transition_days: Some(30),
                transition_storage_class: Some(StorageClass::InfrequentAccess),
                ..Default::default()
            },
        ],
    };

    // Set lifecycle configuration
    services
        .lifecycle_service
        .set_lifecycle_configuration(&bucket_name, config)
        .await
        .unwrap();

    println!("Lifecycle configuration set successfully");

    // Get and verify configuration
    let retrieved = services
        .lifecycle_service
        .get_lifecycle_configuration(&bucket_name)
        .await
        .unwrap()
        .expect("Configuration should exist");

    assert_eq!(retrieved.rules.len(), 2);
    assert_eq!(retrieved.rules[0].id, "delete-temp-files");
    assert_eq!(retrieved.rules[1].id, "transition-old-logs");

    // Create test objects
    let temp_key = ObjectKey::new("temp/test-file.txt".to_string()).unwrap();
    let log_key = ObjectKey::new("logs/app.log".to_string()).unwrap();

    // Create temp file
    let temp_request = CreateObjectRequest {
        key: temp_key.clone(),
        data: b"temporary data".to_vec(),
        content_type: None,
        custom_metadata: HashMap::new(),
    };

    services
        .object_service
        .create_object(temp_request)
        .await
        .unwrap();

    // Create log file
    let log_request = CreateObjectRequest {
        key: log_key.clone(),
        data: b"log data".to_vec(),
        content_type: None,
        custom_metadata: HashMap::new(),
    };

    services
        .object_service
        .create_object(log_request)
        .await
        .unwrap();

    println!("Test objects created");

    // Process lifecycle rules
    let result = services
        .lifecycle_service
        .process_bucket_lifecycle(&bucket_name)
        .await
        .unwrap();

    println!(
        "Processed {} objects with {} actions applied",
        result.objects_processed, result.actions_applied
    );

    // Clean up - delete lifecycle configuration
    services
        .lifecycle_service
        .delete_lifecycle_configuration(&bucket_name)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "requires MinIO server to be running"]
async fn test_minio_multipart_upload() {
    let endpoint =
        std::env::var("MINIO_ENDPOINT").unwrap_or_else(|_| "http://localhost:9000".to_string());
    let access_key =
        std::env::var("MINIO_ACCESS_KEY_ID").unwrap_or_else(|_| "minioadmin".to_string());
    let secret_key =
        std::env::var("MINIO_SECRET_ACCESS_KEY").unwrap_or_else(|_| "minioadmin".to_string());
    let bucket =
        std::env::var("MINIO_BUCKET").unwrap_or_else(|_| "test-multipart-bucket".to_string());

    // Create MinIO-backed application
    let services = create_minio_app(endpoint, bucket, access_key, secret_key, true)
        .await
        .unwrap();

    // Create a large object (10MB) that would benefit from multipart upload
    let key = ObjectKey::new("large-file.bin".to_string()).unwrap();
    let size = 10 * 1024 * 1024; // 10MB
    let data = vec![42u8; size];

    let create_request = CreateObjectRequest {
        key: key.clone(),
        data: data.clone(),
        content_type: Some("application/octet-stream".to_string()),
        custom_metadata: HashMap::new(),
    };

    println!("Uploading {}MB file...", size / 1024 / 1024);

    let created = services
        .object_service
        .create_object(create_request)
        .await
        .unwrap();

    println!("Upload complete");

    // Verify the object
    let get_request = GetObjectRequest {
        key: key.clone(),
        version_id: None,
    };

    let retrieved = services
        .object_service
        .get_object(get_request)
        .await
        .unwrap();

    assert_eq!(retrieved.data.len(), size);
    assert_eq!(retrieved.data[0], 42);
    assert_eq!(retrieved.data[size - 1], 42);

    // Clean up
    services.object_service.delete_object(&key).await.unwrap();
}
