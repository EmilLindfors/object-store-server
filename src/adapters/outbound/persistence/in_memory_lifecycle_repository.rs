use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::{
    domain::{
        errors::{LifecycleError, LifecycleResult},
        models::{LifecycleConfiguration, LifecycleRule},
        value_objects::BucketName,
    },
    ports::repositories::LifecycleRepository,
};

/// In-memory implementation of LifecycleRepository for testing and development
#[derive(Clone)]
pub struct InMemoryLifecycleRepository {
    data: Arc<RwLock<RepositoryData>>,
}

#[derive(Default)]
struct RepositoryData {
    // Map of bucket name -> lifecycle configuration
    configurations: HashMap<String, LifecycleConfiguration>,
    // Map of bucket name -> last processed time
    last_processed: HashMap<String, std::time::SystemTime>,
}

impl InMemoryLifecycleRepository {
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(RepositoryData::default())),
        }
    }
}

#[async_trait]
impl LifecycleRepository for InMemoryLifecycleRepository {
    async fn save_configuration(
        &self,
        bucket: &BucketName,
        config: &LifecycleConfiguration,
    ) -> LifecycleResult<()> {
        let mut data = self.data.write().await;

        // Validate configuration before saving
        config.validate().map_err(|e| LifecycleError::InvalidRule {
            rule_id: String::new(),
            reason: e.to_string(),
        })?;

        data.configurations
            .insert(bucket.as_str().to_string(), config.clone());
        Ok(())
    }

    async fn get_configuration(
        &self,
        bucket: &BucketName,
    ) -> LifecycleResult<Option<LifecycleConfiguration>> {
        let data = self.data.read().await;
        Ok(data.configurations.get(bucket.as_str()).cloned())
    }

    async fn delete_configuration(&self, bucket: &BucketName) -> LifecycleResult<()> {
        let mut data = self.data.write().await;
        data.configurations.remove(bucket.as_str());
        data.last_processed.remove(bucket.as_str());
        Ok(())
    }

    async fn configuration_exists(&self, bucket: &BucketName) -> LifecycleResult<bool> {
        let data = self.data.read().await;
        Ok(data.configurations.contains_key(bucket.as_str()))
    }

    async fn get_rule(
        &self,
        bucket: &BucketName,
        rule_id: &str,
    ) -> LifecycleResult<Option<LifecycleRule>> {
        let data = self.data.read().await;

        Ok(data
            .configurations
            .get(bucket.as_str())
            .and_then(|config| config.rules.iter().find(|rule| rule.id == rule_id).cloned()))
    }

    async fn update_rule(&self, bucket: &BucketName, rule: &LifecycleRule) -> LifecycleResult<()> {
        let mut data = self.data.write().await;

        let config = data
            .configurations
            .get_mut(bucket.as_str())
            .ok_or_else(|| LifecycleError::ConfigurationNotFound {
                bucket: bucket.clone(),
            })?;

        // Find and update the rule
        let mut found = false;
        for existing_rule in &mut config.rules {
            if existing_rule.id == rule.id {
                *existing_rule = rule.clone();
                found = true;
                break;
            }
        }

        if !found {
            return Err(LifecycleError::InvalidRule {
                rule_id: rule.id.clone(),
                reason: "Rule not found".to_string(),
            });
        }

        // Validate the updated configuration
        config.validate().map_err(|e| LifecycleError::InvalidRule {
            rule_id: rule.id.clone(),
            reason: e.to_string(),
        })?;

        Ok(())
    }

    async fn list_configured_buckets(&self) -> LifecycleResult<Vec<BucketName>> {
        let data = self.data.read().await;

        let buckets: Vec<BucketName> = data
            .configurations
            .keys()
            .filter_map(|name| BucketName::new(name.clone()).ok())
            .collect();

        Ok(buckets)
    }

    async fn get_last_processed_time(
        &self,
        bucket: &BucketName,
    ) -> LifecycleResult<Option<std::time::SystemTime>> {
        let data = self.data.read().await;
        Ok(data.last_processed.get(bucket.as_str()).copied())
    }

    async fn set_last_processed_time(
        &self,
        bucket: &BucketName,
        time: std::time::SystemTime,
    ) -> LifecycleResult<()> {
        let mut data = self.data.write().await;
        data.last_processed
            .insert(bucket.as_str().to_string(), time);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::{Filter, RuleStatus};

    #[tokio::test]
    async fn test_save_and_get_configuration() {
        let repo = InMemoryLifecycleRepository::new();
        let bucket = BucketName::new("test-bucket".to_string()).unwrap();

        let config = LifecycleConfiguration {
            bucket: bucket.clone(),
            rules: vec![LifecycleRule {
                id: "rule1".to_string(),
                status: RuleStatus::Enabled,
                filter: Filter::with_prefix("logs/".to_string()),
                expiration_days: Some(30),
                ..Default::default()
            }],
        };

        // Save configuration
        repo.save_configuration(&bucket, &config).await.unwrap();

        // Retrieve configuration
        let retrieved = repo.get_configuration(&bucket).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().rules.len(), 1);
    }

    #[tokio::test]
    async fn test_update_rule() {
        let repo = InMemoryLifecycleRepository::new();
        let bucket = BucketName::new("test-bucket".to_string()).unwrap();

        let mut config = LifecycleConfiguration {
            bucket: bucket.clone(),
            rules: vec![LifecycleRule {
                id: "rule1".to_string(),
                status: RuleStatus::Enabled,
                filter: Filter::with_prefix("logs/".to_string()),
                expiration_days: Some(30),
                ..Default::default()
            }],
        };

        // Save initial configuration
        repo.save_configuration(&bucket, &config).await.unwrap();

        // Update the rule
        config.rules[0].status = RuleStatus::Disabled;
        repo.update_rule(&bucket, &config.rules[0]).await.unwrap();

        // Verify update
        let updated_rule = repo.get_rule(&bucket, "rule1").await.unwrap().unwrap();
        assert_eq!(updated_rule.status, RuleStatus::Disabled);
    }
}
