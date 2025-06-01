use crate::domain::errors::ValidationError;

/// A unique identifier for an object version
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VersionId(String);

impl VersionId {
    /// Create a new VersionId with validation
    pub fn new(value: String) -> Result<Self, ValidationError> {
        if value.is_empty() {
            return Err(ValidationError::EmptyVersionId);
        }

        // Version IDs are typically UUIDs or similar identifiers
        // We'll allow alphanumeric characters, hyphens, and underscores
        if value.len() > 1024 {
            return Err(ValidationError::VersionIdTooLong {
                actual: value.len(),
                max: 1024,
            });
        }

        // Check for valid characters
        for c in value.chars() {
            if !c.is_alphanumeric() && c != '-' && c != '_' && c != '.' {
                return Err(ValidationError::InvalidVersionIdCharacter(c));
            }
        }

        Ok(Self(value))
    }

    /// Generate a new unique version ID
    pub fn generate() -> Self {
        // Using timestamp + random component for uniqueness
        // In production, you might use UUID v4 or similar
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_micros();

        let random: u32 = Self::simple_random();

        Self(format!("{:x}-{:08x}", timestamp, random))
    }

    /// Get the version ID as a string slice
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Simple random number generator for version IDs
    /// In production, use a proper RNG
    fn simple_random() -> u32 {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();

        // Simple hash-like transformation
        nanos.wrapping_mul(1664525).wrapping_add(1013904223)
    }
}

impl std::fmt::Display for VersionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Default for VersionId {
    fn default() -> Self {
        Self::generate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_version_id() {
        assert!(VersionId::new("v1.0.0".to_string()).is_ok());
        assert!(VersionId::new("550e8400-e29b-41d4-a716-446655440000".to_string()).is_ok());
        assert!(VersionId::new("2023-10-01_12-34-56".to_string()).is_ok());
    }

    #[test]
    fn test_invalid_version_id() {
        assert!(VersionId::new("".to_string()).is_err());
        assert!(VersionId::new("version with spaces".to_string()).is_err());
        assert!(VersionId::new("version/with/slashes".to_string()).is_err());
        assert!(VersionId::new("x".repeat(1025)).is_err());
    }

    #[test]
    fn test_generate_version_id() {
        let v1 = VersionId::generate();
        let v2 = VersionId::generate();

        // Should generate different IDs
        assert_ne!(v1, v2);

        // Should be valid
        assert!(VersionId::new(v1.as_str().to_string()).is_ok());
        assert!(VersionId::new(v2.as_str().to_string()).is_ok());
    }
}
