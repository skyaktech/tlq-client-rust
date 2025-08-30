use tlq_client::{TlqClient, TlqError};

#[tokio::test]
async fn test_client_creation() {
    let client = TlqClient::new("localhost", 1337);
    assert!(client.is_ok());
}

#[tokio::test]
async fn test_client_builder() {
    let client = TlqClient::builder()
        .host("localhost")
        .port(1337)
        .timeout_ms(5000)
        .max_retries(3)
        .retry_delay_ms(100)
        .build();

    let _client = TlqClient::with_config(client);
}

#[tokio::test]
async fn test_message_size_validation() {
    let client = TlqClient::new("localhost", 1337).unwrap();
    let large_message = "x".repeat(100_000);

    let result = client.add_message(large_message).await;

    match result {
        Err(TlqError::MessageTooLarge { size }) => {
            assert_eq!(size, 100_000);
        }
        _ => panic!("Expected MessageTooLarge error"),
    }
}

#[tokio::test]
async fn test_error_types() {
    let timeout_err = TlqError::Timeout(5000);
    assert!(timeout_err.is_retryable());

    let connection_err = TlqError::Connection("test".to_string());
    assert!(connection_err.is_retryable());

    let validation_err = TlqError::Validation("test".to_string());
    assert!(!validation_err.is_retryable());

    let server_err = TlqError::Server {
        status: 500,
        message: "Internal Server Error".to_string(),
    };
    assert!(!server_err.is_retryable());
}

#[cfg(test)]
mod config_tests {
    use std::time::Duration;
    use tlq_client::ConfigBuilder;

    #[test]
    fn test_config_builder() {
        let config = ConfigBuilder::new()
            .host("example.com")
            .port(8080)
            .timeout(Duration::from_secs(10))
            .max_retries(5)
            .retry_delay(Duration::from_millis(200))
            .build();

        assert_eq!(config.host, "example.com");
        assert_eq!(config.port, 8080);
        assert_eq!(config.timeout, Duration::from_secs(10));
        assert_eq!(config.max_retries, 5);
        assert_eq!(config.retry_delay, Duration::from_millis(200));
    }

    #[test]
    fn test_config_default() {
        let config = ConfigBuilder::new().build();

        assert_eq!(config.host, "localhost");
        assert_eq!(config.port, 1337);
        assert_eq!(config.timeout, Duration::from_secs(30));
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.retry_delay, Duration::from_millis(100));
    }
}

#[cfg(test)]
mod message_tests {
    use tlq_client::{Message, MessageState};

    #[test]
    fn test_message_creation() {
        let message = Message::new("Test message".to_string());

        assert_eq!(message.body, "Test message");
        assert_eq!(message.state, MessageState::Ready);
        assert_eq!(message.retry_count, 0);
    }

    #[test]
    fn test_message_state() {
        let ready = MessageState::Ready;
        let processing = MessageState::Processing;
        let failed = MessageState::Failed;

        assert_eq!(ready, MessageState::Ready);
        assert_ne!(ready, processing);
        assert_ne!(processing, failed);
    }
}
