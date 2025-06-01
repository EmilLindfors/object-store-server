use chrono::{Duration, Utc};
use object_store_server::{
    BucketName, LifecycleConfiguration, LifecycleRule, create_in_memory_app,
    domain::models::{
        Filter,
        lifecycle::{RuleStatus, StorageClass},
    },
    ports::services::LifecycleService,
};

#[tokio::test]
async fn test_lifecycle_config() {
    // Create application services with in-memory storage
    let services = create_in_memory_app().await.unwrap();

    // Create a test configuration
    let bucket = BucketName::new("test-bucket".to_string()).unwrap();
    let mut filter = Filter::new();
    filter.prefix = Some("docs/".to_string());

    let config = LifecycleConfiguration {
        bucket: bucket.clone(),
        rules: vec![LifecycleRule {
            id: "test-rule".to_string(),
            status: RuleStatus::Enabled,
            filter,
            expiration_days: Some(30),
            ..Default::default()
        }],
    };

    // Set the configuration
    services
        .lifecycle_service
        .set_lifecycle_configuration(&bucket, config.clone())
        .await
        .unwrap();

    // Get the configuration back
    let retrieved_config = services
        .lifecycle_service
        .get_lifecycle_configuration(&bucket)
        .await
        .unwrap();

    // Verify the configuration was stored correctly
    let config = retrieved_config.expect("Configuration should exist");
    assert_eq!(config.rules.len(), 1);
    assert_eq!(config.rules[0].id, "test-rule");
    assert_eq!(config.rules[0].filter.prefix.as_deref(), Some("docs/"));
    assert_eq!(config.rules[0].expiration_days, Some(30));
}

#[tokio::test]
async fn test_multiple_lifecycle_rules() {
    // Create application services with in-memory storage
    let services = create_in_memory_app().await.unwrap();

    // Create a configuration with multiple rules
    let bucket = BucketName::new("multi-rule-bucket".to_string()).unwrap();

    let mut filter1 = Filter::new();
    filter1.prefix = Some("logs/".to_string());

    let mut filter2 = Filter::new();
    filter2.prefix = Some("temp/".to_string());

    let config = LifecycleConfiguration {
        bucket: bucket.clone(),
        rules: vec![
            LifecycleRule {
                id: "archive-old-logs".to_string(),
                status: RuleStatus::Enabled,
                filter: filter1,
                expiration_days: Some(90),
                transition_days: Some(30),
                transition_storage_class: Some(StorageClass::InfrequentAccess),
                ..Default::default()
            },
            LifecycleRule {
                id: "delete-temp-files".to_string(),
                status: RuleStatus::Enabled,
                filter: filter2,
                expiration_days: Some(7),
                ..Default::default()
            },
        ],
    };

    // Set the configuration
    services
        .lifecycle_service
        .set_lifecycle_configuration(&bucket, config.clone())
        .await
        .unwrap();

    // Get the configuration back
    let retrieved_config = services
        .lifecycle_service
        .get_lifecycle_configuration(&bucket)
        .await
        .unwrap();

    // Verify both rules were stored
    let config = retrieved_config.expect("Configuration should exist");
    assert_eq!(config.rules.len(), 2);

    // Find and verify the archive rule
    let archive_rule = config
        .rules
        .iter()
        .find(|r| r.id == "archive-old-logs")
        .expect("Should find archive rule");
    assert_eq!(archive_rule.filter.prefix.as_deref(), Some("logs/"));
    assert_eq!(archive_rule.expiration_days, Some(90));
    assert_eq!(archive_rule.transition_days, Some(30));
    assert_eq!(
        archive_rule.transition_storage_class,
        Some(StorageClass::InfrequentAccess)
    );

    // Find and verify the temp files rule
    let temp_rule = config
        .rules
        .iter()
        .find(|r| r.id == "delete-temp-files")
        .expect("Should find temp rule");
    assert_eq!(temp_rule.filter.prefix.as_deref(), Some("temp/"));
    assert_eq!(temp_rule.expiration_days, Some(7));
}

