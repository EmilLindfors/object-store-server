use chrono::{DateTime, Utc};
use object_store::ObjectStore;
use object_store::path::Path;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tokio::time::{Duration, sleep};

use crate::adapters::outbound::storage::{error::StoreError, versioning::VersionedStore};

/// Action to take when a lifecycle rule is triggered
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum LifecycleAction {
    /// Delete the object
    Delete,

    /// Transition to another storage class
    Transition { storage_class: String },
}

/// Rule for lifecycle management
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LifecycleRule {
    /// Unique identifier for the rule
    pub id: String,

    /// Object prefix this rule applies to
    pub prefix: String,

    /// Whether this rule is enabled
    #[serde(default)]
    pub status: RuleStatus,

    /// Optional expiration configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expiration: Option<ExpirationConfig>,

    /// Optional transitions configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transitions: Option<Vec<TransitionConfig>>,

    /// Optional filter tags that must be present on the object
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter_tags: Option<HashMap<String, String>>,
}

/// Status of a lifecycle rule
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RuleStatus {
    Enabled,
    Disabled,
}

impl Default for RuleStatus {
    fn default() -> Self {
        RuleStatus::Enabled
    }
}

/// Expiration configuration for a lifecycle rule
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ExpirationConfig {
    /// Number of days after object creation when the specific rule action takes effect
    #[serde(skip_serializing_if = "Option::is_none")]
    pub days: Option<u32>,

    /// Date when rule action takes effect
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<DateTime<Utc>>,

    /// Whether to expire incomplete multipart uploads
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expired_object_delete_marker: Option<bool>,
}

/// Transition configuration for a lifecycle rule
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TransitionConfig {
    /// Number of days after object creation to transition
    #[serde(skip_serializing_if = "Option::is_none")]
    pub days: Option<u32>,

    /// Date when transition takes effect
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<DateTime<Utc>>,

    /// Storage class to transition to
    pub storage_class: String,
}

/// Full lifecycle configuration for a bucket
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct LifecycleConfiguration {
    /// List of lifecycle rules
    #[serde(default)]
    pub rules: Vec<LifecycleRule>,
}

impl LifecycleConfiguration {
    /// Create a new empty lifecycle configuration
    pub fn new() -> Self {
        LifecycleConfiguration { rules: Vec::new() }
    }

    /// Add a new rule
    pub fn add_rule(&mut self, rule: LifecycleRule) {
        // Check if a rule with this ID already exists
        if let Some(existing) = self.rules.iter_mut().find(|r| r.id == rule.id) {
            // Replace the existing rule
            *existing = rule;
        } else {
            // Add a new rule
            self.rules.push(rule);
        }
    }

    /// Remove a rule by ID
    pub fn remove_rule(&mut self, rule_id: &str) -> bool {
        let len_before = self.rules.len();
        self.rules.retain(|r| r.id != rule_id);
        self.rules.len() != len_before
    }

    /// Get a rule by ID
    pub fn get_rule(&self, rule_id: &str) -> Option<&LifecycleRule> {
        self.rules.iter().find(|r| r.id == rule_id)
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<(), StoreError> {
        for rule in &self.rules {
            // Check that at least one action is specified
            if rule.expiration.is_none() && rule.transitions.is_none() {
                return Err(StoreError::InvalidLifecycleConfig(format!(
                    "Rule {} must specify at least one action",
                    rule.id
                )));
            }

            // Check expiration config
            if let Some(expiration) = &rule.expiration {
                if expiration.days.is_none() && expiration.date.is_none() {
                    return Err(StoreError::InvalidLifecycleConfig(format!(
                        "Expiration in rule {} must specify days or date",
                        rule.id
                    )));
                }
            }

            // Check transitions
            if let Some(transitions) = &rule.transitions {
                for (i, transition) in transitions.iter().enumerate() {
                    if transition.days.is_none() && transition.date.is_none() {
                        return Err(StoreError::InvalidLifecycleConfig(format!(
                            "Transition {} in rule {} must specify days or date",
                            i, rule.id
                        )));
                    }
                }
            }
        }

        Ok(())
    }
}

/// Manager for handling lifecycle configurations and applying rules
pub struct LifecycleManager<T: object_store::ObjectStore + Send + Sync> {
    /// The store to manage lifecycles for
    store: Arc<VersionedStore<T>>,

    /// Map of bucket name to lifecycle configuration
    configs: Arc<RwLock<HashMap<String, LifecycleConfiguration>>>,

    /// Whether the lifecycle manager is running
    running: Arc<RwLock<bool>>,
}

impl<T: object_store::ObjectStore + Send + Sync + 'static> LifecycleManager<T> {
    /// Create a new lifecycle manager
    pub fn new(store: Arc<VersionedStore<T>>) -> Self {
        LifecycleManager {
            store,
            configs: Arc::new(RwLock::new(HashMap::new())),
            running: Arc::new(RwLock::new(false)),
        }
    }

