use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Represents a message in the TLQ queue system.
///
/// Each message has a unique identifier, content, and metadata about its processing state.
/// Messages are automatically assigned UUID v7 identifiers which provide time-ordering.
///
/// # Examples
///
/// ```
/// use tlq_client::Message;
///
/// // Create a new message
/// let message = Message::new("Hello, World!".to_string());
/// println!("Message ID: {}", message.id);
/// println!("Message body: {}", message.body);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message {
    /// Unique identifier for the message (UUID v7 format for time-ordering)
    pub id: Uuid,
    /// The message content/body as a string
    pub body: String,
    /// Current processing state of the message
    pub state: MessageState,
    /// Optional ISO datetime string indicating when the message lock expires
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lock_until: Option<String>, // ISO datetime string
    /// Number of times this message has been retried after failure
    pub retry_count: u32,
}

/// Represents the current processing state of a message in the queue.
///
/// Messages transition through these states as they are processed:
/// - `Ready` → `Processing` (when retrieved by a consumer)
/// - `Processing` → `Failed` (if processing fails)
/// - `Failed` → `Ready` (when retried)
/// - Any state → deleted (when explicitly deleted)
///
/// # Serialization
///
/// States are serialized in PascalCase format ("Ready", "Processing", "Failed")
/// to match the TLQ server API expectations.
///
/// # Examples
///
/// ```
/// use tlq_client::MessageState;
///
/// let state = MessageState::Ready;
/// assert_eq!(serde_json::to_string(&state).unwrap(), "\"Ready\"");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub enum MessageState {
    /// Message is ready to be processed by a consumer
    Ready,
    /// Message is currently being processed by a consumer
    Processing,
    /// Message processing failed and may need to be retried
    Failed,
}

impl Message {
    /// Creates a new message with the specified body content.
    ///
    /// The message is initialized with:
    /// - A new UUID v7 identifier (provides time-ordering)
    /// - State set to [`MessageState::Ready`]
    /// - No lock expiration time
    /// - Zero retry count
    ///
    /// # Arguments
    ///
    /// * `body` - The message content as a String
    ///
    /// # Examples
    ///
    /// ```
    /// use tlq_client::{Message, MessageState};
    ///
    /// let message = Message::new("Process this task".to_string());
    /// assert_eq!(message.body, "Process this task");
    /// assert_eq!(message.state, MessageState::Ready);
    /// assert_eq!(message.retry_count, 0);
    /// assert!(message.lock_until.is_none());
    /// ```
    pub fn new(body: String) -> Self {
        Self {
            id: Uuid::now_v7(),
            body,
            state: MessageState::Ready,
            lock_until: None,
            retry_count: 0,
        }
    }
}

// Internal request structures for TLQ API communication

/// Request structure for adding a message to the queue
#[derive(Debug, Serialize)]
pub struct AddMessageRequest {
    pub body: String,
}

/// Request structure for retrieving messages from the queue
#[derive(Debug, Serialize)]
pub struct GetMessagesRequest {
    pub count: u32,
}

/// Request structure for deleting messages from the queue
#[derive(Debug, Serialize)]
pub struct DeleteMessagesRequest {
    pub ids: Vec<Uuid>,
}

/// Request structure for retrying failed messages
#[derive(Debug, Serialize)]
pub struct RetryMessagesRequest {
    pub ids: Vec<Uuid>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    #[test]
    fn test_message_creation() {
        let message = Message::new("Test message".to_string());

        assert_eq!(message.body, "Test message");
        assert_eq!(message.state, MessageState::Ready);
        assert_eq!(message.retry_count, 0);

        // UUID should be valid
        assert!(!message.id.to_string().is_empty());
    }

    #[test]
    fn test_message_state_serialization() {
        // Test that MessageState serializes to the expected Pascal case
        assert_eq!(
            serde_json::to_string(&MessageState::Ready).unwrap(),
            "\"Ready\""
        );
        assert_eq!(
            serde_json::to_string(&MessageState::Processing).unwrap(),
            "\"Processing\""
        );
        assert_eq!(
            serde_json::to_string(&MessageState::Failed).unwrap(),
            "\"Failed\""
        );
    }

    #[test]
    fn test_message_state_deserialization() {
        // Test that MessageState deserializes from Pascal case
        assert_eq!(
            serde_json::from_str::<MessageState>("\"Ready\"").unwrap(),
            MessageState::Ready
        );
        assert_eq!(
            serde_json::from_str::<MessageState>("\"Processing\"").unwrap(),
            MessageState::Processing
        );
        assert_eq!(
            serde_json::from_str::<MessageState>("\"Failed\"").unwrap(),
            MessageState::Failed
        );
    }

    #[test]
    fn test_message_state_invalid_deserialization() {
        // Test that invalid states fail to deserialize
        let result = serde_json::from_str::<MessageState>("\"Invalid\"");
        assert!(result.is_err());

        let result = serde_json::from_str::<MessageState>("\"ready\""); // lowercase
        assert!(result.is_err());

        let result = serde_json::from_str::<MessageState>("\"READY\""); // uppercase
        assert!(result.is_err());
    }

