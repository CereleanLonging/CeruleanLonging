use super::*;
use crate::api_generation::{ApiClient, ApiFailure, ApiTarget};
use cerulean_domain::{credentials::ProviderEndpoint, secret::SecretInput};
use serde_json::json;
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::oneshot,
};

#[tokio::test]
async fn transport_285_observes_real_socket_failure_phases_without_retry_or_error_text() {
    for case in [
        "connect",
        "send",
        "request-timeout",
        "body",
        "body-timeout",
        "http",
        "cancel",
        "ok",
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (signal, mut cancel) = oneshot::channel();
        let server = if case == "connect" {
            drop(listener);
            None
        } else {
            Some(tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    let n = socket.read(&mut chunk).await.unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&chunk[..n]);
                    if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        let header = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                        let length: usize = header
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length: "))
                            .unwrap()
                            .parse()
                            .unwrap();
                        if bytes.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                match case {
                    "send" => {
                        socket
                            .write_all(b"INVALID HTTP RESPONSE\r\n\r\n")
                            .await
                            .unwrap();
                    }
                    "body" | "body-timeout" => {
                        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\n{").await.unwrap();
                    }
                    "http" => {
                        socket.write_all(b"HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
                    }
                    "ok" => {
                        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").await.unwrap();
                    }
                    _ => (),
                }
                if ["request-timeout", "body-timeout"].contains(&case) {
                    let mut byte = [0];
                    let _ =
                        tokio::time::timeout(Duration::from_secs(3), socket.read(&mut byte)).await;
                }
                drop(socket);
                assert!(
                    tokio::time::timeout(Duration::from_millis(40), listener.accept())
                        .await
                        .is_err(),
                    "unexpected retry"
                );
            }))
        };
        let client = ApiClient {
            http: ApiClient::http(false).unwrap(),
            key: SecretInput::new("synthetic-transport-285-only".into()).unwrap(),
        };
        let mut detail = None;
        if case == "cancel" {
            // Cancellation before send must not even connect. The server was
            // never given work, so drop it after checking the client result.
            signal.send(()).unwrap();
            let result = client
                .send_json_observed(
                    &ApiTarget::Provider {
                        endpoint: ProviderEndpoint::A6Api {},
                    },
                    &json!({"request":"fixture"}),
                    1,
                    &mut cancel,
                    &format!("http://{address}/generate"),
                    &mut detail,
                )
                .await;
            assert_eq!(result.unwrap_err(), ApiFailure::NotSent {});
            assert!(detail.is_none());
            let server = server.unwrap();
            server.abort();
            let _ = server.await;
            continue;
        }
        let result = client
            .send_json_observed(
                &ApiTarget::Provider {
                    endpoint: ProviderEndpoint::A6Api {},
                },
                &json!({"request":"fixture"}),
                if case == "connect" { 5 } else { 1 },
                &mut cancel,
                &format!("http://{address}/generate"),
                &mut detail,
            )
            .await;
        let expected = match case {
            "connect" => Some(TransportStage::Connect),
            "send" => Some(TransportStage::RequestSend),
            "request-timeout" => Some(TransportStage::RequestTimeout),
            "body" => Some(TransportStage::BodyRead),
            "body-timeout" => Some(TransportStage::BodyTimeout),
            _ => None,
        };
        assert_eq!(detail, expected, "{case}");
        match case {
            "http" => assert_eq!(result.unwrap_err(), ApiFailure::Http { status: 429 }),
            "ok" => assert_eq!(result.unwrap(), json!({})),
            _ => assert_eq!(result.unwrap_err(), ApiFailure::Transport {}),
        }
        if let Some(server) = server {
            server.await.unwrap();
        }
        drop(signal);
    }
}
