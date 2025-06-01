use crate::domain::errors::ValidationError;

/// A validated bucket name
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BucketName(String);

impl BucketName {
    /// Create a new BucketName with S3-compatible validation rules
    pub fn new(value: String) -> Result<Self, ValidationError> {
        // Length validation
        if value.len() < 3 {
            return Err(ValidationError::BucketNameTooShort {
                actual: value.len(),
                min: 3,
            });
        }

        if value.len() > 63 {
            return Err(ValidationError::BucketNameTooLong {
                actual: value.len(),
                max: 63,
            });
        }

        // Must start and end with lowercase letter or number
        if !value
            .chars()
            .next()
            .map_or(false, |c| c.is_ascii_lowercase() || c.is_ascii_digit())
        {
            return Err(ValidationError::BucketNameInvalidStart);
        }

        if !value
            .chars()
            .last()
            .map_or(false, |c| c.is_ascii_lowercase() || c.is_ascii_digit())
        {
            return Err(ValidationError::BucketNameInvalidEnd);
        }

        // Check for valid characters (lowercase, numbers, hyphens)
        for c in value.chars() {
            if !c.is_ascii_lowercase() && !c.is_ascii_digit() && c != '-' {
                return Err(ValidationError::BucketNameInvalidCharacter(c));
            }
        }

        // Cannot contain consecutive hyphens
        if value.contains("--") {
            return Err(ValidationError::BucketNameConsecutiveHyphens);
        }

        // Cannot be formatted as IP address
        if Self::looks_like_ip_address(&value) {
            return Err(ValidationError::BucketNameLooksLikeIpAddress);
        }

        Ok(Self(value))
    }

    /// Get the bucket name as a string slice
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Check if a string looks like an IP address
    fn looks_like_ip_address(s: &str) -> bool {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 4 {
            return false;
        }

        parts.iter().all(|part| part.parse::<u8>().is_ok())
    }
}

impl std::fmt::Display for BucketName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_bucket_names() {
        assert!(BucketName::new("my-bucket".to_string()).is_ok());
        assert!(BucketName::new("bucket123".to_string()).is_ok());
        assert!(BucketName::new("123bucket".to_string()).is_ok());
        assert!(BucketName::new("my-bucket-123".to_string()).is_ok());
    }

    #[test]
    fn test_invalid_bucket_names() {
        // Too short
        assert!(BucketName::new("ab".to_string()).is_err());

        // Too long
        assert!(BucketName::new("a".repeat(64)).is_err());

        // Invalid start/end
        assert!(BucketName::new("-bucket".to_string()).is_err());
        assert!(BucketName::new("bucket-".to_string()).is_err());
        assert!(BucketName::new("Bucket".to_string()).is_err()); // uppercase

        // Invalid characters
        assert!(BucketName::new("my_bucket".to_string()).is_err());
        assert!(BucketName::new("my.bucket".to_string()).is_err());
        assert!(BucketName::new("my bucket".to_string()).is_err());

        // Consecutive hyphens
        assert!(BucketName::new("my--bucket".to_string()).is_err());

        // IP address format
        assert!(BucketName::new("192.168.1.1".to_string()).is_err());
    }
}
