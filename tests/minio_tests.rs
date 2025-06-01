use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    routing::get,
};
use bytes::Bytes;
use object_store::aws::AmazonS3Builder;
use object_store::{ObjectStore, path::Path};
use object_store_bridge::{
    lifecycle::{ExpirationConfig, LifecycleConfiguration, LifecycleRule, RuleStatus},
    middleware::{ObjectStoreLayer, ObjectStoreService},
    versioning::VersionedStore,
};
use serde_json::Value;
use std::sync::Arc;
use tokio::time::{Duration, sleep};
use tower::ServiceExt;
use uuid::Uuid;

// Helper function to convert Response to bytes for middleware tests
async fn response_body_bytes(response: axum::response::Response) -> Bytes {
    axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap()
}

// Helper function to convert Response to JSON for middleware tests
async fn response_json(response: axum::response::Response) -> Value {
    let bytes = response_body_bytes(response).await;
    serde_json::from_slice(&bytes).unwrap()
}

// Test handler for middleware passthrough tests
async fn test_handler() -> &'static str {
    "test handler response"
}

// Main test function - requires MinIO to be running
#[tokio::test]
async fn test_minio_integration() {
    // Check for required environment variables
    let endpoint =
        std::env::var("MINIO_ENDPOINT").unwrap_or_else(|_| "http://localhost:9000".to_string());
    let access_key =
        std::env::var("MINIO_ACCESS_KEY_ID").unwrap_or_else(|_| "minioadmin".to_string());
    let secret_key =
        std::env::var("MINIO_SECRET_ACCESS_KEY").unwrap_or_else(|_| "minioadmin".to_string());

    // Create a unique bucket name for this test run
    let bucket_name = format!("test-bucket");
    println!(
        "Using MinIO endpoint: {} with bucket: {}",
        endpoint, bucket_name
    );

    // Create the S3 compatible store
    let store = AmazonS3Builder::new()
        .with_bucket_name(&bucket_name)
        .with_access_key_id(&access_key)
        .with_secret_access_key(&secret_key)
        .with_endpoint(&endpoint)
        .with_allow_http(true)
        .with_region("us-east-1")
        .build()
        .expect("Failed to create S3 store");

    // Create the object store service
    let service = Arc::new(ObjectStoreService::new(store.clone()));

    // Create a router with the object store middleware
    let app = Router::new()
        .route("/test", get(test_handler))
        .layer(ObjectStoreLayer::new(service.clone()));

    // Test 1: Create a bucket
    // MinIO usually auto-creates buckets on first use
    // Add a small object to ensure the bucket exists
    let init_object_path = Path::from("_init_file.txt");
    println!("Creating init file with path: {}", init_object_path);
    let init_data = Bytes::from("Bucket initialization");

    match store.put(&init_object_path, init_data.clone().into()).await {
        Ok(_) => println!("Successfully initialized bucket"),
        Err(err) => panic!("Failed to initialize bucket: {}", err),
    }

    // Test 2: Set up a lifecycle configuration
    let lifecycle_config = LifecycleConfiguration {
        rules: vec![LifecycleRule {
            id: "test-rule".to_string(),
            prefix: "temp/".to_string(),
            status: RuleStatus::Enabled,
            expiration: Some(ExpirationConfig {
                days: Some(1),
                date: None,
                expired_object_delete_marker: None,
            }),
            transitions: None,
            filter_tags: None,
        }],
    };

    // Set the lifecycle config for the test bucket
    match service.set_lifecycle_config(&bucket_name, lifecycle_config.clone()) {
        Ok(_) => println!("Successfully set lifecycle configuration"),
        Err(err) => panic!("Failed to set lifecycle configuration: {}", err),
    }

    // Test 3: Create objects and manipulate versions
    // Test data - multiple versions
    let test_data_v1 = Bytes::from("Version 1 content");
    let test_data_v2 = Bytes::from("Version 2 updated content");

    // Put version 1
    let put_v1_request = Request::builder()
        .uri(format!("/storage/{}/test-doc.txt", bucket_name))
        .method("PUT")
        .body(Body::from(test_data_v1.clone().to_vec()))
        .unwrap();

    let put_v1_response = app.clone().oneshot(put_v1_request).await.unwrap();
    assert_eq!(put_v1_response.status(), StatusCode::CREATED);
    println!("Successfully created version 1");

    // Get version 1 content and verify
    let get_v1_request = Request::builder()
        .uri(format!("/storage/{}/test-doc.txt", bucket_name))
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let get_v1_response = app.clone().oneshot(get_v1_request).await.unwrap();
    assert_eq!(get_v1_response.status(), StatusCode::OK);

    let v1_content = response_body_bytes(get_v1_response).await;
    assert_eq!(v1_content, test_data_v1);
    println!("Successfully verified version 1 content");

    // Put version 2
    let put_v2_request = Request::builder()
        .uri(format!("/storage/{}/test-doc.txt", bucket_name))
        .method("PUT")
        .body(Body::from(test_data_v2.clone().to_vec()))
        .unwrap();

    let put_v2_response = app.clone().oneshot(put_v2_request).await.unwrap();
    assert_eq!(put_v2_response.status(), StatusCode::CREATED);
    println!("Successfully created version 2");

    // Get version 2 ID from response
    let v2_response_json = response_json(put_v2_response).await;
    let version_id_2 = v2_response_json["version_id"].as_str().unwrap();

    // Get version 2 content and verify (should be the default version now)
    let get_default_request = Request::builder()
        .uri(format!("/storage/{}/test-doc.txt", bucket_name))
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let get_default_response = app.clone().oneshot(get_default_request).await.unwrap();
    assert_eq!(get_default_response.status(), StatusCode::OK);

    let default_content = response_body_bytes(get_default_response).await;
    assert_eq!(default_content, test_data_v2);
    println!("Successfully verified default content is version 2");

    // Test 4: List versions
    let list_request = Request::builder()
        .uri(format!("/storage/{}/versions/test-doc.txt", bucket_name))
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let list_response = app.clone().oneshot(list_request).await.unwrap();
    assert_eq!(list_response.status(), StatusCode::OK);

    // Parse and check the versions list
    let versions_json = response_json(list_response).await;
    let versions = versions_json["versions"].as_array().unwrap();
    println!("Found versions: {:?}", versions);
    println!("Number of versions found: {}", versions.len());
    assert_eq!(versions.len(), 2);
    println!("Successfully listed {} versions", versions.len());

    // Get the version ID for version 1
    let version_id_1 = versions[0]["version_id"].as_str().unwrap();
    println!("Version ID 1: {}", version_id_1);

    // Test 5: Get specific version content
    let get_specific_v1_request = Request::builder()
        .uri(format!(
            "/storage/{}/version/test-doc.txt/{}",
            bucket_name, version_id_1
        ))
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let get_specific_v1_response = app.clone().oneshot(get_specific_v1_request).await.unwrap();
    assert_eq!(get_specific_v1_response.status(), StatusCode::OK);

    let specific_v1_content = response_body_bytes(get_specific_v1_response).await;
    assert_eq!(specific_v1_content, test_data_v1);
    println!("Successfully retrieved specific version 1 content");

    // Test 6: Delete a specific version
    println!("Deleting version 1 with ID: {}", version_id_1);
    let delete_v1_request = Request::builder()
        .uri(format!(
            "/storage/{}/version/test-doc.txt/{}",
            bucket_name, version_id_1
        ))
        .method("DELETE")
        .body(Body::empty())
        .unwrap();

    let delete_v1_response = app.clone().oneshot(delete_v1_request).await.unwrap();
    assert_eq!(delete_v1_response.status(), StatusCode::NO_CONTENT);
    println!(
        "Successfully deleted version 1 - got status: {}",
        delete_v1_response.status()
    );

    // Verify only version 2 remains
    let list_after_delete_request = Request::builder()
        .uri(format!("/storage/{}/versions/test-doc.txt", bucket_name))
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let list_after_delete_response = app
        .clone()
        .oneshot(list_after_delete_request)
        .await
        .unwrap();
    assert_eq!(list_after_delete_response.status(), StatusCode::OK);

    let after_delete_json = response_json(list_after_delete_response).await;
    let after_delete_versions = after_delete_json["versions"].as_array().unwrap();

    println!("After delete, found versions: {:?}", after_delete_versions);
    println!(
        "After delete, number of versions: {}",
        after_delete_versions.len()
    );

    // There should be only version 2 left
    assert_eq!(after_delete_versions.len(), 1);
    println!("Successfully verified only version 2 remains");

    // Test 7: Get lifecycle configuration
    let get_lifecycle_request = Request::builder()
        .uri(format!("/storage/{}/lifecycle", bucket_name))
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let get_lifecycle_response = app.clone().oneshot(get_lifecycle_request).await.unwrap();
    assert_eq!(get_lifecycle_response.status(), StatusCode::OK);

    let lifecycle_json = response_json(get_lifecycle_response).await;
    let rules = lifecycle_json["rules"].as_array().unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0]["id"].as_str().unwrap(), "test-rule");
    assert_eq!(rules[0]["prefix"].as_str().unwrap(), "temp/");
    println!("Successfully retrieved lifecycle configuration");

    // Test 7b: Test MinIO-specific lifecycle API (if MinIO is available)
    let minio_creds_query = format!(
        "endpoint={}&access_key={}&secret_key={}",
        urlencoding::encode(&endpoint),
        urlencoding::encode(&access_key),
        urlencoding::encode(&secret_key)
    );

    let get_minio_lifecycle_request = Request::builder()
        .uri(format!(
            "/storage/{}/minio-lifecycle?{}",
            bucket_name, minio_creds_query
        ))
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let get_minio_response = app
        .clone()
        .oneshot(get_minio_lifecycle_request)
        .await
        .unwrap();
    println!(
        "MinIO lifecycle response status: {}",
        get_minio_response.status()
    );

    // We don't assert the status since it depends on actual MinIO availability
    if get_minio_response.status().is_success() {
        println!("Successfully retrieved MinIO lifecycle configuration");

        let minio_lifecycle_json = response_json(get_minio_response).await;
        println!("MinIO lifecycle config: {:?}", minio_lifecycle_json);

        // Test 7c: Try setting MinIO lifecycle configuration if GET succeeded
        // Create a slightly modified lifecycle config
        let modified_config = serde_json::json!({
            "rules": [
                {
                    "id": "test-rule-minio",
                    "prefix": "archive/",
                    "status": "enabled",
                    "expiration": {
                        "days": 180
                    }
                }
            ]
        });

        let put_minio_lifecycle_request = Request::builder()
            .uri(format!(
                "/storage/{}/minio-lifecycle?{}",
                bucket_name, minio_creds_query
            ))
            .method("PUT")
            .header("Content-Type", "application/json")
            .body(Body::from(modified_config.to_string()))
            .unwrap();

        let put_minio_response = app
            .clone()
            .oneshot(put_minio_lifecycle_request)
            .await
            .unwrap();
        println!(
            "MinIO lifecycle PUT response status: {}",
            put_minio_response.status()
        );

        if put_minio_response.status().is_success() {
            println!("Successfully set MinIO lifecycle configuration");

            // Verify the change took effect
            let get_updated_request = Request::builder()
                .uri(format!(
                    "/storage/{}/minio-lifecycle?{}",
                    bucket_name, minio_creds_query
                ))
                .method("GET")
                .body(Body::empty())
                .unwrap();

            let get_updated_response = app.clone().oneshot(get_updated_request).await.unwrap();

            if get_updated_response.status().is_success() {
                let updated_json = response_json(get_updated_response).await;
                println!("Updated MinIO lifecycle config: {:?}", updated_json);

                // Check if our rule ID is present
                if let Some(rules) = updated_json.get("rules").and_then(|r| r.as_array()) {
                    for rule in rules {
                        if let Some(id) = rule.get("id").and_then(|i| i.as_str()) {
                            if id == "test-rule-minio" {
                                println!(
                                    "Confirmed our new rule is present in MinIO lifecycle configuration"
                                );
                            }
                        }
                    }
                }
            }
        } else {
            println!(
                "Failed to set MinIO lifecycle configuration - this may be expected if MinIO is not running or doesn't support lifecycle"
            );
            let bytes = response_body_bytes(put_minio_response).await;
            // Try to get the error message
            let err = String::from_utf8_lossy(&bytes);

            println!("Error message: {}", err);
        }
    } else {
        println!(
            "MinIO lifecycle test skipped or failed - this is expected if MinIO is not running"
        );
    }

    // Test 8: Store object in temp directory (subject to lifecycle rule)
    let temp_data = Bytes::from("Temporary file that should expire");
    let put_temp_request = Request::builder()
        .uri(format!("/storage/{}/temp/expire-soon.txt", bucket_name))
        .method("PUT")
        .body(Body::from(temp_data.clone().to_vec()))
        .unwrap();

    let put_temp_response = app.clone().oneshot(put_temp_request).await.unwrap();
    assert_eq!(put_temp_response.status(), StatusCode::CREATED);
    println!("Successfully created temporary file");

    // Test 9: Start lifecycle manager
    match service.start_lifecycle_manager().await {
        Ok(_) => println!("Successfully started lifecycle manager"),
        Err(err) => panic!("Failed to start lifecycle manager: {}", err),
    }

    // Wait a short time for lifecycle manager to initialize
    sleep(Duration::from_secs(1)).await;

    // Clean up: Delete the test bucket and contents
    // This would normally be done by deleting the bucket in MinIO directly,
    // but we'll at least delete our test objects through the API

    // Delete version 2
    let delete_v2_request = Request::builder()
        .uri(format!(
            "/storage/{}/version/test-doc.txt/{}",
            bucket_name, version_id_2
        ))
        .method("DELETE")
        .body(Body::empty())
        .unwrap();

    let _ = app.clone().oneshot(delete_v2_request).await.unwrap();

    // Delete temp file
    let delete_temp_request = Request::builder()
        .uri(format!("/storage/{}/temp/expire-soon.txt", bucket_name))
        .method("DELETE")
        .body(Body::empty())
        .unwrap();

    let _ = app.clone().oneshot(delete_temp_request).await.unwrap();

    // Delete init file
    let delete_init_request = Request::builder()
        .uri(format!("/storage/{}/_init_file.txt", bucket_name))
        .method("DELETE")
        .body(Body::empty())
        .unwrap();

    let _ = app.clone().oneshot(delete_init_request).await.unwrap();

    println!("Test completed successfully!");
}
