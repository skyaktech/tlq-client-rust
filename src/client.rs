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

        let mut response = Vec::new();
        stream.read_to_end(&mut response).await?;

        let response_str = String::from_utf8_lossy(&response);

        if let Some(body_start) = response_str.find("\r\n\r\n") {
            let headers = &response_str[..body_start];
            let body = &response_str[body_start + 4..];

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

            serde_json::from_str(body).map_err(Into::into)
        } else {
            Err(TlqError::Connection("Invalid HTTP response".to_string()))
        }
    }

    pub async fn health_check(&self) -> Result<bool> {
        let mut stream = timeout(Duration::from_secs(5), TcpStream::connect(&self.base_url))
            .await
            .map_err(|_| TlqError::Timeout(5000))?
            .map_err(|e| TlqError::Connection(e.to_string()))?;

        let request = format!(
            "GET /hello HTTP/1.1\r\n\
             Host: {}\r\n\
             \r\n",
            self.base_url
        );

        stream.write_all(request.as_bytes()).await?;

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
        let response: AddMessageResponse = self.request("/add", &request).await?;
        Ok(response.message)
    }

    pub async fn get_messages(&self, count: u32) -> Result<Vec<Message>> {
        if count == 0 {
            return Err(TlqError::Validation(
                "Count must be greater than 0".to_string(),
            ));
        }

        let request = GetMessagesRequest { count };
        let response: GetMessagesResponse = self.request("/get", &request).await?;
        Ok(response.messages)
    }

    pub async fn get_message(&self) -> Result<Option<Message>> {
        let messages = self.get_messages(1).await?;
        Ok(messages.into_iter().next())
    }

    pub async fn delete_message(&self, id: Uuid) -> Result<u32> {
        self.delete_messages(&[id]).await
    }

    pub async fn delete_messages(&self, ids: &[Uuid]) -> Result<u32> {
        if ids.is_empty() {
            return Err(TlqError::Validation("No message IDs provided".to_string()));
        }

        let request = DeleteMessagesRequest { ids: ids.to_vec() };
        let response: DeleteMessagesResponse = self.request("/delete", &request).await?;
        Ok(response.deleted_count)
    }

    pub async fn retry_message(&self, id: Uuid) -> Result<u32> {
        self.retry_messages(&[id]).await
    }

    pub async fn retry_messages(&self, ids: &[Uuid]) -> Result<u32> {
        if ids.is_empty() {
            return Err(TlqError::Validation("No message IDs provided".to_string()));
        }

        let request = RetryMessagesRequest { ids: ids.to_vec() };
        let response: RetryMessagesResponse = self.request("/retry", &request).await?;
        Ok(response.retry_count)
    }

    pub async fn purge_queue(&self) -> Result<u32> {
        let response: PurgeQueueResponse = self.request("/purge", &serde_json::json!({})).await?;
        Ok(response.purged_count)
    }
}
