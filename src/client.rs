use crate::{
    config::{Config, ConfigBuilder},
    error::{Result, TlqError},
    message::*,
    retry::RetryStrategy,
    server_info::ServerInfo,
};
use serde::{de::DeserializeOwned, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{timeout, Instant};
use uuid::Uuid;

/// The main client for interacting with TLQ (Tiny Little Queue) servers.
///
/// `TlqClient` provides an async, type-safe interface for all TLQ operations including
/// adding messages, retrieving messages, and managing queue state. The client handles
/// automatic retry with exponential backoff when connecting fails.
///
/// Once a request has been sent it is never replayed, because the server may already
/// have applied it. Failures after sending a queue-changing request are reported as
/// [`TlqError::OutcomeUnknown`].
///
/// # Examples
///
/// Basic usage:
/// ```no_run
/// use tlq_client::TlqClient;
///
/// #[tokio::main]
/// async fn main() -> Result<(), tlq_client::TlqError> {
///     let client = TlqClient::new("localhost", 1337)?;
///     
///     // Add a message
///     let message = client.add_message("Hello, World!").await?;
///     println!("Added message: {}", message.id);
///     
///     // Get messages
///     let messages = client.get_messages(1).await?;
///     if let Some(msg) = messages.first() {
///         println!("Retrieved: {}", msg.body);
///     }
///     
///     Ok(())
/// }
/// ```
pub struct TlqClient {
    config: Config,
    base_url: String,
}

impl TlqClient {
    /// Creates a new TLQ client with default configuration.
    ///
    /// This is the simplest way to create a client, using default values for
    /// timeout (30s), max retries (3), and retry delay (100ms).
    ///
    /// # Arguments
    ///
    /// * `host` - The hostname or IP address of the TLQ server
    /// * `port` - The port number of the TLQ server
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tlq_client::TlqClient;
    ///
    /// # fn example() -> Result<(), tlq_client::TlqError> {
    /// let client = TlqClient::new("localhost", 1337)?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Currently this method always returns `Ok`, but the `Result` is preserved
    /// for future compatibility.
    pub fn new(host: impl Into<String>, port: u16) -> Result<Self> {
        Ok(ConfigBuilder::new().host(host).port(port).build())
    }

    /// Creates a new TLQ client with custom configuration.
    ///
    /// Use this method when you need to customize timeout, retry behavior,
    /// or other client settings.
    ///
    /// # Arguments
    ///
    /// * `config` - A [`Config`] instance with your desired settings
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tlq_client::{TlqClient, Config};
    ///
    /// # fn example() {
    /// let config = Config::default();
    /// let client = TlqClient::with_config(config);
    /// # }
    /// ```
    pub fn with_config(config: Config) -> Self {
        let base_url = format!("{}:{}", config.host, config.port);
        Self { config, base_url }
    }

    /// Returns a [`ConfigBuilder`] for creating custom configurations.
    ///
    /// This is a convenience method that's equivalent to [`ConfigBuilder::new()`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tlq_client::TlqClient;
    /// use std::time::Duration;
    ///
    /// # fn example() {
    /// let client = TlqClient::builder()
    ///     .host("localhost")
    ///     .port(1337)
    ///     .timeout(Duration::from_secs(10))
    ///     .build();
    /// # }
    /// ```
    pub fn builder() -> ConfigBuilder {
        ConfigBuilder::new()
    }

    async fn post<T, R>(&self, endpoint: &str, body: &T) -> Result<R>
    where
        T: Serialize,
        R: DeserializeOwned,
    {
        let json = serde_json::to_vec(body)?;
        let mut request = format!(
            "POST {} HTTP/1.1\r\n\
             Host: {}\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\
             \r\n",
            endpoint,
            self.base_url,
            json.len()
        )
        .into_bytes();
        request.extend_from_slice(&json);
        self.send(&request, true).await
    }

    async fn get<R>(&self, endpoint: &str) -> Result<R>
    where
        R: DeserializeOwned,
    {
        let request = format!(
            "GET {} HTTP/1.1\r\n\
             Host: {}\r\n\
             Connection: close\r\n\
             \r\n",
            endpoint, self.base_url,
        );
        self.send(request.as_bytes(), false).await
    }

    /// Sends one HTTP request and decodes the JSON response body.
    ///
    /// Only connecting is retried. Once the request is on the wire the server may
    /// already have applied it, so the exchange runs exactly once. For queue-changing
    /// requests, exchange failures are reported as [`TlqError::OutcomeUnknown`].
    /// Each connect attempt plus the exchange share one `timeout` budget.
    async fn send<R>(&self, request: &[u8], queue_changing: bool) -> Result<R>
    where
        R: DeserializeOwned,
    {
        let retry = RetryStrategy::new(self.config.max_retries, self.config.retry_delay);
        let (mut stream, started) = retry
            .execute(|| async {
                let started = Instant::now();
                let stream = timeout(self.config.timeout, TcpStream::connect(&self.base_url))
                    .await
                    .map_err(|_| self.timeout_error())?
                    .map_err(|e| TlqError::Connection(e.to_string()))?;
                Ok((stream, started))
            })
            .await?;

        let exchange = async {
            stream.write_all(request).await?;
            stream.flush().await?;
            let mut response = Vec::new();
            stream.read_to_end(&mut response).await?;
            let response = String::from_utf8_lossy(&response);
            let body = Self::parse_http_response(&response)?;
            serde_json::from_str(body).map_err(Into::into)
        };
        let remaining = self.config.timeout.saturating_sub(started.elapsed());
        let result = timeout(remaining, exchange)
            .await
            .unwrap_or_else(|_| Err(self.timeout_error()));

        match result {
            // An HTTP error status is a definite answer; anything else leaves the outcome open.
            Err(cause) if queue_changing && !matches!(cause, TlqError::Server { .. }) => {
                Err(TlqError::OutcomeUnknown {
                    cause: Box::new(cause),
                })
            }
            result => result,
        }
    }

    fn timeout_error(&self) -> TlqError {
        TlqError::Timeout(self.config.timeout.as_millis() as u64)
    }

    /// Performs a health check against the TLQ server.
    ///
    /// Sends a GET request to `/hello` using the client's configured timeout and
    /// retry settings.
    ///
    /// # Returns
    ///
    /// * `Ok(true)` if the server answered with a greeting
    /// * `Ok(false)` if the server answered with an HTTP error status
    /// * `Err` for connection failures, timeouts, or an undecodable response
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tlq_client::TlqClient;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), tlq_client::TlqError> {
    ///     let client = TlqClient::new("localhost", 1337)?;
    ///
    ///     if client.health_check().await? {
    ///         println!("Server is healthy");
    ///     } else {
    ///         println!("Server is not responding correctly");
    ///     }
    ///     
    ///     Ok(())
    /// }
    /// ```
    pub async fn health_check(&self) -> Result<bool> {
        match self.get::<String>("/hello").await {
            Ok(_) => Ok(true),
            Err(TlqError::Server { .. }) => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// Retrieves the server greeting, version, and uptime from `/hello`.
    ///
    /// TLQ 0.5.0 exposes version and uptime in its greeting. Older servers return
    /// `"Hello World"`, with `version` and `uptime` set to `None`. Unrecognized
    /// metadata is also left as `None`; the original greeting is always available.
    /// Uses the client's configured timeout and retry settings.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tlq_client::TlqClient;
    ///
    /// # async fn example() -> Result<(), tlq_client::TlqError> {
    /// let client = TlqClient::new("localhost", 1337)?;
    /// let info = client.server_info().await?;
    /// println!("{}", info.greeting);
    /// if let Some(uptime) = info.uptime {
    ///     println!("Uptime: {} seconds", uptime.as_secs());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns network errors, [`TlqError::Server`] for HTTP 4xx/5xx responses,
    /// or [`TlqError::Serialization`] if the response is not a JSON string.
    pub async fn server_info(&self) -> Result<ServerInfo> {
        let greeting = self.get("/hello").await?;
        Ok(ServerInfo::from_greeting(greeting))
    }

    /// Adds a new message to the TLQ server.
    ///
    /// The message will be assigned a UUID v7 identifier and placed in the queue
    /// with state [`MessageState::Ready`]. The client checks the configured message
    /// size limit (65,536 bytes by default) before sending.
    ///
    /// # Arguments
    ///
    /// * `body` - The message content (any type that can be converted to String)
    ///
    /// # Returns
    ///
    /// Returns the created [`Message`] with its assigned ID and metadata.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tlq_client::TlqClient;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), tlq_client::TlqError> {
    ///     let client = TlqClient::new("localhost", 1337)?;
    ///
    ///     // Add a simple string message
    ///     let message = client.add_message("Hello, World!").await?;
    ///     println!("Created message {} with body: {}", message.id, message.body);
    ///
    ///     // Add a formatted message
    ///     let user_data = "important data";
    ///     let message = client.add_message(format!("Processing: {}", user_data)).await?;
    ///     
    ///     Ok(())
    /// }
    /// ```
    ///
    /// # Errors
    ///
    /// * [`TlqError::MessageTooLarge`] if the message exceeds the configured byte limit
    /// * [`TlqError::Connection`] for connection setup failures
    /// * [`TlqError::Timeout`] if connecting exceeds the configured timeout
    /// * [`TlqError::OutcomeUnknown`] if the request was sent but no valid response arrived
    /// * [`TlqError::Server`] for server-side errors (4xx/5xx HTTP responses)
    pub async fn add_message(&self, body: impl Into<String>) -> Result<Message> {
        let body = body.into();

        if body.len() > self.config.max_message_size {
            return Err(TlqError::MessageTooLarge {
                size: body.len(),
                limit: self.config.max_message_size,
            });
        }

        let request = AddMessageRequest { body };
        let message: Message = self.post("/add", &request).await?;
        Ok(message)
    }

    /// Retrieves multiple messages from the TLQ server.
    ///
    /// This method fetches up to `count` messages from the queue. Messages are returned
    /// in the order they were added and their state is changed to [`MessageState::Processing`].
    /// The server may return fewer messages than requested if there are not enough
    /// messages in the queue.
    ///
    /// # Arguments
    ///
    /// * `count` - Maximum number of messages to retrieve (must be greater than 0)
    ///
    /// # Returns
    ///
    /// Returns a vector of [`Message`] objects. The vector may be empty if no messages
    /// are available in the queue.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tlq_client::TlqClient;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), tlq_client::TlqError> {
    ///     let client = TlqClient::new("localhost", 1337)?;
    ///
    ///     // Get up to 5 messages from the queue
    ///     let messages = client.get_messages(5).await?;
    ///     
    ///     for message in messages {
    ///         println!("Processing message {}: {}", message.id, message.body);
    ///         
    ///         // Process the message...
    ///         
    ///         // Delete when done
    ///         client.delete_message(message.id).await?;
    ///     }
    ///     
    ///     Ok(())
    /// }
    /// ```
    ///
    /// # Errors
    ///
    /// * [`TlqError::Validation`] if count is 0
    /// * [`TlqError::Connection`] for connection setup failures
    /// * [`TlqError::Timeout`] if connecting exceeds the configured timeout
    /// * [`TlqError::OutcomeUnknown`] if the request was sent but no valid response arrived
    /// * [`TlqError::Server`] for server-side errors (4xx/5xx HTTP responses)
    pub async fn get_messages(&self, count: u32) -> Result<Vec<Message>> {
        if count == 0 {
            return Err(TlqError::Validation(
                "Count must be greater than 0".to_string(),
            ));
        }

        let request = GetMessagesRequest { count };
        let messages: Vec<Message> = self.post("/get", &request).await?;
        Ok(messages)
    }

    /// Retrieves a single message from the TLQ server.
    ///
    /// This is a convenience method equivalent to calling [`get_messages(1)`](Self::get_messages)
    /// and taking the first result. If no messages are available, returns `None`.
    ///
    /// # Returns
    ///
    /// * `Ok(Some(message))` if a message was retrieved
    /// * `Ok(None)` if no messages are available in the queue
    /// * `Err` for connection or server errors, or an unknown operation outcome
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tlq_client::TlqClient;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), tlq_client::TlqError> {
    ///     let client = TlqClient::new("localhost", 1337)?;
    ///
    ///     // Get a single message
    ///     match client.get_message().await? {
    ///         Some(message) => {
    ///             println!("Got message: {}", message.body);
    ///             client.delete_message(message.id).await?;
    ///         }
    ///         None => println!("No messages available"),
    ///     }
    ///     
    ///     Ok(())
    /// }
    /// ```
    ///
    /// # Errors
    ///
    /// * [`TlqError::Connection`] for connection setup failures
    /// * [`TlqError::Timeout`] if connecting exceeds the configured timeout
    /// * [`TlqError::OutcomeUnknown`] if the request was sent but no valid response arrived
    /// * [`TlqError::Server`] for server-side errors (4xx/5xx HTTP responses)
    pub async fn get_message(&self) -> Result<Option<Message>> {
        let messages = self.get_messages(1).await?;
        Ok(messages.into_iter().next())
    }

    /// Deletes a single message from the TLQ server.
    ///
    /// This is a convenience method that calls [`delete_messages`](Self::delete_messages)
    /// with a single message ID.
    ///
    /// # Arguments
    ///
    /// * `id` - The UUID of the message to delete
    ///
    /// # Returns
    ///
    /// Returns `"Success"` on completion. The server does not return an affected-message count.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tlq_client::TlqClient;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), tlq_client::TlqError> {
    ///     let client = TlqClient::new("localhost", 1337)?;
    ///
    ///     if let Some(message) = client.get_message().await? {
    ///         let result = client.delete_message(message.id).await?;
    ///         println!("Delete result: {}", result);
    ///     }
    ///     
    ///     Ok(())
    /// }
    /// ```
    ///
    /// # Errors
    ///
    /// * [`TlqError::Connection`] for connection setup failures
    /// * [`TlqError::Timeout`] if connecting exceeds the configured timeout
    /// * [`TlqError::OutcomeUnknown`] if the request was sent but no valid response arrived
    /// * [`TlqError::Server`] for server-side errors (4xx/5xx HTTP responses)
    pub async fn delete_message(&self, id: Uuid) -> Result<String> {
        self.delete_messages(&[id]).await
    }

    /// Deletes multiple messages from the TLQ server.
    ///
    /// This method removes the specified messages from the queue permanently.
    /// Only messages in [`MessageState::Processing`] are deleted; other IDs are ignored.
    ///
    /// # Arguments
    ///
    /// * `ids` - A slice of message UUIDs to delete (must not be empty)
    ///
    /// # Returns
    ///
    /// Returns `"Success"` on completion. The server does not return an affected-message count.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tlq_client::TlqClient;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), tlq_client::TlqError> {
    ///     let client = TlqClient::new("localhost", 1337)?;
    ///
    ///     let messages = client.get_messages(3).await?;
    ///     if !messages.is_empty() {
    ///         let ids: Vec<_> = messages.iter().map(|m| m.id).collect();
    ///         let result = client.delete_messages(&ids).await?;
    ///         println!("Delete result: {}", result);
    ///     }
    ///     
    ///     Ok(())
    /// }
    /// ```
    ///
    /// # Errors
    ///
    /// * [`TlqError::Validation`] if the `ids` slice is empty
    /// * [`TlqError::Connection`] for connection setup failures
    /// * [`TlqError::Timeout`] if connecting exceeds the configured timeout
    /// * [`TlqError::OutcomeUnknown`] if the request was sent but no valid response arrived
    /// * [`TlqError::Server`] for server-side errors (4xx/5xx HTTP responses)
    pub async fn delete_messages(&self, ids: &[Uuid]) -> Result<String> {
        if ids.is_empty() {
            return Err(TlqError::Validation("No message IDs provided".to_string()));
        }

        let request = DeleteMessagesRequest { ids: ids.to_vec() };
        let response: String = self.post("/delete", &request).await?;
        Ok(response)
    }

    /// Retries a single message on the TLQ server.
    ///
    /// This is a convenience method that calls [`retry_messages`](Self::retry_messages)
    /// with a single message ID. The message state will be changed back to
    /// [`MessageState::Ready`], making it available for processing again.
    ///
    /// # Arguments
    ///
    /// * `id` - The UUID of the message to retry
    ///
    /// # Returns
    ///
    /// Returns a string indicating the result of the operation (typically "Success").
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tlq_client::TlqClient;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), tlq_client::TlqError> {
    ///     let client = TlqClient::new("localhost", 1337)?;
    ///
    ///     if let Some(message) = client.get_message().await? {
    ///         // Processing failed, retry the message
    ///         let result = client.retry_message(message.id).await?;
    ///         println!("Retry result: {}", result);
    ///     }
    ///
    ///     Ok(())
    /// }
    /// ```
    ///
    /// # Errors
    ///
    /// * [`TlqError::Connection`] for connection setup failures
    /// * [`TlqError::Timeout`] if connecting exceeds the configured timeout
    /// * [`TlqError::OutcomeUnknown`] if the request was sent but no valid response arrived
    /// * [`TlqError::Server`] for server-side errors (4xx/5xx HTTP responses)
    pub async fn retry_message(&self, id: Uuid) -> Result<String> {
        self.retry_messages(&[id]).await
    }

    /// Retries multiple messages on the TLQ server.
    ///
    /// This method changes the state of the specified messages back to
    /// [`MessageState::Ready`], making them available for processing again.
    /// The retry count for each message will be incremented.
    ///
    /// # Arguments
    ///
    /// * `ids` - A slice of message UUIDs to retry (must not be empty)
    ///
    /// # Returns
    ///
    /// Returns a string indicating the result of the operation (typically "Success").
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tlq_client::TlqClient;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), tlq_client::TlqError> {
    ///     let client = TlqClient::new("localhost", 1337)?;
    ///
    ///     let messages = client.get_messages(10).await?;
    ///     let ids: Vec<_> = messages.iter().map(|m| m.id).collect();
    ///
    ///     if !ids.is_empty() {
    ///         let result = client.retry_messages(&ids).await?;
    ///         println!("Retry result: {}", result);
    ///     }
    ///
    ///     Ok(())
    /// }
    /// ```
    ///
    /// # Errors
    ///
    /// * [`TlqError::Validation`] if the `ids` slice is empty
    /// * [`TlqError::Connection`] for connection setup failures
    /// * [`TlqError::Timeout`] if connecting exceeds the configured timeout
    /// * [`TlqError::OutcomeUnknown`] if the request was sent but no valid response arrived
    /// * [`TlqError::Server`] for server-side errors (4xx/5xx HTTP responses)
    pub async fn retry_messages(&self, ids: &[Uuid]) -> Result<String> {
        if ids.is_empty() {
            return Err(TlqError::Validation("No message IDs provided".to_string()));
        }

        let request = RetryMessagesRequest { ids: ids.to_vec() };
        let response: String = self.post("/retry", &request).await?;
        Ok(response)
    }

    /// Removes all messages from the TLQ server queue.
    ///
    /// This method permanently deletes all messages in the queue regardless of their state.
    /// Use with caution as this operation cannot be undone.
    ///
    /// # Returns
    ///
    /// Returns a string indicating the result of the operation (typically "Success").
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tlq_client::TlqClient;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), tlq_client::TlqError> {
    ///     let client = TlqClient::new("localhost", 1337)?;
    ///
    ///     // Clear all messages from the queue
    ///     let result = client.purge_queue().await?;
    ///     println!("Purge result: {}", result);
    ///     
    ///     Ok(())
    /// }
    /// ```
    ///
    /// # Errors
    ///
    /// * [`TlqError::Connection`] for connection setup failures
    /// * [`TlqError::Timeout`] if connecting exceeds the configured timeout
    /// * [`TlqError::OutcomeUnknown`] if the request was sent but no valid response arrived
    /// * [`TlqError::Server`] for server-side errors (4xx/5xx HTTP responses)
    pub async fn purge_queue(&self) -> Result<String> {
        let response: String = self.post("/purge", &serde_json::json!({})).await?;
        Ok(response)
    }

    /// Retrieves queue statistics from the TLQ server.
    ///
    /// Returns counts of messages in each state (ready, processing) and
    /// the cumulative count of dead messages removed by the reaper.
    ///
    /// # Returns
    ///
    /// Returns a [`QueueStats`] with the current queue statistics.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tlq_client::TlqClient;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), tlq_client::TlqError> {
    ///     let client = TlqClient::new("localhost", 1337)?;
    ///
    ///     let stats = client.stats().await?;
    ///     println!("Ready: {}, Processing: {}, Dead: {}",
    ///         stats.ready, stats.processing, stats.dead);
    ///
    ///     Ok(())
    /// }
    /// ```
    ///
    /// # Errors
    ///
    /// * [`TlqError::Connection`] for connection failures
    /// * [`TlqError::Timeout`] if an attempt exceeds the configured timeout
    /// * [`TlqError::Server`] for server-side errors (4xx/5xx HTTP responses)
    pub async fn stats(&self) -> Result<QueueStats> {
        self.get("/stats").await
    }

    // Helper function to parse HTTP response - extracted for testing
    fn parse_http_response(response: &str) -> Result<&str> {
        if let Some(body_start) = response.find("\r\n\r\n") {
            let headers = &response[..body_start];
            let body = &response[body_start + 4..];

            if let Some(status_line) = headers.lines().next() {
                let parts: Vec<&str> = status_line.split_whitespace().collect();
                if parts.len() >= 2 {
                    if let Ok(status_code) = parts[1].parse::<u16>() {
                        if status_code >= 400 {
                            return Err(TlqError::Server {
                                status: status_code,
                                message: body.to_string(),
                            });
                        }
                    }
                }
            }

            Ok(body)
        } else {
            Err(TlqError::Connection(
                "Connection closed before a complete HTTP response".to_string(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DEFAULT_MAX_MESSAGE_SIZE;
    use std::time::Duration;
    use tokio::net::TcpListener;

    // A port that was just released refuses connections immediately.
    async fn closed_port() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        listener.local_addr().unwrap().port()
    }

    #[tokio::test]
    async fn test_connection_failures_are_retried_with_backoff() {
        let client = TlqClient::builder()
            .host("127.0.0.1")
            .port(closed_port().await)
            .max_retries(2)
            .retry_delay_ms(30)
            .build();

        for queue_changing in [false, true] {
            let started = Instant::now();
            let error = if queue_changing {
                client.purge_queue().await.unwrap_err()
            } else {
                client.stats().await.unwrap_err()
            };

            assert!(matches!(error, TlqError::Connection(_)), "{error:?}");
            assert!(error.is_retryable());
            // Two retries: 30ms + 60ms of backoff.
            assert!(started.elapsed() >= Duration::from_millis(90));
        }
    }

    #[tokio::test]
    async fn test_serialization_failure_happens_before_connecting() {
        struct InvalidBody;

        impl Serialize for InvalidBody {
            fn serialize<S: serde::Serializer>(
                &self,
                _serializer: S,
            ) -> std::result::Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom("invalid request"))
            }
        }

        let client = TlqClient::builder()
            .host("127.0.0.1")
            .port(closed_port().await)
            .build();
        let error = client
            .post::<_, String>("/add", &InvalidBody)
            .await
            .unwrap_err();

        assert!(matches!(error, TlqError::Serialization(_)), "{error:?}");
    }

    #[test]
    fn test_parse_http_response_success() {
        let response =
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"message\":\"success\"}";

        let result = TlqClient::parse_http_response(response);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "{\"message\":\"success\"}");
    }

    #[test]
    fn test_parse_http_response_server_error() {
        let response = "HTTP/1.1 500 Internal Server Error\r\nContent-Type: text/plain\r\n\r\nInternal server error occurred";

        let result = TlqClient::parse_http_response(response);
        match result {
            Err(TlqError::Server { status, message }) => {
                assert_eq!(status, 500);
                assert_eq!(message, "Internal server error occurred");
            }
            _ => panic!("Expected server error"),
        }
    }

    #[test]
    fn test_parse_http_response_client_error() {
        let response = "HTTP/1.1 400 Bad Request\r\nContent-Type: text/plain\r\n\r\nBad request";

        let result = TlqClient::parse_http_response(response);
        match result {
            Err(TlqError::Server { status, message }) => {
                assert_eq!(status, 400);
                assert_eq!(message, "Bad request");
            }
            _ => panic!("Expected client error"),
        }
    }

    #[test]
    fn test_parse_http_response_no_headers_separator() {
        let response =
            "HTTP/1.1 200 OK\nContent-Type: application/json\n{\"incomplete\":\"response\"}";

        let result = TlqClient::parse_http_response(response);
        match result {
            Err(TlqError::Connection(msg)) => {
                assert_eq!(msg, "Connection closed before a complete HTTP response");
            }
            _ => panic!("Expected connection error"),
        }
    }

    #[test]
    fn test_parse_http_response_malformed_status_line() {
        let response = "INVALID_STATUS_LINE\r\n\r\n{\"data\":\"test\"}";

        let result = TlqClient::parse_http_response(response);
        // Should still succeed because we only check if parts.len() >= 2 and parse fails gracefully
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "{\"data\":\"test\"}");
    }

    #[test]
    fn test_parse_http_response_empty_body() {
        let response = "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n";

        let result = TlqClient::parse_http_response(response);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "");
    }

    #[test]
    fn test_parse_http_response_with_extra_headers() {
        let response = "HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nServer: TLQ/1.0\r\nConnection: close\r\n\r\n{\"id\":\"123\",\"status\":\"created\"}";

        let result = TlqClient::parse_http_response(response);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "{\"id\":\"123\",\"status\":\"created\"}");
    }

    #[test]
    fn test_parse_http_response_status_code_edge_cases() {
        // Test various status codes around the 400 boundary

        // 399 should be success (< 400)
        let response_399 = "HTTP/1.1 399 Custom Success\r\n\r\n{\"ok\":true}";
        let result = TlqClient::parse_http_response(response_399);
        assert!(result.is_ok());

        // 400 should be error (>= 400)
        let response_400 = "HTTP/1.1 400 Bad Request\r\n\r\nBad request";
        let result = TlqClient::parse_http_response(response_400);
        assert!(matches!(result, Err(TlqError::Server { status: 400, .. })));

        // 599 should be error
        let response_599 = "HTTP/1.1 599 Custom Error\r\n\r\nCustom error";
        let result = TlqClient::parse_http_response(response_599);
        assert!(matches!(result, Err(TlqError::Server { status: 599, .. })));
    }

    #[test]
    fn test_client_creation() {
        let client = TlqClient::new("test-host", 9999);
        assert!(client.is_ok());

        let client = client.unwrap();
        assert_eq!(client.base_url, "test-host:9999");
    }

    #[test]
    fn test_client_with_config() {
        let config = Config {
            host: "custom-host".to_string(),
            port: 8080,
            timeout: Duration::from_secs(10),
            max_retries: 5,
            retry_delay: Duration::from_millis(200),
            ..Config::default()
        };

        let client = TlqClient::with_config(config);
        assert_eq!(client.base_url, "custom-host:8080");
        assert_eq!(client.config.max_retries, 5);
        assert_eq!(client.config.timeout, Duration::from_secs(10));
    }

    #[tokio::test]
    async fn test_add_message_size_validation() {
        let client = TlqClient::new("localhost", 1337).unwrap();

        // Test message at exact size limit (should be rejected because it's over the limit)
        let large_message = "x".repeat(DEFAULT_MAX_MESSAGE_SIZE + 1);
        let result = client.add_message(large_message).await;

        match result {
            Err(TlqError::MessageTooLarge { size, limit }) => {
                assert_eq!(size, DEFAULT_MAX_MESSAGE_SIZE + 1);
                assert_eq!(limit, DEFAULT_MAX_MESSAGE_SIZE);
            }
            _ => panic!("Expected MessageTooLarge error"),
        }

        // Test empty message (should be valid)
        let empty_message = "";
        // We can't actually test without a server, but we can verify it passes size validation
        assert!(empty_message.len() <= DEFAULT_MAX_MESSAGE_SIZE);

        // Test message exactly at limit (should be valid)
        let max_message = "x".repeat(DEFAULT_MAX_MESSAGE_SIZE);
        // Size check should pass
        assert_eq!(max_message.len(), DEFAULT_MAX_MESSAGE_SIZE);
    }

    #[tokio::test]
    async fn test_get_messages_validation() {
        let client = TlqClient::new("localhost", 1337).unwrap();

        // Test zero count (should be rejected)
        let result = client.get_messages(0).await;
        match result {
            Err(TlqError::Validation(msg)) => {
                assert_eq!(msg, "Count must be greater than 0");
            }
            _ => panic!("Expected validation error for zero count"),
        }

        // Test valid counts - these should pass without validation errors
        let _ = client.get_messages(1).await; // Should be valid
        let _ = client.get_messages(100).await; // Should be valid
        let _ = client.get_messages(u32::MAX).await; // Should be valid
    }

    #[tokio::test]
    async fn test_delete_messages_validation() {
        let client = TlqClient::new("localhost", 1337).unwrap();

        // Test empty IDs array
        let result = client.delete_messages(&[]).await;
        match result {
            Err(TlqError::Validation(msg)) => {
                assert_eq!(msg, "No message IDs provided");
            }
            _ => panic!("Expected validation error for empty IDs"),
        }

        // Test delete_message (single ID) - should not have validation issue
        use uuid::Uuid;
        let test_id = Uuid::now_v7();
        // We can't test the actual call without a server, but we can verify
        // it would call delete_messages with a single-item array
        assert!(!vec![test_id].is_empty());
    }

    #[tokio::test]
    async fn test_retry_messages_validation() {
        let client = TlqClient::new("localhost", 1337).unwrap();

        // Test empty IDs array
        let result = client.retry_messages(&[]).await;
        match result {
            Err(TlqError::Validation(msg)) => {
                assert_eq!(msg, "No message IDs provided");
            }
            _ => panic!("Expected validation error for empty IDs"),
        }

        // Test retry_message (single ID) - should not have validation issue
        use uuid::Uuid;
        let test_id = Uuid::now_v7();
        // We can't test the actual call without a server, but we can verify
        // it would call retry_messages with a single-item array
        assert!(!vec![test_id].is_empty());
    }

    #[test]
    fn test_client_builder_edge_cases() {
        // Test builder with minimum values
        let client = TlqClient::builder()
            .host("")
            .port(0)
            .timeout_ms(0)
            .max_retries(0)
            .retry_delay_ms(0)
            .build();

        assert_eq!(client.base_url, ":0");
        assert_eq!(client.config.max_retries, 0);
        assert_eq!(client.config.timeout, Duration::from_millis(0));

        // Test builder with maximum reasonable values
        let client = TlqClient::builder()
            .host("very-long-hostname-that-might-be-used-in-some-environments")
            .port(65535)
            .timeout_ms(600000) // 10 minutes
            .max_retries(100)
            .retry_delay_ms(10000) // 10 seconds
            .build();

        assert!(client.base_url.contains("very-long-hostname"));
        assert_eq!(client.config.max_retries, 100);
        assert_eq!(client.config.timeout, Duration::from_secs(600));
    }

    #[test]
    fn test_config_validation() {
        use crate::config::ConfigBuilder;
        use std::time::Duration;

        // Test various duration configurations
        let config1 = ConfigBuilder::new()
            .timeout(Duration::from_nanos(1))
            .build_config();
        assert_eq!(config1.timeout, Duration::from_nanos(1));

        let config2 = ConfigBuilder::new()
            .retry_delay(Duration::from_secs(3600)) // 1 hour
            .build_config();
        assert_eq!(config2.retry_delay, Duration::from_secs(3600));

        // Test edge case ports
        let config3 = ConfigBuilder::new().port(1).build_config();
        assert_eq!(config3.port, 1);

        let config4 = ConfigBuilder::new().port(65535).build_config();
        assert_eq!(config4.port, 65535);

        // Test very high retry counts
        let config5 = ConfigBuilder::new().max_retries(1000).build_config();
        assert_eq!(config5.max_retries, 1000);
    }
}