#[tokio::test]
async fn test_lifecycle_rule_modification() {
    // Create application services with in-memory storage
    let services = create_in_memory_app().await.unwrap();

    let bucket = BucketName::new("mod-bucket".to_string()).unwrap();

    // Create initial configuration
    let mut filter = Filter::new();
    filter.prefix = Some("data/".to_string());

    let initial_config = LifecycleConfiguration {
        bucket: bucket.clone(),
        rules: vec![LifecycleRule {
            id: "initial-rule".to_string(),
            status: RuleStatus::Enabled,
            filter: filter.clone(),
            expiration_days: Some(30),
            ..Default::default()
        }],
    };

    // Set initial configuration
    services
        .lifecycle_service
        .set_lifecycle_configuration(&bucket, initial_config)
        .await
        .unwrap();

    // Modify the configuration
    let modified_config = LifecycleConfiguration {
        bucket: bucket.clone(),
        rules: vec![LifecycleRule {
            id: "initial-rule".to_string(),
            status: RuleStatus::Disabled, // Changed to disabled
            filter,
            expiration_days: Some(60), // Changed from 30 to 60
            ..Default::default()
        }],
    };

    // Update configuration
    services
        .lifecycle_service
        .set_lifecycle_configuration(&bucket, modified_config)
        .await
        .unwrap();

    // Verify modifications
    let retrieved_config = services
        .lifecycle_service
        .get_lifecycle_configuration(&bucket)
        .await
        .unwrap();

    let config = retrieved_config.expect("Configuration should exist");
    assert_eq!(config.rules.len(), 1);
    assert_eq!(config.rules[0].status, RuleStatus::Disabled);
    assert_eq!(config.rules[0].expiration_days, Some(60));
}

#[tokio::test]
async fn test_delete_lifecycle_configuration() {
    // Create application services with in-memory storage
    let services = create_in_memory_app().await.unwrap();

    let bucket = BucketName::new("delete-config-bucket".to_string()).unwrap();

    // Create and set a configuration
    let mut filter = Filter::new();
    filter.prefix = Some("deleteme/".to_string());

    let config = LifecycleConfiguration {
        bucket: bucket.clone(),
        rules: vec![LifecycleRule {
            id: "to-be-deleted".to_string(),
            status: RuleStatus::Enabled,
            filter,
            expiration_days: Some(1),
            ..Default::default()
        }],
    };

    services
        .lifecycle_service
        .set_lifecycle_configuration(&bucket, config)
        .await
        .unwrap();

    // Verify it exists
    let retrieved = services
        .lifecycle_service
        .get_lifecycle_configuration(&bucket)
        .await
        .unwrap();
    let config = retrieved.expect("Configuration should exist");
    assert_eq!(config.rules.len(), 1);

    // Delete the configuration
    services
        .lifecycle_service
        .delete_lifecycle_configuration(&bucket)
        .await
        .unwrap();

    // Verify it's gone - should return None or empty configuration
    let after_delete = services
        .lifecycle_service
        .get_lifecycle_configuration(&bucket)
        .await
        .unwrap();

    match after_delete {
        Some(config) => assert!(config.rules.is_empty()),
        None => {} // Also acceptable
    }
}

#[tokio::test]
async fn test_lifecycle_with_date_expiration() {
    // Create application services with in-memory storage
    let services = create_in_memory_app().await.unwrap();

    let bucket = BucketName::new("date-expiry-bucket".to_string()).unwrap();
    let future_date = Utc::now() + Duration::days(90);

    // Create configuration with date-based expiration
    let mut filter = Filter::new();
    filter.prefix = Some("archive/".to_string());

    let config = LifecycleConfiguration {
        bucket: bucket.clone(),
        rules: vec![LifecycleRule {
            id: "expire-by-date".to_string(),
            status: RuleStatus::Enabled,
            filter,
            expiration_date: Some(future_date),
            ..Default::default()
        }],
    };

    services
        .lifecycle_service
        .set_lifecycle_configuration(&bucket, config)
        .await
        .unwrap();

    // Verify the date was stored
    let retrieved = services
        .lifecycle_service
        .get_lifecycle_configuration(&bucket)
        .await
        .unwrap();

    let config = retrieved.expect("Configuration should exist");
    assert_eq!(config.rules[0].expiration_date, Some(future_date));
}

#[tokio::test]
async fn test_lifecycle_with_storage_transitions() {
    // Create application services with in-memory storage
    let services = create_in_memory_app().await.unwrap();

    let bucket = BucketName::new("transition-bucket".to_string()).unwrap();

    // Create configuration with storage class transitions
    let mut filter = Filter::new();
    filter.prefix = Some("cold-data/".to_string());

    let config = LifecycleConfiguration {
        bucket: bucket.clone(),
        rules: vec![LifecycleRule {
            id: "transition-to-glacier".to_string(),
            status: RuleStatus::Enabled,
            filter,
            transition_days: Some(30),
            transition_storage_class: Some(StorageClass::Glacier),
            ..Default::default()
        }],
    };

    services
        .lifecycle_service
        .set_lifecycle_configuration(&bucket, config)
        .await
        .unwrap();

    // Verify the transition was stored
    let retrieved = services
        .lifecycle_service
        .get_lifecycle_configuration(&bucket)
        .await
        .unwrap();

    let config = retrieved.expect("Configuration should exist");
    assert_eq!(config.rules[0].transition_days, Some(30));
    assert_eq!(
        config.rules[0].transition_storage_class,
        Some(StorageClass::Glacier)
    );
}
