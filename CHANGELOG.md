# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.5.0] - 2026-10-31

### Added
- `server_info()` and `ServerInfo` with the original greeting, optional server version, and optional uptime as `Duration`
- Support for TLQ 0.5.0 health metadata and legacy `"Hello World"` responses
- `Config::max_message_size` and `ConfigBuilder::max_message_size(bytes)` for configurable client-side message limits (default: 65,536 bytes)
- `TlqError::OutcomeUnknown { cause }`, returned when a queue-changing request was sent but no valid response arrived; not retryable
- HTTP compatibility tests against a mock server for health responses, errors, timeouts, replay safety, and message-size boundaries

### Changed (breaking)
- `Config` gains `max_message_size`; struct literals must include it or use `..Config::default()`
- `MessageTooLarge` gains `limit: usize` and reports the configured byte limit
- `TlqError::MaxRetriesExceeded` removed; it was never returned
- `health_check()` uses the configured timeout and retry settings instead of a fixed 5-second timeout, and returns `Ok(false)` on HTTP error responses
- Retries now apply only to connecting. A request that has been sent is never replayed; previously POST requests could be re-sent after a timeout or I/O error, duplicating queue operations
- Corrected delete/retry/purge documentation to describe `"Success"` responses rather than affected-message counts

### Fixed
- Request timeouts now cover writing and reading, not only connecting
- Exponential backoff uses checked arithmetic with a 30-second cap, so large retry counts or base delays no longer overflow

## [0.4.0] - 2026-03-21

### Added
- `QueueStats` type and `stats()` method for retrieving queue statistics via GET /stats
- `ConfigBuilder::build_config()` method for direct `Config` access

### Changed
- `ConfigBuilder::build()` now returns `TlqClient` directly instead of `Config`
- `lock_until` field type changed from `Option<String>` to `Option<u64>` (Unix timestamp in milliseconds)

### Removed
- `MessageState::Failed` variant (server v0.4.0 uses automatic background reaper)

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
