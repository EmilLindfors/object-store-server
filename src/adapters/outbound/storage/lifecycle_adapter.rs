use crate::adapters::outbound::storage::lifecycle::{
    ExpirationConfig, LifecycleConfiguration, LifecycleRule, RuleStatus, TransitionConfig,
};
use crate::adapters::outbound::storage::minio::{
    MinioFilter, MinioLifecycleConfig, MinioLifecycleRule,
};

/// Convert from MinIO lifecycle model to service lifecycle model
pub fn domain_to_service(config: &MinioLifecycleConfig) -> LifecycleConfiguration {
    let mut service_config = LifecycleConfiguration::new();

    for domain_rule in &config.rules {
        // Create a new service rule
        let prefix = domain_rule
            .filter
            .prefix
            .as_ref()
            .map(|p| p.to_string())
            .unwrap_or_default();

        let mut service_rule = LifecycleRule {
            id: domain_rule.id.clone(),
            prefix,
            status: if domain_rule.status {
                RuleStatus::Enabled
            } else {
                RuleStatus::Disabled
            },
            expiration: None,
            transitions: None,
            filter_tags: None,
        };

        // Handle expiration
        if domain_rule.expiration_days.is_some()
            || domain_rule.expiration_date.is_some()
            || domain_rule
                .expiration_expired_object_delete_marker
                .is_some()
        {
            service_rule.expiration = Some(ExpirationConfig {
                days: domain_rule.expiration_days.map(|d| d as u32),
                date: domain_rule.expiration_date,
                expired_object_delete_marker: domain_rule.expiration_expired_object_delete_marker,
            });
        }

        // Handle transition
        if domain_rule.transition_days.is_some()
            || domain_rule.transition_date.is_some()
            || domain_rule.transition_storage_class.is_some()
        {
            let transition = TransitionConfig {
                days: domain_rule.transition_days.map(|d| d as u32),
                date: domain_rule.transition_date,
                storage_class: domain_rule
                    .transition_storage_class
                    .clone()
                    .unwrap_or_default(),
            };

            service_rule.transitions = Some(vec![transition]);
        }

        // Add the rule to the configuration
        service_config.add_rule(service_rule);
    }

    service_config
}

/// Convert from service lifecycle model to MinIO lifecycle model
pub fn service_to_domain(config: &LifecycleConfiguration) -> MinioLifecycleConfig {
    let mut domain_config = MinioLifecycleConfig::default();

    for service_rule in &config.rules {
        // Create a filter
        let filter = MinioFilter {
            prefix: Some(service_rule.prefix.clone()),
            tag: None,
            and: None,
        };

        // Create a new domain rule
        let mut domain_rule = MinioLifecycleRule {
            id: service_rule.id.clone(),
            status: service_rule.status == RuleStatus::Enabled,
            filter,
            abort_incomplete_multipart_upload_days_after_initiation: None,
            expiration_date: None,
            expiration_days: None,
            expiration_expired_object_delete_marker: None,
            noncurrent_version_expiration_noncurrent_days: None,
            noncurrent_version_transition_noncurrent_days: None,
            noncurrent_version_transition_storage_class: None,
            transition_date: None,
            transition_days: None,
            transition_storage_class: None,
        };

        // Handle expiration
        if let Some(expiration) = &service_rule.expiration {
            domain_rule.expiration_days = expiration.days.map(|d| d as usize);
            domain_rule.expiration_date = expiration.date;
            domain_rule.expiration_expired_object_delete_marker =
                expiration.expired_object_delete_marker;
        }

        // Handle transitions
        if let Some(transitions) = &service_rule.transitions {
            if let Some(first_transition) = transitions.first() {
                domain_rule.transition_days = first_transition.days.map(|d| d as usize);
                domain_rule.transition_date = first_transition.date;
                domain_rule.transition_storage_class = Some(first_transition.storage_class.clone());
            }
        }

        // Add the rule to the domain configuration
        domain_config.rules.push(domain_rule);
    }

    domain_config
}
