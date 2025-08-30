# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2025-08-30

### Added
- Comprehensive API documentation following Rust best practices
- Complete documentation for all public APIs with examples
- Crate-level documentation with quick start guide and feature overview
- Method-level documentation with parameters, returns, examples, and error conditions
- Error type documentation with detailed descriptions and retryable classification
- Configuration builder pattern documentation with usage examples
- Internal retry mechanism documentation explaining exponential backoff

### Documentation
- Added 32+ working documentation examples that are tested with `cargo test --doc`
- All examples follow `no_run` pattern for network-dependent operations
- Cross-references between related types using proper Rust doc links
- Comprehensive error documentation with usage patterns
- Professional docs.rs compatible documentation structure

## [0.1.1] - 2025-08-30

### Changed
- Updated dependencies and bumped version
- Code formatting improvements
- CI/CD pipeline optimizations

## [0.1.0] - 2025-08-29

### Added
- Initial release of TLQ Rust client
- Async/await support with Tokio
- Builder pattern for client configuration
- Comprehensive error handling with custom error types
- Automatic retry with exponential backoff
- Full API coverage:
  - Health check
  - Add message
  - Get messages (single and batch)
  - Delete messages (single and batch)
  - Retry messages (single and batch)
  - Purge queue
- Message size validation (64KB limit)
- UUID v7 support for message IDs
- Comprehensive test suite
- Example applications (basic usage, worker, batch processing)
- CI/CD with GitHub Actions