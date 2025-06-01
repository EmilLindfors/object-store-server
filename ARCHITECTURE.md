# Hexagonal Architecture Guide for Rust Projects

## Overview

This guide instructs how to structure Rust applications using hexagonal architecture (ports & adapters). Follow these patterns to create maintainable, testable, and scalable applications.

## Core Principles

1. **Domain Independence**: Business logic must never depend on external libraries or infrastructure
2. **Dependency Direction**: All dependencies point inward toward the domain
3. **Port Abstraction**: All external interactions go through trait interfaces (ports)
4. **Adapter Implementation**: Concrete implementations (adapters) live outside the domain

## Project Structure

```
project-root/
├── Cargo.toml
├── src/
│   ├── lib.rs                    # Library root, exports public API
│   ├── domain/                   # Core business logic
│   │   ├── mod.rs
│   │   ├── models/              # Domain entities and value objects
│   │   │   ├── mod.rs
│   │   │   ├── author.rs
│   │   │   └── post.rs
│   │   ├── errors/              # Domain-specific errors
│   │   │   ├── mod.rs
│   │   │   └── author_errors.rs
│   │   └── value_objects/       # Strongly typed values
│   │       ├── mod.rs
│   │       ├── author_id.rs
│   │       ├── email.rs
│   │       └── author_name.rs
│   ├── ports/                   # Trait definitions (interfaces)
│   │   ├── mod.rs
│   │   ├── repositories/        # Data persistence ports
│   │   │   ├── mod.rs
│   │   │   └── author_repository.rs
│   │   ├── notifiers/          # External notification ports
│   │   │   ├── mod.rs
│   │   │   └── author_notifier.rs
│   │   └── services/           # Service trait definitions
│   │       ├── mod.rs
│   │       └── author_service.rs
│   ├── adapters/               # Port implementations
│   │   ├── mod.rs
│   │   ├── inbound/           # Handle incoming requests
│   │   │   ├── mod.rs
│   │   │   ├── http/          # HTTP handlers
│   │   │   │   ├── mod.rs
│   │   │   │   ├── handlers/
│   │   │   │   │   ├── mod.rs
│   │   │   │   │   └── author_handlers.rs
│   │   │   │   ├── dto/       # Data Transfer Objects
│   │   │   │   │   ├── mod.rs
│   │   │   │   │   └── author_dto.rs
│   │   │   │   └── server.rs  # HTTP server setup
│   │   │   └── cli/           # CLI handlers
│   │   │       └── mod.rs
│   │   └── outbound/          # External service implementations
│   │       ├── mod.rs
│   │       ├── persistence/   # Database implementations
│   │       │   ├── mod.rs
│   │       │   ├── sqlite/
│   │       │   │   ├── mod.rs
│   │       │   │   └── author_repository.rs
│   │       │   └── postgres/
│   │       │       └── mod.rs
│   │       └── notifications/ # Notification implementations
│   │           ├── mod.rs
│   │           └── email/
│   │               └── author_notifier.rs
│   ├── services/              # Business logic orchestration
│   │   ├── mod.rs
│   │   └── author_service.rs
│   └── bin/                   # Application entry points
│       └── server/
│           └── main.rs
└── tests/                     # Integration tests
    └── integration/
```

## Implementation Guidelines

### 1. Domain Models

```rust
// src/domain/models/author.rs
use crate::domain::value_objects::{AuthorId, AuthorName, Email};

#[derive(Debug, Clone, PartialEq)]
pub struct Author {
    pub id: AuthorId,
    pub name: AuthorName,
    pub email: Email,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct CreateAuthorRequest {
    pub name: AuthorName,
    pub email: Email,
}

impl CreateAuthorRequest {
    pub fn new(name: String, email: String) -> Result<Self, ValidationError> {
        Ok(Self {
            name: AuthorName::new(name)?,
            email: Email::new(email)?,
        })
    }
}
```

### 2. Value Objects

```rust
// src/domain/value_objects/email.rs
#[derive(Debug, Clone, PartialEq)]
pub struct Email(String);

impl Email {
    pub fn new(value: String) -> Result<Self, ValidationError> {
        // Email validation logic
        if value.contains('@') && value.len() > 3 {
            Ok(Self(value))
        } else {
            Err(ValidationError::InvalidEmail)
        }
    }
    
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
```

### 3. Domain Errors

```rust
// src/domain/errors/author_errors.rs
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CreateAuthorError {
    #[error("Author with this email already exists")]
    DuplicateEmail,
    
    #[error("Invalid author data")]
    ValidationError(#[from] ValidationError),
    
    #[error("Unexpected error occurred")]
    Unknown(#[from] anyhow::Error),
}

#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("Invalid email format")]
    InvalidEmail,
    
    #[error("Name must be between 1 and 100 characters")]
    InvalidName,
}
```

