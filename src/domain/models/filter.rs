use std::collections::HashMap;

/// Filter for lifecycle rules and object selection
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Filter {
    /// Prefix to match object keys
    pub prefix: Option<String>,
    /// Tags that objects must have
    pub tags: HashMap<String, String>,
    /// Object size greater than this value (in bytes)
    pub object_size_greater_than: Option<u64>,
    /// Object size less than this value (in bytes)  
    pub object_size_less_than: Option<u64>,
}

impl Filter {
    /// Create a new empty filter
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a filter with just a prefix
    pub fn with_prefix(prefix: String) -> Self {
        Self {
            prefix: Some(prefix),
            ..Default::default()
        }
    }

    /// Check if this filter matches an object
    pub fn matches(
        &self,
        key: &str,
        object_tags: &HashMap<String, String>,
        object_size: u64,
    ) -> bool {
        // Check prefix
        if let Some(prefix) = &self.prefix {
            if !key.starts_with(prefix) {
                return false;
            }
        }

        // Check tags
        if !self.tags.is_empty() {
            for (k, v) in &self.tags {
                if object_tags.get(k) != Some(v) {
                    return false;
                }
            }
        }

        // Check size constraints
        if let Some(min_size) = self.object_size_greater_than {
            if object_size <= min_size {
                return false;
            }
        }

        if let Some(max_size) = self.object_size_less_than {
            if object_size >= max_size {
                return false;
            }
        }

        true
    }

    /// Check if filter is empty (matches everything)
    pub fn is_empty(&self) -> bool {
        self.prefix.is_none()
            && self.tags.is_empty()
            && self.object_size_greater_than.is_none()
            && self.object_size_less_than.is_none()
    }
}
