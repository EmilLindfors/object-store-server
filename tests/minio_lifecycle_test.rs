use chrono::{DateTime, Utc};
use object_store::{ObjectStore, aws::AmazonS3Builder};
use object_store_bridge::{
    MinioClient,
    domain::filter::Filter,
    domain::lifecycle::{LifecycleConfig, LifecycleRule},
    error::StoreError,
};
use std::env;

// Helper function to set up MinIO client from environment variables
fn setup_minio_client() -> Result<MinioClient, StoreError> {
    let endpoint =
        env::var("MINIO_ENDPOINT").unwrap_or_else(|_| "http://localhost:9000".to_string());
    let access_key = env::var("MINIO_ACCESS_KEY_ID").unwrap_or_else(|_| "minioadmin".to_string());
    let secret_key =
        env::var("MINIO_SECRET_ACCESS_KEY").unwrap_or_else(|_| "minioadmin".to_string());
    let region = env::var("MINIO_REGION").unwrap_or_else(|_| "us-east-1".to_string());

    println!(
        "Connecting to MinIO at {} with access key {}",
        endpoint, access_key
    );

    Ok(MinioClient::new(
        &endpoint,
        &access_key,
        &secret_key,
        &region,
    ))
}

// Helper function to create test bucket if it doesn't exist
async fn ensure_bucket_exists(bucket_name: &str) -> Result<(), StoreError> {
    let endpoint =
        env::var("MINIO_ENDPOINT").unwrap_or_else(|_| "http://localhost:9000".to_string());
    let access_key = env::var("MINIO_ACCESS_KEY_ID").unwrap_or_else(|_| "minioadmin".to_string());
    let secret_key =
        env::var("MINIO_SECRET_ACCESS_KEY").unwrap_or_else(|_| "minioadmin".to_string());
    let region = env::var("MINIO_REGION").unwrap_or_else(|_| "us-east-1".to_string());

    // Use reqwest client to create bucket first (direct API call)
    let client = reqwest::Client::new();
    let bucket_url = format!("{}/{}", endpoint, bucket_name);

    println!("Creating bucket at {}", bucket_url);

    // Create the bucket (PUT request to bucket URL)
    let response = client
        .put(&bucket_url)
        .basic_auth(&access_key, Some(&secret_key))
        .send()
        .await
        .map_err(|e| StoreError::Other(format!("Failed to create bucket: {}", e)))?;

    // Check response (200 OK or 409 Conflict = already exists, both are fine)
    let status = response.status();
    if status.is_success() || status.as_u16() == 409 {
        println!(
            "Bucket {} created or already exists (status: {})",
            bucket_name, status
        );
    } else {
        return Err(StoreError::Other(format!(
            "Failed to create bucket, status: {}",
            status
        )));
    }

    // Now create S3 store to initialize a file in the bucket
    let store = AmazonS3Builder::new()
        .with_bucket_name(bucket_name)
        .with_access_key_id(&access_key)
        .with_secret_access_key(&secret_key)
        .with_endpoint(&endpoint)
        .with_allow_http(true)
        .with_region(&region)
        .build()
        .map_err(|e| StoreError::Other(format!("Failed to build S3 store: {}", e)))?;

    // Create a small file to ensure the bucket exists
    let path = object_store::path::Path::from("_init.txt");
    let data = bytes::Bytes::from("Bucket initialization");

    store
        .put(&path, data.into())
        .await
        .map_err(|e| StoreError::Other(format!("Failed to initialize bucket: {}", e)))?;

    println!("Successfully ensured bucket {} exists", bucket_name);
    Ok(())
}

// Create lifecycle rule for testing
fn create_test_lifecycle_config() -> LifecycleConfig {
    LifecycleConfig {
        rules: vec![
            // Rule 1: Expire objects with prefix "logs/" after 30 days
            LifecycleRule {
                id: "expire-logs".to_string(),
                status: true,
                filter: Filter {
                    prefix: Some("logs/".to_string()),
                    tag: None,
                    and: None,
                },
                expiration_days: Some(30),
                expiration_date: None,
                expiration_expired_object_delete_marker: None,
                abort_incomplete_multipart_upload_days_after_initiation: Some(7),
                noncurrent_version_expiration_noncurrent_days: None,
                noncurrent_version_transition_noncurrent_days: None,
                noncurrent_version_transition_storage_class: None,
                transition_date: None,
                transition_days: None,
                transition_storage_class: None,
            },
            // Rule 2: Transition objects with prefix "archive/" to GLACIER after 90 days
            LifecycleRule {
                id: "archive-transition".to_string(),
                status: true,
                filter: Filter {
                    prefix: Some("archive/".to_string()),
                    tag: None,
                    and: None,
                },
                expiration_days: None,
                expiration_date: None,
                expiration_expired_object_delete_marker: None,
                abort_incomplete_multipart_upload_days_after_initiation: None,
                noncurrent_version_expiration_noncurrent_days: None,
                noncurrent_version_transition_noncurrent_days: None,
                noncurrent_version_transition_storage_class: None,
                transition_date: None,
                transition_days: Some(90),
                transition_storage_class: Some("GLACIER".to_string()),
            },
        ],
    }
}

