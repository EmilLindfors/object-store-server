use chrono::{Duration, Utc};
use object_store_server::{
    BucketName, LifecycleConfiguration, LifecycleRule, ObjectKey, create_minio_app,
    domain::models::{
        Filter,
        lifecycle::{RuleStatus, StorageClass},
    },
    ports::services::LifecycleService,
};

// Note: This test requires MinIO to be running with lifecycle support enabled
// MinIO must be started with: MINIO_API_ENABLE_LIFECYCLE=on
// Configure via environment variables:
// - MINIO_ENDPOINT (default: http://localhost:9000)
// - MINIO_ACCESS_KEY_ID (default: minioadmin)
// - MINIO_SECRET_ACCESS_KEY (default: minioadmin)

#[tokio::test]
#[ignore = "requires MinIO server with lifecycle support"]
async fn test_minio_lifecycle_api() {
    // Get MinIO configuration from environment
    let endpoint =
        std::env::var("MINIO_ENDPOINT").unwrap_or_else(|_| "http://localhost:9000".to_string());
    let access_key =
        std::env::var("MINIO_ACCESS_KEY_ID").unwrap_or_else(|_| "minioadmin".to_string());
    let secret_key =
        std::env::var("MINIO_SECRET_ACCESS_KEY").unwrap_or_else(|_| "minioadmin".to_string());
    let bucket =
        std::env::var("MINIO_BUCKET").unwrap_or_else(|_| "lifecycle-test-bucket".to_string());

    println!("Testing MinIO lifecycle API at {}", endpoint);

    // Create MinIO-backed application
    let services = create_minio_app(
        endpoint,
        bucket.clone(),
        access_key,
        secret_key,
        true, // use_ssl = false for http
    )
    .await
    .unwrap();

    let bucket_name = BucketName::new(bucket).unwrap();

    // Test 1: Set a simple expiration rule
    let mut expiration_filter = Filter::new();
    expiration_filter.prefix = Some("temp/".to_string());

    let expiration_config = LifecycleConfiguration {
        bucket: bucket_name.clone(),
        rules: vec![LifecycleRule {
            id: "expire-temp-files".to_string(),
            status: RuleStatus::Enabled,
            filter: expiration_filter,
            expiration_days: Some(7),
            ..Default::default()
        }],
    };

    services
        .lifecycle_service
        .set_lifecycle_configuration(&bucket_name, expiration_config)
        .await
        .unwrap();

    println!("✓ Set expiration rule successfully");

    // Test 2: Get and verify the configuration
    let retrieved = services
        .lifecycle_service
        .get_lifecycle_configuration(&bucket_name)
        .await
        .unwrap()
        .expect("Configuration should exist");

    assert_eq!(retrieved.rules.len(), 1);
    assert_eq!(retrieved.rules[0].id, "expire-temp-files");
    assert_eq!(retrieved.rules[0].expiration_days, Some(7));

    println!("✓ Retrieved configuration successfully");

    // Test 3: Update with multiple rules including transitions
    let config_with_transitions = LifecycleConfiguration {
        bucket: bucket_name.clone(),
        rules: vec![
            LifecycleRule {
                id: "expire-temp".to_string(),
                status: RuleStatus::Enabled,
                filter: {
                    let mut f = Filter::new();
                    f.prefix = Some("temp/".to_string());
                    f
                },
                expiration_days: Some(7),
                ..Default::default()
            },
            LifecycleRule {
                id: "archive-logs".to_string(),
                status: RuleStatus::Enabled,
                filter: {
                    let mut f = Filter::new();
                    f.prefix = Some("logs/".to_string());
                    f
                },
                transition_days: Some(30),
                transition_storage_class: Some(StorageClass::InfrequentAccess),
                expiration_days: Some(365),
                ..Default::default()
            },
            LifecycleRule {
                id: "delete-old-backups".to_string(),
                status: RuleStatus::Enabled,
                filter: {
                    let mut f = Filter::new();
                    f.prefix = Some("backups/".to_string());
                    f
                },
                expiration_days: Some(90),
                ..Default::default()
            },
        ],
    };

    services
        .lifecycle_service
        .set_lifecycle_configuration(&bucket_name, config_with_transitions)
        .await
        .unwrap();

    println!("✓ Updated configuration with multiple rules");

    // Test 4: Verify the updated configuration
    let updated = services
        .lifecycle_service
        .get_lifecycle_configuration(&bucket_name)
        .await
        .unwrap()
        .expect("Updated configuration should exist");

    assert_eq!(updated.rules.len(), 3);

    // Find and verify each rule
    let expire_rule = updated
        .rules
        .iter()
        .find(|r| r.id == "expire-temp")
        .expect("Should find expire-temp rule");
    assert_eq!(expire_rule.expiration_days, Some(7));

    let archive_rule = updated
        .rules
        .iter()
        .find(|r| r.id == "archive-logs")
        .expect("Should find archive-logs rule");
    assert_eq!(archive_rule.transition_days, Some(30));
    assert_eq!(archive_rule.expiration_days, Some(365));

    let backup_rule = updated
        .rules
        .iter()
        .find(|r| r.id == "delete-old-backups")
        .expect("Should find delete-old-backups rule");
    assert_eq!(backup_rule.expiration_days, Some(90));

    println!("✓ Verified all rules in updated configuration");

    // Test 5: Test rule management operations
    // Disable a specific rule
    services
        .lifecycle_service
        .disable_rule(&bucket_name, "expire-temp")
        .await
        .unwrap();

    println!("✓ Disabled specific rule");

    // Add a new rule
    let new_rule = LifecycleRule {
        id: "cleanup-uploads".to_string(),
        status: RuleStatus::Enabled,
        filter: {
            let mut f = Filter::new();
            f.prefix = Some("uploads/incomplete/".to_string());
            f
        },
        abort_incomplete_multipart_upload_days_after_initiation: Some(7),
        ..Default::default()
    };

    services
        .lifecycle_service
        .add_rule(&bucket_name, new_rule)
        .await
        .unwrap();

    println!("✓ Added new rule");

    // Remove a rule
    services
        .lifecycle_service
        .remove_rule(&bucket_name, "delete-old-backups")
        .await
        .unwrap();

    println!("✓ Removed specific rule");

    // Test 6: Test date-based expiration
    let future_date = Utc::now() + Duration::days(180);

    let date_based_rule = LifecycleRule {
        id: "expire-by-date".to_string(),
        status: RuleStatus::Enabled,
        filter: {
            let mut f = Filter::new();
            f.prefix = Some("archive/2023/".to_string());
            f
        },
        expiration_date: Some(future_date),
        ..Default::default()
    };

    services
        .lifecycle_service
        .add_rule(&bucket_name, date_based_rule)
        .await
        .unwrap();

    println!("✓ Added date-based expiration rule");

    // Test 7: Test filter with tags
    let mut tagged_filter = Filter::new();
    tagged_filter.prefix = Some("documents/".to_string());
    tagged_filter
        .tags
        .insert("department".to_string(), "finance".to_string());
    tagged_filter
        .tags
        .insert("type".to_string(), "report".to_string());

    let tagged_rule = LifecycleRule {
        id: "expire-finance-reports".to_string(),
        status: RuleStatus::Enabled,
        filter: tagged_filter,
        expiration_days: Some(365 * 7), // 7 years
        ..Default::default()
    };

    services
        .lifecycle_service
        .add_rule(&bucket_name, tagged_rule)
        .await
        .unwrap();

    println!("✓ Added rule with tag filters");

    // Test 8: Validate configuration
    let final_config = services
        .lifecycle_service
        .get_lifecycle_configuration(&bucket_name)
        .await
        .unwrap()
        .expect("Final configuration should exist");

    let validation = services
        .lifecycle_service
        .validate_configuration(&final_config)
        .await
        .unwrap();

    assert!(validation.is_valid);
    println!("✓ Configuration validation passed");

    // Test 9: Clean up - delete lifecycle configuration
    services
        .lifecycle_service
        .delete_lifecycle_configuration(&bucket_name)
        .await
        .unwrap();

    println!("✓ Deleted lifecycle configuration");

    // Verify deletion
    let after_delete = services
        .lifecycle_service
        .get_lifecycle_configuration(&bucket_name)
        .await
        .unwrap();

    assert!(after_delete.is_none() || after_delete.unwrap().rules.is_empty());

    println!("✓ Verified configuration deletion");
    println!("\n✅ All MinIO lifecycle API tests passed!");
}

