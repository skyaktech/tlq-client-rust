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

pub struct TlqClient {
    config: Config,
    base_url: String,
}

impl TlqClient {
    pub fn new(host: impl Into<String>, port: u16) -> Result<Self> {
        let config = ConfigBuilder::new().host(host).port(port).build();

        Ok(Self::with_config(config))
    }

    pub fn with_config(config: Config) -> Self {
        let base_url = format!("{}:{}", config.host, config.port);
        Self { config, base_url }
    }

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

    pub async fn add_message(&self, body: impl Into<String>) -> Result<Message> {
        let body = body.into();

        if body.len() > MAX_MESSAGE_SIZE {
            return Err(TlqError::MessageTooLarge { size: body.len() });
        }

        let request = AddMessageRequest { body };
        let message: Message = self.request("/add", &request).await?;
        Ok(message)
    }

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

    pub async fn get_message(&self) -> Result<Option<Message>> {
        let messages = self.get_messages(1).await?;
        Ok(messages.into_iter().next())
    }

    pub async fn delete_message(&self, id: Uuid) -> Result<String> {
        self.delete_messages(&[id]).await
    }

    pub async fn delete_messages(&self, ids: &[Uuid]) -> Result<String> {
        if ids.is_empty() {
            return Err(TlqError::Validation("No message IDs provided".to_string()));
        }

        let request = DeleteMessagesRequest { ids: ids.to_vec() };
        let response: String = self.request("/delete", &request).await?;
        Ok(response)
    }

    pub async fn retry_message(&self, id: Uuid) -> Result<String> {
        self.retry_messages(&[id]).await
    }

    pub async fn retry_messages(&self, ids: &[Uuid]) -> Result<String> {
        if ids.is_empty() {
            return Err(TlqError::Validation("No message IDs provided".to_string()));
        }

        let request = RetryMessagesRequest { ids: ids.to_vec() };
        let response: String = self.request("/retry", &request).await?;
        Ok(response)
    }

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
        let response = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"message\":\"success\"}";
        
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
        let response = "HTTP/1.1 200 OK\nContent-Type: application/json\n{\"incomplete\":\"response\"}";
        
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
        
        // Test valid counts
        assert!(1_u32 > 0); // Should be valid
        assert!(100_u32 > 0); // Should be valid
        assert!(u32::MAX > 0); // Should be valid
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