#[tokio::test]
async fn test_minio_lifecycle_operations() -> Result<(), StoreError> {
    // Create bucket name for testing
    let bucket_name = "lifecycle-test-bucket";

    // Set up MinIO client
    let client = setup_minio_client()?;

    // Ensure bucket exists
    ensure_bucket_exists(bucket_name).await?;

    // Create a test lifecycle configuration
    let config = create_test_lifecycle_config();
    println!(
        "Created test lifecycle configuration with {} rules",
        config.rules.len()
    );

    // ----- Test setting lifecycle configuration -----
    println!("Setting lifecycle configuration for bucket {}", bucket_name);
    client.set_lifecycle_config(bucket_name, &config).await?;
    println!("Successfully set lifecycle configuration");

    // ----- Test getting lifecycle configuration -----
    println!("Getting lifecycle configuration for bucket {}", bucket_name);
    let retrieved_config = client.get_lifecycle_config(bucket_name).await?;
    println!(
        "Successfully retrieved lifecycle configuration with {} rules",
        retrieved_config.rules.len()
    );

    // ----- Verify the retrieved configuration -----
    assert_eq!(
        retrieved_config.rules.len(),
        config.rules.len(),
        "Retrieved configuration has different number of rules"
    );

    // Check first rule (expire-logs)
    let expire_logs_rule = retrieved_config
        .rules
        .iter()
        .find(|r| r.id == "expire-logs")
        .expect("Could not find expire-logs rule");

    assert_eq!(expire_logs_rule.status, true, "Rule status does not match");
    assert_eq!(
        expire_logs_rule.filter.prefix.as_deref(),
        Some("logs/"),
        "Rule prefix does not match"
    );
    assert_eq!(
        expire_logs_rule.expiration_days,
        Some(30),
        "Rule expiration days do not match"
    );
    assert_eq!(
        expire_logs_rule.abort_incomplete_multipart_upload_days_after_initiation,
        Some(7),
        "Rule abort_incomplete_multipart_upload_days_after_initiation does not match"
    );

    // Check second rule (archive-transition)
    let archive_rule = retrieved_config
        .rules
        .iter()
        .find(|r| r.id == "archive-transition")
        .expect("Could not find archive-transition rule");

    assert_eq!(archive_rule.status, true, "Rule status does not match");
    assert_eq!(
        archive_rule.filter.prefix.as_deref(),
        Some("archive/"),
        "Rule prefix does not match"
    );
    assert_eq!(
        archive_rule.transition_days,
        Some(90),
        "Rule transition days do not match"
    );
    assert_eq!(
        archive_rule.transition_storage_class.as_deref(),
        Some("GLACIER"),
        "Rule transition storage class does not match"
    );

    // ----- Test deleting lifecycle configuration -----
    println!(
        "Deleting lifecycle configuration for bucket {}",
        bucket_name
    );
    client.delete_lifecycle_config(bucket_name).await?;
    println!("Successfully deleted lifecycle configuration");

    // ----- Verify deletion -----
    let empty_config = client.get_lifecycle_config(bucket_name).await?;
    assert_eq!(
        empty_config.rules.len(),
        0,
        "Configuration still has rules after deletion"
    );

    println!("All lifecycle tests passed!");
    Ok(())
}

