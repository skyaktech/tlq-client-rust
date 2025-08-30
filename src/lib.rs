//! # TLQ Rust Client
//!
//! A Rust client library for [TLQ (Tiny Little Queue)](https://github.com/skyak/tlq) - a minimal, in-memory message queue server.
//!
//! This library provides an async, type-safe interface for interacting with TLQ servers,
//! featuring automatic retry with exponential backoff, comprehensive error handling,
//! and a builder pattern for flexible configuration.
//!
//! ## Quick Start
//!
//! ```no_run
//! use tlq_client::TlqClient;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create a client
//!     let client = TlqClient::new("localhost", 1337)?;
//!
//!     // Add a message to the queue
//!     let message = client.add_message("Hello, TLQ!").await?;
//!     println!("Added message with ID: {}", message.id);
//!
//!     // Retrieve messages from the queue
//!     let messages = client.get_messages(5).await?;
//!     for msg in messages {
//!         println!("Message: {} - {}", msg.id, msg.body);
//!         
//!         // Delete the message when done
//!         client.delete_message(msg.id).await?;
//!     }
//!     
//!     Ok(())
//! }
//! ```
//!
//! ## Features
//!
//! - **Async/await support** - Built on Tokio for high-performance async operations
//! - **Automatic retry** - Exponential backoff for transient failures
//! - **Type safety** - Strong typing with `serde` for JSON serialization
//! - **Builder pattern** - Flexible configuration with [`ConfigBuilder`]
//! - **Error handling** - Comprehensive error types with retryable classification
//! - **Message validation** - Enforces 64KB message size limit
//! - **UUID v7 IDs** - Time-ordered message identifiers
//!
//! ## Configuration
//!
//! Use [`ConfigBuilder`] for advanced configuration:
//!
//! ```no_run
//! use tlq_client::{TlqClient, ConfigBuilder};
//! use std::time::Duration;
//!
//! # async fn example() -> Result<(), tlq_client::TlqError> {
//! let client = TlqClient::with_config(
//!     ConfigBuilder::new()
//!         .host("queue.example.com")
//!         .port(8080)
//!         .timeout(Duration::from_secs(10))
//!         .max_retries(5)
//!         .retry_delay(Duration::from_millis(200))
//!         .build()
//! );
//! # Ok(())
//! # }
//! ```

pub mod client;
pub mod config;
pub mod error;
pub mod message;
mod retry;

pub use client::TlqClient;
pub use config::{Config, ConfigBuilder};
pub use error::{Result, TlqError};
pub use message::{Message, MessageState};
