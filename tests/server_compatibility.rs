use std::time::Duration;
use tlq_client::{Message, TlqClient, TlqError};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio::time::timeout;

// Serve one complete HTTP exchange on an ephemeral port, without a running TLQ server.
async fn mock_server(status: &str, body: &str) -> (u16, JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );

    let task = tokio::spawn(async move {
        timeout(Duration::from_secs(5), async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 4096];
            let header_end = loop {
                let read = stream.read(&mut buffer).await.unwrap();
                assert_ne!(read, 0, "request ended before headers");
                request.extend_from_slice(&buffer[..read]);
                if let Some(end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                    break end + 4;
                }
            };
            let headers = std::str::from_utf8(&request[..header_end]).unwrap();
            let content_length = headers
                .lines()
                .filter_map(|line| line.split_once(':'))
                .find(|(name, _)| name.eq_ignore_ascii_case("Content-Length"))
                .map(|(_, value)| value.trim().parse::<usize>().unwrap())
                .unwrap_or(0);

            while request.len() < header_end + content_length {
                let read = stream.read(&mut buffer).await.unwrap();
                assert_ne!(read, 0, "request ended before body");
                request.extend_from_slice(&buffer[..read]);
            }

            stream.write_all(response.as_bytes()).await.unwrap();
            stream.shutdown().await.unwrap();
            String::from_utf8(request).unwrap()
        })
        .await
        .expect("mock HTTP exchange timed out")
    });

    (port, task)
}

fn client(port: u16) -> TlqClient {
    TlqClient::builder()
        .host("127.0.0.1")
        .port(port)
        .max_retries(0)
        .build()
}

#[tokio::test]
async fn server_info_reads_current_legacy_and_unknown_greetings() {
    for (greeting, version, uptime) in [
        (
            "Hello TLQ v0.5.0 (uptime: 1d 2h 3m 4s)",
            Some("0.5.0"),
            Some(Duration::from_secs(93_784)),
        ),
        ("Hello World", None, None),
        ("A future greeting format", None, None),
    ] {
        let (port, server) = mock_server("200 OK", &serde_json::to_string(greeting).unwrap()).await;
        let info = client(port).server_info().await.unwrap();

        assert_eq!(info.greeting, greeting);
        assert_eq!(info.version.as_deref(), version);
        assert_eq!(info.uptime, uptime);
        assert!(server.await.unwrap().starts_with("GET /hello HTTP/1.1\r\n"));
    }
}

#[tokio::test]
async fn server_info_propagates_http_errors() {
    let (port, server) = mock_server("503 Service Unavailable", r#""unavailable""#).await;
    let error = client(port).server_info().await.unwrap_err();

    assert!(
        matches!(error, TlqError::Server { status: 503, message } if message == r#""unavailable""#)
    );
    assert!(server.await.unwrap().starts_with("GET /hello HTTP/1.1\r\n"));
}

#[tokio::test]
async fn server_info_rejects_invalid_json_and_non_string_responses() {
    for body in ["not json", r#"{"version":"0.5.0"}"#, "null"] {
        let (port, server) = mock_server("200 OK", body).await;
        let error = client(port).server_info().await.unwrap_err();

        assert!(matches!(error, TlqError::Serialization(_)));
        assert!(server.await.unwrap().starts_with("GET /hello HTTP/1.1\r\n"));
    }
}

#[tokio::test]
async fn health_check_accepts_current_and_legacy_greetings() {
    for greeting in ["Hello TLQ v0.5.0 (uptime: 5s)", "Hello World"] {
        let (port, server) = mock_server("200 OK", &serde_json::to_string(greeting).unwrap()).await;

        assert!(client(port).health_check().await.unwrap());
        assert!(server.await.unwrap().starts_with("GET /hello HTTP/1.1\r\n"));
    }
}

#[tokio::test]
async fn add_message_sends_bodies_at_default_and_custom_byte_limits() {
    for (limit, body) in [
        (None, "x".repeat(65_536)),
        (Some(131_072), "x".repeat(131_072)),
        (Some(4), "éé".to_string()),
        (Some(0), String::new()),
    ] {
        let expected = Message::new(body.clone());
        let response = serde_json::to_string(&expected).unwrap();
        let (port, server) = mock_server("200 OK", &response).await;
        let mut builder = TlqClient::builder()
            .host("127.0.0.1")
            .port(port)
            .max_retries(0);
        if let Some(limit) = limit {
            builder = builder.max_message_size(limit);
        }

        let message = builder.build().add_message(body.clone()).await.unwrap();
        assert_eq!(message, expected);
        let request = server.await.unwrap();
        assert!(request.starts_with("POST /add HTTP/1.1\r\n"));
        let (_, request_body) = request.split_once("\r\n\r\n").unwrap();
        let sent: serde_json::Value = serde_json::from_str(request_body).unwrap();
        assert_eq!(sent["body"], body);
    }
}

#[tokio::test]
async fn add_message_rejects_oversized_utf8_bodies_before_connecting() {
    for (limit, body) in [
        (None, "x".repeat(65_537)),
        (Some(131_072), "x".repeat(131_073)),
        (Some(4), "ééé".to_string()),
        (Some(0), "x".to_string()),
    ] {
        let mut builder = TlqClient::builder()
            .host("127.0.0.1")
            .port(0)
            .max_retries(0);
        if let Some(limit) = limit {
            builder = builder.max_message_size(limit);
        }

        let size = body.len();
        let error = builder.build().add_message(body).await.unwrap_err();
        assert!(matches!(error, TlqError::MessageTooLarge { size: actual } if actual == size));
    }
}
