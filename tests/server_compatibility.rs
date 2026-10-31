use std::time::Duration;
use tlq_client::{Message, TlqClient, TlqError};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpSocket, TcpStream};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use uuid::Uuid;

const HELLO_RESPONSE: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 13\r\nConnection: close\r\n\r\n\"Hello World\"";

async fn read_request(stream: &mut TcpStream) -> String {
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

    String::from_utf8(request).unwrap()
}

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
            let request = read_request(&mut stream).await;

            stream.write_all(response.as_bytes()).await.unwrap();
            stream.shutdown().await.unwrap();
            request
        })
        .await
        .expect("mock HTTP exchange timed out")
    });

    (port, task)
}

// Keep the response open until the client's deadline closes the connection.
async fn stalled_server(response_prefix: &'static [u8]) -> (u16, JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = tokio::spawn(async move {
        timeout(Duration::from_secs(10), async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = read_request(&mut stream).await;
            stream.write_all(response_prefix).await.unwrap();
            let mut buffer = [0; 1];
            assert_eq!(stream.read(&mut buffer).await.unwrap(), 0);
            request
        })
        .await
        .expect("client did not close the stalled connection")
    });
    (port, task)
}

fn client(port: u16) -> TlqClient {
    TlqClient::builder().host("127.0.0.1").port(port).build()
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
async fn server_info_propagates_http_errors_without_retrying() {
    for (status, code) in [("400 Bad Request", 400), ("503 Service Unavailable", 503)] {
        let (port, server) = mock_server(status, r#""unavailable""#).await;
        let error = client(port).server_info().await.unwrap_err();

        assert!(
            matches!(error, TlqError::Server { status, message } if status == code && message == r#""unavailable""#)
        );
        assert!(server.await.unwrap().starts_with("GET /hello HTTP/1.1\r\n"));
    }
}

#[tokio::test]
async fn server_info_rejects_invalid_json_and_non_string_responses_without_retrying() {
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
async fn health_check_reports_http_errors_as_unhealthy() {
    let (port, server) = mock_server("503 Service Unavailable", r#""unavailable""#).await;

    assert!(!client(port).health_check().await.unwrap());
    assert!(server.await.unwrap().starts_with("GET /hello HTTP/1.1\r\n"));
}

#[tokio::test]
async fn add_message_does_not_retry_http_or_json_errors() {
    for (status, body, code) in [
        ("400 Bad Request", "invalid request", Some(400)),
        ("503 Service Unavailable", "unavailable", Some(503)),
        ("200 OK", "not json", None),
    ] {
        let (port, server) = mock_server(status, body).await;
        let error = client(port).add_message("test").await.unwrap_err();

        assert!(!error.is_retryable());
        if let Some(code) = code {
            assert!(matches!(error, TlqError::Server { status, .. } if status == code));
        } else {
            assert!(matches!(
                error,
                TlqError::OutcomeUnknown { cause } if matches!(*cause, TlqError::Serialization(_))
            ));
        }
        assert!(server.await.unwrap().starts_with("POST /add HTTP/1.1\r\n"));
    }
}

#[tokio::test]
async fn requests_are_not_replayed_after_receipt() {
    for endpoint in [
        "/hello", "/stats", "/add", "/get", "/delete", "/retry", "/purge",
    ] {
        for failure in ["timeout", "reset", "malformed", "json"] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let client = TlqClient::builder()
                .host("127.0.0.1")
                .port(listener.local_addr().unwrap().port())
                .timeout_ms(100)
                .retry_delay_ms(1)
                .build();
            let receive = async {
                let (mut stream, _) = listener.accept().await.unwrap();
                // The complete request has arrived and could already have changed the queue.
                let request = read_request(&mut stream).await;
                match failure {
                    "timeout" => {
                        let mut buffer = [0; 1];
                        assert_eq!(stream.read(&mut buffer).await.unwrap(), 0);
                    }
                    "reset" => stream.set_zero_linger().unwrap(),
                    "malformed" => {
                        stream.write_all(b"broken response\r\n").await.unwrap();
                        stream.shutdown().await.unwrap();
                    }
                    "json" => {
                        stream
                            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\nnot json")
                            .await
                            .unwrap();
                        stream.shutdown().await.unwrap();
                    }
                    _ => unreachable!(),
                }
                request
            };
            let operation = async {
                match endpoint {
                    "/hello" => client.server_info().await.map(|_| ()),
                    "/stats" => client.stats().await.map(|_| ()),
                    "/add" => client.add_message("test").await.map(|_| ()),
                    "/get" => client.get_messages(1).await.map(|_| ()),
                    "/delete" => client.delete_message(Uuid::now_v7()).await.map(|_| ()),
                    "/retry" => client.retry_message(Uuid::now_v7()).await.map(|_| ()),
                    "/purge" => client.purge_queue().await.map(|_| ()),
                    _ => unreachable!(),
                }
            };
            let (result, request) = tokio::join!(
                timeout(Duration::from_secs(2), operation),
                timeout(Duration::from_secs(2), receive),
            );
            let error = result.expect("request did not finish").unwrap_err();
            let queue_changing = !matches!(endpoint, "/hello" | "/stats");
            let cause = if queue_changing {
                assert!(!error.is_retryable(), "{endpoint} failed with {error:?}");
                match error {
                    TlqError::OutcomeUnknown { cause } => *cause,
                    error => panic!("expected an unknown outcome for {endpoint}, got {error:?}"),
                }
            } else {
                error
            };
            match failure {
                "timeout" => assert!(matches!(cause, TlqError::Timeout(100)), "{cause:?}"),
                "reset" => assert!(matches!(cause, TlqError::Io(_)), "{cause:?}"),
                "malformed" => assert!(matches!(cause, TlqError::Connection(_)), "{cause:?}"),
                "json" => assert!(matches!(cause, TlqError::Serialization(_)), "{cause:?}"),
                _ => unreachable!(),
            }
            let method = if queue_changing { "POST" } else { "GET" };
            assert!(request
                .unwrap()
                .starts_with(&format!("{method} {endpoint} HTTP/1.1\r\n")));
            // Keep listening throughout the operation so retries cannot hide as refused connects.
            assert!(
                timeout(Duration::from_millis(25), listener.accept())
                    .await
                    .is_err(),
                "{endpoint} was replayed after {failure}"
            );
        }
    }
}

#[tokio::test]
async fn get_and_post_requests_time_out_after_connecting() {
    for endpoint in ["/hello", "/stats", "/add"] {
        let (port, server) = stalled_server(b"").await;
        let client = TlqClient::builder()
            .host("127.0.0.1")
            .port(port)
            .timeout_ms(100)
            .max_retries(0)
            .build();
        let result = timeout(Duration::from_secs(2), async {
            match endpoint {
                "/hello" => client.server_info().await.map(|_| ()),
                "/stats" => client.stats().await.map(|_| ()),
                "/add" => client.add_message("test").await.map(|_| ()),
                _ => unreachable!(),
            }
        })
        .await
        .expect("request ignored its timeout");

        let error = result.unwrap_err();
        if endpoint == "/add" {
            assert!(!error.is_retryable());
            assert!(matches!(
                error,
                TlqError::OutcomeUnknown { cause } if matches!(*cause, TlqError::Timeout(100))
            ));
        } else {
            assert!(error.is_retryable());
            assert!(matches!(error, TlqError::Timeout(100)));
        }
        let method = if endpoint == "/add" { "POST" } else { "GET" };
        assert!(server
            .await
            .unwrap()
            .starts_with(&format!("{method} {endpoint} HTTP/1.1\r\n")));
    }
}

#[tokio::test]
async fn response_reads_time_out_for_partial_bodies_and_missing_eof() {
    for response in [
        b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n\"Hello".as_slice(),
        HELLO_RESPONSE,
    ] {
        let (port, server) = stalled_server(response).await;
        let client = TlqClient::builder()
            .host("127.0.0.1")
            .port(port)
            .timeout_ms(100)
            .max_retries(0)
            .build();
        let result = timeout(Duration::from_secs(2), client.server_info())
            .await
            .expect("response read ignored its timeout");

        assert!(matches!(result, Err(TlqError::Timeout(100))));
        assert!(server.await.unwrap().starts_with("GET /hello HTTP/1.1\r\n"));
    }
}

#[tokio::test]
async fn request_writes_time_out_without_replaying_when_server_does_not_read() {
    let socket = TcpSocket::new_v4().unwrap();
    socket.set_recv_buffer_size(1024).unwrap();
    socket.bind("127.0.0.1:0".parse().unwrap()).unwrap();
    let listener = socket.listen(1).unwrap();
    let port = listener.local_addr().unwrap().port();
    let (accepted_tx, accepted_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        accepted_tx.send(()).unwrap();
        let _ = release_rx.await;
        drop(stream);
        listener
    });

    // Exceed socket buffers so write_all must wait for the non-reading peer.
    let size = 8 * 1024 * 1024;
    let client = TlqClient::builder()
        .host("127.0.0.1")
        .port(port)
        .max_message_size(size)
        .timeout_ms(1000)
        .retry_delay_ms(1)
        .build();
    let result = timeout(Duration::from_secs(3), client.add_message("x".repeat(size))).await;
    release_tx.send(()).unwrap();
    let listener = timeout(Duration::from_secs(2), server)
        .await
        .unwrap()
        .unwrap();
    accepted_rx
        .await
        .expect("server never accepted a connection");

    let error = result
        .expect("request write ignored its timeout")
        .unwrap_err();
    assert!(!error.is_retryable());
    assert!(matches!(
        error,
        TlqError::OutcomeUnknown { cause } if matches!(*cause, TlqError::Timeout(1000))
    ));
    // The listener stayed open throughout the request, so any replay is queued here.
    assert!(
        timeout(Duration::from_millis(25), listener.accept())
            .await
            .is_err(),
        "POST was replayed after a stalled write"
    );
}

#[tokio::test]
async fn health_check_uses_the_configured_timeout() {
    let (port, server) = stalled_server(b"").await;
    let client = TlqClient::builder()
        .host("127.0.0.1")
        .port(port)
        .timeout_ms(100)
        .max_retries(0)
        .build();
    let result = timeout(Duration::from_secs(2), client.health_check())
        .await
        .expect("health check ignored its timeout");

    assert!(matches!(result, Err(TlqError::Timeout(100))));
    assert!(server.await.unwrap().starts_with("GET /hello HTTP/1.1\r\n"));
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
        let expected_limit = limit.unwrap_or(65_536);
        assert_eq!(
            error.to_string(),
            format!("Message too large: {size} bytes (max: {expected_limit})")
        );
        assert!(matches!(
            error,
            TlqError::MessageTooLarge { size: actual, limit: actual_limit }
                if actual == size && actual_limit == expected_limit
        ));
    }
}
