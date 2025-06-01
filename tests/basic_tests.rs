use bytes::Bytes;
use object_store_server::{
    BucketName, ObjectKey, create_in_memory_app,
    domain::models::{CreateObjectRequest, GetObjectRequest},
    ports::services::ObjectService,
};
use std::collections::HashMap;

#[tokio::test]
async fn basic_put_get() {
    // Create application services with in-memory storage
    let services = create_in_memory_app().await.unwrap();

    // Put an object
    let key = ObjectKey::new("test.txt".to_string()).unwrap();
    let data = Bytes::from("hello world");

    let create_request = CreateObjectRequest {
        key: key.clone(),
        data: data.to_vec(),
        content_type: Some("text/plain".to_string()),
        custom_metadata: HashMap::new(),
    };

    let put_result = services
        .object_service
        .create_object(create_request)
        .await
        .unwrap();

    // Get it back
    let get_request = GetObjectRequest {
        key: key.clone(),
        version_id: None,
    };

    let result = services
        .object_service
        .get_object(get_request)
        .await
        .unwrap();

    assert_eq!(result.data, data.to_vec());
    assert_eq!(result.key, key);
    assert_eq!(result.metadata.content_length, data.len() as u64);
}

#[tokio::test]
async fn basic_delete() {
    // Create application services with in-memory storage
    let services = create_in_memory_app().await.unwrap();

    // Put an object
    let key = ObjectKey::new("to_delete.txt".to_string()).unwrap();
    let data = Bytes::from("delete me");

    let create_request = CreateObjectRequest {
        key: key.clone(),
        data: data.to_vec(),
        content_type: None,
        custom_metadata: HashMap::new(),
    };

    services
        .object_service
        .create_object(create_request)
        .await
        .unwrap();

    // Verify it exists
    let get_request = GetObjectRequest {
        key: key.clone(),
        version_id: None,
    };

    let exists = services
        .object_service
        .get_object(get_request)
        .await
        .is_ok();
    assert!(exists);

    // Delete it
    services.object_service.delete_object(&key).await.unwrap();

    // Verify it's gone
    let get_request = GetObjectRequest {
        key: key.clone(),
        version_id: None,
    };

    let result = services.object_service.get_object(get_request).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_list_objects() {
    // Create application services with in-memory storage
    let services = create_in_memory_app().await.unwrap();

    // Put multiple objects
    let keys = vec!["file1.txt", "file2.txt", "dir/file3.txt"];
    for key_str in &keys {
        let key = ObjectKey::new(key_str.to_string()).unwrap();
        let data = Bytes::from(format!("content of {}", key_str));

        let create_request = CreateObjectRequest {
            key,
            data: data.to_vec(),
            content_type: None,
            custom_metadata: HashMap::new(),
        };

        services
            .object_service
            .create_object(create_request)
            .await
            .unwrap();
    }

    // List all objects
    let objects = services
        .object_service
        .list_objects(None, None)
        .await
        .unwrap();

    assert_eq!(objects.len(), 3);

    // Verify all keys are present
    let object_keys: Vec<String> = objects.iter().map(|o| o.key.as_str().to_string()).collect();

    for key in keys {
        assert!(object_keys.contains(&key.to_string()));
    }
}

#[tokio::test]
async fn test_object_exists() {
    // Create application services with in-memory storage
    let services = create_in_memory_app().await.unwrap();

    // Create an object
    let key = ObjectKey::new("exists-test.txt".to_string()).unwrap();
    let data = Bytes::from("test content");

    let create_request = CreateObjectRequest {
        key: key.clone(),
        data: data.to_vec(),
        content_type: None,
        custom_metadata: HashMap::new(),
    };

    // Should not exist initially
    assert!(!services.object_service.object_exists(&key).await.unwrap());

    // Create the object
    services
        .object_service
        .create_object(create_request)
        .await
        .unwrap();

    // Should exist now
    assert!(services.object_service.object_exists(&key).await.unwrap());

    // Delete it
    services.object_service.delete_object(&key).await.unwrap();

    // Should not exist anymore
    assert!(!services.object_service.object_exists(&key).await.unwrap());
}

#[tokio::test]
async fn test_copy_object() {
    // Create application services with in-memory storage
    let services = create_in_memory_app().await.unwrap();

    // Create source object
    let source_key = ObjectKey::new("source.txt".to_string()).unwrap();
    let dest_key = ObjectKey::new("destination.txt".to_string()).unwrap();
    let data = Bytes::from("copy me");

    let create_request = CreateObjectRequest {
        key: source_key.clone(),
        data: data.to_vec(),
        content_type: Some("text/plain".to_string()),
        custom_metadata: HashMap::new(),
    };

    services
        .object_service
        .create_object(create_request)
        .await
        .unwrap();

    // Copy the object
    let copied = services
        .object_service
        .copy_object(&source_key, &dest_key)
        .await
        .unwrap();

    assert_eq!(copied.key, dest_key);
    assert_eq!(copied.data, data.to_vec());

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
async fn test_object_size() {
    // Create application services with in-memory storage
    let services = create_in_memory_app().await.unwrap();

    // Create an object
    let key = ObjectKey::new("size-test.txt".to_string()).unwrap();
    let data = Bytes::from("test content for size");
    let expected_size = data.len() as u64;

    let create_request = CreateObjectRequest {
        key: key.clone(),
        data: data.to_vec(),
        content_type: None,
        custom_metadata: HashMap::new(),
    };

    services
        .object_service
        .create_object(create_request)
        .await
        .unwrap();

    // Get object size
    let size = services.object_service.get_object_size(&key).await.unwrap();

    assert_eq!(size, expected_size);
}
