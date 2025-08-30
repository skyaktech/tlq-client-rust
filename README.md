# TLQ Rust Client

A high-performance, async Rust client for [TLQ (Tiny Little Queue)](https://github.com/skyaktech/tlq) - a minimal, in-memory message queue server.

## Features

- **Async/await support** with Tokio
- **Automatic retry** with exponential backoff
- **Builder pattern** for flexible configuration
- **Type-safe** message handling
- **Zero external dependencies** (only tokio, serde, uuid, thiserror)
- **High performance** with minimal overhead

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
tlq-client = "0.1.0"
```

## Quick Start

```rust
use tlq_client::TlqClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a client
    let client = TlqClient::new("localhost", 1337)?;

    // Add a message to the queue
    let message = client.add_message("Hello, TLQ!").await?;
    println!("Added message: {}", message.id);

    // Get messages from the queue
    let messages = client.get_messages(1).await?;
    for msg in &messages {
        println!("Got message: {}", msg.body);
        
        // Delete the message after processing
        client.delete_message(msg.id).await?;
    }

    Ok(())
}
```

## Configuration

### Using the Builder Pattern

```rust
use tlq_client::TlqClient;
use std::time::Duration;

let client = TlqClient::builder()
    .host("localhost")
    .port(1337)
    .timeout_ms(5000)
    .max_retries(3)
    .retry_delay_ms(100)
    .build();

let client = TlqClient::with_config(client);
```

### Configuration Options

- `host`: Server hostname (default: "localhost")
- `port`: Server port (default: 1337)
- `timeout`: Request timeout (default: 30 seconds)
- `max_retries`: Maximum retry attempts (default: 3)
- `retry_delay`: Base delay between retries (default: 100ms)

## API Reference

### Core Methods

#### `health_check()`
Check if the TLQ server is responsive.

```rust
let is_healthy = client.health_check().await?;
```

#### `add_message(body)`
Add a message to the queue.

```rust
let message = client.add_message("Hello, World!").await?;
```

#### `get_messages(count)`
Retrieve multiple messages from the queue.

```rust
let messages = client.get_messages(5).await?;
```

#### `get_message()`
Retrieve a single message from the queue.

```rust
if let Some(message) = client.get_message().await? {
    println!("Got: {}", message.body);
}
```

#### `delete_message(id)` / `delete_messages(ids)`
Delete processed messages from the queue.

```rust
// Delete single message
client.delete_message(message_id).await?;

// Delete multiple messages
client.delete_messages(&[id1, id2, id3]).await?;
```

#### `retry_message(id)` / `retry_messages(ids)`
Return messages to the queue for retry.

```rust
// Retry single message
client.retry_message(message_id).await?;

// Retry multiple messages
client.retry_messages(&[id1, id2, id3]).await?;
```

#### `purge_queue()`
Remove all messages from the queue.

```rust
let purged_count = client.purge_queue().await?;
```

## Error Handling

The client provides comprehensive error types:

```rust
use tlq_client::TlqError;

match client.add_message("test").await {
    Ok(msg) => println!("Success: {}", msg.id),
    Err(TlqError::Connection(e)) => println!("Connection error: {}", e),
    Err(TlqError::Timeout(ms)) => println!("Timeout after {}ms", ms),
    Err(TlqError::MessageTooLarge { size }) => println!("Message too large: {} bytes", size),
    Err(e) => println!("Other error: {}", e),
}
```

### Retryable Errors

Some errors are automatically retried:

```rust
if error.is_retryable() {
    // Will be retried automatically
}
```

## Examples

### Worker Pattern

```rust
use tlq_client::TlqClient;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = TlqClient::new("localhost", 1337)?;

    loop {
        match client.get_message().await? {
            Some(message) => {
                // Process the message
                println!("Processing: {}", message.body);
                
                // Delete after successful processing
                client.delete_message(message.id).await?;
            }
            None => {
                // No messages available, wait before polling again
                sleep(Duration::from_secs(1)).await;
            }
        }
    }
}
```

### Batch Processing

```rust
let messages = client.get_messages(10).await?;

let mut successful = Vec::new();
let mut failed = Vec::new();

for msg in messages {
    if process_message(&msg).await.is_ok() {
        successful.push(msg.id);
    } else {
        failed.push(msg.id);
    }
}

// Delete successful messages
if !successful.is_empty() {
    client.delete_messages(&successful).await?;
}

// Retry failed messages
if !failed.is_empty() {
    client.retry_messages(&failed).await?;
}
```

## Running Examples

The repository includes several example programs:

```bash
# Basic usage
cargo run --example basic_usage

# Worker pattern
cargo run --example worker

# Batch processing
cargo run --example batch_processing
```

## Testing

Run the test suite:

```bash
cargo test
```

Run tests with coverage:

```bash
cargo tarpaulin
```

## Performance

The client is designed for high performance:

- Minimal allocations
- Efficient serialization with serde
- Connection pooling (coming soon)
- Zero-copy where possible

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

This project is licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

## Related Projects

- [TLQ Server](https://github.com/skyaktech/tlq) - The TLQ server implementation
- [TLQ Node.js Client](https://github.com/skyaktech/tlq-client-node)
- [TLQ Go Client](https://github.com/skyaktech/tlq-client-go)
- [TLQ Python Client](https://github.com/skyaktech/tlq-client-py)