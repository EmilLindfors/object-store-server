use async_trait::async_trait;
use sqlx::{PgPool, Row};
use std::collections::HashMap;
use uuid::Uuid;

use crate::{
    domain::{
        models::ObjectMetadata,
        value_objects::ObjectKey,
        errors::StorageResult,
    },
    ports::repositories::ObjectRepository,
};

/// SQL-based implementation of ObjectRepository using PostgreSQL
#[derive(Clone)]
pub struct SqlObjectRepository {
    pool: PgPool,
}

impl SqlObjectRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Initialize database tables
    pub async fn migrate(&self) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS object_metadata (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                object_key VARCHAR NOT NULL UNIQUE,
                content_type VARCHAR,
                content_length BIGINT NOT NULL,
                etag VARCHAR,
                last_modified TIMESTAMPTZ NOT NULL,
                custom_metadata JSONB DEFAULT '{}',
                created_at TIMESTAMPTZ DEFAULT NOW(),
                updated_at TIMESTAMPTZ DEFAULT NOW()
            );

            CREATE INDEX IF NOT EXISTS idx_object_metadata_key ON object_metadata(object_key);
            CREATE INDEX IF NOT EXISTS idx_object_metadata_last_modified ON object_metadata(last_modified);
            CREATE INDEX IF NOT EXISTS idx_object_metadata_content_length ON object_metadata(content_length);
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}

#[async_trait]
impl ObjectRepository for SqlObjectRepository {
    async fn store_metadata(
        &self,
        key: &ObjectKey,
        metadata: &ObjectMetadata,
    ) -> StorageResult<()> {
        let custom_metadata_json = serde_json::to_value(&metadata.custom_metadata)
            .map_err(|e| crate::domain::errors::StorageError::InternalError {
                message: format!("Failed to serialize custom metadata: {}", e),
            })?;

        sqlx::query(
            r#"
            INSERT INTO object_metadata (
                object_key, content_type, content_length, etag, 
                last_modified, custom_metadata, updated_at
            ) 
            VALUES ($1, $2, $3, $4, $5, $6, NOW())
            ON CONFLICT (object_key) 
            DO UPDATE SET 
                content_type = EXCLUDED.content_type,
                content_length = EXCLUDED.content_length,
                etag = EXCLUDED.etag,
                last_modified = EXCLUDED.last_modified,
                custom_metadata = EXCLUDED.custom_metadata,
                updated_at = NOW()
            "#,
        )
        .bind(key.as_str())
        .bind(&metadata.content_type)
        .bind(metadata.content_length as i64)
        .bind(&metadata.etag)
        .bind(metadata.last_modified)
        .bind(&custom_metadata_json)
        .execute(&self.pool)
        .await
        .map_err(|e| crate::domain::errors::StorageError::InfrastructureError {
            message: format!("Database error storing metadata: {}", e),
            source: Some(e.to_string()),
        })?;

        Ok(())
    }

    async fn get_metadata(&self, key: &ObjectKey) -> StorageResult<Option<ObjectMetadata>> {
        let row = sqlx::query(
            r#"
            SELECT content_type, content_length, etag, last_modified, custom_metadata
            FROM object_metadata 
            WHERE object_key = $1
            "#,
        )
        .bind(key.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| crate::domain::errors::StorageError::InfrastructureError {
            message: format!("Database error retrieving metadata: {}", e),
            source: Some(e.to_string()),
        })?;

        match row {
            Some(row) => {
                let custom_metadata: HashMap<String, String> = 
                    serde_json::from_value(row.get("custom_metadata"))
                        .unwrap_or_default();

                Ok(Some(ObjectMetadata {
                    content_type: row.get("content_type"),
                    content_length: row.get::<i64, _>("content_length") as u64,
                    etag: row.get("etag"),
                    last_modified: row.get("last_modified"),
                    custom_metadata,
                }))
            }
            None => Ok(None),
        }
    }

    async fn delete_metadata(&self, key: &ObjectKey) -> StorageResult<bool> {
        let result = sqlx::query(
            r#"
            DELETE FROM object_metadata 
            WHERE object_key = $1
            "#,
        )
        .bind(key.as_str())
        .execute(&self.pool)
        .await
        .map_err(|e| crate::domain::errors::StorageError::InfrastructureError {
            message: format!("Database error deleting metadata: {}", e),
            source: Some(e.to_string()),
        })?;

        Ok(result.rows_affected() > 0)
    }

