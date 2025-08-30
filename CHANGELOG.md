# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2024-XX-XX

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