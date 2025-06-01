use async_trait::async_trait;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::{
    domain::{
        models::{LifecycleConfiguration, LifecycleRule},
        value_objects::{ObjectKey, BucketName, VersionId},
        errors::LifecycleResult,
    },
    ports::repositories::LifecycleRepository,
};

/// SQL-based implementation of LifecycleRepository using PostgreSQL
#[derive(Clone)]
pub struct SqlLifecycleRepository {
    pool: PgPool,
}

impl SqlLifecycleRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Initialize database tables
    pub async fn migrate(&self) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS lifecycle_configurations (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                bucket_name VARCHAR NOT NULL UNIQUE,
                configuration JSONB NOT NULL,
                enabled BOOLEAN NOT NULL DEFAULT true,
                created_at TIMESTAMPTZ DEFAULT NOW(),
                updated_at TIMESTAMPTZ DEFAULT NOW()
            );

            CREATE INDEX IF NOT EXISTS idx_lifecycle_bucket ON lifecycle_configurations(bucket_name);
            CREATE INDEX IF NOT EXISTS idx_lifecycle_enabled ON lifecycle_configurations(enabled);

            CREATE TABLE IF NOT EXISTS lifecycle_execution_log (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                bucket_name VARCHAR NOT NULL,
                object_key VARCHAR NOT NULL,
                rule_id VARCHAR NOT NULL,
                action VARCHAR NOT NULL,
                execution_time TIMESTAMPTZ DEFAULT NOW(),
                success BOOLEAN NOT NULL,
                error_message TEXT,
                details JSONB
            );

            CREATE INDEX IF NOT EXISTS idx_lifecycle_log_bucket ON lifecycle_execution_log(bucket_name);
            CREATE INDEX IF NOT EXISTS idx_lifecycle_log_time ON lifecycle_execution_log(execution_time);
            CREATE INDEX IF NOT EXISTS idx_lifecycle_log_object ON lifecycle_execution_log(object_key);
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}

#[async_trait]
impl LifecycleRepository for SqlLifecycleRepository {
    async fn save_configuration(
        &self,
        bucket: &BucketName,
        config: &LifecycleConfiguration,
    ) -> LifecycleResult<()> {
        let config_json = serde_json::to_value(config)
            .map_err(|e| crate::domain::errors::LifecycleError::InvalidRule {
                rule_id: "configuration".to_string(),
                reason: format!("Failed to serialize lifecycle configuration: {}", e),
            })?;

        sqlx::query(
            r#"
            INSERT INTO lifecycle_configurations (bucket_name, configuration, enabled, updated_at)
            VALUES ($1, $2, $3, NOW())
            ON CONFLICT (bucket_name)
            DO UPDATE SET 
                configuration = EXCLUDED.configuration,
                enabled = EXCLUDED.enabled,
                updated_at = NOW()
            "#,
        )
        .bind(bucket.as_str())
        .bind(&config_json)
        .bind(config.enabled)
        .execute(&self.pool)
        .await
        .map_err(|e| crate::domain::errors::StorageError::InfrastructureError {
            message: format!("Database error storing lifecycle configuration: {}", e),
            source: Some(e.to_string()),
        })?;

        Ok(())
    }

    async fn get_configuration(
        &self,
        bucket: &BucketName,
    ) -> StorageResult<Option<LifecycleConfiguration>> {
        let row = sqlx::query(
            r#"
            SELECT configuration, enabled
            FROM lifecycle_configurations 
            WHERE bucket_name = $1
            "#,
        )
        .bind(bucket.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| crate::domain::errors::StorageError::InfrastructureError {
            message: format!("Database error retrieving lifecycle configuration: {}", e),
            source: Some(e.to_string()),
        })?;

        match row {
            Some(row) => {
                let mut config: LifecycleConfiguration = 
                    serde_json::from_value(row.get("configuration"))
                        .map_err(|e| crate::domain::errors::StorageError::InternalError {
                            message: format!("Failed to deserialize lifecycle configuration: {}", e),
                        })?;
                
                config.enabled = row.get("enabled");
                Ok(Some(config))
            }
            None => Ok(None),
        }
    }

    async fn delete_configuration(&self, bucket: &BucketName) -> StorageResult<bool> {
        let result = sqlx::query(
            r#"
            DELETE FROM lifecycle_configurations 
            WHERE bucket_name = $1
            "#,
        )
        .bind(bucket.as_str())
        .execute(&self.pool)
        .await
        .map_err(|e| crate::domain::errors::StorageError::InfrastructureError {
            message: format!("Database error deleting lifecycle configuration: {}", e),
            source: Some(e.to_string()),
        })?;

        Ok(result.rows_affected() > 0)
    }

    async fn list_configurations(&self) -> StorageResult<Vec<(BucketName, LifecycleConfiguration)>> {
        let rows = sqlx::query(
            r#"
            SELECT bucket_name, configuration, enabled
            FROM lifecycle_configurations 
            ORDER BY bucket_name
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| crate::domain::errors::StorageError::InfrastructureError {
            message: format!("Database error listing lifecycle configurations: {}", e),
            source: Some(e.to_string()),
        })?;

        let mut configs = Vec::new();
        for row in rows {
            let bucket_name = BucketName::new(row.get::<String, _>("bucket_name"))
                .map_err(|e| crate::domain::errors::StorageError::ValidationError {
                    message: format!("Invalid bucket name: {}", e),
                })?;

            let mut config: LifecycleConfiguration = 
                serde_json::from_value(row.get("configuration"))
                    .map_err(|e| crate::domain::errors::StorageError::InternalError {
                        message: format!("Failed to deserialize lifecycle configuration: {}", e),
                    })?;
            
            config.enabled = row.get("enabled");
            configs.push((bucket_name, config));
        }

        Ok(configs)
    }