    async fn list_objects(
        &self,
        prefix: Option<&str>,
        max_results: Option<usize>,
    ) -> StorageResult<Vec<ObjectKey>> {
        let mut query_str = String::from(
            "SELECT object_key FROM object_metadata"
        );
        
        let mut conditions = Vec::new();
        if prefix.is_some() {
            conditions.push("object_key LIKE $1");
        }
        
        if !conditions.is_empty() {
            query_str.push_str(" WHERE ");
            query_str.push_str(&conditions.join(" AND "));
        }
        
        query_str.push_str(" ORDER BY object_key");
        
        if let Some(limit) = max_results {
            query_str.push_str(&format!(" LIMIT {}", limit));
        }

        let mut query = sqlx::query(&query_str);
        
        if let Some(prefix) = prefix {
            query = query.bind(format!("{}%", prefix));
        }

        let rows = query
            .fetch_all(&self.pool)
            .await
            .map_err(|e| crate::domain::errors::StorageError::InfrastructureError {
                message: format!("Database error listing objects: {}", e),
                source: Some(e.to_string()),
            })?;

        let keys = rows
            .iter()
            .map(|row| {
                let key_str: String = row.get("object_key");
                ObjectKey::new(key_str).unwrap()
            })
            .collect();

        Ok(keys)
    }

    async fn object_exists(&self, key: &ObjectKey) -> StorageResult<bool> {
        let count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*) 
            FROM object_metadata 
            WHERE object_key = $1
            "#,
        )
        .bind(key.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(|e| crate::domain::errors::StorageError::InfrastructureError {
            message: format!("Database error checking object existence: {}", e),
            source: Some(e.to_string()),
        })?;

        Ok(count > 0)
    }

    async fn get_object_size(&self, key: &ObjectKey) -> StorageResult<Option<u64>> {
        let size: Option<i64> = sqlx::query_scalar(
            r#"
            SELECT content_length 
            FROM object_metadata 
            WHERE object_key = $1
            "#,
        )
        .bind(key.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| crate::domain::errors::StorageError::InfrastructureError {
            message: format!("Database error getting object size: {}", e),
            source: Some(e.to_string()),
        })?;

        Ok(size.map(|s| s as u64))
    }

    async fn update_metadata(
        &self,
        key: &ObjectKey,
        metadata: &ObjectMetadata,
    ) -> StorageResult<bool> {
        // Reuse store_metadata since it handles updates via ON CONFLICT
        self.store_metadata(key, metadata).await?;
        Ok(true)
    }

    async fn get_objects_by_size_range(
        &self,
        min_size: Option<u64>,
        max_size: Option<u64>,
    ) -> StorageResult<Vec<ObjectKey>> {
        let mut conditions = Vec::new();
        let mut query_str = String::from("SELECT object_key FROM object_metadata WHERE");
        
        if let Some(min) = min_size {
            conditions.push(format!("content_length >= {}", min));
        }
        if let Some(max) = max_size {
            conditions.push(format!("content_length <= {}", max));
        }
        
        if conditions.is_empty() {
            return Ok(Vec::new());
        }
        
        query_str.push_str(&format!(" {} ORDER BY object_key", conditions.join(" AND ")));

        let rows = sqlx::query(&query_str)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| crate::domain::errors::StorageError::InfrastructureError {
                message: format!("Database error querying by size: {}", e),
                source: Some(e.to_string()),
            })?;

        let keys = rows
            .iter()
            .map(|row| {
                let key_str: String = row.get("object_key");
                ObjectKey::new(key_str).unwrap()
            })
            .collect();

        Ok(keys)
    }

    async fn get_total_storage_size(&self) -> StorageResult<u64> {
        let total: Option<i64> = sqlx::query_scalar(
            "SELECT SUM(content_length) FROM object_metadata"
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| crate::domain::errors::StorageError::InfrastructureError {
            message: format!("Database error calculating total size: {}", e),
            source: Some(e.to_string()),
        })?;

        Ok(total.unwrap_or(0) as u64)
    }
}