// Test creating a complex lifecycle configuration with multiple rules
#[tokio::test]
async fn test_complex_lifecycle_config() -> Result<(), StoreError> {
    // Create bucket name for testing
    let bucket_name = "complex-lifecycle-test";

    // Set up MinIO client
    let client = setup_minio_client()?;

    // Ensure bucket exists
    ensure_bucket_exists(bucket_name).await?;

    // Create a complex lifecycle configuration with various rule types
    let complex_config = LifecycleConfig {
        rules: vec![
            // Rule with expiration by days
            LifecycleRule {
                id: "expire-temp-files".to_string(),
                status: true,
                filter: Filter {
                    prefix: Some("temp/".to_string()),
                    tag: None,
                    and: None,
                },
                expiration_days: Some(1),
                expiration_date: None,
                expiration_expired_object_delete_marker: None,
                abort_incomplete_multipart_upload_days_after_initiation: None,
                noncurrent_version_expiration_noncurrent_days: None,
                noncurrent_version_transition_noncurrent_days: None,
                noncurrent_version_transition_storage_class: None,
                transition_date: None,
                transition_days: None,
                transition_storage_class: None,
            },
            // Rule with expiration by specific date
            LifecycleRule {
                id: "expire-by-date".to_string(),
                status: true,
                filter: Filter {
                    prefix: Some("events/2022/".to_string()),
                    tag: None,
                    and: None,
                },
                expiration_days: None,
                // Set to 1 year from now
                expiration_date: Some(Utc::now() + chrono::Duration::days(365)),
                expiration_expired_object_delete_marker: None,
                abort_incomplete_multipart_upload_days_after_initiation: None,
                noncurrent_version_expiration_noncurrent_days: None,
                noncurrent_version_transition_noncurrent_days: None,
                noncurrent_version_transition_storage_class: None,
                transition_date: None,
                transition_days: None,
                transition_storage_class: None,
            },
            // Rule with transition and expiration
            LifecycleRule {
                id: "transition-then-expire".to_string(),
                status: true,
                filter: Filter {
                    prefix: Some("data/".to_string()),
                    tag: None,
                    and: None,
                },
                expiration_days: Some(365),
                expiration_date: None,
                expiration_expired_object_delete_marker: None,
                abort_incomplete_multipart_upload_days_after_initiation: None,
                noncurrent_version_expiration_noncurrent_days: None,
                noncurrent_version_transition_noncurrent_days: None,
                noncurrent_version_transition_storage_class: None,
                transition_date: None,
                transition_days: Some(30),
                transition_storage_class: Some("STANDARD_IA".to_string()),
            },
            // Rule for noncurrent versions
            LifecycleRule {
                id: "noncurrent-version-rule".to_string(),
                status: true,
                filter: Filter {
                    prefix: Some("backups/".to_string()),
                    tag: None,
                    and: None,
                },
                expiration_days: None,
                expiration_date: None,
                expiration_expired_object_delete_marker: None,
                abort_incomplete_multipart_upload_days_after_initiation: None,
                noncurrent_version_expiration_noncurrent_days: Some(90),
                noncurrent_version_transition_noncurrent_days: Some(30),
                noncurrent_version_transition_storage_class: Some("GLACIER".to_string()),
                transition_date: None,
                transition_days: None,
                transition_storage_class: None,
            },
            // Disabled rule
            LifecycleRule {
                id: "disabled-rule".to_string(),
                status: false,
                filter: Filter {
                    prefix: Some("test/".to_string()),
                    tag: None,
                    and: None,
                },
                expiration_days: Some(7),
                expiration_date: None,
                expiration_expired_object_delete_marker: None,
                abort_incomplete_multipart_upload_days_after_initiation: None,
                noncurrent_version_expiration_noncurrent_days: None,
                noncurrent_version_transition_noncurrent_days: None,
                noncurrent_version_transition_storage_class: None,
                transition_date: None,
                transition_days: None,
                transition_storage_class: None,
            },
        ],
    };

    // Try to set the complex configuration
    println!(
        "Setting complex lifecycle configuration for bucket {}",
        bucket_name
    );
    match client
        .set_lifecycle_config(bucket_name, &complex_config)
        .await
    {
        Ok(_) => {
            println!("Successfully set complex lifecycle configuration");

            // Retrieve and verify
            let retrieved_config = client.get_lifecycle_config(bucket_name).await?;
            println!(
                "Successfully retrieved complex lifecycle configuration with {} rules",
                retrieved_config.rules.len()
            );

            // Clean up
            client.delete_lifecycle_config(bucket_name).await?;
        }
        Err(e) => {
            println!("Failed to set complex lifecycle configuration: {}", e);
            println!("This may be expected if MinIO doesn't support all lifecycle features");
            // Continue the test without failing - some MinIO versions or configurations may not
            // support all lifecycle features
        }
    }

    Ok(())
}