#[tokio::test]
#[ignore = "requires MinIO server with lifecycle support"]
async fn test_minio_lifecycle_processing() {
    let endpoint =
        std::env::var("MINIO_ENDPOINT").unwrap_or_else(|_| "http://localhost:9000".to_string());
    let access_key =
        std::env::var("MINIO_ACCESS_KEY_ID").unwrap_or_else(|_| "minioadmin".to_string());
    let secret_key =
        std::env::var("MINIO_SECRET_ACCESS_KEY").unwrap_or_else(|_| "minioadmin".to_string());
    let bucket =
        std::env::var("MINIO_BUCKET").unwrap_or_else(|_| "lifecycle-processing-bucket".to_string());

    // Create MinIO-backed application
    let services = create_minio_app(endpoint, bucket.clone(), access_key, secret_key, true)
        .await
        .unwrap();

    let bucket_name = BucketName::new(bucket).unwrap();

    // Set up lifecycle rules
    let config = LifecycleConfiguration {
        bucket: bucket_name.clone(),
        rules: vec![LifecycleRule {
            id: "process-temp".to_string(),
            status: RuleStatus::Enabled,
            filter: {
                let mut f = Filter::new();
                f.prefix = Some("temp/".to_string());
                f
            },
            expiration_days: Some(1),
            ..Default::default()
        }],
    };

    services
        .lifecycle_service
        .set_lifecycle_configuration(&bucket_name, config)
        .await
        .unwrap();

    // Check processing status
    let status = services
        .lifecycle_service
        .get_processing_status(&bucket_name)
        .await
        .unwrap();

    println!(
        "Processing status: running={}, last_run={:?}",
        status.is_running, status.last_run
    );

    // Process lifecycle rules
    let result = services
        .lifecycle_service
        .process_bucket_lifecycle(&bucket_name)
        .await
        .unwrap();

    println!("Lifecycle processing results:");
    println!("  Objects processed: {}", result.objects_processed);
    println!("  Objects affected: {}", result.objects_affected);
    println!("  Actions applied: {}", result.actions_applied);
    println!("  Errors: {}", result.errors.len());
    println!("  Duration: {:?}", result.duration);

    // Clean up
    services
        .lifecycle_service
        .delete_lifecycle_configuration(&bucket_name)
        .await
        .unwrap();
}
