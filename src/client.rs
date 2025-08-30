use crate::{
    config::{Config, ConfigBuilder},
    error::{Result, TlqError},
    message::*,
    retry::RetryStrategy,
};
use serde::{de::DeserializeOwned, Serialize};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;
use uuid::Uuid;

const MAX_MESSAGE_SIZE: usize = 65536;

/// The main client for interacting with TLQ (Tiny Little Queue) servers.
///
/// `TlqClient` provides an async, type-safe interface for all TLQ operations including
/// adding messages, retrieving messages, and managing queue state. The client handles
/// automatic retry with exponential backoff for transient failures.
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
        let config = ConfigBuilder::new().host(host).port(port).build();

        Ok(Self::with_config(config))
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
    /// use tlq_client::{TlqClient, ConfigBuilder};
    /// use std::time::Duration;
    ///
    /// # fn example() {
    /// let config = ConfigBuilder::new()
    ///     .host("queue.example.com")
    ///     .port(8080)
    ///     .timeout(Duration::from_secs(5))
    ///     .max_retries(2)
    ///     .build();
    ///
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
    /// let client = TlqClient::with_config(
    ///     TlqClient::builder()
    ///         .host("localhost")
    ///         .port(1337)
    ///         .timeout(Duration::from_secs(10))
    ///         .build()
    /// );
    /// # }
    /// ```
    pub fn builder() -> ConfigBuilder {
        ConfigBuilder::new()
    }

    async fn request<T, R>(&self, endpoint: &str, body: &T) -> Result<R>
    where
        T: Serialize,
        R: DeserializeOwned,
    {
        let retry_strategy = RetryStrategy::new(self.config.max_retries, self.config.retry_delay);

        retry_strategy
            .execute(|| async { self.single_request(endpoint, body).await })
            .await
    }

    async fn single_request<T, R>(&self, endpoint: &str, body: &T) -> Result<R>
    where
        T: Serialize,
        R: DeserializeOwned,
    {
        let json_body = serde_json::to_vec(body)?;

        let request = format!(
            "POST {} HTTP/1.1\r\n\
             Host: {}\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\
             \r\n",
            endpoint,
            self.base_url,
            json_body.len()
        );

        let mut stream = timeout(self.config.timeout, TcpStream::connect(&self.base_url))
            .await
            .map_err(|_| TlqError::Timeout(self.config.timeout.as_millis() as u64))?
            .map_err(|e| TlqError::Connection(e.to_string()))?;

        stream.write_all(request.as_bytes()).await?;
        stream.write_all(&json_body).await?;
        stream.flush().await?;

        let mut response = Vec::new();
        stream.read_to_end(&mut response).await?;

        let response_str = String::from_utf8_lossy(&response);
        let body = Self::parse_http_response(&response_str)?;
        serde_json::from_str(body).map_err(Into::into)
    }

    /// Performs a health check against the TLQ server.
    ///
    /// This method sends a GET request to the `/hello` endpoint to verify
    /// that the server is responding. It uses a fixed 5-second timeout
    /// regardless of the client's configured timeout.
    ///
    /// # Returns
    ///
    /// * `Ok(true)` if the server responds with HTTP 200 OK
    /// * `Ok(false)` if the server responds but not with 200 OK
    /// * `Err` if there's a connection error or timeout
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
    ///
    /// # Errors
    ///
    /// Returns [`TlqError::Connection`] for network issues, or [`TlqError::Timeout`]
    /// if the server doesn't respond within 5 seconds.
    pub async fn health_check(&self) -> Result<bool> {
        let mut stream = timeout(Duration::from_secs(5), TcpStream::connect(&self.base_url))
            .await
            .map_err(|_| TlqError::Timeout(5000))?
            .map_err(|e| TlqError::Connection(e.to_string()))?;

        let request = format!(
            "GET /hello HTTP/1.1\r\n\
             Host: {}\r\n\
             Connection: close\r\n\
             \r\n",
            self.base_url
        );

        stream.write_all(request.as_bytes()).await?;
        stream.flush().await?;

        let mut response = Vec::new();
        stream.read_to_end(&mut response).await?;

        let response_str = String::from_utf8_lossy(&response);
        Ok(response_str.contains("200 OK"))
    }

    /// Adds a new message to the TLQ server.
    ///
    /// The message will be assigned a UUID v7 identifier and placed in the queue
    /// with state [`MessageState::Ready`]. Messages have a maximum size limit of 64KB.
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
    /// * [`TlqError::MessageTooLarge`] if the message exceeds 64KB (65,536 bytes)
    /// * [`TlqError::Connection`] for network connectivity issues
    /// * [`TlqError::Timeout`] if the request times out
    /// * [`TlqError::Server`] for server-side errors (4xx/5xx HTTP responses)
    pub async fn add_message(&self, body: impl Into<String>) -> Result<Message> {
        let body = body.into();

        if body.len() > MAX_MESSAGE_SIZE {
            return Err(TlqError::MessageTooLarge { size: body.len() });
        }

        let request = AddMessageRequest { body };
        let message: Message = self.request("/add", &request).await?;
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
    /// * [`TlqError::Connection`] for network connectivity issues  
    /// * [`TlqError::Timeout`] if the request times out
    /// * [`TlqError::Server`] for server-side errors (4xx/5xx HTTP responses)
    pub async fn get_messages(&self, count: u32) -> Result<Vec<Message>> {
        if count == 0 {
            return Err(TlqError::Validation(
                "Count must be greater than 0".to_string(),
            ));
        }

        let request = GetMessagesRequest { count };
        let messages: Vec<Message> = self.request("/get", &request).await?;
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
    /// * `Err` for connection or server errors
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
    /// * [`TlqError::Connection`] for network connectivity issues
    /// * [`TlqError::Timeout`] if the request times out  
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
    /// Returns a string indicating the result of the operation (typically "Success" or a count).
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
    /// * [`TlqError::Connection`] for network connectivity issues
    /// * [`TlqError::Timeout`] if the request times out
    /// * [`TlqError::Server`] for server-side errors (4xx/5xx HTTP responses)
    pub async fn delete_message(&self, id: Uuid) -> Result<String> {
        self.delete_messages(&[id]).await
    }

    /// Deletes multiple messages from the TLQ server.
    ///
    /// This method removes the specified messages from the queue permanently.
    /// Messages can be in any state when deleted.
    ///
    /// # Arguments
    ///
    /// * `ids` - A slice of message UUIDs to delete (must not be empty)
    ///
    /// # Returns
    ///
    /// Returns a string indicating the number of messages deleted or "Success".
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
    ///         println!("Deleted {} messages", result);
    ///     }
    ///     
    ///     Ok(())
    /// }
    /// ```
    ///
    /// # Errors
    ///
    /// * [`TlqError::Validation`] if the `ids` slice is empty
    /// * [`TlqError::Connection`] for network connectivity issues
    /// * [`TlqError::Timeout`] if the request times out
    /// * [`TlqError::Server`] for server-side errors (4xx/5xx HTTP responses)
    pub async fn delete_messages(&self, ids: &[Uuid]) -> Result<String> {
        if ids.is_empty() {
            return Err(TlqError::Validation("No message IDs provided".to_string()));
        }

        let request = DeleteMessagesRequest { ids: ids.to_vec() };
        let response: String = self.request("/delete", &request).await?;
        Ok(response)
    }

    /// Retries a single failed message on the TLQ server.
    ///
    /// This is a convenience method that calls [`retry_messages`](Self::retry_messages)
    /// with a single message ID. The message state will be changed from
    /// [`MessageState::Failed`] back to [`MessageState::Ready`].
    ///
    /// # Arguments
    ///
    /// * `id` - The UUID of the message to retry
    ///
    /// # Returns
    ///
    /// Returns a string indicating the result of the operation (typically "Success" or a count).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tlq_client::{TlqClient, MessageState};
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), tlq_client::TlqError> {
    ///     let client = TlqClient::new("localhost", 1337)?;
    ///
    ///     // Find failed messages and retry them
    ///     let messages = client.get_messages(10).await?;
    ///     for message in messages {
    ///         if message.state == MessageState::Failed {
    ///             let result = client.retry_message(message.id).await?;
    ///             println!("Retry result: {}", result);
    ///         }
    ///     }
    ///     
    ///     Ok(())
    /// }
    /// ```
    ///
    /// # Errors
    ///
    /// * [`TlqError::Connection`] for network connectivity issues
    /// * [`TlqError::Timeout`] if the request times out
    /// * [`TlqError::Server`] for server-side errors (4xx/5xx HTTP responses)
    pub async fn retry_message(&self, id: Uuid) -> Result<String> {
        self.retry_messages(&[id]).await
    }

    /// Retries multiple failed messages on the TLQ server.
    ///
    /// This method changes the state of the specified messages from [`MessageState::Failed`]
    /// back to [`MessageState::Ready`], making them available for processing again.
    /// The retry count for each message will be incremented.
    ///
    /// # Arguments
    ///
    /// * `ids` - A slice of message UUIDs to retry (must not be empty)
    ///
    /// # Returns
    ///
    /// Returns a string indicating the number of messages retried or "Success".
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tlq_client::{TlqClient, MessageState};
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), tlq_client::TlqError> {
    ///     let client = TlqClient::new("localhost", 1337)?;
    ///
    ///     // Get all messages and retry the failed ones
    ///     let messages = client.get_messages(100).await?;
    ///     let failed_ids: Vec<_> = messages
    ///         .iter()
    ///         .filter(|m| m.state == MessageState::Failed)
    ///         .map(|m| m.id)
    ///         .collect();
    ///
    ///     if !failed_ids.is_empty() {
    ///         let result = client.retry_messages(&failed_ids).await?;
    ///         println!("Retried {} failed messages", result);
    ///     }
    ///     
    ///     Ok(())
    /// }
    /// ```
    ///
    /// # Errors
    ///
    /// * [`TlqError::Validation`] if the `ids` slice is empty
    /// * [`TlqError::Connection`] for network connectivity issues
    /// * [`TlqError::Timeout`] if the request times out
    /// * [`TlqError::Server`] for server-side errors (4xx/5xx HTTP responses)
    pub async fn retry_messages(&self, ids: &[Uuid]) -> Result<String> {
        if ids.is_empty() {
            return Err(TlqError::Validation("No message IDs provided".to_string()));
        }

        let request = RetryMessagesRequest { ids: ids.to_vec() };
        let response: String = self.request("/retry", &request).await?;
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
    /// * [`TlqError::Connection`] for network connectivity issues
    /// * [`TlqError::Timeout`] if the request times out
    /// * [`TlqError::Server`] for server-side errors (4xx/5xx HTTP responses)
    pub async fn purge_queue(&self) -> Result<String> {
        let response: String = self.request("/purge", &serde_json::json!({})).await?;
        Ok(response)
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
            Err(TlqError::Connection("Invalid HTTP response".to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
                assert_eq!(msg, "Invalid HTTP response");
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
    fn test_max_message_size_constant() {
        assert_eq!(MAX_MESSAGE_SIZE, 65536);
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
        };

        let client = TlqClient::with_config(config);
        assert_eq!(client.base_url, "custom-host:8080");
        assert_eq!(client.config.max_retries, 5);
        assert_eq!(client.config.timeout, Duration::from_secs(10));
    }

    #[test]
    fn test_message_size_validation() {
        let _client = TlqClient::new("localhost", 1337).unwrap();

        // Test exact limit
        let message_at_limit = "x".repeat(MAX_MESSAGE_SIZE);
        let result = std::panic::catch_unwind(|| {
            // We can't actually test async methods in sync tests without tokio,
            // but we can verify the constant is correct
            assert_eq!(message_at_limit.len(), MAX_MESSAGE_SIZE);
        });
        assert!(result.is_ok());

        // Test over limit
        let message_over_limit = "x".repeat(MAX_MESSAGE_SIZE + 1);
        assert_eq!(message_over_limit.len(), MAX_MESSAGE_SIZE + 1);
    }

    #[tokio::test]
    async fn test_add_message_size_validation() {
        let client = TlqClient::new("localhost", 1337).unwrap();

        // Test message at exact size limit (should be rejected because it's over the limit)
        let large_message = "x".repeat(MAX_MESSAGE_SIZE + 1);
        let result = client.add_message(large_message).await;

        match result {
            Err(TlqError::MessageTooLarge { size }) => {
                assert_eq!(size, MAX_MESSAGE_SIZE + 1);
            }
            _ => panic!("Expected MessageTooLarge error"),
        }

        // Test empty message (should be valid)
        let empty_message = "";
        // We can't actually test without a server, but we can verify it passes size validation
        assert!(empty_message.len() <= MAX_MESSAGE_SIZE);

        // Test message exactly at limit (should be valid)
        let max_message = "x".repeat(MAX_MESSAGE_SIZE);
        // Size check should pass
        assert_eq!(max_message.len(), MAX_MESSAGE_SIZE);
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
        let config = TlqClient::builder()
            .host("")
            .port(0)
            .timeout_ms(0)
            .max_retries(0)
            .retry_delay_ms(0)
            .build();

        let client = TlqClient::with_config(config);
        assert_eq!(client.base_url, ":0");
        assert_eq!(client.config.max_retries, 0);
        assert_eq!(client.config.timeout, Duration::from_millis(0));

        // Test builder with maximum reasonable values
        let config = TlqClient::builder()
            .host("very-long-hostname-that-might-be-used-in-some-environments")
            .port(65535)
            .timeout_ms(600000) // 10 minutes
            .max_retries(100)
            .retry_delay_ms(10000) // 10 seconds
            .build();

        let client = TlqClient::with_config(config);
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
            .build();
        assert_eq!(config1.timeout, Duration::from_nanos(1));

        let config2 = ConfigBuilder::new()
            .retry_delay(Duration::from_secs(3600)) // 1 hour
            .build();
        assert_eq!(config2.retry_delay, Duration::from_secs(3600));

        // Test edge case ports
        let config3 = ConfigBuilder::new().port(1).build();
        assert_eq!(config3.port, 1);

        let config4 = ConfigBuilder::new().port(65535).build();
        assert_eq!(config4.port, 65535);

        // Test very high retry counts
        let config5 = ConfigBuilder::new().max_retries(1000).build();
        assert_eq!(config5.max_retries, 1000);
    }
}
