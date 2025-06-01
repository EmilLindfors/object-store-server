use crate::domain::errors::ValidationError;

/// A validated object key (path) in the storage system
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ObjectKey(String);

impl ObjectKey {
    /// Create a new ObjectKey with validation
    pub fn new(value: String) -> Result<Self, ValidationError> {
        // Validate the key
        if value.is_empty() {
            return Err(ValidationError::EmptyObjectKey);
        }

        if value.len() > 1024 {
            return Err(ValidationError::ObjectKeyTooLong {
                actual: value.len(),
                max: 1024,
            });
        }

        // Check for invalid characters (null bytes)
        if value.contains('\0') {
            return Err(ValidationError::InvalidObjectKeyCharacter('\0'));
        }

        // Check for invalid patterns
        if value.starts_with('/') {
            return Err(ValidationError::ObjectKeyStartsWithSlash);
        }

        if value.contains("//") {
            return Err(ValidationError::ObjectKeyContainsDoubleSlash);
        }

        Ok(Self(value))
    }

    /// Get the key as a string slice
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Get the directory part of the key (everything before the last '/')
    pub fn parent(&self) -> Option<String> {
        self.0.rfind('/').map(|idx| self.0[..idx].to_string())
    }

    /// Get the file name part of the key (everything after the last '/')
    pub fn file_name(&self) -> &str {
        self.0.rfind('/').map_or(&self.0, |idx| &self.0[idx + 1..])
    }

    /// Check if this key has the given prefix
    pub fn has_prefix(&self, prefix: &str) -> bool {
        self.0.starts_with(prefix)
    }

    /// Join this key with a suffix
    pub fn join(&self, suffix: &str) -> Result<ObjectKey, ValidationError> {
        let mut new_key = self.0.clone();
        if !new_key.ends_with('/') && !suffix.starts_with('/') {
            new_key.push('/');
        }
        new_key.push_str(suffix);
        ObjectKey::new(new_key)
    }
}

impl std::fmt::Display for ObjectKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_object_key() {
        assert!(ObjectKey::new("file.txt".to_string()).is_ok());
        assert!(ObjectKey::new("folder/file.txt".to_string()).is_ok());
        assert!(ObjectKey::new("deep/folder/structure/file.txt".to_string()).is_ok());
    }

    #[test]
    fn test_invalid_object_key() {
        assert!(ObjectKey::new("".to_string()).is_err());
        assert!(ObjectKey::new("/leading-slash".to_string()).is_err());
        assert!(ObjectKey::new("double//slash".to_string()).is_err());
        assert!(ObjectKey::new("null\0byte".to_string()).is_err());
        assert!(ObjectKey::new("x".repeat(1025)).is_err());
    }

    #[test]
    fn test_object_key_parts() {
        let key = ObjectKey::new("folder/subfolder/file.txt".to_string()).unwrap();
        assert_eq!(key.parent(), Some("folder/subfolder".to_string()));
        assert_eq!(key.file_name(), "file.txt");

        let root_key = ObjectKey::new("file.txt".to_string()).unwrap();
        assert_eq!(root_key.parent(), None);
        assert_eq!(root_key.file_name(), "file.txt");
    }
}
