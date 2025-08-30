use thiserror::Error;

#[derive(Error, Debug)]
pub enum TlqError {
    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Timeout error after {0}ms")]
    Timeout(u64),

    #[error("Server error: {status} - {message}")]
    Server { status: u16, message: String },

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Max retries exceeded ({max_retries}) for operation")]
    MaxRetriesExceeded { max_retries: u32 },

    #[error("Message too large: {size} bytes (max: 65536)")]
    MessageTooLarge { size: usize },
}

impl TlqError {
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            TlqError::Connection(_) | TlqError::Timeout(_) | TlqError::Io(_)
        )
    }
}

pub type Result<T> = std::result::Result<T, TlqError>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Error as IoError, ErrorKind};

    #[test]
    fn test_connection_error_retryable() {
        let error = TlqError::Connection("Connection refused".to_string());
        assert!(error.is_retryable());

        let error_msg = format!("{}", error);
        assert_eq!(error_msg, "Connection error: Connection refused");
    }

    #[test]
    fn test_timeout_error_retryable() {
        let error = TlqError::Timeout(5000);
        assert!(error.is_retryable());

        let error_msg = format!("{}", error);
        assert_eq!(error_msg, "Timeout error after 5000ms");
    }

    #[test]
    fn test_io_error_retryable() {
        let io_error = IoError::new(ErrorKind::ConnectionRefused, "Connection refused");
        let error = TlqError::Io(io_error);
        assert!(error.is_retryable());

        let error_msg = format!("{}", error);
        assert!(error_msg.contains("IO error:"));
        assert!(error_msg.contains("Connection refused"));
    }

    #[test]
    fn test_server_error_not_retryable() {
        let error = TlqError::Server {
            status: 500,
            message: "Internal Server Error".to_string(),
        };
        assert!(!error.is_retryable());

        let error_msg = format!("{}", error);
        assert_eq!(error_msg, "Server error: 500 - Internal Server Error");
    }

    #[test]
    fn test_validation_error_not_retryable() {
        let error = TlqError::Validation("Invalid input".to_string());
        assert!(!error.is_retryable());

        let error_msg = format!("{}", error);
        assert_eq!(error_msg, "Validation error: Invalid input");
    }

    #[test]
    fn test_serialization_error_not_retryable() {
        // Create a serde_json error
        let json_error = serde_json::from_str::<serde_json::Value>("invalid json").unwrap_err();
        let error = TlqError::Serialization(json_error);
        assert!(!error.is_retryable());

        let error_msg = format!("{}", error);
        assert!(error_msg.contains("Serialization error:"));
    }

    #[test]
    fn test_max_retries_exceeded_not_retryable() {
        let error = TlqError::MaxRetriesExceeded { max_retries: 3 };
        assert!(!error.is_retryable());

        let error_msg = format!("{}", error);
        assert_eq!(error_msg, "Max retries exceeded (3) for operation");
    }

    #[test]
    fn test_message_too_large_not_retryable() {
        let error = TlqError::MessageTooLarge { size: 70000 };
        assert!(!error.is_retryable());

        let error_msg = format!("{}", error);
        assert_eq!(error_msg, "Message too large: 70000 bytes (max: 65536)");
    }

    #[test]
    fn test_error_from_io_error() {
        let io_error = IoError::new(ErrorKind::PermissionDenied, "Access denied");
        let tlq_error: TlqError = io_error.into();

        assert!(tlq_error.is_retryable()); // IO errors are retryable
        assert!(matches!(tlq_error, TlqError::Io(_)));
    }

    #[test]
    fn test_error_from_serde_json_error() {
        let json_error = serde_json::from_str::<serde_json::Value>("{invalid}").unwrap_err();
        let tlq_error: TlqError = json_error.into();

        assert!(!tlq_error.is_retryable()); // Serialization errors are not retryable
        assert!(matches!(tlq_error, TlqError::Serialization(_)));
    }

    #[test]
    fn test_different_io_error_kinds() {
        let error_kinds = vec![
            ErrorKind::NotFound,
            ErrorKind::PermissionDenied,
            ErrorKind::ConnectionRefused,
            ErrorKind::ConnectionReset,
            ErrorKind::TimedOut,
            ErrorKind::Interrupted,
        ];

        for kind in error_kinds {
            let io_error = IoError::new(kind, format!("{:?} error", kind));
            let tlq_error = TlqError::Io(io_error);

            // All IO errors should be retryable
            assert!(tlq_error.is_retryable());
        }
    }

    #[test]
    fn test_server_error_status_codes() {
        let test_cases = vec![
            (400, "Bad Request"),
            (401, "Unauthorized"),
            (403, "Forbidden"),
            (404, "Not Found"),
            (500, "Internal Server Error"),
            (502, "Bad Gateway"),
            (503, "Service Unavailable"),
            (504, "Gateway Timeout"),
        ];

        for (status, message) in test_cases {
            let error = TlqError::Server {
                status,
                message: message.to_string(),
            };

            // Server errors should not be retryable
            assert!(!error.is_retryable());

            let error_msg = format!("{}", error);
            assert!(error_msg.contains(&status.to_string()));
            assert!(error_msg.contains(message));
        }
    }

    #[test]
    fn test_error_debug_formatting() {
        let error = TlqError::Connection("test connection error".to_string());
        let debug_str = format!("{:?}", error);
        assert!(debug_str.contains("Connection"));
        assert!(debug_str.contains("test connection error"));
    }

    #[test]
    fn test_result_type_alias() {
        // Test that our Result type alias works correctly
        let success: Result<String> = Ok("success".to_string());
        assert!(success.is_ok());
        if let Ok(value) = success {
            assert_eq!(value, "success");
        }

        let failure: Result<String> = Err(TlqError::Validation("test error".to_string()));
        assert!(failure.is_err());

        match failure {
            Err(TlqError::Validation(msg)) => assert_eq!(msg, "test error"),
            _ => panic!("Expected validation error"),
        }
    }

    #[test]
    fn test_timeout_edge_cases() {
        // Test various timeout values
        let timeout_0 = TlqError::Timeout(0);
        assert!(timeout_0.is_retryable());
        assert_eq!(format!("{}", timeout_0), "Timeout error after 0ms");

        let timeout_max = TlqError::Timeout(u64::MAX);
        assert!(timeout_max.is_retryable());
        assert_eq!(
            format!("{}", timeout_max),
            format!("Timeout error after {}ms", u64::MAX)
        );
    }

    #[test]
    fn test_message_size_edge_cases() {
        // Test various message sizes
        let size_0 = TlqError::MessageTooLarge { size: 0 };
        assert_eq!(
            format!("{}", size_0),
            "Message too large: 0 bytes (max: 65536)"
        );

        let size_max = TlqError::MessageTooLarge { size: usize::MAX };
        assert_eq!(
            format!("{}", size_max),
            format!("Message too large: {} bytes (max: 65536)", usize::MAX)
        );

        let size_just_over = TlqError::MessageTooLarge { size: 65537 };
        assert_eq!(
            format!("{}", size_just_over),
            "Message too large: 65537 bytes (max: 65536)"
        );
    }

    #[test]
    fn test_empty_error_messages() {
        let connection_error = TlqError::Connection("".to_string());
        assert_eq!(format!("{}", connection_error), "Connection error: ");

        let validation_error = TlqError::Validation("".to_string());
        assert_eq!(format!("{}", validation_error), "Validation error: ");

        let server_error = TlqError::Server {
            status: 500,
            message: "".to_string(),
        };
        assert_eq!(format!("{}", server_error), "Server error: 500 - ");
    }
}