    async fn add_rule(
        &self,
        bucket: &BucketName,
        rule: &LifecycleRule,
    ) -> StorageResult<()> {
        // Get existing configuration
        let mut config = self.get_configuration(bucket).await?
            .unwrap_or_else(|| LifecycleConfiguration {
                rules: Vec::new(),
                enabled: true,
            });

        // Add new rule
        config.rules.push(rule.clone());

        // Store updated configuration
        self.store_configuration(bucket, &config).await
    }

    async fn remove_rule(
        &self,
        bucket: &BucketName,
        rule_id: &str,
    ) -> StorageResult<bool> {
        // Get existing configuration
        let mut config = match self.get_configuration(bucket).await? {
            Some(config) => config,
            None => return Ok(false),
        };

        // Remove rule by ID
        let original_len = config.rules.len();
        config.rules.retain(|rule| rule.id != rule_id);

        if config.rules.len() == original_len {
            return Ok(false);
        }

        // Store updated configuration
        self.store_configuration(bucket, &config).await?;
        Ok(true)
    }

    async fn get_rules_for_bucket(&self, bucket: &BucketName) -> StorageResult<Vec<LifecycleRule>> {
        match self.get_configuration(bucket).await? {
            Some(config) => Ok(config.rules),
            None => Ok(Vec::new()),
        }
    }

    async fn mark_rule_executed(
        &self,
        bucket: &BucketName,
        key: &ObjectKey,
        rule_id: &str,
        action: &str,
        success: bool,
        error_message: Option<&str>,
    ) -> StorageResult<()> {
        sqlx::query(
            r#"
            INSERT INTO lifecycle_execution_log (
                bucket_name, object_key, rule_id, action, success, error_message, execution_time
            ) 
            VALUES ($1, $2, $3, $4, $5, $6, NOW())
            "#,
        )
        .bind(bucket.as_str())
        .bind(key.as_str())
        .bind(rule_id)
        .bind(action)
        .bind(success)
        .bind(error_message)
        .execute(&self.pool)
        .await
        .map_err(|e| crate::domain::errors::StorageError::InfrastructureError {
            message: format!("Database error logging rule execution: {}", e),
            source: Some(e.to_string()),
        })?;

        Ok(())
    }

    async fn is_rule_executed(
        &self,
        bucket: &BucketName,
        key: &ObjectKey,
        rule_id: &str,
    ) -> StorageResult<bool> {
        let count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*) 
            FROM lifecycle_execution_log 
            WHERE bucket_name = $1 AND object_key = $2 AND rule_id = $3 AND success = true
            "#,
        )
        .bind(bucket.as_str())
        .bind(key.as_str())
        .bind(rule_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| crate::domain::errors::StorageError::InfrastructureError {
            message: format!("Database error checking rule execution: {}", e),
            source: Some(e.to_string()),
        })?;

        Ok(count > 0)
    }

    async fn get_execution_history(
        &self,
        bucket: &BucketName,
        key: Option<&ObjectKey>,
        limit: Option<usize>,
    ) -> StorageResult<Vec<(String, String, chrono::DateTime<chrono::Utc>, bool, Option<String>)>> {
        let mut query_str = String::from(
            "SELECT rule_id, action, execution_time, success, error_message 
             FROM lifecycle_execution_log 
             WHERE bucket_name = $1"
        );
        
        let mut param_count = 1;
        if key.is_some() {
            param_count += 1;
            query_str.push_str(&format!(" AND object_key = ${}", param_count));
        }
        
        query_str.push_str(" ORDER BY execution_time DESC");
        
        if let Some(limit) = limit {
            query_str.push_str(&format!(" LIMIT {}", limit));
        }

        let mut query = sqlx::query(&query_str).bind(bucket.as_str());
        
        if let Some(key) = key {
            query = query.bind(key.as_str());
        }

        let rows = query
            .fetch_all(&self.pool)
            .await
            .map_err(|e| crate::domain::errors::StorageError::InfrastructureError {
                message: format!("Database error getting execution history: {}", e),
                source: Some(e.to_string()),
            })?;

        let history = rows
            .iter()
            .map(|row| {
                (
                    row.get::<String, _>("rule_id"),
                    row.get::<String, _>("action"),
                    row.get("execution_time"),
                    row.get("success"),
                    row.get("error_message"),
                )
            })
            .collect();

        Ok(history)
    }

    async fn cleanup_execution_logs(&self, days_to_keep: u32) -> StorageResult<u64> {
        let result = sqlx::query(
            r#"
            DELETE FROM lifecycle_execution_log 
            WHERE execution_time < NOW() - INTERVAL '%1 days'
            "#,
        )
        .bind(days_to_keep as i32)
        .execute(&self.pool)
        .await
        .map_err(|e| crate::domain::errors::StorageError::InfrastructureError {
            message: format!("Database error cleaning up execution logs: {}", e),
            source: Some(e.to_string()),
        })?;

        Ok(result.rows_affected())
    }
}