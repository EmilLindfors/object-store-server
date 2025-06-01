use std::sync::Arc;

use bytes::Bytes;
use chrono::{Duration, Utc};
use object_store::memory::InMemory;
use object_store::{ObjectStore, path::Path};
use object_store_bridge::lifecycle::{
    ExpirationConfig, LifecycleConfiguration, LifecycleManager, LifecycleRule, RuleStatus,
};
use object_store_bridge::versioning::VersionedStore;

#[tokio::test]
async fn test_lifecycle_config() {
    // Create an in-memory store
    let inner_store = InMemory::new();
    let versioned_store = VersionedStore::new(inner_store);

    // Create a lifecycle manager
    let lifecycle_manager = LifecycleManager::new(Arc::new(versioned_store));

    // Create a test configuration
    let config = LifecycleConfiguration {
        rules: vec![LifecycleRule {
            id: "test-rule".to_string(),
            prefix: "docs/".to_string(),
            status: RuleStatus::Enabled,
            expiration: Some(ExpirationConfig {
                days: Some(30),
                date: None,
                expired_object_delete_marker: None,
            }),
            transitions: None,
            filter_tags: None,
        }],
    };

    // Set the configuration
    let bucket = "test-bucket";
    lifecycle_manager
        .set_lifecycle_config(bucket, config.clone())
        .unwrap();

    // Get the configuration back
    let retrieved_config = lifecycle_manager.get_lifecycle_config(bucket).unwrap();

    // Verify the configuration was stored correctly
    assert!(retrieved_config.is_some());
    let retrieved_config = retrieved_config.unwrap();
    assert_eq!(retrieved_config.rules.len(), 1);
    assert_eq!(retrieved_config.rules[0].id, "test-rule");
    assert_eq!(retrieved_config.rules[0].prefix, "docs/");

    // Verify expiration config
    let expiration = retrieved_config.rules[0].expiration.as_ref().unwrap();
    assert_eq!(expiration.days, Some(30));
}
