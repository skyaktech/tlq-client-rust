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