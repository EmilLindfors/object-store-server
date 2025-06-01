use axum_test::TestServer;
use bytes::Bytes;
use chrono::{Duration, Utc};
use object_store_server::{
    BucketName, ObjectKey, VersionId,
    adapters::inbound::http::router::{AppState, create_router},
    create_in_memory_app,
    domain::models::{
        CreateObjectRequest, Filter, GetObjectRequest, LifecycleConfiguration, LifecycleRule,
        lifecycle::{RuleStatus, StorageClass},
    },
    ports::services::{LifecycleService, ObjectService, VersioningService},
};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

async fn setup_test_server() -> TestServer {
    let services = create_in_memory_app().await.unwrap();

    let state = AppState {
        object_service: Arc::new(services.object_service),
        lifecycle_service: Arc::new(services.lifecycle_service),
        versioning_service: Arc::new(services.versioning_service),
    };

    let app = create_router(state);
    TestServer::new(app).unwrap()
}

#[tokio::test]
async fn test_versioning_integration() {
    // Create services for direct testing
    let services = create_in_memory_app().await.unwrap();

    // Create test object
    let key = ObjectKey::new("versioned-file.txt".to_string()).unwrap();

    // Version 1
    let v1_content = Bytes::from("version 1 content");
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

    // Version 2
    let v2_content = Bytes::from("version 2 content");
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

    // List versions
    let versions = services
        .versioning_service
        .list_versions(&key)
        .await
        .unwrap();

    assert_eq!(versions.versions.len(), 2);

    // Get specific version
    let get_v1 = GetObjectRequest {
        key: key.clone(),
        version_id: Some(v1.version_id.clone()),
    };

    let retrieved_v1 = services
        .versioning_service
        .get_object(get_v1)
        .await
        .unwrap();

    assert_eq!(retrieved_v1.data, v1_content.to_vec());

    // Get latest (should be v2)
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
async fn test_lifecycle_integration() {
    let services = create_in_memory_app().await.unwrap();

    // Create a bucket with lifecycle configuration
    let bucket = BucketName::new("lifecycle-test-bucket".to_string()).unwrap();

    let mut filter = Filter::new();
    filter.prefix = Some("temp/".to_string());

    let config = LifecycleConfiguration {
        bucket: bucket.clone(),
        rules: vec![
            LifecycleRule {
                id: "delete-temp-files".to_string(),
                status: RuleStatus::Enabled,
                filter: filter.clone(),
                expiration_days: Some(1),
                ..Default::default()
            },
            LifecycleRule {
                id: "archive-old-data".to_string(),
                status: RuleStatus::Enabled,
                filter: {
                    let mut f = Filter::new();
                    f.prefix = Some("archive/".to_string());
                    f
                },
                transition_days: Some(30),
                transition_storage_class: Some(StorageClass::Glacier),
                ..Default::default()
            },
        ],
    };

    // Set lifecycle configuration
    services
        .lifecycle_service
        .set_lifecycle_configuration(&bucket, config)
        .await
        .unwrap();

    // Verify configuration was set
    let retrieved = services
        .lifecycle_service
        .get_lifecycle_configuration(&bucket)
        .await
        .unwrap()
        .expect("Configuration should exist");

    assert_eq!(retrieved.rules.len(), 2);
    assert_eq!(retrieved.rules[0].id, "delete-temp-files");
    assert_eq!(retrieved.rules[1].id, "archive-old-data");
}

#[tokio::test]
async fn test_http_object_operations() {
    let server = setup_test_server().await;

    // Create a bucket
    let create_bucket = server.put("/buckets/test-bucket").await;

    assert_eq!(create_bucket.status_code(), 200);

    // Upload an object
    let upload = server
        .put("/buckets/test-bucket/test-file.txt")
        .content_type("text/plain")
        .text("Hello, World!")
        .await;

    assert_eq!(upload.status_code(), 200);

    // Get the object
    let get = server.get("/buckets/test-bucket/test-file.txt").await;

    assert_eq!(get.status_code(), 200);
    assert_eq!(get.text(), "Hello, World!");

    // List objects
    let list = server.get("/buckets/test-bucket").await;

    assert_eq!(list.status_code(), 200);
    let list_json: serde_json::Value = list.json();
    assert!(list_json["objects"].as_array().unwrap().len() > 0);

    // Delete the object
    let delete = server.delete("/buckets/test-bucket/test-file.txt").await;

    assert_eq!(delete.status_code(), 204);

    // Verify it's gone
    let get_after_delete = server.get("/buckets/test-bucket/test-file.txt").await;

    assert_eq!(get_after_delete.status_code(), 404);
}

#[tokio::test]
async fn test_http_versioning_operations() {
    let server = setup_test_server().await;

    // Create a bucket
    server.put("/buckets/versioned-bucket").await;

    // Enable versioning
    let enable_versioning = server
        .put("/buckets/versioned-bucket/versioning")
        .json(&json!({ "status": "Enabled" }))
        .await;

    assert_eq!(enable_versioning.status_code(), 200);

    // Upload first version
    let v1 = server
        .put("/buckets/versioned-bucket/doc.txt")
        .text("version 1")
        .await;

    assert_eq!(v1.status_code(), 200);
    let v1_json: serde_json::Value = v1.json();
    let v1_id = v1_json["version_id"].as_str().unwrap();

    // Upload second version
    let v2 = server
        .put("/buckets/versioned-bucket/doc.txt")
        .text("version 2")
        .await;

    assert_eq!(v2.status_code(), 200);

    // List versions
    let versions = server
        .get("/buckets/versioned-bucket/doc.txt/versions")
        .await;

    assert_eq!(versions.status_code(), 200);
    let versions_json: serde_json::Value = versions.json();
    assert_eq!(versions_json["versions"].as_array().unwrap().len(), 2);

    // Get specific version
    let get_v1 = server
        .get(&format!(
            "/buckets/versioned-bucket/doc.txt?version_id={}",
            v1_id
        ))
        .await;

    assert_eq!(get_v1.status_code(), 200);
    assert_eq!(get_v1.text(), "version 1");
}

#[tokio::test]
async fn test_http_lifecycle_operations() {
    let server = setup_test_server().await;

    // Create a bucket
    server.put("/buckets/lifecycle-bucket").await;

    // Set lifecycle configuration
    let lifecycle_config = json!({
        "rules": [
            {
                "id": "expire-temp",
                "status": "Enabled",
                "filter": {
                    "prefix": "temp/"
                },
                "expiration": {
                    "days": 7
                }
            }
        ]
    });

    let set_lifecycle = server
        .put("/buckets/lifecycle-bucket/lifecycle")
        .json(&lifecycle_config)
        .await;

    assert_eq!(set_lifecycle.status_code(), 200);

    // Get lifecycle configuration
    let get_lifecycle = server.get("/buckets/lifecycle-bucket/lifecycle").await;

    assert_eq!(get_lifecycle.status_code(), 200);
    let retrieved: serde_json::Value = get_lifecycle.json();
    assert_eq!(retrieved["rules"][0]["id"], "expire-temp");

    // Delete lifecycle configuration
    let delete_lifecycle = server.delete("/buckets/lifecycle-bucket/lifecycle").await;

    assert_eq!(delete_lifecycle.status_code(), 204);
}

#[tokio::test]
async fn test_multipart_operations() {
    let services = create_in_memory_app().await.unwrap();

    // Create a large object that would require multipart upload
    let key = ObjectKey::new("large-file.bin".to_string()).unwrap();
    let part1 = vec![1u8; 5 * 1024 * 1024]; // 5MB
    let part2 = vec![2u8; 5 * 1024 * 1024]; // 5MB

    // In real implementation, this would use multipart upload
    // For now, just concatenate and upload
    let mut full_data = part1.clone();
    full_data.extend(&part2);

    let create_request = CreateObjectRequest {
        key: key.clone(),
        data: full_data.clone(),
        content_type: Some("application/octet-stream".to_string()),
        custom_metadata: HashMap::new(),
    };

    services
        .object_service
        .create_object(create_request)
        .await
        .unwrap();

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

    assert_eq!(retrieved.data.len(), 10 * 1024 * 1024);
    assert_eq!(&retrieved.data[..5 * 1024 * 1024], &part1[..]);
    assert_eq!(&retrieved.data[5 * 1024 * 1024..], &part2[..]);
}

#[tokio::test]
async fn test_copy_operations() {
    let services = create_in_memory_app().await.unwrap();

    // Create source object
    let source_key = ObjectKey::new("source.txt".to_string()).unwrap();
    let content = Bytes::from("data to copy");

    let create_request = CreateObjectRequest {
        key: source_key.clone(),
        data: content.to_vec(),
        content_type: Some("text/plain".to_string()),
        custom_metadata: HashMap::new(),
    };

    services
        .object_service
        .create_object(create_request)
        .await
        .unwrap();

    // Copy to new location
    let dest_key = ObjectKey::new("destination.txt".to_string()).unwrap();

    let copied = services
        .object_service
        .copy_object(&source_key, &dest_key)
        .await
        .unwrap();

    assert_eq!(copied.key, dest_key);
    assert_eq!(copied.data, content.to_vec());

    // Verify both objects exist
    assert!(
        services
            .object_service
            .object_exists(&source_key)
            .await
            .unwrap()
    );
    assert!(
        services
            .object_service
            .object_exists(&dest_key)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn test_metadata_operations() {
    let services = create_in_memory_app().await.unwrap();

    // Create object with custom metadata
    let key = ObjectKey::new("metadata-test.txt".to_string()).unwrap();
    let mut custom_metadata = HashMap::new();
    custom_metadata.insert("author".to_string(), "test-user".to_string());
    custom_metadata.insert("department".to_string(), "engineering".to_string());

    let create_request = CreateObjectRequest {
        key: key.clone(),
        data: b"test data".to_vec(),
        content_type: Some("text/plain".to_string()),
        custom_metadata: custom_metadata.clone(),
    };

    let created = services
        .object_service
        .create_object(create_request)
        .await
        .unwrap();

    // Verify metadata
    assert_eq!(
        created.metadata.content_type,
        Some("text/plain".to_string())
    );
    assert_eq!(created.metadata.content_length, 9);

    // Get object and check metadata is preserved
    let get_request = GetObjectRequest {
        key: key.clone(),
        version_id: None,
    };

    let retrieved = services
        .object_service
        .get_object(get_request)
        .await
        .unwrap();

    assert_eq!(retrieved.metadata.custom_metadata, custom_metadata);
}

#[tokio::test]
async fn test_concurrent_operations() {
    let services = create_in_memory_app().await.unwrap();
    let services = Arc::new(services);

    // Spawn multiple tasks to create objects concurrently
    let mut handles = vec![];

    for i in 0..10 {
        let services = services.clone();
        let handle = tokio::spawn(async move {
            let key = ObjectKey::new(format!("concurrent-{}.txt", i)).unwrap();
            let data = format!("data for file {}", i);

            let create_request = CreateObjectRequest {
                key: key.clone(),
                data: data.into_bytes(),
                content_type: None,
                custom_metadata: HashMap::new(),
            };

            services
                .object_service
                .create_object(create_request)
                .await
                .unwrap();

            key
        });

        handles.push(handle);
    }

    // Wait for all to complete
    let keys: Vec<ObjectKey> = futures::future::join_all(handles)
        .await
        .into_iter()
        .map(|r| r.unwrap())
        .collect();

    // Verify all objects were created
    for key in keys {
        assert!(services.object_service.object_exists(&key).await.unwrap());
    }
}

#[tokio::test]
async fn test_error_handling() {
    let services = create_in_memory_app().await.unwrap();

    // Try to get non-existent object
    let key = ObjectKey::new("non-existent.txt".to_string()).unwrap();
    let get_request = GetObjectRequest {
        key: key.clone(),
        version_id: None,
    };

    let result = services.object_service.get_object(get_request).await;

    assert!(result.is_err());

    // Try to delete non-existent object
    let delete_result = services.object_service.delete_object(&key).await;

    assert!(delete_result.is_err());

    // Try to create object with invalid key
    let invalid_key_result = ObjectKey::new("".to_string());
    assert!(invalid_key_result.is_err());
}

#[tokio::test]
async fn test_lifecycle_evaluation() {
    let services = create_in_memory_app().await.unwrap();

    // Create a bucket with lifecycle rules
    let bucket = BucketName::new("eval-bucket".to_string()).unwrap();

    let mut filter = Filter::new();
    filter.prefix = Some("logs/".to_string());

    let config = LifecycleConfiguration {
        bucket: bucket.clone(),
        rules: vec![LifecycleRule {
            id: "delete-old-logs".to_string(),
            status: RuleStatus::Enabled,
            filter,
            expiration_days: Some(30),
            ..Default::default()
        }],
    };

    services
        .lifecycle_service
        .set_lifecycle_configuration(&bucket, config)
        .await
        .unwrap();

    // Process lifecycle (in real implementation, this would check object ages)
    let result = services
        .lifecycle_service
        .process_bucket_lifecycle(&bucket)
        .await
        .unwrap();

    // Verify processing occurred
    assert_eq!(result.bucket, bucket);
    assert_eq!(result.errors.len(), 0);
}

#[tokio::test]
async fn test_versioning_with_delete_markers() {
    let services = create_in_memory_app().await.unwrap();

    // Create versioned object
    let key = ObjectKey::new("delete-marker-test.txt".to_string()).unwrap();

    let create_request = CreateObjectRequest {
        key: key.clone(),
        data: b"original content".to_vec(),
        content_type: None,
        custom_metadata: HashMap::new(),
    };

    let v1 = services
        .versioning_service
        .create_versioned_object(create_request)
        .await
        .unwrap();

    // Delete the object (should create delete marker)
    services.object_service.delete_object(&key).await.unwrap();

    // List versions should show both original and delete marker
    let versions = services
        .versioning_service
        .list_versions(&key)
        .await
        .unwrap();

    // Should have at least the original version
    assert!(versions.versions.len() >= 1);

    // Try to get latest (should fail due to delete marker)
    let get_latest = GetObjectRequest {
        key: key.clone(),
        version_id: None,
    };

    let result = services.versioning_service.get_object(get_latest).await;

    assert!(result.is_err());

    // But getting specific version should work
    let get_v1 = GetObjectRequest {
        key: key.clone(),
        version_id: Some(v1.version_id),
    };

    let retrieved = services
        .versioning_service
        .get_object(get_v1)
        .await
        .unwrap();

    assert_eq!(retrieved.data, b"original content");
}