### 4. Port Definitions

```rust
// src/ports/repositories/author_repository.rs
use async_trait::async_trait;
use crate::domain::{
    models::{Author, CreateAuthorRequest},
    errors::CreateAuthorError,
    value_objects::Email,
};

#[async_trait]
pub trait AuthorRepository: Send + Sync + Clone + 'static {
    async fn create(
        &self,
        request: &CreateAuthorRequest
    ) -> Result<Author, CreateAuthorError>;
    
    async fn find_by_email(
        &self,
        email: &Email
    ) -> Result<Option<Author>, anyhow::Error>;
    
    async fn find_by_id(
        &self,
        id: &AuthorId
    ) -> Result<Option<Author>, anyhow::Error>;
}

// src/ports/notifiers/author_notifier.rs
#[async_trait]
pub trait AuthorNotifier: Send + Sync + Clone + 'static {
    async fn send_welcome(&self, author: &Author) -> Result<(), anyhow::Error>;
}
```

### 5. Service Implementation

```rust
// src/services/author_service.rs
use std::sync::Arc;
use crate::{
    domain::{models::*, errors::*},
    ports::{repositories::*, notifiers::*},
};

#[derive(Clone)]
pub struct AuthorService {
    repository: Arc<dyn AuthorRepository>,
    notifier: Arc<dyn AuthorNotifier>,
}

impl AuthorService {
    pub fn new(
        repository: Arc<dyn AuthorRepository>,
        notifier: Arc<dyn AuthorNotifier>,
    ) -> Self {
        Self { repository, notifier }
    }
    
    pub async fn create_author(
        &self,
        request: CreateAuthorRequest
    ) -> Result<Author, CreateAuthorError> {
        // Check for duplicate
        if let Some(_) = self.repository
            .find_by_email(&request.email)
            .await? {
            return Err(CreateAuthorError::DuplicateEmail);
        }
        
        // Create author
        let author = self.repository.create(&request).await?;
        
        // Send notification (fire-and-forget)
        let notifier = self.notifier.clone();
        let author_clone = author.clone();
        tokio::spawn(async move {
            if let Err(e) = notifier.send_welcome(&author_clone).await {
                tracing::error!("Failed to send welcome email: {}", e);
            }
        });
        
        Ok(author)
    }
}
```

### 6. Adapter Implementation

```rust
// src/adapters/outbound/persistence/sqlite/author_repository.rs
use async_trait::async_trait;
use sqlx::SqlitePool;
use crate::{
    domain::{models::*, errors::*, value_objects::*},
    ports::repositories::AuthorRepository,
};

#[derive(Clone)]
pub struct SqliteAuthorRepository {
    pool: SqlitePool,
}

impl SqliteAuthorRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl AuthorRepository for SqliteAuthorRepository {
    async fn create(
        &self,
        request: &CreateAuthorRequest
    ) -> Result<Author, CreateAuthorError> {
        let id = AuthorId::new();
        let now = chrono::Utc::now();
        
        sqlx::query!(
            r#"
            INSERT INTO authors (id, name, email, created_at)
            VALUES (?1, ?2, ?3, ?4)
            "#,
            id.as_str(),
            request.name.as_str(),
            request.email.as_str(),
            now
        )
        .execute(&self.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(db_err) 
                if db_err.constraint() == Some("authors_email_key") => {
                CreateAuthorError::DuplicateEmail
            },
            _ => CreateAuthorError::Unknown(e.into()),
        })?;
        
        Ok(Author {
            id,
            name: request.name.clone(),
            email: request.email.clone(),
            created_at: now,
        })
    }
    
    async fn find_by_email(
        &self,
        email: &Email
    ) -> Result<Option<Author>, anyhow::Error> {
        // Implementation details...
        Ok(None)
    }
}
```

### 7. HTTP Handler

```rust
// src/adapters/inbound/http/handlers/author_handlers.rs
use axum::{extract::State, Json, http::StatusCode};
use crate::{
    services::AuthorService,
    adapters::inbound::http::dto::{CreateAuthorDto, AuthorDto, ApiError},
};

pub async fn create_author(
    State(service): State<AuthorService>,
    Json(dto): Json<CreateAuthorDto>,
) -> Result<(StatusCode, Json<AuthorDto>), ApiError> {
    // Convert DTO to domain request
    let request = dto.try_into_domain()
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    
    // Call service
    let author = service.create_author(request).await
        .map_err(|e| match e {
            CreateAuthorError::DuplicateEmail => {
                ApiError::conflict("Email already exists")
            },
            CreateAuthorError::ValidationError(ve) => {
                ApiError::bad_request(ve.to_string())
            },
            _ => ApiError::internal_server_error(),
        })?;
    
    // Convert to DTO
    Ok((StatusCode::CREATED, Json(AuthorDto::from(author))))
}
```

