/// Validation errors for domain value objects
#[derive(Debug, Clone, PartialEq)]
pub enum ValidationError {
    // ObjectKey validation errors
    EmptyObjectKey,
    ObjectKeyTooLong {
        actual: usize,
        max: usize,
    },
    InvalidObjectKeyCharacter(char),
    ObjectKeyStartsWithSlash,
    ObjectKeyContainsDoubleSlash,

    // BucketName validation errors
    BucketNameTooShort {
        actual: usize,
        min: usize,
    },
    BucketNameTooLong {
        actual: usize,
        max: usize,
    },
    BucketNameInvalidStart,
    BucketNameInvalidEnd,
    BucketNameInvalidCharacter(char),
    BucketNameConsecutiveHyphens,
    BucketNameLooksLikeIpAddress,

    // VersionId validation errors
    EmptyVersionId,
    VersionIdTooLong {
        actual: usize,
        max: usize,
    },
    InvalidVersionIdCharacter(char),

    // Lifecycle validation errors
    DuplicateRuleId(String),
    RuleIdTooLong(String),
    NoActionsInRule(String),
    InvalidField {
        field: String,
        value: String,
        expected: String,
    },
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // ObjectKey errors
            ValidationError::EmptyObjectKey => write!(f, "Object key cannot be empty"),
            ValidationError::ObjectKeyTooLong { actual, max } => {
                write!(f, "Object key too long: {} bytes (max: {})", actual, max)
            }
            ValidationError::InvalidObjectKeyCharacter(c) => {
                write!(f, "Invalid character in object key: '{}'", c)
            }
            ValidationError::ObjectKeyStartsWithSlash => {
                write!(f, "Object key cannot start with '/'")
            }
            ValidationError::ObjectKeyContainsDoubleSlash => {
                write!(f, "Object key cannot contain '//'")
            }

            // BucketName errors
            ValidationError::BucketNameTooShort { actual, min } => {
                write!(
                    f,
                    "Bucket name too short: {} characters (min: {})",
                    actual, min
                )
            }
            ValidationError::BucketNameTooLong { actual, max } => {
                write!(
                    f,
                    "Bucket name too long: {} characters (max: {})",
                    actual, max
                )
            }
            ValidationError::BucketNameInvalidStart => {
                write!(f, "Bucket name must start with lowercase letter or number")
            }
            ValidationError::BucketNameInvalidEnd => {
                write!(f, "Bucket name must end with lowercase letter or number")
            }
            ValidationError::BucketNameInvalidCharacter(c) => {
                write!(
                    f,
                    "Invalid character in bucket name: '{}'. Only lowercase letters, numbers, and hyphens allowed",
                    c
                )
            }
            ValidationError::BucketNameConsecutiveHyphens => {
                write!(f, "Bucket name cannot contain consecutive hyphens")
            }
            ValidationError::BucketNameLooksLikeIpAddress => {
                write!(f, "Bucket name cannot be formatted as an IP address")
            }

            // VersionId errors
            ValidationError::EmptyVersionId => write!(f, "Version ID cannot be empty"),
            ValidationError::VersionIdTooLong { actual, max } => {
                write!(
                    f,
                    "Version ID too long: {} characters (max: {})",
                    actual, max
                )
            }
            ValidationError::InvalidVersionIdCharacter(c) => {
                write!(f, "Invalid character in version ID: '{}'", c)
            }

            // Lifecycle errors
            ValidationError::DuplicateRuleId(id) => {
                write!(f, "Duplicate lifecycle rule ID: {}", id)
            }
            ValidationError::RuleIdTooLong(id) => {
                write!(f, "Lifecycle rule ID too long (max 255 characters): {}", id)
            }
            ValidationError::NoActionsInRule(id) => {
                write!(f, "Lifecycle rule '{}' has no actions defined", id)
            }
            ValidationError::InvalidField {
                field,
                value,
                expected,
            } => {
                write!(
                    f,
                    "Invalid value for field '{}': '{}' (expected: {})",
                    field, value, expected
                )
            }
        }
    }
}

impl std::error::Error for ValidationError {}
