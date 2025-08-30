use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message {
    pub id: Uuid,
    pub body: String,
    pub state: MessageState,
    pub retry_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub enum MessageState {
    Ready,
    Processing,
    Failed,
}

impl Message {
    pub fn new(body: String) -> Self {
        Self {
            id: Uuid::now_v7(),
            body,
            state: MessageState::Ready,
            retry_count: 0,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct AddMessageRequest {
    pub body: String,
}

#[derive(Debug, Deserialize)]
pub struct AddMessageResponse {
    pub message: Message,
}

#[derive(Debug, Serialize)]
pub struct GetMessagesRequest {
    pub count: u32,
}

#[derive(Debug, Deserialize)]
pub struct GetMessagesResponse {
    pub messages: Vec<Message>,
}

#[derive(Debug, Serialize)]
pub struct DeleteMessagesRequest {
    pub ids: Vec<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct DeleteMessagesResponse {
    pub deleted_count: u32,
}

#[derive(Debug, Serialize)]
pub struct RetryMessagesRequest {
    pub ids: Vec<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct RetryMessagesResponse {
    pub retry_count: u32,
}

#[derive(Debug, Deserialize)]
pub struct PurgeQueueResponse {
    pub purged_count: u32,
}

#[derive(Debug, Deserialize)]
pub struct HealthCheckResponse {
    pub status: String,
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
        assert_eq!(serde_json::to_string(&MessageState::Ready).unwrap(), "\"Ready\"");
        assert_eq!(serde_json::to_string(&MessageState::Processing).unwrap(), "\"Processing\"");
        assert_eq!(serde_json::to_string(&MessageState::Failed).unwrap(), "\"Failed\"");
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
        let retry_req = RetryMessagesRequest {
            ids: vec![id1],
        };
        let json = serde_json::to_string(&retry_req).unwrap();
        assert!(json.contains("\"ids\":"));
    }

    #[test]
    fn test_response_deserialization() {
        // Test AddMessageResponse
        let message = Message::new("test".to_string());
        let add_resp_json = format!(
            r#"{{"message":{{"id":"{}","body":"{}","state":"Ready","retry_count":0}}}}"#,
            message.id, message.body
        );
        let add_resp: AddMessageResponse = serde_json::from_str(&add_resp_json).unwrap();
        assert_eq!(add_resp.message.body, "test");

        // Test GetMessagesResponse
        let get_resp_json = r#"{"messages":[]}"#;
        let get_resp: GetMessagesResponse = serde_json::from_str(&get_resp_json).unwrap();
        assert!(get_resp.messages.is_empty());

        // Test DeleteMessagesResponse
        let delete_resp_json = r#"{"deleted_count":3}"#;
        let delete_resp: DeleteMessagesResponse = serde_json::from_str(&delete_resp_json).unwrap();
        assert_eq!(delete_resp.deleted_count, 3);

        // Test RetryMessagesResponse
        let retry_resp_json = r#"{"retry_count":2}"#;
        let retry_resp: RetryMessagesResponse = serde_json::from_str(&retry_resp_json).unwrap();
        assert_eq!(retry_resp.retry_count, 2);

        // Test PurgeQueueResponse
        let purge_resp_json = r#"{"purged_count":10}"#;
        let purge_resp: PurgeQueueResponse = serde_json::from_str(&purge_resp_json).unwrap();
        assert_eq!(purge_resp.purged_count, 10);

        // Test HealthCheckResponse
        let health_resp_json = r#"{"status":"OK"}"#;
        let health_resp: HealthCheckResponse = serde_json::from_str(&health_resp_json).unwrap();
        assert_eq!(health_resp.status, "OK");
    }

    #[test]
    fn test_malformed_response_deserialization() {
        // Test that malformed JSON fails gracefully
        let malformed_json = r#"{"message": invalid}"#;
        let result = serde_json::from_str::<AddMessageResponse>(&malformed_json);
        assert!(result.is_err());

        // Test missing required fields
        let incomplete_json = r#"{"deleted_count":}"#; // Missing value
        let result = serde_json::from_str::<DeleteMessagesResponse>(&incomplete_json);
        assert!(result.is_err());

        // Test wrong field types in responses
        let wrong_type_json = r#"{"deleted_count":"not_a_number"}"#;
        let result = serde_json::from_str::<DeleteMessagesResponse>(&wrong_type_json);
        assert!(result.is_err());

        // Test malformed message in response
        let bad_message_json = r#"{"message":{"id":"invalid-uuid","body":"test","state":"Ready","retry_count":0}}"#;
        let result = serde_json::from_str::<AddMessageResponse>(&bad_message_json);
        assert!(result.is_err()); // Should fail due to invalid UUID
    }
}