### 8. Main Entry Point

```rust
// src/bin/server/main.rs
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();
    
    // Load configuration
    let config = Config::from_env()?;
    
    // Setup database
    let db_pool = SqlitePool::connect(&config.database_url).await?;
    
    // Create adapters
    let author_repository = Arc::new(
        SqliteAuthorRepository::new(db_pool.clone())
    );
    let author_notifier = Arc::new(
        EmailAuthorNotifier::new(config.smtp)
    );
    
    // Create services
    let author_service = AuthorService::new(
        author_repository,
        author_notifier,
    );
    
    // Create HTTP server
    let app = create_app(author_service);
    
    // Start server
    let addr = format!("0.0.0.0:{}", config.port);
    tracing::info!("Starting server on {}", addr);
    
    axum::Server::bind(&addr.parse()?)
        .serve(app.into_make_service())
        .await?;
    
    Ok(())
}
```

## Testing Strategy

### 1. Unit Tests for Domain Logic

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_email_validation() {
        assert!(Email::new("valid@email.com".to_string()).is_ok());
        assert!(Email::new("invalid".to_string()).is_err());
    }
}
```

### 2. Service Tests with Mocks

```rust
#[cfg(test)]
mod tests {
    use mockall::mock;
    
    mock! {
        AuthorRepo {}
        
        #[async_trait]
        impl AuthorRepository for AuthorRepo {
            async fn create(&self, request: &CreateAuthorRequest) 
                -> Result<Author, CreateAuthorError>;
            async fn find_by_email(&self, email: &Email) 
                -> Result<Option<Author>, anyhow::Error>;
        }
    }
    
    #[tokio::test]
    async fn test_create_author_success() {
        let mut mock_repo = MockAuthorRepo::new();
        mock_repo.expect_find_by_email()
            .returning(|_| Ok(None));
        mock_repo.expect_create()
            .returning(|req| Ok(Author { /* ... */ }));
            
        let service = AuthorService::new(
            Arc::new(mock_repo),
            Arc::new(NoOpNotifier),
        );
        
        let result = service.create_author(request).await;
        assert!(result.is_ok());
    }
}
```

## Key Rules

1. **NEVER** import external crates (sqlx, axum, etc.) in domain code
2. **ALWAYS** use Result types with domain-specific errors
3. **NEVER** pass DTOs into domain or service layers
4. **ALWAYS** convert between DTOs and domain models at adapter boundaries
5. **NEVER** use primitive types directly - wrap in value objects
6. **ALWAYS** make ports async with Send + Sync + Clone + 'static bounds
7. **NEVER** expose implementation details through ports
8. **ALWAYS** handle errors appropriately at each layer

## Common Patterns

### Error Conversion at Boundaries

```rust
impl From<CreateAuthorError> for ApiError {
    fn from(err: CreateAuthorError) -> Self {
        match err {
            CreateAuthorError::DuplicateEmail => {
                ApiError::Conflict("Email already in use".into())
            }
            CreateAuthorError::ValidationError(e) => {
                ApiError::BadRequest(e.to_string())
            }
            CreateAuthorError::Unknown(_) => {
                ApiError::InternalServerError
            }
        }
    }
}
```

### Repository Pattern with Transactions

```rust
#[async_trait]
pub trait UnitOfWork: Send + Sync {
    async fn begin(&self) -> Result<Box<dyn Transaction>, anyhow::Error>;
}

#[async_trait]
pub trait Transaction: Send + Sync {
    async fn commit(self: Box<Self>) -> Result<(), anyhow::Error>;
    async fn rollback(self: Box<Self>) -> Result<(), anyhow::Error>;
}
```

### Service Composition

```rust
pub struct BlogService {
    author_service: AuthorService,
    post_service: PostService,
}

impl BlogService {
    pub async fn create_author_with_first_post(
        &self,
        author_request: CreateAuthorRequest,
        post_content: String,
    ) -> Result<(Author, Post), BlogError> {
        let author = self.author_service
            .create_author(author_request)
            .await?;
            
        let post = self.post_service
            .create_post(CreatePostRequest {
                author_id: author.id.clone(),
                content,
            })
            .await?;
            
        Ok((author, post))
    }
}
```

## Migration from Existing Code

When migrating existing code to hexagonal architecture:

1. Start by identifying your core domain models
2. Extract business logic into domain services
3. Define ports for external dependencies
4. Wrap external libraries in adapter implementations
5. Gradually move HTTP/CLI handlers to use services
6. Add comprehensive tests at each layer

Remember: The goal is to protect your business logic from external changes while maintaining flexibility and testability.