    #[test]
    fn test_message_serialization() {
        let message = Message::new("test body".to_string());

        let json = serde_json::to_string(&message).unwrap();

        // Should contain all fields
        assert!(json.contains("\"id\":"));
        assert!(json.contains("\"body\":\"test body\""));
        assert!(json.contains("\"state\":\"Ready\""));
        assert!(json.contains("\"retry_count\":0"));

        // Should deserialize back correctly
        let deserialized: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.body, message.body);
        assert_eq!(deserialized.state, message.state);
        assert_eq!(deserialized.retry_count, message.retry_count);
        assert_eq!(deserialized.id, message.id);
    }

    #[test]
    fn test_message_with_special_characters() {
        let special_body = "Test with 🦀 emojis and \"quotes\" and \n newlines \t tabs";
        let message = Message::new(special_body.to_string());

        assert_eq!(message.body, special_body);

        // Should serialize and deserialize correctly
        let json = serde_json::to_string(&message).unwrap();
        let deserialized: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.body, special_body);
    }

    #[test]
    fn test_message_with_very_long_body() {
        let long_body = "a".repeat(100_000);
        let message = Message::new(long_body.clone());

        assert_eq!(message.body, long_body);
        assert_eq!(message.body.len(), 100_000);
    }

    #[test]
    fn test_message_with_empty_body() {
        let message = Message::new("".to_string());

        assert_eq!(message.body, "");
        assert_eq!(message.state, MessageState::Ready);
        assert_eq!(message.retry_count, 0);
    }

    #[test]
    fn test_request_response_structures() {
        // Test AddMessageRequest
        let add_req = AddMessageRequest {
            body: "test message".to_string(),
        };
        let json = serde_json::to_string(&add_req).unwrap();
        assert!(json.contains("\"body\":\"test message\""));

        // Test GetMessagesRequest
        let get_req = GetMessagesRequest { count: 5 };
        let json = serde_json::to_string(&get_req).unwrap();
        assert!(json.contains("\"count\":5"));

        // Test DeleteMessagesRequest
        use uuid::Uuid;
        let id1 = Uuid::now_v7();
        let id2 = Uuid::now_v7();
        let delete_req = DeleteMessagesRequest {
            ids: vec![id1, id2],
        };
        let json = serde_json::to_string(&delete_req).unwrap();
        assert!(json.contains("\"ids\":"));

        // Test RetryMessagesRequest
        let retry_req = RetryMessagesRequest { ids: vec![id1] };
        let json = serde_json::to_string(&retry_req).unwrap();
        assert!(json.contains("\"ids\":"));
    }

    #[test]
    fn test_response_deserialization() {
        // Test direct Message response (for add_message)
        let message_json = r#"{"id":"0198fbd8-344e-7b70-841f-3fbd4b371e4c","body":"test","state":"Ready","lock_until":null,"retry_count":0}"#;
        let message: Message = serde_json::from_str(message_json).unwrap();
        assert_eq!(message.body, "test");
        assert_eq!(message.state, MessageState::Ready);
        assert_eq!(message.retry_count, 0);
        assert_eq!(message.lock_until, None);

        // Test array of messages response (for get_messages)
        let messages_json = r#"[{"id":"0198fbd8-344e-7b70-841f-3fbd4b371e4c","body":"test1","state":"Processing","lock_until":null,"retry_count":1}]"#;
        let messages: Vec<Message> = serde_json::from_str(messages_json).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].body, "test1");
        assert_eq!(messages[0].state, MessageState::Processing);

        // Test success string responses (for delete/retry/purge)
        let success_response: String = serde_json::from_str(r#""Success""#).unwrap();
        assert_eq!(success_response, "Success");

        // Test health check response
        let health_response: String = serde_json::from_str(r#""Hello World""#).unwrap();
        assert_eq!(health_response, "Hello World");
    }

    #[test]
    fn test_malformed_response_deserialization() {
        // Test that malformed JSON fails gracefully
        let malformed_json = r#"{"id": invalid}"#;
        let result = serde_json::from_str::<Message>(malformed_json);
        assert!(result.is_err());

        // Test missing required fields in Message
        let incomplete_json = r#"{"id":"0198fbd8-344e-7b70-841f-3fbd4b371e4c","body":"test"}"#; // Missing state and retry_count
        let result = serde_json::from_str::<Message>(incomplete_json);
        assert!(result.is_err());

        // Test wrong field types in Message
        let wrong_type_json = r#"{"id":"0198fbd8-344e-7b70-841f-3fbd4b371e4c","body":"test","state":"Ready","retry_count":"not_a_number"}"#;
        let result = serde_json::from_str::<Message>(wrong_type_json);
        assert!(result.is_err());

        // Test malformed message with invalid UUID
        let bad_uuid_json = r#"{"id":"invalid-uuid","body":"test","state":"Ready","lock_until":null,"retry_count":0}"#;
        let result = serde_json::from_str::<Message>(bad_uuid_json);
        assert!(result.is_err()); // Should fail due to invalid UUID

        // Test malformed array
        let bad_array_json = r#"[{"id":"invalid"}]"#;
        let result = serde_json::from_str::<Vec<Message>>(bad_array_json);
        assert!(result.is_err());
    }
}