    /// Get the lifecycle configuration for a bucket
    pub fn get_lifecycle_config(
        &self,
        bucket: &str,
    ) -> Result<Option<LifecycleConfiguration>, StoreError> {
        let configs = self
            .configs
            .read()
            .map_err(|e| StoreError::Other(format!("Failed to acquire read lock: {}", e)))?;

        Ok(configs.get(bucket).cloned())
    }

    /// Set the lifecycle configuration for a bucket
    pub fn set_lifecycle_config(
        &self,
        bucket: &str,
        config: LifecycleConfiguration,
    ) -> Result<(), StoreError> {
        // Validate the configuration first
        config.validate()?;

        // Update the configuration
        let mut configs = self
            .configs
            .write()
            .map_err(|e| StoreError::Other(format!("Failed to acquire write lock: {}", e)))?;

        configs.insert(bucket.to_string(), config);

        Ok(())
    }

    /// Start the lifecycle manager worker
    pub async fn start(&self) -> Result<(), StoreError> {
        // Check if already running
        {
            let running = self
                .running
                .read()
                .map_err(|e| StoreError::Other(format!("Failed to acquire read lock: {}", e)))?;

            if *running {
                return Ok(());
            }
        }

        // Set running flag
        {
            let mut running = self
                .running
                .write()
                .map_err(|e| StoreError::Other(format!("Failed to acquire write lock: {}", e)))?;
            *running = true;
        }

        // Clone references for the worker
        let store = self.store.clone();
        let configs = self.configs.clone();
        let running = self.running.clone();

        // Spawn the worker
        tokio::spawn(async move {
            // Check every day by default
            let check_interval = Duration::from_secs(86400);

            // Worker loop
            loop {
                // Check if we should continue running
                {
                    let is_running = *running.read().unwrap();
                    if !is_running {
                        break;
                    }
                }

                // Process all buckets with configurations
                {
                    // Get a copy of the current configurations to avoid holding the lock during await
                    let config_snapshot: Vec<(String, LifecycleConfiguration)> = {
                        let configs_guard = configs.read().unwrap();
                        configs_guard
                            .iter()
                            .map(|(bucket, config)| (bucket.clone(), config.clone()))
                            .collect()
                    };

                    // Process each bucket configuration
                    for (bucket, config) in config_snapshot {
                        // Apply lifecycle rules for this bucket
                        if let Err(e) = Self::apply_lifecycle_rules(&store, &bucket, &config).await
                        {
                            eprintln!(
                                "Error applying lifecycle rules for bucket {}: {}",
                                bucket, e
                            );
                        }
                    }
                }

                // Wait for next check
                sleep(check_interval).await;
            }
        });

        Ok(())
    }

    /// Stop the lifecycle manager worker
    pub fn stop(&self) -> Result<(), StoreError> {
        let mut running = self
            .running
            .write()
            .map_err(|e| StoreError::Other(format!("Failed to acquire write lock: {}", e)))?;

        *running = false;

        Ok(())
    }

    /// Apply lifecycle rules for a bucket
    async fn apply_lifecycle_rules(
        store: &Arc<VersionedStore<T>>,
        bucket: &str,
        config: &LifecycleConfiguration,
    ) -> Result<(), StoreError> {
        // In a real implementation, we would:
        // 1. List all objects in the bucket (with the given prefix)
        // 2. For each object, check if any rule applies
        // 3. Apply the actions (delete, transition) as needed

        // For simplicity in this example, we'll just log what would happen
        for rule in &config.rules {
            // Skip disabled rules
            if rule.status == RuleStatus::Disabled {
                continue;
            }

            // Create the prefix path
            let prefix = Path::from(format!("{}/{}", bucket, rule.prefix));

            // List objects with the prefix
            let objects = store.list(Some(&prefix));

            // Process each object
            use futures::StreamExt;
            let mut stream = objects;

            while let Some(result) = stream.next().await {
                if let Ok(meta) = result {
                    let path = meta.location;

                    // Get the object's last modified time
                    let last_modified = meta.last_modified;

                    // Check if expiration applies
                    if let Some(expiration) = &rule.expiration {
                        if let Some(days) = expiration.days {
                            let expiration_time =
                                last_modified + chrono::Duration::days(days as i64);

                            if expiration_time <= Utc::now() {
                                // Object should be expired
                                println!(
                                    "Would delete object {} due to lifecycle rule {}",
                                    path.as_ref(),
                                    rule.id
                                );

                                // In a real implementation:
                                // store.delete(&path).await?;
                            }
                        }
                    }

                    // Check if transitions apply
                    if let Some(transitions) = &rule.transitions {
                        for transition in transitions {
                            if let Some(days) = transition.days {
                                let transition_time =
                                    last_modified + chrono::Duration::days(days as i64);

                                if transition_time <= Utc::now() {
                                    // Object should be transitioned
                                    println!(
                                        "Would transition object {} to storage class {} due to lifecycle rule {}",
                                        path.as_ref(),
                                        transition.storage_class,
                                        rule.id
                                    );

                                    // In a real implementation, we would apply the transition
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }
}
