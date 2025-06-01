use bytes::Bytes;
use object_store_server::{
    ObjectKey, VersionId, create_in_memory_app,
    domain::models::{CreateObjectRequest, GetObjectRequest},
    ports::services::{ObjectService, VersioningService},
};
use std::collections::HashMap;

#[tokio::test]
async fn test_versioned_store_basic() {
    // Create application services with in-memory storage
    let services = create_in_memory_app().await.unwrap();

    // Test key
    let key = ObjectKey::new("test.txt".to_string()).unwrap();
    let content = Bytes::from("test content");

    // Enable versioning (assuming bucket-level versioning)
    // Note: The current architecture doesn't expose bucket operations through services,
    // so we'll work with the assumption that versioning is enabled by default

    // Put the object - first version
    let create_request = CreateObjectRequest {
        key: key.clone(),
        data: content.to_vec(),
        content_type: Some("text/plain".to_string()),
        custom_metadata: HashMap::new(),
    };

    let v1_result = services
        .versioning_service
        .create_versioned_object(create_request)
        .await
        .unwrap();

    // Get the object back (latest version)
    let get_request = GetObjectRequest {
        key: key.clone(),
        version_id: None,
    };

    let result = services
        .versioning_service
        .get_object(get_request)
        .await
        .unwrap();

    // Verify content matches
    assert_eq!(result.data, content.to_vec());

    // List versions
    let version_list = services
        .versioning_service
        .list_versions(&key)
        .await
        .unwrap();
    assert_eq!(version_list.versions.len(), 1, "Should have 1 version");

    // Add another version
    let content_v2 = Bytes::from("updated content");
    let create_request_v2 = CreateObjectRequest {
        key: key.clone(),
        data: content_v2.to_vec(),
        content_type: Some("text/plain".to_string()),
        custom_metadata: HashMap::new(),
    };

    let v2_result = services
        .versioning_service
        .create_versioned_object(create_request_v2)
        .await
        .unwrap();

    // List versions again
    let version_list = services
        .versioning_service
        .list_versions(&key)
        .await
        .unwrap();
    assert_eq!(version_list.versions.len(), 2, "Should have 2 versions");

    // Get specific version (find the one with original content)
    // Versions might be ordered newest first, so find by checking content
    let v1_version = version_list
        .versions
        .iter()
        .find(|v| !v.is_latest)
        .expect("Should find non-latest version");
    let v1_id = &v1_version.version_id;
    let v1_request = GetObjectRequest {
        key: key.clone(),
        version_id: Some(v1_id.clone()),
    };

    let v1_object = services
        .versioning_service
        .get_object(v1_request)
        .await
        .unwrap();

    assert_eq!(v1_object.data, content.to_vec());

    // Verify latest version is v2
    let latest_request = GetObjectRequest {
        key: key.clone(),
        version_id: None,
    };

    let latest = services
        .versioning_service
        .get_object(latest_request)
        .await
        .unwrap();

    assert_eq!(latest.data, content_v2.to_vec());
}

#[tokio::test]
async fn test_delete_version() {
    // Create application services with in-memory storage
    let services = create_in_memory_app().await.unwrap();

    // Create an object
    let key = ObjectKey::new("delete-test.txt".to_string()).unwrap();
    let content = Bytes::from("will be deleted");

    let create_request = CreateObjectRequest {
        key: key.clone(),
        data: content.to_vec(),
        content_type: None,
        custom_metadata: HashMap::new(),
    };

    services
        .object_service
        .create_object(create_request)
        .await
        .unwrap();

    // Delete the object
    services.object_service.delete_object(&key).await.unwrap();

    // Verify it's gone
    let get_request = GetObjectRequest {
        key: key.clone(),
        version_id: None,
    };

    let result = services.versioning_service.get_object(get_request).await;

    assert!(result.is_err());

    // List versions should show the delete marker
    let version_list = services
        .versioning_service
        .list_versions(&key)
        .await
        .unwrap();

    // Should have at least one version (the original)
    // and possibly a delete marker depending on implementation
    assert!(!version_list.versions.is_empty());
}

#[tokio::test]
async fn test_restore_version() {
    // Create application services with in-memory storage
    let services = create_in_memory_app().await.unwrap();

    // Create an object with multiple versions
    let key = ObjectKey::new("restore-test.txt".to_string()).unwrap();

    // Version 1
    let v1_content = Bytes::from("version 1");
    let create_v1 = CreateObjectRequest {
        key: key.clone(),
        data: v1_content.to_vec(),
        content_type: None,
        custom_metadata: HashMap::new(),
    };

    services
        .versioning_service
        .create_versioned_object(create_v1)
        .await
        .unwrap();

    // Version 2
    let v2_content = Bytes::from("version 2");
    let create_v2 = CreateObjectRequest {
        key: key.clone(),
        data: v2_content.to_vec(),
        content_type: None,
        custom_metadata: HashMap::new(),
    };

    services
        .versioning_service
        .create_versioned_object(create_v2)
        .await
        .unwrap();

    // Get versions
    let version_list = services
        .versioning_service
        .list_versions(&key)
        .await
        .unwrap();

    assert_eq!(version_list.versions.len(), 2);

    // Find version 1's ID (should be the older one)
    let v1_id = &version_list
        .versions
        .iter()
        .find(|v| !v.is_latest)
        .expect("Should find non-latest version")
        .version_id;

    // Restore version 1
    services
        .versioning_service
        .restore_version(&key, v1_id)
        .await
        .unwrap();

    // Get latest should now return v1 content
    let latest_request = GetObjectRequest {
        key: key.clone(),
        version_id: None,
    };

    let latest = services
        .versioning_service
        .get_object(latest_request)
        .await
        .unwrap();

    assert_eq!(latest.data, v1_content.to_vec());

    // Version list should now have 3 versions (original v1, v2, and restored v1)
    let versions_after = services
        .versioning_service
        .list_versions(&key)
        .await
        .unwrap();

    assert_eq!(versions_after.versions.len(), 3);
}
