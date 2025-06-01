use bytes::Bytes;
use object_store::memory::InMemory;
use object_store::{ObjectStore, path::Path};
use object_store_bridge::versioning::VersionedStore;
use std::sync::Arc;

#[tokio::test]
async fn test_versioned_store_basic() {
    // Create an in-memory store from the object_store crate
    let inner_store = InMemory::new();

    // Wrap it in our VersionedStore
    let versioned_store = VersionedStore::new(inner_store);

    // Test path
    let path = Path::from("test.txt");
    let content = Bytes::from("test content");

    // Put the object
    versioned_store
        .put(&path, content.clone().into())
        .await
        .unwrap();

    // Get the object back
    let result = versioned_store.get(&path).await.unwrap();

    // Get the content as bytes for comparison
    let result_bytes = result.bytes().await.unwrap();

    // Verify content matches
    assert_eq!(result_bytes, content);

    // List versions
    let versions = versioned_store.list_versions(&path).unwrap();
    assert_eq!(versions.len(), 1, "Should have 1 version");

    // Add another version
    let content_v2 = Bytes::from("updated content");
    versioned_store
        .put(&path, content_v2.clone().into())
        .await
        .unwrap();

    // List versions again
    let versions = versioned_store.list_versions(&path).unwrap();
    assert_eq!(versions.len(), 2, "Should have 2 versions");

    // Get specific version
    let v1_id = &versions[0].version_id;
    let v1_content = versioned_store.get_version(&path, v1_id).await.unwrap();
    assert_eq!(v1_content, content);
}
