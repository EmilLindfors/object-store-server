# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Object Store Bridge is a Rust library that extends Apache Arrow's `object_store` crate with advanced features including:

1. **Versioning**: Automatic tracking and management of object versions
2. **Lifecycle Management**: Expiration and transition rules for objects
3. **Axum Web Framework Integration**: Tower middleware for HTTP API access

The library serves as a bridge between object storage systems (like S3, Azure Blob Storage, etc.) and web applications built with Axum, providing a RESTful API for object operations.

## Core Architecture

The project is organized around these key components:

1. **VersionedStore**: A wrapper around any `ObjectStore` implementation that adds versioning capabilities
   - Tracks object versions with metadata
   - Allows retrieving specific versions
   - Stores versions in the underlying store with version-specific paths

2. **LifecycleManager**: Manages object lifecycle policies
   - Configurable expiration and transition rules
   - Background worker for applying rules
   - Rule filtering by object prefix and tags

3. **ObjectStoreService**: Core service exposing storage operations
   - Provides CRUD operations for objects with version tracking
   - Manages lifecycle configurations
   - Handles multipart uploads

4. **ObjectStoreLayer**: Tower middleware for HTTP integration
   - Intercepts requests to specific API paths
   - Routes to appropriate handlers
   - Integrates with Axum routing

## Common Commands

### Building
```bash
# Build the library
cargo build

# Build with optimizations
cargo build --release
```

### Testing
```bash
# Run all tests
cargo test

# Run specific test module
cargo test --package object_store_bridge --test lifecycle_tests

# Run specific test
cargo test --package object_store_bridge --test lifecycle_tests tests::test_lifecycle_configuration
```

### Running Examples
```bash
# Run the simple example
cargo run --example simple
```

### Documentation
```bash
# Generate and open documentation for this crate and all dependencies
cargo doc --open

# View documentation for specific dependencies
cargo doc --open --package object_store
cargo doc --open --package axum
```

### Project Information
```bash
# View detailed project metadata
cargo metadata

# View dependency graph
cargo metadata --format-version 1 | jq '.resolve.nodes'
```

### Code Style and Linting
```bash
# Format code
cargo fmt

# Check code formatting
cargo fmt --check

# Run clippy lints
cargo clippy
```

## Development Guidelines

When extending or modifying the code, consider the following:

1. **Implementation Compatibility**: All implementations should be compatible with the `object_store` trait interfaces.

2. **Error Handling**: Use the custom `StoreError` type for internal errors, which can be converted to HTTP status codes.

3. **Async Execution**: The project heavily uses async/await patterns with Tokio for asynchronous operation.

4. **Testing Approach**: Use the mock store pattern in `tests/` for unit testing, with the ability to manipulate timestamps for lifecycle testing.

5. **Middleware Integration**: Tower middleware follows the standard pattern with layer and service implementations.

## Key API Features

The library exposes these main capabilities:

- Object CRUD operations with automatic versioning
- Version management (retrieve, list, delete specific versions)
- Lifecycle configuration (expiration and transition rules)
- Integration with Axum via Tower middleware
- Multipart upload support

## Testing Strategy

The test suite uses mock implementations of the `ObjectStore` trait to validate functionality without requiring actual cloud storage. Key test areas include:

1. Versioning behavior (creation, retrieval, deletion)
2. Lifecycle rule configuration and validation
3. Lifecycle rule application logic (with timestamp manipulation)
4. Middleware request handling and routing

When adding new features, ensure appropriate test coverage using the existing patterns.